use serde_json::{json, Value};
use sodeck_core::{Caller, Core, WorkspaceScope};
use sodeck_communication::binary::{Frame,Kind};
use std::{io::{BufRead,BufReader,Read,Write},net::{TcpStream,Shutdown},process::{Child,Command,Stdio},time::{Duration,Instant}};

fn scope()->Value {json!({"workspaceId":"network-test","workspaceGeneration":"generation-one"})}
fn setup(core:&Core) {
    core.register_workspace("network-test","generation-one");
    core.register_network_view("shell","network-view",serde_json::from_value::<WorkspaceScope>(scope()).unwrap()).unwrap();
}
fn start(core:&Core,method:&str,mut p:Value)->Value {
    if p.get("scope").is_none(){p["scope"]=scope();}
    if p.get("operationId").is_none(){p["operationId"]=json!(uuid::Uuid::new_v4().to_string());}
    core.call(&Caller::shell(),method,p).unwrap()
}
fn finish(core:&Core,mut receipt:Value)->Value {
    let until=Instant::now()+Duration::from_secs(25);
    while receipt["state"]=="pending" {
        assert!(Instant::now()<until,"operation did not finish: {receipt}");
        std::thread::sleep(Duration::from_millis(5));
        receipt=core.call(&Caller::shell(),"net.operations.get",json!({"scope":scope(),"operationId":receipt["operationId"]})).unwrap();
    }
    receipt
}
fn run(core:&Core,method:&str,p:Value)->Value {
    let receipt=finish(core,start(core,method,p));
    assert_eq!(receipt["state"],"succeeded","{receipt}");
    core.call(&Caller::shell(),"net.operations.claim",json!({"scope":scope(),"operationId":receipt["operationId"]})).unwrap();
    receipt["result"].clone()
}
fn group(core:&Core)->Value {run(core,"net.groups.create",json!({"viewId":"network-view"}))["group"].clone()}
fn listener(core:&Core,g:&Value)->Value {
    let l=run(core,"net.listeners.open",json!({"group":g,"host":"127.0.0.1","port":0}));
    run(core,"net.listeners.setAccepting",json!({"group":g,"listener":l["listener"],"enabled":true}));l
}
fn accept(core:&Core,g:&Value,l:&Value)->Value {
    let until=Instant::now()+Duration::from_secs(3);
    loop {
        let r=run(core,"net.listeners.accept",json!({"group":g,"listener":l["listener"],"maxItems":16}));
        if let Some(s)=r["items"].as_array().unwrap().first(){return s.clone()}
        assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn global_stop_receipt_and_repeated_cleanup_preserve_identity() {
    let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);
    let g=group(&core);let l=listener(&core,&g);
    run(&core,"net.listeners.close",json!({"group":g,"listener":l["listener"]}));
    run(&core,"net.listeners.close",json!({"group":g,"listener":l["listener"]}));
    let mut stale=l["listener"].clone();stale["generation"]=json!(999999);
    let r=finish(&core,start(&core,"net.listeners.close",json!({"group":g,"listener":stale})));assert_eq!(r["error"]["code"],"STALE_HANDLE");
    let receipt=core.call(&Caller::shell(),"net.resources.stop",json!({"resource":g,"operationId":"global-stop"})).unwrap();
    assert_eq!(receipt["state"],"succeeded");assert_eq!(receipt["result"]["affectedGroups"],json!([g]));
    assert_eq!(core.call(&Caller::shell(),"net.operations.claim",json!({"operationId":"global-stop"})).unwrap()["claimed"],true);
    assert_eq!(core.call(&Caller::shell(),"net.operations.get",json!({"scope":scope(),"operationId":"global-stop"})).unwrap_err().code,"STALE_HANDLE");
    run(&core,"net.listeners.close",json!({"group":g,"listener":l["listener"]}));
    core.shutdown();
    assert!(core.call(&Caller::shell(),"net.groups.create",json!({"scope":scope(),"viewId":"network-view","operationId":"after-shutdown"})).is_err());
}

#[test]
fn stream_audit_excludes_payload_and_cleanup_survives_log_failure() {
    let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);let g=group(&core);let l=listener(&core,&g);
    let mut socket=TcpStream::connect(("127.0.0.1",l["address"]["port"].as_u64().unwrap() as u16)).unwrap();let stream=accept(&core,&g,&l)["stream"].clone();
    run(&core,"net.streams.exchange",json!({"group":g,"stream":stream}));socket.write_all(b"PRIVATE-NETWORK-BODY").unwrap();std::thread::sleep(Duration::from_millis(30));
    let db=rusqlite::Connection::open(temp.path().join("observability/facts.sqlite")).unwrap();db.execute_batch("CREATE TRIGGER log_failure BEFORE INSERT ON calls BEGIN SELECT RAISE(FAIL,'test log unavailable'); END;").unwrap();
    assert_eq!(finish(&core,start(&core,"net.groups.close",json!({"group":g})))["state"],"succeeded");assert!(TcpStream::connect(("127.0.0.1",l["address"]["port"].as_u64().unwrap() as u16)).is_err());
    db.execute_batch("DROP TRIGGER log_failure;").unwrap();let until=Instant::now()+Duration::from_secs(2);
    loop{let ended:Option<i64>=db.query_row("SELECT ended FROM calls WHERE method='net.streams.exchange.lifecycle'",[],|r|r.get(0)).unwrap();if ended.is_some(){break;}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(5));}
    let summary:String=db.query_row("SELECT summary FROM calls WHERE method='net.streams.exchange.lifecycle'",[],|r|r.get(0)).unwrap();assert!(!summary.contains("PRIVATE"));assert_eq!(serde_json::from_str::<Value>(&summary).unwrap()["bytesRead"],b"PRIVATE-NETWORK-BODY".len());
    let calls=core.call(&Caller::shell(),"calls.list",json!({})).unwrap().to_string();assert!(!calls.contains("PRIVATE-NETWORK-BODY"));
}

#[test]
fn actual_listener_receipts_scope_and_view_cleanup() {
    let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);
    let params=json!({"scope":scope(),"viewId":"network-view","operationId":"same"});
    let first=core.call(&Caller::shell(),"net.groups.create",params.clone()).unwrap();
    assert_eq!(first,core.call(&Caller::shell(),"net.groups.create",params.clone()).unwrap());
    let mut different=params;different["viewId"]=json!("other");
    assert_eq!(core.call(&Caller::shell(),"net.groups.create",different).unwrap_err().code,"OPERATION_CONFLICT");
    let g=first["result"]["group"].clone();let l=listener(&core,&g);
    let p=run(&core,"net.listeners.connectSelf",json!({"group":g,"listener":l["listener"]}));
    let accepted=accept(&core,&g,&l);assert_eq!(accepted["probeId"],p["probeId"]);
    let other=Caller{instance_id:"unrelated".into(),..Caller::shell()};
    assert_eq!(core.call(&other,"net.groups.get",json!({"scope":scope(),"group":g})).unwrap_err().code,"STALE_HANDLE");
    core.close_network_view("shell","network-view");
    assert!(TcpStream::connect(("127.0.0.1",l["address"]["port"].as_u64().unwrap() as u16)).is_err());
    let r=core.call(&Caller::shell(),"net.groups.get",json!({"scope":scope(),"group":g})).unwrap();assert_eq!(r["state"],"closed");
    core.revoke_workspace("network-test","generation-one");
    assert_eq!(core.call(&Caller::shell(),"net.groups.get",json!({"scope":scope(),"group":g})).unwrap_err().code,"STALE_SCOPE");
}

#[test]
fn binary_window_retains_unconsumed_tail_across_real_socket_splice() {
    let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);let g=group(&core);let l=listener(&core,&g);
    let address=("127.0.0.1",l["address"]["port"].as_u64().unwrap() as u16);
    let mut a=TcpStream::connect(address).unwrap();let left=accept(&core,&g,&l)["stream"].clone();
    let mut b=TcpStream::connect(address).unwrap();let right=accept(&core,&g,&l)["stream"].clone();
    a.set_read_timeout(Some(Duration::from_secs(3))).unwrap();b.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let attach=run(&core,"net.streams.exchange",json!({"group":g,"stream":left}))["attachment"].clone();
    a.write_all(b"HELLOtail").unwrap();let mut frames=Vec::new();let until=Instant::now()+Duration::from_secs(3);
    while !frames.iter().any(|f:&Frame|f.kind==Kind::Data){core.network_output(&Caller::shell(),|f|{frames.push(f);true});assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(2));}
    let slot=attach["slot"].as_u64().unwrap() as u32;let epoch=attach["epoch"].as_u64().unwrap() as u32;
    core.network_input(&Caller::shell(),Frame::control(Kind::Ack,slot,epoch,5)).unwrap();
    run(&core,"net.streams.splice",json!({"group":g,"left":{"stream":left,"epoch":epoch,"consumed":5,"written":0},"right":{"stream":right,"epoch":0,"consumed":0,"written":0}}));
    let mut bytes=[0;4];b.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"tail");
    a.shutdown(Shutdown::Write).unwrap();let mut end=[0;1];assert_eq!(b.read(&mut end).unwrap(),0);
    b.write_all(b"back").unwrap();a.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"back");
    core.close_network_view("shell","network-view");
}

struct Fixture {child:Child,root:tempfile::TempDir,port:u16}
impl Fixture {
    fn start()->Self {
        let root=tempfile::tempdir().unwrap();
        let mut command=Command::new(std::env::var("SODECK_TEST_PYTHON").expect("Python/Paramiko fixture required"));
        command.args(["-u",concat!(env!("CARGO_MANIFEST_DIR"),"/../../tests/core/network_fixture.py")]).arg(root.path()).arg("3").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().unwrap();let mut line=String::new();BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let info:Value=serde_json::from_str(&line).unwrap();Self{child,root,port:info["port"].as_u64().unwrap() as u16}
    }
    fn route(&self,core:&Core,g:&Value)->Value {
        let mut previous:Option<Value>=None;
        for index in 0..3 {
            let node=core.call(&Caller::shell(),"servers.routes.create",json!({"name":format!("node-{index}"),"host":if index==0{"127.0.0.1".into()}else{format!("node-{index}.internal")},"port":if index==0{self.port}else{22},"username":"network-test","accessPath":previous.as_ref().map_or(json!({"kind":"direct"}),|p|{let mut ids=p["accessPath"]["jumpServerIds"].as_array().cloned().unwrap_or_default();ids.push(p["id"].clone());json!({"kind":"hostTunnel","jumpServerIds":ids})})})).unwrap();
            core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":node["id"],"password":"isolated-network-password"})).unwrap();previous=Some(node);
        }
        let target=previous.unwrap();
        let status=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":target["id"],"workspaceId":"network-test"})).unwrap();
        let revision=status["saved"]["routeRevision"].clone();
        assert!(revision.is_string(),"{status}");
        for _ in 0..4 {
            let receipt=finish(core,start(core,"net.routes.acquire",json!({"group":g,"serverId":target["id"],"expectedRouteRevision":revision})));
            if receipt["state"]=="succeeded" {
                core.call(&Caller::shell(),"net.operations.claim",json!({"scope":scope(),"operationId":receipt["operationId"]})).unwrap();
                return receipt["result"]["route"].clone();
            }
            assert_eq!(receipt["error"]["code"],"HOST_KEY_REQUIRED","{receipt}");
            core.call(&Caller::shell(),"hosts.trust",receipt["error"]["details"].clone()).unwrap();
        }
        panic!("route did not connect")
    }
}
impl Drop for Fixture {fn drop(&mut self){drop(self.child.stdin.take());let _=self.child.wait();}}

#[test]
#[ignore="Requires isolated Python/Paramiko fault fixture"]
fn target_timeout_and_four_draining_members_keep_existing_channels() {
    let f=Fixture::start();let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);let g=group(&core);let route=f.route(&core,&g);
    let mut held=Vec::new();
    for index in 0..4 {
        held.push(run(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"echo.internal","port":9999}))["stream"].clone());
        let before=Instant::now();let stuck=start(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"stuck.internal","port":9999}));
        let until=Instant::now()+Duration::from_secs(5);
        loop {let events=std::fs::read_to_string(f.root.path().join("events.jsonl")).unwrap_or_default();if events.matches("withheld-open").count()>index {break;}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
        if index==0 {let done=finish(&core,stuck);assert_eq!(done["error"]["code"],"TARGET_CONNECT_TIMEOUT");assert!(before.elapsed()>=Duration::from_secs(15));}
        else {core.call(&Caller::shell(),"net.operations.cancel",json!({"scope":scope(),"operationId":stuck["operationId"]})).unwrap();}
        let until=Instant::now()+Duration::from_secs(3);
        loop {let r=core.call(&Caller::shell(),"net.resources.list",json!({})).unwrap();if r["items"].as_array().unwrap().iter().filter(|x|x["state"]=="draining").count()>index {break;}assert!(Instant::now()<until,"{r}");std::thread::sleep(Duration::from_millis(10));}
    }
    let rejected=finish(&core,start(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"echo.internal","port":9999})));assert_eq!(rejected["error"]["code"],"CAPACITY");
    let list=core.call(&Caller::shell(),"net.resources.list",json!({})).unwrap();assert_eq!(list["items"].as_array().unwrap().iter().filter(|x|x["kind"]=="transport").count(),4);
    for stream in held {let a=run(&core,"net.streams.exchange",json!({"group":g,"stream":stream}))["attachment"].clone();let slot=a["slot"].as_u64().unwrap() as u32;let epoch=a["epoch"].as_u64().unwrap() as u32;core.network_input(&Caller::shell(),Frame{kind:Kind::Data,slot,epoch,offset:0,payload:b"alive".to_vec()}).unwrap();let mut data=Vec::new();let until=Instant::now()+Duration::from_secs(4);while data.len()<5 {core.network_output(&Caller::shell(),|f|{if f.kind==Kind::Data&&f.slot==slot{data.extend(f.payload);}true});assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(5));}assert_eq!(data,b"alive");}
    run(&core,"net.groups.close",json!({"group":g}));let until=Instant::now()+Duration::from_secs(5);loop{let r=core.call(&Caller::shell(),"connections.route.list",json!({})).unwrap();if r["usage"]==json!({"sessions":0,"bridges":0}){break;}assert!(Instant::now()<until,"{r}");std::thread::sleep(Duration::from_millis(10));}
}

#[test]
#[ignore="Requires isolated Python/Paramiko; protocol test, not real OpenSSH platform proof"]
fn three_node_target_channel_splice_and_stuck_open_isolation() {
    let f=Fixture::start();let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();setup(&core);let g=group(&core);let route=f.route(&core,&g);let l=listener(&core,&g);
    let mut client=TcpStream::connect(("127.0.0.1",l["address"]["port"].as_u64().unwrap() as u16)).unwrap();client.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    let local=accept(&core,&g,&l)["stream"].clone();let remote=run(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"echo.internal","port":9999}))["stream"].clone();
    run(&core,"net.streams.splice",json!({"group":g,"left":{"stream":local,"epoch":0,"consumed":0,"written":0},"right":{"stream":remote,"epoch":0,"consumed":0,"written":0}}));
    client.write_all(b"before").unwrap();let mut bytes=[0;6];client.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"before");
    let refused=finish(&core,start(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"refuse.internal","port":9})));assert_eq!(refused["error"]["code"],"TARGET_CONNECT_FAILED");
    let stuck=start(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"stuck.internal","port":9}));
    std::thread::sleep(Duration::from_millis(100));
    let second=run(&core,"net.tcp.open",json!({"group":g,"route":route,"host":"other.internal","port":9999}));assert!(second["stream"].is_object());
    core.call(&Caller::shell(),"net.operations.cancel",json!({"scope":scope(),"operationId":stuck["operationId"]})).unwrap();
    client.write_all(b"during").unwrap();client.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"during");
    let until=Instant::now()+Duration::from_secs(2);
    loop {let list=core.call(&Caller::shell(),"net.resources.list",json!({})).unwrap();if list["items"].as_array().unwrap().iter().any(|v|v["state"]=="draining"){break;}assert!(Instant::now()<until,"{list}");std::thread::sleep(Duration::from_millis(10));}
    client.shutdown(Shutdown::Write).unwrap();let mut tail=Vec::new();client.read_to_end(&mut tail).unwrap();assert_eq!(tail,b"AFTER-EOF");
    run(&core,"net.groups.close",json!({"group":g}));
    let until=Instant::now()+Duration::from_secs(5);
    loop {let usage=core.call(&Caller::shell(),"connections.route.list",json!({})).unwrap();if usage["usage"]==json!({"sessions":0,"bridges":0}){break;}assert!(Instant::now()<until,"{usage}");std::thread::sleep(Duration::from_millis(20));}
    let events=std::fs::read_to_string(f.root.path().join("events.jsonl")).unwrap();assert!(events.contains("withheld-open"));assert!(events.contains("echo.internal"));
}
