//! Real isolated OpenSSH integration; opt in with SODECK_TEST_REMOTE_CONFIG.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sodeck_core::{Caller, Core};
use std::time::{Duration, Instant};
fn invoke(core: &Core, caller: &Caller, method: &str, p: Value) -> Value {
    core.call(caller, method, p)
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}
#[test]
#[ignore = "requires isolated real Linux: SODECK_TEST_REMOTE_CONFIG"]
fn real_sftp_exec_and_shared_pty() {
    let config =
        std::env::var("SODECK_TEST_REMOTE_CONFIG").expect("isolated Linux config required");
    let cfg: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path()).unwrap();
    let shell = Caller::shell();
    let files = Caller {
        source: "plugin".into(),
        plugin_id: Some("sodeck.files".into()),
        instance_id: "files-test".into(),
        ..shell.clone()
    };
    let server = invoke(
        &core,
        &shell,
        "servers.create",
        json!({"name":"isolated Linux","host":cfg["host"],"port":cfg["port"],"username":cfg["username"],"authType":"key"}),
    );
    invoke(
        &core,
        &shell,
        "auth.set",
        json!({"serverId":server["id"],"privateKeyPath":cfg["privateKeyPath"]}),
    );
    let acquisition =
        json!({"serverId":server["id"],"workspaceId":"test","operationId":"connect-1"});
    let challenge = core
        .call(&shell, "connections.acquire", acquisition.clone())
        .unwrap_err();
    assert_eq!(challenge.code, "HOST_KEY_REQUIRED");
    invoke(
        &core,
        &shell,
        "hosts.trust",
        json!({"serverId":server["id"],"fingerprint":challenge.details.unwrap()["fingerprint"]}),
    );
    let mut retry = acquisition.clone();
    retry["operationId"] = json!("connect-2");
    let terminal = invoke(&core, &shell, "connections.acquire", retry);
    let pty = invoke(
        &core,
        &shell,
        "pty.open",
        json!({"leaseId":terminal["leaseId"],"cols":100,"rows":30}),
    );
    core.register_workspace("test", "one");
    let scope = json!({"workspaceId":"test","workspaceGeneration":"one"});
    let lease = invoke(
        &core,
        &files,
        "connections.acquireExisting",
        json!({"scope":scope,"serverId":server["id"],"configRevision":server["revision"],"operationId":"existing"}),
    );
    assert_eq!(terminal["connectionId"], lease["connectionId"]);
    let replay = invoke(
        &core,
        &files,
        "connections.acquireExisting",
        json!({"scope":scope,"serverId":server["id"],"configRevision":server["revision"],"operationId":"existing"}),
    );
    assert_eq!(lease, replay, "only-reuse acquisition must be idempotent");
    let common = json!({"scope":scope,"leaseId":lease["leaseId"],"connectionGeneration":lease["generation"]});
    let params = |extra: Value| {
        let mut p = common.clone();
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        p
    };
    let remote_path = |p: &str| json!({"bytesBase64":STANDARD.encode(p)});
    let stat = invoke(
        &core,
        &files,
        "files.stat",
        params(json!({"operationId":"stat","path":remote_path("/etc/os-release")})),
    );
    assert!(stat["type"] == "file" || stat["type"] == "symlink");
    let directory = invoke(
        &core,
        &files,
        "files.openDirectory",
        params(json!({"operationId":"open","path":remote_path("/")})),
    );
    let page_params = params(
        json!({"operationId":"page","directoryId":directory["directoryId"],"cursor":0,"maxEntries":3}),
    );
    let page = invoke(&core, &files, "files.readDirectory", page_params.clone());
    assert_eq!(page["entries"].as_array().unwrap().len(), 3);
    assert_eq!(
        page,
        invoke(&core, &files, "files.readDirectory", page_params)
    );
    let mut wrong = params(json!({"directoryId":directory["directoryId"]}));
    wrong["connectionGeneration"] = json!(lease["generation"].as_u64().unwrap() + 1);
    assert_eq!(
        core.call(&files, "files.closeDirectory", wrong)
            .unwrap_err()
            .code,
        "STALE_HANDLE"
    );
    let other = Caller {
        instance_id: "other-plugin".into(),
        ..files.clone()
    };
    assert_eq!(
        core.call(
            &other,
            "files.closeDirectory",
            params(json!({"directoryId":directory["directoryId"]}))
        )
        .unwrap_err()
        .code,
        "STALE_HANDLE"
    );
    assert_eq!(core.call(&files,"files.readDirectory",params(json!({"operationId":"page","directoryId":directory["directoryId"],"cursor":1,"maxEntries":3}))).unwrap_err().code,"REQUEST_CONFLICT");
    invoke(
        &core,
        &files,
        "files.closeDirectory",
        params(json!({"directoryId":directory["directoryId"]})),
    );
    let file = invoke(
        &core,
        &files,
        "files.openRead",
        params(json!({"operationId":"proc-open","path":remote_path("/proc/self/stat")})),
    );
    let bytes = invoke(
        &core,
        &files,
        "files.read",
        params(
            json!({"operationId":"proc-read","fileId":file["fileId"],"offset":"0","maxBytes":65536}),
        ),
    );
    assert!(!STANDARD
        .decode(bytes["bytesBase64"].as_str().unwrap())
        .unwrap()
        .is_empty());
    invoke(
        &core,
        &files,
        "files.close",
        params(json!({"fileId":file["fileId"]})),
    );
    let exec = invoke(
        &core,
        &files,
        "exec.start",
        params(
            json!({"operationId":"exec","command":"printf 'real-linux-output'; printf 'stderr-output' >&2; exit 7"}),
        ),
    );
    let mut output = String::new();
    let mut after = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = invoke(
            &core,
            &files,
            "exec.read",
            params(json!({"execId":exec["execId"],"afterSequence":after})),
        );
        for frame in result["frames"].as_array().unwrap() {
            output.push_str(
                &String::from_utf8(
                    STANDARD
                        .decode(frame["bytesBase64"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap(),
            );
            after = frame["sequence"].as_u64().unwrap();
        }
        invoke(
            &core,
            &files,
            "exec.ack",
            params(json!({"execId":exec["execId"],"sequence":after})),
        );
        if result["state"] == "completed" {
            assert_eq!(result["exitCode"], 7);
            break;
        }
        assert!(Instant::now() < deadline, "exec did not finish: {result}");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(output.contains("real-linux-output") && output.contains("stderr-output"));
    invoke(
        &core,
        &files,
        "exec.close",
        params(json!({"execId":exec["execId"]})),
    );
    // A canceled exec channel and a released file consumer must leave PTY alive.
    let slow = invoke(
        &core,
        &files,
        "exec.start",
        params(json!({"operationId":"slow","command":"sleep 20; printf late"})),
    );
    invoke(
        &core,
        &files,
        "exec.cancel",
        params(json!({"execId":slow["execId"]})),
    );
    invoke(
        &core,
        &files,
        "exec.close",
        params(json!({"execId":slow["execId"]})),
    );
    // Output overflow and deadlines report terminal errors, never completed data.
    for (name, command, limit, timeout, reason) in [
        (
            "overflow",
            "head -c 8192 /dev/zero",
            1024,
            3000,
            "LIMIT_EXCEEDED",
        ),
        ("timeout", "sleep 2", 65536, 200, "TIMEOUT"),
    ] {
        let ex = invoke(
            &core,
            &files,
            "exec.start",
            params(
                json!({"operationId":name,"command":command,"maxOutputBytes":limit,"timeoutMs":timeout}),
            ),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = invoke(
                &core,
                &files,
                "exec.read",
                params(json!({"execId":ex["execId"]})),
            );
            if state["state"] == "failed" {
                assert_eq!(state["reason"], reason);
                assert_eq!(state["truncated"], name == "overflow");
                break;
            }
            assert!(Instant::now() < deadline, "missing terminal error: {state}");
            std::thread::sleep(Duration::from_millis(10));
        }
        invoke(
            &core,
            &files,
            "exec.close",
            params(json!({"execId":ex["execId"]})),
        );
    }
    // Interrupt channel creation at different points while SFTP shares libssh2.
    for index in 0..12 {
        let operation = format!("cancel-open-{index}");
        std::thread::scope(|threads| {
            let pending=threads.spawn(||core.call(&shell,"pty.open",json!({"leaseId":terminal["leaseId"],"cols":80,"rows":24,"operationId":operation})));
            std::thread::sleep(Duration::from_millis(index));
            invoke(
                &core,
                &shell,
                "operations.cancel",
                json!({"operationId":operation}),
            );
            if let Ok(p) = pending.join().unwrap() {
                invoke(
                    &core,
                    &shell,
                    "pty.close",
                    json!({"ptyId":p["ptyId"],"generation":p["generation"]}),
                );
            }
        });
        invoke(
            &core,
            &files,
            "files.stat",
            params(json!({"operationId":format!("after-cancel-{index}"),"path":remote_path("/")})),
        );
    }
    invoke(
        &core,
        &shell,
        "pty.write",
        json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"inputSeq":1,"bytes":STANDARD.encode(b"printf 'T008-PTY-SURVIVED\\n'\r")}),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut terminal_text = String::new();
    loop {
        let result = invoke(
            &core,
            &shell,
            "pty.read",
            json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":0}),
        );
        for frame in result["frames"].as_array().unwrap() {
            terminal_text.push_str(&String::from_utf8_lossy(
                &STANDARD.decode(frame["bytes"].as_str().unwrap()).unwrap(),
            ));
        }
        if terminal_text.contains("T008-PTY-SURVIVED") {
            break;
        }
        assert!(Instant::now() < deadline, "PTY stopped during SFTP/exec");
        std::thread::sleep(Duration::from_millis(10));
    }
    let binding=core.bind_owned_view_resources(&[
        (shell.instance_id.clone(),json!({"viewId":"candidate","workspaceId":"test","mountGeneration":1,"resources":[{"kind":"pty","id":pty["ptyId"],"generation":pty["generation"]}]})),
        (files.instance_id.clone(),json!({"viewId":"other-candidate","workspaceId":"test","mountGeneration":1,"resources":[{"kind":"lease","id":"missing-lease"}]}))
    ]);
    assert!(binding.is_err());
    invoke(&core,&shell,"pty.read",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":0}));
    core.revoke_workspace("test", "one");
    assert_eq!(
        core.call(
            &files,
            "files.stat",
            params(json!({"operationId":"stale","path":remote_path("/")}))
        )
        .unwrap_err()
        .code,
        "STALE_WORKSPACE"
    );
    invoke(
        &core,
        &files,
        "connections.release",
        json!({"leaseId":lease["leaseId"]}),
    );
    assert_eq!(core.resources()["connections"].as_array().unwrap().len(), 1);
    assert!(core.resources()["remote"]["handles"]
        .as_array()
        .unwrap()
        .is_empty());
    // Injected sleep may delay OpenSSH's close acknowledgement. Its local
    // ownership is revoked now; expose native cleanup instead of dropping it.
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let diagnostic = core.resources();
        let io = &diagnostic["remote"]["connections"][0]["io"];
        if io["sessions"] == 0
            && io["handles"] == 0
            && io["execs"] == 0
            && io["cleanupPending"] == 0
        {
            break;
        }
        assert!(
            io["cleanupPending"].as_u64().unwrap() > 0,
            "unreported native cleanup: {io}"
        );
        assert!(
            Instant::now() < deadline,
            "native remote handles leaked: {diagnostic}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    core.register_workspace("test","two");
    let scope2=json!({"workspaceId":"test","workspaceGeneration":"two"});
    let lease2=invoke(&core,&files,"connections.acquireExisting",json!({"scope":scope2,"serverId":server["id"],"configRevision":server["revision"],"operationId":"existing-after-generation"}));
    let ex=invoke(&core,&files,"exec.start",json!({"scope":scope2,"leaseId":lease2["leaseId"],"connectionGeneration":lease2["generation"],"operationId":"disconnect","command":"sleep 2"}));
    invoke(&core,&shell,"resources.stop",json!({"resourceId":terminal["connectionId"]}));
    let deadline=Instant::now()+Duration::from_secs(2);
    loop {let state=invoke(&core,&files,"exec.read",json!({"scope":scope2,"leaseId":lease2["leaseId"],"connectionGeneration":lease2["generation"],"execId":ex["execId"]}));if state["state"]=="failed"{assert_eq!(state["reason"],"CONNECTION_LOST");break;}assert!(Instant::now()<deadline,"disconnected execution remained running");std::thread::sleep(Duration::from_millis(10));}
    core.revoke_workspace("test","two");
    invoke(
        &core,
        &shell,
        "pty.close",
        json!({"ptyId":pty["ptyId"],"generation":pty["generation"]}),
    );
    invoke(
        &core,
        &shell,
        "connections.release",
        json!({"leaseId":terminal["leaseId"]}),
    );
    assert!(core.resources()["connections"]
        .as_array()
        .unwrap()
        .is_empty());
}
