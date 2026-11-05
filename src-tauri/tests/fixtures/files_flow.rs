// Independent OpenSSH SFTP through the real external file plugin.
async fn fd(core: &Arc<AppCore>, token: &str, id: &str, mut p: Value) -> Result<Value, String> {
    p["id"] = json!(id);
    core.files_data("main", token, p).await
}
async fn job(core: &Arc<AppCore>, token: &str, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let v = fd(core, token, id, json!({"action":"status"}))
                .await
                .unwrap();
            if v["state"] != "running" {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}
fn file_editor(
    f: &Fixture,
    context: &sodeck::workspaces::Context,
    remote: &str,
) -> (String, String) {
    let created = f
        .core
        .ui_action(
            "main",
            context,
            json!({
                "op":"open", "key":"sodeck.files:editor", "requestId":remote,
                "resourceKey":remote, "params":{"path":remote}
            }),
        )
        .unwrap();
    let mounted = f
        .core
        .ui_attach("main", created["viewId"].as_str().unwrap())
        .unwrap();
    let token = mounted["surfaceId"].as_str().unwrap().to_string();
    f.core.ui_surface_ready("main", &token).unwrap();
    (
        token,
        mounted["data"]["fileId"].as_str().unwrap().to_string(),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated OpenSSH and actual files package; run npm run files:test"]
async fn openssh_files_plugin() {
    let f = Fixture::new();
    let remote = f.path.join("remote 中文");
    std::fs::create_dir_all(&remote).unwrap();
    let document = remote.join("设置 file.txt");
    let original = "\u{feff}hello\r\n中文\r\n".as_bytes();
    std::fs::write(&document, original).unwrap();
    let large = remote.join("large.bin");
    std::fs::write(&large, vec![42u8; 2 * 1024 * 1024]).unwrap();
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
            &std::env::var("SODECK_TEST_FILES_PLUGIN").unwrap(),
        ))
        .unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    let browser = state["workspaces"][&context.workspace_id]["left"]
        .as_str()
        .unwrap();
    let mounted = f.core.ui_attach("main", browser).unwrap();
    let token = mounted["surfaceId"].as_str().unwrap();
    let id = mounted["data"]["fileId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    let remote_path = remote.to_str().unwrap().replace('\\', "/");
    let path = document.to_str().unwrap().replace('\\', "/");
    let entries = fd(
        &f.core,
        token,
        id,
        json!({"action":"list","path":remote_path}),
    )
    .await
    .unwrap();
    assert!(entries["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["name"] == "设置 file.txt"));
    let created=f.core.ui_action("main",&context,json!({"op":"open","key":"sodeck.files:editor","requestId":"edit-file","resourceKey":path,"params":{"path":path}})).unwrap();
    let editor = created["viewId"].as_str().unwrap();
    let mount = f.core.ui_attach("main", editor).unwrap();
    let token = mount["surfaceId"].as_str().unwrap();
    let id = mount["data"]["fileId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    let read = fd(&f.core, token, id, json!({"action":"read","path":path}))
        .await
        .unwrap();
    assert_eq!(read["size"], original.len());
    let chunk = fd(&f.core, token, id, json!({"action":"chunk","offset":0}))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(chunk["data"].clone()).unwrap(),
        original
    );
    let replacement = "\u{feff}changed\r\n中文\r\n".as_bytes();
    fd(
        &f.core,
        token,
        id,
        json!({"action":"stage","begin":true,"done":true,"offset":0,"data":replacement}),
    )
    .await
    .unwrap();
    f.core
        .ui_event("main", token, json!({"action":"staged","revision":1}))
        .unwrap();
    fd(&f.core, token, id, json!({"action":"save"}))
        .await
        .unwrap();
    let saved = job(&f.core, token, id).await;
    assert_eq!(saved["state"], "completed", "{saved}");
    assert_eq!(std::fs::read(&document).unwrap(), replacement);
    std::fs::write(&document, b"external-change").unwrap();
    fd(&f.core,token,id,json!({"action":"stage","begin":true,"done":true,"offset":0,"data":b"should-not-overwrite"})).await.unwrap();
    fd(&f.core, token, id, json!({"action":"save"}))
        .await
        .unwrap();
    let failed = job(&f.core, token, id).await;
    assert_eq!(failed["state"], "failed");
    assert!(failed["error"].as_str().unwrap().contains("已被修改"));
    assert_eq!(std::fs::read(&document).unwrap(), b"external-change");
    let local = f.path.join("download.bin");
    std::fs::write(&local, b"local-original").unwrap();
    f.core
        .files_download(
            "main",
            token,
            json!({"id":id,"path":large.to_str().unwrap().replace('\\',"/")}),
            local.clone(),
        )
        .await
        .unwrap();
    let downloaded = job(&f.core, token, id).await;
    assert_eq!(downloaded["state"], "completed", "{downloaded}");
    assert_eq!(std::fs::read(local).unwrap(), vec![42u8; 2 * 1024 * 1024]);
    // Cancel only after remote bytes have arrived, while the original local file
    // is still present. A new download on this same view must reopen SFTP.
    let cancel_remote = remote.join("cancel.bin");
    std::fs::File::create(&cancel_remote)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();
    let cancel_local = f.path.join("cancel-download.bin");
    std::fs::write(&cancel_local, b"keep-local-original").unwrap();
    f.core
        .files_download(
            "main",
            token,
            json!({"id":id,"path":cancel_remote.to_str().unwrap().replace('\\',"/")}),
            cancel_local.clone(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let v = fd(&f.core, token, id, json!({"action":"status"}))
                .await
                .unwrap();
            assert_eq!(
                v["state"], "running",
                "download finished before cancellation: {v}"
            );
            if v["received"].as_str().unwrap().parse::<u64>().unwrap() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    fd(&f.core, token, id, json!({"action":"cancel"}))
        .await
        .unwrap();
    let cancelled = job(&f.core, token, id).await;
    assert_eq!(cancelled["state"], "failed", "{cancelled}");
    assert!(cancelled["error"].as_str().unwrap().contains("取消"));
    assert_eq!(
        std::fs::read(&cancel_local).unwrap(),
        b"keep-local-original"
    );
    assert!(!std::fs::read_dir(&f.path).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".part")));
    f.core
        .files_download(
            "main",
            token,
            json!({"id":id,"path":path}),
            cancel_local.clone(),
        )
        .await
        .unwrap();
    let retried = job(&f.core, token, id).await;
    assert_eq!(retried["state"], "completed", "{retried}");
    assert_eq!(std::fs::read(cancel_local).unwrap(), b"external-change");
    // Exact editor limit is supported; an oversized read/stage never replaces
    // the last complete local draft, including when a chunked stage is aborted.
    const LIMIT: usize = 1024 * 1024;
    let boundary = remote.join("boundary.txt");
    std::fs::write(&boundary, vec![b'x'; LIMIT]).unwrap();
    let boundary = boundary.to_str().unwrap().replace('\\', "/");
    let (bt, bi) = file_editor(&f, &context, &boundary);
    let exact = fd(&f.core, &bt, &bi, json!({"action":"read", "path":boundary}))
        .await
        .unwrap();
    assert_eq!(exact["size"], LIMIT);
    assert!(fd(
        &f.core,
        &bt,
        &bi,
        json!({"action":"stage", "begin":true, "done":true, "offset":0, "data":vec![0u8; 16385]})
    )
    .await
    .unwrap_err()
    .contains("16 KiB"));
    for offset in (0..LIMIT).step_by(16384) {
        fd(&f.core, &bt, &bi, json!({"action":"stage", "begin":offset==0, "done":false, "offset":offset, "data":vec![b'y'; 16384]})).await.unwrap();
    }
    assert!(fd(
        &f.core,
        &bt,
        &bi,
        json!({"action":"stage", "done":true, "offset":LIMIT, "data":[121]})
    )
    .await
    .unwrap_err()
    .contains("1 MiB"));
    let intact = fd(&f.core, &bt, &bi, json!({"action":"chunk", "offset":0}))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(intact["data"].clone()).unwrap(),
        vec![b'x'; 16384]
    );
    let too_large = remote.join("too-large.txt");
    std::fs::write(&too_large, vec![b'x'; LIMIT + 1]).unwrap();
    let too_large = too_large.to_str().unwrap().replace('\\', "/");
    let (ot, oi) = file_editor(&f, &context, &too_large);
    assert!(fd(
        &f.core,
        &ot,
        &oi,
        json!({"action":"read", "path":too_large})
    )
    .await
    .unwrap_err()
    .contains("1 MiB"));
    let metrics = f.core.network_snapshot();
    let payload = metrics["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pluginId"] == "sodeck.files")
        .unwrap();
    assert!(
        payload["received"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 2 * 1024 * 1024
    );
    f.core.disconnect_connection("main", &context).unwrap();
    let draft = fd(&f.core, token, id, json!({"action":"chunk","offset":0}))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(draft["data"].clone()).unwrap(),
        b"should-not-overwrite"
    );
    let plan = f
        .core
        .ui_close_prepare("main", vec![context.workspace_id], None)
        .unwrap();
    f.core
        .ui_close_finish(
            plan["token"].as_str().unwrap(),
            serde_json::from_value(plan["views"].clone()).unwrap(),
            "discard",
        )
        .unwrap();
    f.core.shutdown().unwrap();
}
