// Actual external plugin, shared fixture transport, and real client sockets.
fn network_surface(f: &Fixture, context: &sodeck::workspaces::Context) -> (String, String) {
    f.core.workspace_state().ready("main").unwrap();
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
        .ui_event("main", token, json!({"action":"save"}))
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
    if network_row(f, window, token, id)["run"].is_null() {
        // Saving produced no listener; starting is a separate user intent.
        f.core.ui_event(window, token, json!({"action":"start","id":id})).unwrap();
    }
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
async fn network_curl(address: SocketAddr, server: &SshServer) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    for host in ["curl.only.remote.invalid", "127.0.0.1"] {
        server.state.routes.lock().unwrap().insert(host.into(), listener.local_addr().unwrap());
    }
    let http = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                assert!(header.len() < 4096);
                header.push(socket.read_u8().await.unwrap());
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nsocks-curl\n").await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    let rejected_host = "curl.rejected.remote.invalid";
    assert!(!server.state.routes.lock().unwrap().contains_key(rejected_host));
    for host in ["curl.only.remote.invalid", "127.0.0.1", rejected_host] {
        let result = tokio::task::spawn_blocking(move || {
            let curl = std::env::var("SODECK_TEST_CURL").unwrap_or_else(|_| if cfg!(windows) { "curl.exe" } else { "curl" }.into());
            let version = std::process::Command::new(&curl).arg("--version").output().unwrap();
            assert!(version.status.success());
            assert!(version.stdout.starts_with(b"curl 8.21.0 "));
            let mut command = std::process::Command::new(curl);
            #[cfg(windows)] {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            command.args(["--silent", "--show-error", "--noproxy", "", "--connect-timeout", "12", "--max-time", "15",
                "--socks5-hostname", &address.to_string(), &format!("http://{host}:{port}/")]).output().unwrap()
        }).await.unwrap();
        if host == rejected_host {
            // CURLE_PROXY confirms SOCKS target failure, rather than local DNS or HTTP failure.
            assert_eq!(result.status.code(), Some(97), "unexpected curl failure: {}", String::from_utf8_lossy(&result.stderr));
            assert!(result.stdout.is_empty(), "rejected target returned HTTP body");
            assert!(server.state.hosts.lock().unwrap().iter().any(|h| h == rejected_host));
        } else {
            assert!(result.status.success(), "curl failed: {}", String::from_utf8_lossy(&result.stderr));
            assert_eq!(result.stdout, b"socks-curl\n");
        }
    }
    tokio::time::timeout(Duration::from_secs(3), http).await.unwrap().unwrap();
    assert!(server.state.hosts.lock().unwrap().iter().any(|h| h == "curl.only.remote.invalid"));
}
async fn network_duplex(address: SocketAddr, server: &SshServer) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    server.state.routes.lock().unwrap().insert("duplex.only.remote.invalid".into(), listener.local_addr().unwrap());
    let remote = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all(&vec![0x5a; 64 * 1024]).await.unwrap();
        socket.shutdown().await.unwrap();
        let mut upload = Vec::new(); socket.read_to_end(&mut upload).await.unwrap();
        assert_eq!(upload, vec![0xa5; 128 * 1024]);
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&[5, 1, 0]).await.unwrap();
        let mut method = [0; 2]; socket.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [5, 0]);
        let host = b"duplex.only.remote.invalid";
        let mut request = vec![5, 1, 0, 3, host.len() as u8];
        request.extend_from_slice(host); request.extend_from_slice(&port.to_be_bytes());
        socket.write_all(&request).await.unwrap();
        let mut response = [0; 10]; socket.read_exact(&mut response).await.unwrap();
        assert_eq!(response, [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        let (mut read, mut write) = socket.into_split();
        tokio::join!(async {
            let mut download = Vec::new(); read.read_to_end(&mut download).await.unwrap();
            assert_eq!(download, vec![0x5a; 64 * 1024]);
        }, async {
            write.write_all(&vec![0xa5; 128 * 1024]).await.unwrap();
            write.shutdown().await.unwrap();
        });
        remote.await.unwrap();
    }).await.expect("bidirectional transfer or remote half-close failed");
}
async fn network_limits(f: &Fixture, server: &SshServer, context: &sodeck::workspaces::Context, token: &str) {
    let (echo, task) = network_echo_server().await;
    let host = "limits.only.remote.invalid";
    server.state.routes.lock().unwrap().insert(host.into(), echo);
    network_save(f, context, json!({"id":"limits","serverId":context.server_id,"revision":0,
        "name":"limits","type":"socks5","bind":{"host":"127.0.0.1","port":0}}));
    let address = network_address(f, "main", token, "limits").await;
    let connections = || network_row(f, "main", token, "limits")["run"]["snapshot"]["connections"].as_u64().unwrap();
    let mut sockets = Vec::new();
    for _ in 0..16 { sockets.push(TcpStream::connect(address).await.unwrap()); }
    until(|| connections() == 16).await;
    async fn rejected(address: SocketAddr) {
        let mut extra = TcpStream::connect(address).await.unwrap();
        let mut byte = [0; 1];
        let closed = tokio::time::timeout(Duration::from_secs(3), extra.read(&mut byte)).await.expect("excess client remained admitted");
        assert!(matches!(closed, Ok(0)) || closed.is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionReset));
    }
    rejected(address).await;
    sockets.truncate(8);
    until(|| connections() == 8).await;
    let mut replacement = TcpStream::connect(address).await.unwrap();
    replacement.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0; 2];
    tokio::time::timeout(Duration::from_secs(3), replacement.read_exact(&mut method)).await.unwrap().unwrap();
    assert_eq!(method, [5, 0]);
    drop(replacement);
    sockets.clear();
    until(|| connections() == 0).await;
    for _ in 0..64 {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&[5, 1, 0]).await.unwrap();
        socket.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [5, 0]);
        let mut request = vec![5, 1, 0, 3, host.len() as u8];
        request.extend_from_slice(host.as_bytes()); request.extend_from_slice(&echo.port().to_be_bytes());
        socket.write_all(&request).await.unwrap();
        let mut response = [0; 10];
        tokio::time::timeout(Duration::from_secs(3), socket.read_exact(&mut response)).await.unwrap().unwrap();
        assert_eq!(response, [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        sockets.push(socket);
    }
    until(|| connections() == 64).await;
    rejected(address).await;
    sockets.truncate(56);
    until(|| connections() == 56).await;
    network_roundtrip(address, true, host, echo.port()).await;
    sockets.clear();
    until(|| connections() == 0).await;
    task.abort();
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
        let mut w = f.core.workspace_state();
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
    f.core.commit_workspace_move(&movement).unwrap();
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
        .workspace_state()
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
    f.core.commit_workspace_move(&movement).unwrap();
    // Edit a running rule and reuse its port only after the old listener has stopped.
    let old = network_row(f, "main", token, "forward");
    let listener = old["run"]["snapshot"]["listenerId"].clone();
    let mut rule = old["rule"].clone();
    rule["name"] = json!("edited running rule");
    rule["bind"]["port"] = json!(fixed.port());
    network_save(f, context, rule);
    assert_eq!(network_row(f,"main",token,"forward")["run"]["snapshot"]["listenerId"],listener);
    f.core.ui_event("main",token,json!({"action":"stop","id":"forward"})).unwrap();
    until(||network_row(f,"main",token,"forward")["run"]["canStart"]==true).await;
    f.core.ui_event("main",token,json!({"action":"start","id":"forward"})).unwrap();
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
    // Running rules cannot be deleted; stop and confirmed cleanup are separate steps.
    let revision=network_row(f,"main",token,"forward")["rule"]["revision"].clone();
    assert!(f.core.ui_event("main",token,json!({"action":"delete","id":"forward","revision":revision})).is_err());
    f.core.ui_event("main",token,json!({"action":"stop","id":"forward"})).unwrap();
    until(||network_row(f,"main",token,"forward")["run"]["canStart"]==true).await;
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
async fn network_socks_protocol() {
    let f = Fixture::new();
    let server = SshServer::new().await;
    let id = f.server("127.0.0.1", server.addr.port(), json!([]));
    let context = connected(&f, &id).await.context.unwrap();
    let (_, token) = network_surface(&f, &context);
    network_save(&f, &context, json!({"id":"protocol","serverId":id,"revision":0,
        "name":"protocol","type":"socks5","bind":{"host":"127.0.0.1","port":0}}));
    let address = network_address(&f, "main", &token, "protocol").await;
    for input in [vec![4], vec![5], vec![5, 0]] {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&input).await.unwrap();
        socket.shutdown().await.unwrap();
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut output))
            .await.unwrap().unwrap();
        assert!(output.is_empty(), "invalid/incomplete greeting received CONNECT reply: {output:?}");
    }
    for methods in [vec![2], vec![0x80], vec![2, 0x80]] {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&[&[5, methods.len() as u8][..], &methods].concat()).await.unwrap();
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut output)).await.unwrap().unwrap();
        assert_eq!(output, [5, 255]);
    }
    for (request, code) in [
        (vec![5, 2, 0, 1], 7), (vec![5, 3, 0, 1], 7), (vec![5, 0x80, 0, 1], 7),
        (vec![5, 1, 1, 1], 1), (vec![5, 1, 0, 0x80], 8),
        (vec![5, 1, 0, 3, 0], 8), (vec![5, 1, 0, 3, 1, 0xff, 0, 80], 1),
        (vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 0], 8),
    ] {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&[5, 1, 0]).await.unwrap();
        let mut method = [0; 2];
        socket.read_exact(&mut method).await.unwrap();
        assert_eq!(method, [5, 0]);
        socket.write_all(&request).await.unwrap();
        let mut output = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut output)).await.unwrap().unwrap();
        assert_eq!(output, [5, code, 0, 1, 0, 0, 0, 0, 0, 0]);
    }
    network_curl(address, &server).await;
    network_duplex(address, &server).await;
    network_limits(&f, &server, &context, &token).await;
    f.core.unload("sodeck.network-proxy").unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires private Linux netem namespace; npm run network:netem"]
async fn network_socks_netem() {
    let profile = std::env::var("SODECK_NETEM_PROFILE").expect("netem profile");
    let count: usize = std::env::var("SODECK_NETEM_COUNT").unwrap().parse().unwrap();
    let loss = std::env::var("SODECK_NETEM_LOSS").unwrap();
    let mut successes = 0;
    let mut failures = Vec::new();
    let began = std::time::Instant::now();
    for trial in 0..count {
        let f = Fixture::new();
        let server = SshServer::new().await;
        let socks = Socks::independent(server.addr, true).await;
        let proxy = f.proxy("127.0.0.1", socks.addr.port(), true);
        let id = f.server("ssh.only.remote.invalid", 22, json!([{"kind":"socks5","id":"","proxyId":proxy}]));
        let start = f.core.start_connection("main", &id).unwrap();
        let view = tokio::time::timeout(Duration::from_secs(65), async {
            loop {
                let view = f.core.connection_views().into_iter().find(|v| v.attempt_id == start.attempt_id).unwrap();
                if let Some(Question::HostKey { id, .. }) = &view.question {
                    f.core.answer_connection("main", &view.attempt_id, id, Answer::HostKey { accept: true }).unwrap();
                }
                if matches!(view.state.as_str(), "connected" | "failed") { break view; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("core attempt exceeded its two stage deadlines");
        let Some(context) = view.context else {
            failures.push(json!({"trial":trial,"stage":"route","error":view.error}));
            continue;
        };
        let (echo, task) = network_echo_server().await;
        server.state.routes.lock().unwrap().insert("echo.only.remote.invalid".into(), echo);
        let (_, token) = network_surface(&f, &context);
        network_save(&f, &context, json!({"id":"netem","serverId":id,"revision":0,
            "name":"netem","type":"socks5","bind":{"host":"127.0.0.1","port":0}}));
        let address = network_address(&f, "main", &token, "netem").await;
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            let mut socket = TcpStream::connect(address).await?;
            socket.write_all(&[5, 1, 0]).await?;
            let mut method = [0; 2]; socket.read_exact(&mut method).await?;
            assert_eq!(method, [5, 0]);
            let host = b"echo.only.remote.invalid";
            let mut request = vec![5, 1, 0, 3, host.len() as u8];
            request.extend_from_slice(host); request.extend_from_slice(&echo.port().to_be_bytes());
            request.extend_from_slice(b"tail"); socket.write_all(&request).await?;
            let mut reply = [0; 10]; socket.read_exact(&mut reply).await?;
            if reply[1] != 0 { return Err(std::io::Error::other(format!("SOCKS REP {}", reply[1]))); }
            assert_eq!(reply, [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
            let data = vec![0xa7; 128 * 1024]; socket.write_all(&data).await?; socket.shutdown().await?;
            let mut output = Vec::new(); socket.read_to_end(&mut output).await?;
            assert_eq!(output, [b"tail".as_slice(), data.as_slice()].concat(), "data corruption under netem");
            Ok::<(), std::io::Error>(())
        }).await;
        match result {
            Ok(Ok(())) => successes += 1,
            other => failures.push(json!({"trial":trial,"stage":"relay","error":format!("{other:?}")})),
        }
        f.core.unload("sodeck.network-proxy").unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(rebound) = TcpListener::bind(address).await { drop(rebound); break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("plugin retained listener under netem");
        task.abort();
    }
    println!("SOCKS_NETEM_RESULT {}", json!({"profile":profile,"successes":successes,"failures":failures,"elapsedMs":began.elapsed().as_millis()}));
    if loss == "0%" { assert_eq!(successes, count, "lossless profile regressed"); }
    else if loss != "100%" { assert!(successes > 0, "no successful data transfer under netem"); }
    else { assert_eq!(successes, 0); }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires actual network plugin package; npm run network:test"]
async fn network_socks_lifecycle() {
    struct StalledTcp(Arc<Mutex<Vec<(Channel<server::Msg>, server::ChannelOpenHandle)>>>);
    impl server::Handler for StalledTcp {
        type Error = russh::Error;
        async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth, Self::Error> {
            Ok(if user == "ops" && password == "secret" { server::Auth::Accept } else { server::Auth::reject() })
        }
        async fn channel_open_direct_tcpip(&mut self, channel: Channel<server::Msg>, _: &str, _: u32,
            _: &str, _: u32, reply: server::ChannelOpenHandle, _: &mut Session) -> Result<(), Self::Error> {
            // Hold the open request without blocking the SSH event loop.
            self.0.lock().unwrap().push((channel, reply));
            Ok(())
        }
    }
    let pending = Arc::new(Mutex::new(Vec::new()));
    let handler = StalledTcp(pending.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Arc::new(server::Config { keys: vec![russh::keys::PrivateKey::random(
        &mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap()], ..Default::default() });
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        server::run_stream(config, socket, handler).await.unwrap().await.unwrap();
    });
    let f = Fixture::new();
    let id = f.server("127.0.0.1", port, json!([]));
    let context = connected(&f, &id).await.context.unwrap();
    let (_, token) = network_surface(&f, &context);
    network_save(&f, &context, json!({"id":"lifecycle","serverId":id,"revision":0,
        "name":"lifecycle","type":"socks5","bind":{"host":"127.0.0.1","port":0}}));
    let address = network_address(&f, "main", &token, "lifecycle").await;
    for greeting in [vec![5], vec![5, 1, 0]] {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&greeting).await.unwrap();
        if greeting.len() == 3 {
            let mut choice = [0; 2]; socket.read_exact(&mut choice).await.unwrap();
            assert_eq!(choice, [5, 0]);
        }
        let mut reply = Vec::new();
        tokio::time::timeout(Duration::from_secs(16), socket.read_to_end(&mut reply)).await
            .expect("stalled negotiation exceeded its existing deadline").unwrap();
        if greeting.len() == 1 { assert!(reply.is_empty()); }
        // The core's I/O deadline may close the stream before the manager can reply.
        else { assert!(reply.is_empty() || reply == [5, 6, 0, 1, 0, 0, 0, 0, 0, 0]); }
    }
    async fn request(address: SocketAddr) -> TcpStream {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(&[5, 1, 0]).await.unwrap();
        let mut choice = [0; 2]; socket.read_exact(&mut choice).await.unwrap();
        assert_eq!(choice, [5, 0]);
        socket.write_all(b"\x05\x01\x00\x03\x07pending\x00\x50").await.unwrap();
        socket
    }
    let mut timed_out = request(address).await;
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(16), timed_out.read_to_end(&mut reply)).await
        .expect("target open exceeded its existing deadline").unwrap();
    assert_eq!(reply, [5, 6, 0, 1, 0, 0, 0, 0, 0, 0]);
    pending.lock().unwrap().clear();
    let mut socket = request(address).await;
    until(|| !pending.lock().unwrap().is_empty()).await;
    let stop = std::time::Instant::now();
    f.core.unload("sodeck.network-proxy").unwrap();
    let mut reply = Vec::new();
    let remaining = Duration::from_secs(5).checked_sub(stop.elapsed()).expect("plugin unload exceeded cleanup deadline");
    let closed = tokio::time::timeout(remaining, socket.read_to_end(&mut reply)).await
        .expect("pending target survived plugin unload");
    assert!(closed.is_ok() || closed.is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionReset));
    assert!(!reply.starts_with(&[5, 0]), "cancelled target reported success");
    TcpListener::bind(address).await.expect("pending target retained listener");
    assert!(stop.elapsed() < Duration::from_secs(5));
    pending.lock().unwrap().clear();
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
    let inner = Socks::independent(target.addr, true).await;
    let gateway = SshServer::new().await;
    gateway
        .state
        .routes
        .lock()
        .unwrap()
        .insert("proxy.remote.invalid".into(), inner.addr);
    let outer = Socks::independent(gateway.addr, false).await;
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
