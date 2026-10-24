use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sodeck_core::{Caller, Core};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    child: Child,
    directory: tempfile::TempDir,
    port: u16,
}
impl Fixture {
    fn start() -> Self {
        let python = std::env::var("SODECK_TEST_PYTHON")
            .expect("set SODECK_TEST_PYTHON to Python with tests/core/requirements.txt installed");
        let directory = tempfile::tempdir().unwrap();
        let mut child = Command::new(python)
            .arg("-u")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/core/ssh_fixture.py"
            ))
            .arg(directory.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let data: Value = serde_json::from_str(&line).unwrap();
        Self {
            child,
            directory,
            port: data["port"].as_u64().unwrap() as u16,
        }
    }
    fn events(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("events.jsonl")).unwrap_or_default()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn output(core: &Core, caller: &Caller, pty: &Value, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut cursor = 0;
    let mut all = Vec::new();
    while Instant::now() < deadline {
        let batch = core
            .call(
                caller,
                "pty.read",
                json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":cursor}),
            )
            .unwrap();
        for frame in batch["frames"].as_array().unwrap() {
            all.extend(STANDARD.decode(frame["bytes"].as_str().unwrap()).unwrap());
            cursor = frame["sequence"].as_u64().unwrap();
        }
        let text = String::from_utf8_lossy(&all);
        if text.contains(needle) {
            return text.into_owned();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("missing expected protocol output");
}
#[test]
#[ignore = "Requires explicit loopback Python/Paramiko fixture; not a Linux/OpenSSH shell test"]
fn real_ssh_host_trust_shared_leases_and_channels() {
    let fixture = Fixture::start();
    let directory = tempfile::tempdir().unwrap();
    let core = Core::open(directory.path()).unwrap();
    let caller = Caller::shell();
    let server=core.call(&caller,"servers.create",json!({"name":"isolated","host":"127.0.0.1","port":fixture.port,"username":"test-user"})).unwrap();
    core.call(
        &caller,
        "auth.set",
        json!({"serverId":server["id"],"password":"isolated-test-password","remember":true}),
    )
    .unwrap();
    let acquire =
        json!({"serverId":server["id"],"workspaceId":"test-workspace","configRevision":1});
    let challenge = core
        .call(&caller, "connections.acquire", acquire.clone())
        .unwrap_err();
    assert_eq!(challenge.code, "HOST_KEY_REQUIRED", "{challenge:?}");
    assert!(!fixture.events().contains("password-auth"));
    core.call(&caller, "hosts.trust", challenge.details.unwrap())
        .unwrap();
    core.call(
        &caller,
        "auth.set",
        json!({"serverId":server["id"],"password":"wrong-test-password"}),
    )
    .unwrap();
    assert_eq!(
        core.call(&caller, "connections.acquire", acquire.clone())
            .unwrap_err()
            .code,
        "AUTH_FAILED"
    );
    core.call(
        &caller,
        "auth.set",
        json!({"serverId":server["id"],"password":"isolated-test-password","remember":true}),
    )
    .unwrap();
    let a = core
        .call(&caller, "connections.acquire", acquire.clone())
        .unwrap();
    let b = core
        .call(&caller, "connections.acquire", acquire.clone())
        .unwrap();
    assert_eq!(a["connectionId"], b["connectionId"]);
    let pa = core
        .call(
            &caller,
            "pty.open",
            json!({"leaseId":a["leaseId"],"cols":80,"rows":24}),
        )
        .unwrap();
    let pb = core
        .call(
            &caller,
            "pty.open",
            json!({"leaseId":b["leaseId"],"cols":90,"rows":30}),
        )
        .unwrap();
    // Interactive echo must arrive without Enter; the load fixture's control
    // line parsing may not buffer ordinary keystrokes until a newline.
    core.call(&caller,"pty.write",json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"inputSeq":1,"bytes":STANDARD.encode(b"alpha-only")})).unwrap();
    core.call(&caller,"pty.write",json!({"ptyId":pb["ptyId"],"generation":pb["generation"],"inputSeq":1,"bytes":STANDARD.encode(b"beta-only\r")})).unwrap();
    assert!(!output(&core, &caller, &pa, "alpha-only").contains("beta-only"));
    assert!(!output(&core, &caller, &pb, "beta-only").contains("alpha-only"));
    assert_eq!(core.call(&caller,"pty.write",json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"inputSeq":1,"bytes":STANDARD.encode(b"duplicate")})).unwrap_err().code,"INPUT_ALREADY_ACCEPTED");
    core.call(&caller,"pty.bind",json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"viewId":"terminal-a","mountGeneration":1})).unwrap();
    let surface_a = Caller {
        view_id: Some("terminal-a".into()),
        mount_generation: Some(1),
        ..caller.clone()
    };
    core.call(&caller,"pty.bind",json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"viewId":"terminal-a","mountGeneration":2})).unwrap();
    assert_eq!(core.call(&surface_a,"pty.write",json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"inputSeq":2,"bytes":STANDARD.encode(b"stale-surface")})).unwrap_err().code,"STALE_MOUNT");
    assert_eq!(
        core.call(
            &surface_a,
            "pty.read",
            json!({"ptyId":pa["ptyId"],"generation":pa["generation"],"after":0})
        )
        .unwrap_err()
        .code,
        "STALE_MOUNT"
    );
    core.call(
        &caller,
        "pty.close",
        json!({"ptyId":pa["ptyId"],"generation":pa["generation"]}),
    )
    .unwrap();
    core.call(
        &caller,
        "connections.release",
        json!({"leaseId":a["leaseId"]}),
    )
    .unwrap();
    core.call(
        &caller,
        "connections.release",
        json!({"leaseId":b["leaseId"]}),
    )
    .unwrap();
    assert_eq!(core.resources()["connections"].as_array().unwrap().len(), 1);
    core.call(&caller,"pty.write",json!({"ptyId":pb["ptyId"],"generation":pb["generation"],"inputSeq":2,"bytes":STANDARD.encode(b"still-alive\r")})).unwrap();
    output(&core, &caller, &pb, "still-alive");
    core.call(
        &caller,
        "pty.close",
        json!({"ptyId":pb["ptyId"],"generation":pb["generation"]}),
    )
    .unwrap();
    assert!(core.resources()["connections"]
        .as_array()
        .unwrap()
        .is_empty());
    let raw = std::fs::read(directory.path().join("core.sqlite")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("isolated-test-password"));
    drop(core);
    let core = Core::open(directory.path()).unwrap();
    core.call(&caller, "connections.acquire", acquire).unwrap();
    core.shutdown();

    // The same real SSH server accepts the test public key; both encrypted and
    // unencrypted key files exercise the production core-only file boundary.
    for (key_name, passphrase) in [
        ("client-key", ""),
        ("client-key-encrypted", "test-passphrase"),
        ("client-openssh", ""),
        ("client-openssh-encrypted", "test-passphrase"),
        ("client-ed25519", ""),
        ("client-ed25519-encrypted", "test-passphrase"),
    ] {
        let key_server = core.call(&caller, "servers.create", json!({"name":key_name,"host":"127.0.0.1","port":fixture.port,"username":"test-user","authType":"key"})).unwrap();
        core.call(&caller, "auth.set", json!({"serverId":key_server["id"],"privateKeyPath":fixture.directory.path().join(key_name),"passphrase":passphrase,"remember":true})).unwrap();
        core.call(
            &caller,
            "connections.acquire",
            json!({"serverId":key_server["id"],"workspaceId":"key-workspace"}),
        )
        .unwrap_or_else(|error| panic!("key {key_name}: {error:?}"));
        core.shutdown();
    }
    let authentication_count = fixture
        .events()
        .lines()
        .filter(|line| line.contains("password-auth") || line.contains("publickey-auth"))
        .count();
    std::fs::write(fixture.directory.path().join("rotate-host-key"), b"rotate").unwrap();
    let changed = core
        .call(
            &caller,
            "connections.acquire",
            json!({"serverId":server["id"],"workspaceId":"changed-key-workspace"}),
        )
        .unwrap_err();
    assert_eq!(changed.code, "HOST_KEY_CHANGED");
    assert!(changed.details.unwrap()["previousFingerprint"].is_string());
    assert_eq!(
        fixture
            .events()
            .lines()
            .filter(|line| line.contains("password-auth") || line.contains("publickey-auth"))
            .count(),
        authentication_count,
        "changed host key must stop before authentication"
    );
}

#[test]
fn shutdown_cancels_an_inflight_real_socket_handshake() {
    use std::{
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicBool::new(false));
    let accepted_clone = accepted.clone();
    let socket_thread = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        accepted_clone.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_secs(1));
        drop(socket);
    });
    let directory = tempfile::tempdir().unwrap();
    let core = Arc::new(Core::open(directory.path()).unwrap());
    let caller = Caller::shell();
    let server = core
        .call(
            &caller,
            "servers.create",
            json!({"name":"hanging","host":"127.0.0.1","port":port,"username":"u"}),
        )
        .unwrap();
    let running_core = core.clone();
    let handle = std::thread::spawn(move || {
        running_core.call(
            &caller,
            "connections.acquire",
            json!({"serverId":server["id"],"workspaceId":"canceled"}),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !accepted.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let canceled = Instant::now();
    core.shutdown();
    let error = handle.join().unwrap().unwrap_err();
    assert_eq!(error.code, "CANCELED");
    assert!(canceled.elapsed() < Duration::from_millis(500));
    socket_thread.join().unwrap();
}

#[test]
fn single_operation_cancellation_interrupts_handshake_and_gate_wait() {
    use std::{net::TcpListener, sync::mpsc};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted, ready) = mpsc::sync_channel(1);
    let (stop, stopped) = mpsc::sync_channel(1);
    let socket_thread = std::thread::spawn(move || {
        let (_socket, _) = listener.accept().unwrap();
        accepted.send(()).unwrap();
        let _ = stopped.recv_timeout(Duration::from_secs(5));
    });
    let directory = tempfile::tempdir().unwrap();
    let core = std::sync::Arc::new(Core::open(directory.path()).unwrap());
    let caller = Caller::shell();
    let server = core
        .call(
            &caller,
            "servers.create",
            json!({
                "name":"cancellation", "host":"127.0.0.1", "port":port, "username":"test"
            }),
        )
        .unwrap();
    let start = |operation: &str| {
        let core = core.clone();
        let caller = caller.clone();
        let params =
            json!({"serverId":server["id"], "workspaceId":"same-gate", "operationId":operation});
        std::thread::spawn(move || core.call(&caller, "connections.acquire", params))
    };
    let first = start("first");
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let queued = start("queued");
    std::thread::sleep(Duration::from_millis(50));
    let at = Instant::now();
    core.call(
        &caller,
        "operations.cancel",
        json!({"operationId":"queued"}),
    )
    .unwrap();
    assert_eq!(queued.join().unwrap().unwrap_err().code, "CANCELED");
    assert!(at.elapsed() < Duration::from_millis(500));
    assert!(
        !first.is_finished(),
        "canceling the queued operation affected the first handshake"
    );
    let at = Instant::now();
    core.call(&caller, "operations.cancel", json!({"operationId":"first"}))
        .unwrap();
    assert_eq!(first.join().unwrap().unwrap_err().code, "CANCELED");
    assert!(at.elapsed() < Duration::from_millis(500));
    assert!(core.resources()["connections"]
        .as_array()
        .unwrap()
        .is_empty());
    stop.send(()).unwrap();
    socket_thread.join().unwrap();
}

#[test]
#[ignore = "Requires explicit loopback Python/Paramiko fixture; not a Linux/OpenSSH shell test"]
fn released_lease_rejects_late_pty_without_stopping_shared_connection() {
    let fixture = Fixture::start();
    let directory = tempfile::tempdir().unwrap();
    let core = std::sync::Arc::new(Core::open(directory.path()).unwrap());
    let caller = Caller::shell();
    let server = core
        .call(
            &caller,
            "servers.create",
            json!({
                "name":"lease-race", "host":"127.0.0.1", "port":fixture.port, "username":"test-user"
            }),
        )
        .unwrap();
    core.call(
        &caller,
        "auth.set",
        json!({
            "serverId":server["id"], "password":"isolated-test-password"
        }),
    )
    .unwrap();
    let params = json!({"serverId":server["id"], "workspaceId":"shared"});
    let challenge = core
        .call(&caller, "connections.acquire", params.clone())
        .unwrap_err();
    assert_eq!(challenge.code, "HOST_KEY_REQUIRED");
    core.call(&caller, "hosts.trust", challenge.details.unwrap())
        .unwrap();
    let first = core
        .call(&caller, "connections.acquire", params.clone())
        .unwrap();
    let second = core.call(&caller, "connections.acquire", params).unwrap();
    assert_eq!(first["connectionId"], second["connectionId"]);
    let kept = core
        .call(&caller, "pty.open", json!({"leaseId":first["leaseId"]}))
        .unwrap();
    output(&core, &caller, &kept, "fixture-ready");

    let marker = fixture.directory.path().join("pause-shell");
    std::fs::write(&marker, b"pause").unwrap();
    let opening_core = core.clone();
    let opening_caller = caller.clone();
    let opening_lease = second["leaseId"].clone();
    let pending = std::thread::spawn(move || {
        opening_core.call(
            &opening_caller,
            "pty.open",
            json!({"leaseId":opening_lease}),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.events().contains("shell-paused") {
        assert!(
            Instant::now() < deadline,
            "shell did not reach the delayed response boundary"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    core.call(
        &caller,
        "connections.release",
        json!({"leaseId":second["leaseId"]}),
    )
    .unwrap();
    std::fs::remove_file(&marker).unwrap();
    assert_eq!(pending.join().unwrap().unwrap_err().code, "CANCELED");
    let resources = core.resources();
    assert_eq!(resources["ptys"].as_array().unwrap().len(), 1);
    assert_eq!(resources["ptys"][0]["id"], kept["ptyId"]);
    assert_eq!(resources["leases"].as_array().unwrap().len(), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.events().contains("shell-close") {
        assert!(
            Instant::now() < deadline,
            "late shell was not physically closed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Cancel a second delayed PTY on the still-live shared connection. This
    // request must finish before the remote shell reply is released.
    let cancel_lease = core
        .call(
            &caller,
            "connections.acquire",
            json!({
                "serverId":server["id"], "workspaceId":"shared", "operationId":"cancel-pty"
            }),
        )
        .unwrap();
    std::fs::write(&marker, b"pause").unwrap();
    let opening_core = core.clone();
    let opening_caller = caller.clone();
    let lease_id = cancel_lease["leaseId"].clone();
    let pending = std::thread::spawn(move || {
        opening_core.call(
            &opening_caller,
            "pty.open",
            json!({
                "leaseId":lease_id, "operationId":"cancel-pty"
            }),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture
        .events()
        .lines()
        .filter(|line| line.contains("shell-paused"))
        .count()
        < 2
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let at = Instant::now();
    core.call(
        &caller,
        "operations.cancel",
        json!({"operationId":"cancel-pty"}),
    )
    .unwrap();
    assert_eq!(pending.join().unwrap().unwrap_err().code, "CANCELED");
    assert!(at.elapsed() < Duration::from_millis(500));
    std::fs::remove_file(&marker).unwrap();
    core.call(
        &caller,
        "connections.release",
        json!({"leaseId":cancel_lease["leaseId"]}),
    )
    .unwrap();
    assert_eq!(core.resources()["ptys"].as_array().unwrap().len(), 1);
    core.call(
        &caller,
        "pty.write",
        json!({
            "ptyId":kept["ptyId"], "generation":kept["generation"], "inputSeq":1,
            "bytes":STANDARD.encode(b"retained-channel-alive")
        }),
    )
    .unwrap();
    output(&core, &caller, &kept, "retained-channel-alive");
    core.shutdown();
    assert!(core.resources()["ptys"].as_array().unwrap().is_empty());
}
