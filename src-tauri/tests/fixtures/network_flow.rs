// Actual external plugin, shared fixture transport, and real client sockets.
fn network_surface(f: &Fixture, context: &sodeck::workspaces::Context) -> (String, String) {
    f.core.workspaces.lock().unwrap().ready("main").unwrap();
    f.core
        .load(std::path::Path::new(
            &std::env::var("SODECK_TEST_NETWORK_PLUGIN").expect("network plugin package"),
        ))
        .unwrap();
    f.core.ui_snapshot("main").unwrap();
    let opened = f
        .core
        .ui_action(
            "main",
            context,
            json!({"op":"open","key":"sodeck.network-proxy:manager","requestId":"network-manager"}),
        )
        .unwrap();
    let view = opened["viewId"].as_str().unwrap().to_owned();
    let mount = f.core.ui_attach("main", &view).unwrap();
    let token = mount["surfaceId"].as_str().unwrap().to_owned();
    f.core.ui_surface_ready("main", &token).unwrap();
    (view, token)
}
fn network_save(f: &Fixture, context: &sodeck::workspaces::Context, rule: Value) {
    let opened=f.core.ui_action("main",context,json!({"op":"open","key":"sodeck.network-proxy:rule-editor","requestId":format!("editor-{}-{}",rule["id"],rule["revision"]),"params":{"rule":rule}})).unwrap();
    let view = opened["viewId"].as_str().unwrap();
    let mount = f.core.ui_attach("main", view).unwrap();
    let token = mount["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    f.core
        .ui_event("main", token, json!({"action":"draft","rule":rule}))
        .unwrap();
    f.core
        .ui_event("main", token, json!({"action":"saveStart"}))
        .unwrap();
    let plan = f
        .core
        .ui_close_prepare(
            "main",
            vec![context.workspace_id.clone()],
            Some(view.into()),
        )
        .unwrap();
    assert!(plan["dirty"].as_array().unwrap().is_empty());
    f.core
        .ui_close_finish(
            plan["token"].as_str().unwrap(),
            serde_json::from_value(plan["views"].clone()).unwrap(),
            "discard",
        )
        .unwrap();
}
fn network_row(f: &Fixture, window: &str, token: &str, id: &str) -> Value {
    f.core
        .ui_event(window, token, json!({"action":"snapshot"}))
        .unwrap()["catalog"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"]["id"] == id)
        .unwrap()
        .clone()
}
async fn network_address(f: &Fixture, window: &str, token: &str, id: &str) -> std::net::SocketAddr {
    let mut result = None;
    until(|| {
        let row = network_row(f, window, token, id);
        assert_ne!(row["run"]["snapshot"]["state"], "failed", "{row}");
        if row["run"]["snapshot"]["state"] == "running" {
            result = Some(
                format!(
                    "127.0.0.1:{}",
                    row["run"]["snapshot"]["address"]["port"].as_u64().unwrap()
                )
                .parse()
                .unwrap(),
            );
            true
        } else {
            false
        }
    })
    .await;
    result.unwrap()
}
async fn network_echo_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let task = tokio::spawn(async move {
        while let Ok((mut s, _)) = socket.accept().await {
            tokio::spawn(async move {
                let mut data = Vec::new();
                s.read_to_end(&mut data).await.unwrap();
                s.write_all(&data).await.unwrap();
                let _ = s.shutdown().await;
            });
        }
    });
    (addr, task)
}
async fn network_roundtrip(addr: std::net::SocketAddr, socks: bool, host: &str, port: u16) {
    let work = async {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        if socks {
            socket.write_all(&[5]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            socket.write_all(&[1, 0]).await.unwrap();
            let mut reply = [0; 2];
            socket.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply, [5, 0]);
            let mut req = vec![5, 1, 0, 3, host.len() as u8];
            req.extend_from_slice(host.as_bytes());
            req.extend_from_slice(&port.to_be_bytes());
            req.extend_from_slice(b"tail");
            socket.write_all(&req).await.unwrap();
            let mut reply = [0; 10];
            socket.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply[1], 0, "{reply:?}");
        }
        let data = vec![0xA7; 128 * 1024];
        socket.write_all(&data).await.unwrap();
        socket.shutdown().await.unwrap();
        let mut output = Vec::new();
        socket.read_to_end(&mut output).await.unwrap();
        let expected = if socks {
            [b"tail".as_slice(), data.as_slice()].concat()
        } else {
            data
        };
        assert_eq!(output.len(), expected.len());
        assert_eq!(output, expected);
    };
    tokio::time::timeout(Duration::from_secs(20), work)
        .await
        .expect("tunnel roundtrip timed out");
}
async fn network_business(
    f: &Fixture,
    context: &sodeck::workspaces::Context,
    host: &str,
    port: u16,
) {
    let (view, token) = network_surface(f, context);
    for (id, kind) in [("forward", "tcpForward"), ("socks", "socks5")] {
        let mut r = json!({"id":id,"revision":0,"serverId":context.server_id,"name":id,"type":kind,"bind":{"host":"127.0.0.1","port":0}});
        if kind == "tcpForward" {
            r["target"] = json!({"host":host,"port":port})
        }
        network_save(f, context, r);
    }
    let fixed = network_address(f, "main", &token, "forward").await;
    let socks = network_address(f, "main", &token, "socks").await;
    let (a, b) = tokio::join!(
        network_roundtrip(fixed, false, host, port),
        network_roundtrip(socks, true, host, port)
    );
    let _ = (a, b);
    // Listener success is distinct from target reachability; real local entry checks both modes.
    for id in ["forward", "socks"] {
        f.core
            .ui_event(
                "main",
                &token,
                json!({"action":"check","id":id,"target":{"host":host,"port":port}}),
            )
            .unwrap();
    }
    until(|| {
        let s = f
            .core
            .ui_event("main", &token, json!({"action":"snapshot"}))
            .unwrap();
        ["forward", "socks"]
            .iter()
            .all(|id| s["catalog"]["checks"][id]["ok"] == true)
    })
    .await;
    let metrics = f.core.network_snapshot();
    let payload = metrics["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["pluginId"] == "sodeck.network-proxy")
        .unwrap();
    assert_eq!(payload["sent"], "262148");
    assert_eq!(payload["received"], "262148");
    // Collapsing/reopening preserves exact listener identity.
    f.core
        .ui_action("main", context, json!({"op":"collapse","side":"right"}))
        .unwrap();
    f.core
        .ui_action(
            "main",
            context,
            json!({"op":"open","key":"sodeck.network-proxy:manager","requestId":"reopen-network"}),
        )
        .unwrap();
    assert_eq!(network_address(f, "main", &token, "forward").await, fixed);
    // A cancelled workspace close must keep existing business usable.
    let plan = f
        .core
        .ui_close_prepare("main", vec![context.workspace_id.clone()], None)
        .unwrap();
    assert!(plan["dirty"].as_array().unwrap().is_empty());
    f.core
        .ui_close_finish(
            plan["token"].as_str().unwrap(),
            serde_json::from_value(plan["views"].clone()).unwrap(),
            "cancel",
        )
        .unwrap();
    network_roundtrip(fixed, false, host, port).await;
    // Remount in the same window doesn't create a new listener.
    let mounted = f.core.ui_attach("main", &view).unwrap();
    let token = mounted["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    assert_eq!(network_address(f, "main", token, "socks").await, socks);
    // Real transfer ownership: stale source surfaces cannot mutate rules, listeners remain unchanged.
    let movement = {
        let mut w = f.core.workspaces.lock().unwrap();
        w.register("network-target").unwrap();
        w.ready("network-target").unwrap();
        w.prepare(
            "main",
            &context.workspace_id,
            "network-target",
            0,
            "network-move".into(),
        )
        .unwrap()
    };
    let mounted = f.core.ui_attach("network-target", &view).unwrap();
    let moved = mounted["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("network-target", moved).unwrap();
    f.core.workspaces.lock().unwrap().commit(&movement).unwrap();
    assert!(f
        .core
        .ui_event("main", token, json!({"action":"stop","id":"socks"}))
        .is_err());
    assert_eq!(
        network_address(f, "network-target", moved, "socks").await,
        socks
    );
    network_roundtrip(socks, true, host, port).await;
    let movement = f
        .core
        .workspaces
        .lock()
        .unwrap()
        .prepare(
            "network-target",
            &context.workspace_id,
            "main",
            0,
            "network-return".into(),
        )
        .unwrap();
    let mounted = f.core.ui_attach("main", &view).unwrap();
    let token = mounted["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    f.core.workspaces.lock().unwrap().commit(&movement).unwrap();
    // Edit a running rule and reuse its port only after the old listener has stopped.
    let old = network_row(f, "main", token, "forward");
    let listener = old["run"]["snapshot"]["listenerId"].clone();
    let mut rule = old["rule"].clone();
    rule["name"] = json!("edited running rule");
    rule["bind"]["port"] = json!(fixed.port());
    network_save(f, context, rule);
    until(|| {
        let row = network_row(f, "main", token, "forward");
        assert_ne!(row["run"]["snapshot"]["state"], "failed", "{row}");
        row["run"]["snapshot"]["state"] == "running"
            && row["run"]["snapshot"]["listenerId"] != listener
            && row["run"]["ruleRevision"] == row["rule"]["revision"]
    })
    .await;
    assert_eq!(network_address(f, "main", token, "forward").await, fixed);
    network_roundtrip(fixed, false, host, port).await;
    // One delete operation stops the running listener and persists deletion.
    let revision = network_row(f, "main", token, "forward")["rule"]["revision"].clone();
    f.core
        .ui_event(
            "main",
            token,
            json!({"action":"delete","id":"forward","revision":revision}),
        )
        .unwrap();
    until(|| {
        f.core
            .ui_event("main", token, json!({"action":"snapshot"}))
            .unwrap()["catalog"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["rule"]["id"] != "forward")
    })
    .await;
    assert!(TcpStream::connect(fixed).await.is_err());
    network_roundtrip(socks, true, host, port).await;
    f.core.disconnect_connection("main", context).unwrap();
    until(|| network_row(f, "main", token, "socks")["run"]["snapshot"]["state"] == "failed").await;
    assert!(TcpStream::connect(socks).await.is_err());
}
async fn network_exit_cleanup(crash: bool) {
    let f = Fixture::new();
    let server = SshServer::new().await;
    let (echo, task) = network_echo_server().await;
    server
        .state
        .routes
        .lock()
        .unwrap()
        .insert("service.cleanup.invalid".into(), echo);
    let id = f.server("127.0.0.1", server.addr.port(), json!([]));
    let context = connected(&f, &id).await.context.unwrap();
    let (_, token) = network_surface(&f, &context);
    network_save(
        &f,
        &context,
        json!({"id":"cleanup","revision":0,"serverId":id,"name":"cleanup","type":"tcpForward","bind":{"host":"127.0.0.1","port":0},"target":{"host":"service.cleanup.invalid","port":echo.port()}}),
    );
    let address = network_address(&f, "main", &token, "cleanup").await;
    network_roundtrip(address, false, "service.cleanup.invalid", echo.port()).await;
    let mut active = TcpStream::connect(address).await.unwrap();
    active.write_all(b"unfinished transfer").await.unwrap();
    if crash {
        // Kill only the isolated plugin process started by this fixture.
        let pid = f
            .core
            .snapshot()
            .plugins
            .iter()
            .find(|p| p.manifest.id == "sodeck.network-proxy")
            .unwrap()
            .pid
            .unwrap();
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("taskkill.exe")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .output()
                .unwrap()
                .status
        };
        #[cfg(not(windows))]
        let status = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output()
            .unwrap()
            .status;
        assert!(status.success());
        until(|| {
            f.core
                .snapshot()
                .plugins
                .iter()
                .any(|p| p.manifest.id == "sodeck.network-proxy" && p.cleanup_complete)
        })
        .await;
    } else {
        f.core.unload("sodeck.network-proxy").unwrap();
    }
    let mut bytes = [0; 16];
    let ended = tokio::time::timeout(Duration::from_secs(3), active.read(&mut bytes))
        .await
        .expect("active client survived plugin exit");
    assert!(ended.is_err() || ended.unwrap() == 0);
    assert!(TcpStream::connect(address).await.is_err());
    let rebound = TcpListener::bind(address)
        .await
        .expect("plugin exit retained listener port");
    drop(rebound);
    assert!(f
        .core
        .connection_views()
        .iter()
        .any(|v| v.server_id == id && v.state == "connected"));
    task.abort();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires actual network plugin package; npm run network:test"]
async fn network_proxy_plugin() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let (echo, task) = network_echo_server().await;
    target
        .state
        .routes
        .lock()
        .unwrap()
        .insert("service.only.remote.invalid".into(), echo);
    let inner = Socks::new(target.addr, true).await;
    let gateway = SshServer::new().await;
    gateway
        .state
        .routes
        .lock()
        .unwrap()
        .insert("proxy.remote.invalid".into(), inner.addr);
    let outer = Socks::new(gateway.addr, false).await;
    let p1 = f.proxy("127.0.0.1", outer.addr.port(), false);
    let p2 = f.proxy("proxy.remote.invalid", 1080, true);
    let jump = f.server("gateway.remote.invalid", 22, json!([]));
    let id=f.server("target.remote.invalid",22,json!([{"kind":"socks5","id":"","proxyId":p1},{"kind":"ssh","id":"","serverId":jump},{"kind":"socks5","id":"","proxyId":p2}]));
    let context = connected(&f, &id).await.context.unwrap();
    network_business(&f, &context, "service.only.remote.invalid", echo.port()).await;
    assert!(target
        .state
        .hosts
        .lock()
        .unwrap()
        .iter()
        .all(|h| h == "service.only.remote.invalid"));
    task.abort();
    network_exit_cleanup(false).await;
    network_exit_cleanup(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated OpenSSH and actual network plugin; npm run network:test"]
async fn openssh_network_plugin() {
    let f = Fixture::new();
    let id = f.server(
        "127.0.0.1",
        std::env::var("SODECK_TEST_SSH_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        json!([]),
    );
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["username"] = json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());
    draft["authentication"] = json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let context = connected(&f, &id).await.context.unwrap();
    let (echo, task) = network_echo_server().await;
    network_business(&f, &context, "127.0.0.1", echo.port()).await;
    task.abort();
}
