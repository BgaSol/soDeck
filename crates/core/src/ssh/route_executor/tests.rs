use super::*;
use crate::route::{plan::{ProxyStep, SshStep}, SocksProxy};
use std::{io::{BufRead, BufReader}, process::{Child, Command, Stdio}};

struct Fixture { child: Child, root: tempfile::TempDir, steps: Vec<ResolvedStep> }
impl Fixture {
    fn start(kinds: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::var("SODECK_TEST_PYTHON").expect("set fixture Python"))
            .args(["-u", concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/core/mixed_route_fixture.py")])
            .arg(root.path()).arg(kinds).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let info: Value = serde_json::from_str(&line).expect("fixture startup JSON");
        let entries = info["steps"].as_array().unwrap();
        let steps = entries.iter().enumerate().map(|(index, entry)| {
            let step_id = if index + 1 == entries.len() { "target".into() } else { format!("step-{index}") };
            let host = entry["host"].as_str().unwrap().into();
            let port = entry["port"].as_u64().unwrap() as u16;
            if entry["kind"] == "ssh" {
                ResolvedStep::Ssh(SshStep { step_id, credential_revision: 0, server: Server {
                    id: id(), name: format!("SSH {index}"), host, port, username: "fixture".into(),
                    auth_type: "password".into(), revision: 1, has_secret: false,
                } })
            } else {
                ResolvedStep::Socks5(ProxyStep { step_id, credential_revision: 0, resolve: ProxyResolve::Proxy, proxy: SocksProxy {
                    id: id(), name: format!("SOCKS {index}"), host, port, username: "proxy".into(),
                    auth_type: "usernamePassword".into(), revision: 1, has_preset: false,
                } })
            }
        }).collect();
        Self { child, root, steps }
    }
    fn events(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("events.jsonl")).unwrap_or_default()
            .lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }
    fn closed(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let events = self.events();
            if events.iter().filter(|e| e["event"] == "open").count() == events.iter().filter(|e| e["event"] == "close").count() { break; }
            assert!(Instant::now() < deadline, "fixture leaked a connection: {events:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { drop(self.child.stdin.take()); let _ = self.child.wait(); }
}

#[derive(Default)]
struct ProofHooks { ssh: Vec<usize>, proxies: Vec<usize>, stages: Vec<(usize, String)> }
impl Hooks for ProofHooks {
    fn verify(&mut self, index: usize, fingerprint: &str, _algorithm: &str, verified: &[String]) -> Result<Credential> {
        assert_eq!(verified.len(), self.ssh.len()); assert!(fingerprint.starts_with("SHA256:"));
        self.ssh.push(index);
        let mut credential = Credential::default(); credential.password = "mixed-route-fixture".into(); Ok(credential)
    }
    fn proxy_auth(&mut self, index: usize, verified: &[String]) -> Result<Credential> {
        assert_eq!(verified.len(), self.ssh.len()); self.proxies.push(index);
        let mut credential = Credential::default(); credential.password = "mixed-proxy-fixture".into(); Ok(credential)
    }
    fn progress(&mut self, index: usize, stage: &str) { self.stages.push((index, stage.into())); }
}

#[test]
fn transport_reservations_are_atomic_at_every_limit_and_fully_returned() {
    use crate::route::budget::Cost;
    let budget=Arc::new(Mutex::new(TransportBudget::default()));
    let cost=Cost{sessions:256,bridges:255,socket_endpoints:511,bridge_bytes:255*512*1024,proxy_contexts:512,snapshot_bytes:16*1024*1024};
    let reservation=TransportReservation::reserve(budget.clone(),&cost).unwrap();
    let one=Cost{sessions:1,bridges:0,socket_endpoints:1,bridge_bytes:0,proxy_contexts:0,snapshot_bytes:1};
    assert!(TransportReservation::reserve(budget.clone(),&one).is_err());assert_eq!(budget.lock().unwrap().sessions,256);
    drop(reservation);let used=budget.lock().unwrap();assert_eq!((used.sessions,used.bridges,used.proxy_contexts,used.snapshot_bytes),(0,0,0,0));drop(used);
    for oversized in [Cost{proxy_contexts:513,..one.clone()},Cost{snapshot_bytes:16*1024*1024+1,..one.clone()},Cost{bridges:257,..one.clone()}]{assert!(TransportReservation::reserve(budget.clone(),&oversized).is_err());assert_eq!(budget.lock().unwrap().sessions,0);}
    assert_eq!(one.attempt_timeout_ms(),120000);assert_eq!(cost.attempt_timeout_ms(),900000);
}

#[test]
fn concurrent_reservations_reach_both_limits_without_overcommitting() {
    let budget=Arc::new(Mutex::new(TransportBudget::default()));
    let ready=Arc::new(std::sync::Barrier::new(17));let release=ready.clone();
    let workers=(0..16).map(|_|{let budget=budget.clone();let ready=ready.clone();thread::spawn(move||{
        let cost=crate::route::budget::Cost{sessions:16,bridges:15,socket_endpoints:31,bridge_bytes:15*512*1024,proxy_contexts:32,snapshot_bytes:4096};
        let reservation=TransportReservation::reserve(budget,&cost).unwrap();ready.wait();ready.wait();drop(reservation);
    })}).collect::<Vec<_>>();
    ready.wait();
    {let used=budget.lock().unwrap();assert_eq!((used.sessions,used.proxy_contexts),(256,512));}
    let extra=crate::route::budget::Cost{sessions:0,bridges:0,socket_endpoints:0,bridge_bytes:0,proxy_contexts:1,snapshot_bytes:0};
    assert!(TransportReservation::reserve(budget.clone(),&extra).is_err());release.wait();
    for worker in workers{worker.join().unwrap();}
    let used=budget.lock().unwrap();assert_eq!((used.sessions,used.bridges,used.proxy_contexts,used.snapshot_bytes),(0,0,0,0));
}

#[test]
#[ignore = "Requires isolated Python/Paramiko fixture and localhost IPv4+IPv6 resolution"]
fn failed_local_candidate_rebuilds_every_upstream_transport() {
    let mut fixture=Fixture::start("ssh,ssh,socks,ssh");
    std::fs::write(fixture.root.path().join("local-candidates-2"),b"").unwrap();
    if let ResolvedStep::Socks5(step)=&mut fixture.steps[2]{step.resolve=ProxyResolve::Local;}
    if let ResolvedStep::Ssh(step)=&mut fixture.steps[3]{step.server.host="localhost".into();}
    struct RetryHooks(Vec<usize>);
    impl Hooks for RetryHooks{
        fn verify(&mut self,index:usize,_:&str,_:&str,verified:&[String])->Result<Credential>{
            assert_eq!(verified.len(),if index==3{2}else{index});self.0.push(index);
            let mut credential=Credential::default();credential.password="mixed-route-fixture".into();Ok(credential)
        }
        fn proxy_auth(&mut self,index:usize,verified:&[String])->Result<Credential>{
            assert_eq!(verified.len(),2);self.0.push(index);
            let mut credential=Credential::default();credential.password="mixed-proxy-fixture".into();Ok(credential)
        }
        fn progress(&mut self,_:usize,_:&str){}
    }
    let cancel=AtomicBool::new(false);let mut hooks=RetryHooks(vec![]);
    let connected=Execution{steps:&fixture.steps,canceled:&cancel,operation:&cancel,deadline:Instant::now()+Duration::from_secs(20),failed:Arc::new(AtomicBool::new(false)),failure_step:Arc::new(AtomicUsize::new(usize::MAX))}.connect(&mut hooks).unwrap();
    assert_eq!(hooks.0,vec![0,1,2,0,1,2,3]);
    assert_eq!(connected.bridges.0.len(),2);drop(connected);fixture.closed();
    let events=fixture.events();
    assert_eq!(events.iter().filter(|e|e["event"]=="candidate-rejected").count(),1);
    for index in 0..3{assert_eq!(events.iter().filter(|e|e["event"]=="open"&&e["index"]==index).count(),2);}
    assert_eq!(events.iter().filter(|e|e["event"]=="open"&&e["index"]==3).count(),1);
}

#[test]
#[ignore = "Requires isolated Python/Paramiko protocol fixture"]
fn mixed_protocol_orders_and_release() {
    for kinds in ["ssh,ssh,socks,ssh", "socks,socks,ssh", "socks,ssh,socks,ssh", "ssh,socks,socks,ssh", "ssh,socks,ssh,socks,ssh"] {
        let fixture = Fixture::start(kinds);
        let cancel = AtomicBool::new(false);
        let mut hooks = ProofHooks::default();
        let established = Execution { steps: &fixture.steps, canceled: &cancel, operation: &cancel,
            deadline: Instant::now() + Duration::from_secs(20), failed: Arc::new(AtomicBool::new(false)),
            failure_step: Arc::new(AtomicUsize::new(usize::MAX)) }.connect(&mut hooks).unwrap();
        assert_eq!(established.bridges.0.len() + 1, hooks.ssh.len());
        assert_eq!(hooks.ssh.len() + hooks.proxies.len(), fixture.steps.len());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut channel = transport::protocol(&cancel, &cancel, deadline, || established.session.channel_session()).unwrap();
        transport::protocol(&cancel, &cancel, deadline, || channel.exec("fixture-proof")).unwrap();
        let mut output = Vec::new();
        loop {
            let mut bytes = [0; 1024];
            match channel.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(4));
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(output, b"MIXED-ROUTE-TARGET\n");
        let events = fixture.events();
        assert!(events.iter().filter(|e| e["event"] == "session").all(|e| e["index"] == fixture.steps.len() - 1));
        for index in 0..fixture.steps.len() - 1 {
            assert!(events.iter().any(|e| e["index"] == index && (e["event"] == "forward" || e["event"] == "connect")), "missing route edge {index}");
        }
        drop(channel);
        established.socket.shutdown(Shutdown::Both).unwrap();
        drop(established);
        fixture.closed();
    }
}

#[test]
#[ignore = "Requires isolated Python/Paramiko protocol fixture"]
fn cancel_during_remote_socks_releases_all_bridges() {
    let fixture = Fixture::start("ssh,ssh,socks,ssh");
    for cycle in 0..50 {
    let before=fixture.events().iter().filter(|e|e["event"]=="open"&&e["index"]==2).count();
    std::fs::write(fixture.root.path().join("pause-2"), b"").unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let marker = fixture.root.path().join("events.jsonl");
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read_to_string(&marker).unwrap_or_default().lines().filter(|line| {
            let event: Value = serde_json::from_str(line).unwrap(); event["event"] == "open" && event["index"] == 2
        }).count()<=before { assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(10)); }
        let start = Instant::now(); signal.store(true, Ordering::SeqCst); start
    });
    let error = Execution { steps: &fixture.steps, canceled: &cancel, operation: &AtomicBool::new(false),
        deadline: Instant::now() + Duration::from_secs(20), failed: Arc::new(AtomicBool::new(false)),
        failure_step: Arc::new(AtomicUsize::new(usize::MAX)) }.connect(&mut ProofHooks::default()).err().unwrap();
    assert_eq!(error.code, "CANCELED");
    assert_eq!(error.details.unwrap()["stepIndex"], 2);
    assert!(worker.join().unwrap().elapsed() < Duration::from_secs(1));
    std::fs::remove_file(fixture.root.path().join("pause-2")).unwrap();
    fixture.closed();
    println!("cancel cycle {cycle}: released");
    }
}

struct OpenSshHooks { credentials: Vec<Credential>, visited: Vec<usize> }
impl Hooks for OpenSshHooks {
    fn verify(&mut self, index: usize, fingerprint: &str, _: &str, _: &[String]) -> Result<Credential> {
        assert!(fingerprint.starts_with("SHA256:")); self.visited.push(index); Ok(self.credentials[index].clone())
    }
    fn proxy_auth(&mut self, index: usize, _: &[String]) -> Result<Credential> {
        self.visited.push(index); Ok(Credential::default())
    }
    fn progress(&mut self, _: usize, _: &str) {}
}

#[test]
#[ignore = "Requires dedicated T-027 OpenSSH namespaces; SODECK_MIXED_OPENSSH_CONFIG"]
fn real_openssh_with_remote_loopback_and_consecutive_proxies() {
    let config: Value = serde_json::from_slice(&std::fs::read(std::env::var("SODECK_MIXED_OPENSSH_CONFIG").unwrap()).unwrap()).unwrap();
    for (case, entries) in config["paths"].as_array().unwrap().iter().enumerate() {
        let entries = entries.as_array().unwrap();
        let mut hooks = OpenSshHooks { credentials: Vec::new(), visited: Vec::new() };
        let steps: Vec<_> = entries.iter().enumerate().map(|(index, value)| {
            let step_id = if index + 1 == entries.len() { "target".into() } else { format!("step-{index}") };
            let host = value["host"].as_str().unwrap().to_owned();
            let port = value["port"].as_u64().unwrap() as u16;
            let mut credential = Credential::default();
            if value["kind"] == "ssh" {
                credential.private_key = std::fs::read_to_string(value["key"].as_str().unwrap()).unwrap();
                hooks.credentials.push(credential);
                ResolvedStep::Ssh(SshStep { step_id, credential_revision: 0, server: Server {
                    id: id(), name: value["name"].as_str().unwrap().into(), host, port,
                    username: value["username"].as_str().unwrap().into(), auth_type: "key".into(), revision: 1, has_secret: false,
                } })
            } else {
                hooks.credentials.push(credential);
                ResolvedStep::Socks5(ProxyStep { step_id, credential_revision: 0, resolve: ProxyResolve::Proxy, proxy: SocksProxy {
                    id: id(), name: "Remote loopback proxy".into(), host, port, username: String::new(),
                    auth_type: "none".into(), revision: 1, has_preset: false,
                } })
            }
        }).collect();
        let cancel = AtomicBool::new(false);
        let established = Execution { steps: &steps, canceled: &cancel, operation: &cancel,
            deadline: Instant::now() + Duration::from_secs(60), failed: Arc::new(AtomicBool::new(false)),
            failure_step: Arc::new(AtomicUsize::new(usize::MAX)) }.connect(&mut hooks).unwrap();
        assert_eq!(hooks.visited, (0..steps.len()).collect::<Vec<_>>());
        assert_eq!(established.bridges.0.len(), steps.iter().filter(|s| matches!(s, ResolvedStep::Ssh(_))).count() - 1);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut channel = transport::protocol(&cancel, &cancel, deadline, || established.session.channel_session()).unwrap();
        transport::protocol(&cancel, &cancel, deadline, || channel.exec("printf 'T027:%s' \"$(id -un)\"")).unwrap();
        let mut output = Vec::new();
        loop {
            let mut bytes = [0; 512];
            match channel.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline); thread::sleep(Duration::from_millis(4));
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(output, b"T027:mr_t");
        let metrics: Vec<_> = established.bridges.0.iter().map(|b| b.metrics.clone()).collect();
        drop(channel);
        let start = Instant::now();
        established.socket.shutdown(Shutdown::Both).unwrap(); drop(established);
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(metrics.iter().all(|m| m.lock().unwrap()["state"] != "active"));
        println!("OpenSSH case {case}: {} steps, {} bridges, final user mr_t, cleanup {} ms", steps.len(), metrics.len(), start.elapsed().as_millis());
    }
}
