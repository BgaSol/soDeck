// Included by connections.rs to reuse its isolated SSH/SOCKS network.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated OpenSSH and actual terminal package; run npm run terminal:test"]
async fn openssh_terminal_plugin() {
    let f = Fixture::new();
    let port = std::env::var("SODECK_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let server = f.server("127.0.0.1", port, json!([]));
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["username"] = json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());
    draft["authentication"] = json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let context = connected(&f, &server).await.context.unwrap();
    f.core
        .load(std::path::Path::new(
            &std::env::var("SODECK_TEST_TERMINAL_PLUGIN").unwrap(),
        ))
        .unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    let view = state["workspaces"][&context.workspace_id]["editor"]
        .as_str()
        .unwrap();
    let mounted = f.core.ui_attach("main", view).unwrap();
    let token = mounted["surfaceId"].as_str().unwrap();
    let id = mounted["data"]["streamId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    let mut output = subscribe(&f.core, "main", token, id).await;
    let command = if cfg!(windows) {
        b"echo sodeck-real-pty-ok\r".as_slice()
    } else {
        b"printf 'sodeck-real-pty-ok\\n'\r".as_slice()
    };
    td(
        &f.core,
        "main",
        token,
        json!({"op":"write","id":id,"sequence":0,"data":command}),
    )
    .await
    .unwrap();
    data_until(
        &f.core,
        "main",
        token,
        id,
        &mut output,
        "sodeck-real-pty-ok",
    )
    .await;
    close_terminal_views(&f);
    f.core.disconnect_connection("main", &context).unwrap();
    f.core.shutdown().unwrap();
}
use sodeck::{
    streams::{Frame, Sink},
    terminal::DataRequest,
};

fn terminal_fixture(f: &Fixture) -> (String, String, String) {
    let plugin = f.path.join("terminal-fixture");
    std::fs::create_dir_all(plugin.join("ui")).unwrap();
    let binary = format!("plugin{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(env!("CARGO_BIN_EXE_plugin-fixture"), plugin.join(&binary)).unwrap();
    std::fs::write(plugin.join("ui/index.html"), "<p>terminal</p>").unwrap();
    std::fs::write(plugin.join("plugin.json"),json!({"id":"test.terminal","name":"PTY test","version":"0.1.0","apiVersion":"1","target":sodeck::TARGET,"entry":binary,"ui":{"apiVersion":"1","entry":"ui/index.html","views":[{"id":"terminal","title":"终端","icon":"terminal","container":"editor","instancePolicy":"multiple","openOnWorkspace":"createOne"}]}}).to_string()).unwrap();
    f.core.load(&plugin).unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    let workspace = state["workspaces"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    let view = workspace["editor"].as_str().unwrap().to_string();
    let mounted = f.core.ui_attach("main", &view).unwrap();
    let token = mounted["surfaceId"].as_str().unwrap().to_string();
    f.core.ui_surface_ready("main", &token).unwrap();
    let terminal = f
        .core
        .ui_event("main", &token, json!({"rpc":"terminal.open"}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    (view, token, terminal)
}
async fn td(core: &Arc<AppCore>, window: &str, token: &str, p: Value) -> Result<(), String> {
    core.terminal_data(
        window,
        token,
        serde_json::from_value::<DataRequest>(p).unwrap(),
        None,
    )
    .await
}
async fn subscribe(
    core: &Arc<AppCore>,
    window: &str,
    token: &str,
    id: &str,
) -> tokio::sync::mpsc::UnboundedReceiver<Frame> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Sink = Arc::new(move |frame| tx.send(frame).map_err(|_| "test consumer gone".into()));
    core.terminal_data(
        window,
        token,
        DataRequest::Attach { id: id.into() },
        Some(sink),
    )
    .await
    .unwrap();
    rx
}
async fn receive(
    core: &Arc<AppCore>,
    window: &str,
    token: &str,
    id: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Frame>,
) -> Frame {
    let frame = tokio::time::timeout(Duration::from_secs(8), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(frame.kind, "error", "{:?}", frame.metadata);
    td(
        core,
        window,
        token,
        json!({"op":"ack","id":id,"packet":frame.packet}),
    )
    .await
    .unwrap();
    frame
}
async fn data_until(
    core: &Arc<AppCore>,
    window: &str,
    token: &str,
    id: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Frame>,
    needle: &str,
) -> (Vec<u8>, u64) {
    let mut bytes = vec![];
    loop {
        let frame = receive(core, window, token, id, rx).await;
        if frame.kind == "data" {
            bytes.extend(frame.data);
        }
        if String::from_utf8_lossy(&bytes).contains(needle) {
            return (bytes, frame.offset);
        }
    }
}
fn close_terminal_views(f: &Fixture) {
    let ids = f.core.workspaces.lock().unwrap().ids();
    let plan = f.core.ui_close_prepare("main", ids, None).unwrap();
    f.core
        .ui_close_finish(
            plan["token"].as_str().unwrap(),
            serde_json::from_value(plan["views"].clone()).unwrap(),
            "discard",
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_pty_keeps_one_shell_across_views_and_rejects_old_surface_input() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let socks = Socks::new(target.addr, false).await;
    let proxy = f.proxy("127.0.0.1", socks.addr.port(), false);
    let server = f.server(
        "terminal.remote.invalid",
        22,
        json!([{"kind":"socks5","id":"","proxyId":proxy}]),
    );
    let context = connected(&f, &server).await.context.unwrap();
    let (view, token, id) = terminal_fixture(&f);
    let mut output = subscribe(&f.core, "main", &token, &id).await;
    let (_, offset) = data_until(&f.core, "main", &token, &id, &mut output, "shell-1>").await;
    assert_eq!(target.state.shells.load(Ordering::Relaxed), 1);
    td(
        &f.core,
        "main",
        &token,
        json!({"op":"write","id":id,"sequence":0,"data":b"hello\r"}),
    )
    .await
    .unwrap();
    let (_, offset2) = data_until(&f.core, "main", &token, &id, &mut output, "hello").await;
    assert!(offset2 > offset);
    assert!(td(
        &f.core,
        "main",
        &token,
        json!({"op":"write","id":id,"sequence":0,"data":b"duplicate"})
    )
    .await
    .is_err());
    td(
        &f.core,
        "main",
        &token,
        json!({"op":"resize","id":id,"cols":101,"rows":31}),
    )
    .await
    .unwrap();
    until(|| target.state.sizes.lock().unwrap().contains(&(101, 31))).await;
    td(&f.core,"main",&token,json!({"op":"checkpoint","id":id,"offset":offset2,"data":b"display-snapshot","metadata":{"cols":101,"rows":31},"begin":true,"done":true})).await.unwrap();
    let movement = {
        let mut w = f.core.workspaces.lock().unwrap();
        w.register("terminal-target").unwrap();
        w.ready("terminal-target").unwrap();
        w.prepare(
            "main",
            &context.workspace_id,
            "terminal-target",
            0,
            "terminal-move".into(),
        )
        .unwrap()
    };
    let mount = f.core.ui_attach("terminal-target", &view).unwrap();
    let next = mount["surfaceId"].as_str().unwrap();
    let mut migrated = subscribe(&f.core, "terminal-target", next, &id).await;
    let mut snapshot = vec![];
    loop {
        let frame = receive(&f.core, "terminal-target", next, &id, &mut migrated).await;
        if frame.kind == "snapshot" {
            snapshot.extend(frame.data);
        }
        if frame.kind == "caughtUp" {
            break;
        }
    }
    assert_eq!(snapshot, b"display-snapshot");
    assert!(td(
        &f.core,
        "terminal-target",
        next,
        json!({"op":"write","id":id,"sequence":0,"data":b"too-early"})
    )
    .await
    .is_err());
    f.core.ui_surface_ready("terminal-target", next).unwrap();
    f.core.workspaces.lock().unwrap().commit(&movement).unwrap();
    assert!(td(
        &f.core,
        "main",
        &token,
        json!({"op":"write","id":id,"sequence":1,"data":b"stale"})
    )
    .await
    .is_err());
    td(
        &f.core,
        "terminal-target",
        next,
        json!({"op":"write","id":id,"sequence":0,"data":b"after-move"}),
    )
    .await
    .unwrap();
    data_until(
        &f.core,
        "terminal-target",
        next,
        &id,
        &mut migrated,
        "after-move",
    )
    .await;
    assert_eq!(target.state.shells.load(Ordering::Relaxed), 1);
    td(
        &f.core,
        "terminal-target",
        next,
        json!({"op":"write","id":id,"sequence":1,"data":b"exit\r"}),
    )
    .await
    .unwrap();
    loop {
        let frame = receive(&f.core, "terminal-target", next, &id, &mut migrated).await;
        if frame.kind == "end" {
            assert_eq!(frame.metadata["exitCode"], 7);
            break;
        }
    }
    close_terminal_views(&f);
    assert!(td(
        &f.core,
        "terminal-target",
        next,
        json!({"op":"resize","id":id,"cols":80,"rows":24})
    )
    .await
    .is_err());
    f.core.shutdown().unwrap();
}
