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
async fn wait_file_batch(core: &Arc<AppCore>, token: &str, id: &str) -> Value {
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let snapshot = core
                .ui_event("main", token, json!({"action":"readTasks"}))
                .unwrap();
            let batch = snapshot["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["id"] == id)
                .unwrap()
                .clone();
            if ["completed", "partial", "failed", "cancelled", "needsReview"]
                .contains(&batch["state"].as_str().unwrap_or(""))
            {
                return batch;
            }
            tokio::time::sleep(Duration::from_millis(35)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!(
            "文件批次超时 {id}: {}",
            core.ui_event("main", token, json!({"action":"readTasks"}))
                .unwrap()
        )
    })
}
async fn file_operation(core: &Arc<AppCore>, token: &str, file: &str, operation: Value) -> Value {
    let request = format!("test-{}", rand::random::<u64>());
    let started = fd(
        core,
        token,
        file,
        json!({"action":"op.start","requestId":request,"operation":operation}),
    )
    .await
    .unwrap();
    let id = started["id"].as_str().unwrap();
    let duplicate = fd(
        core,
        token,
        file,
        json!({"action":"op.start","requestId":request,"operation":operation}),
    )
    .await
    .unwrap();
    assert_eq!(duplicate["id"], started["id"]);
    assert!(fd(
        core,
        token,
        file,
        json!({"action":"op.start","requestId":request,"operation":{"kind":"invalid"}})
    )
    .await
    .unwrap_err()
    .contains("重复操作参数不一致"));
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let state = fd(
                core,
                token,
                file,
                json!({"action":"op.poll","operationId":id}),
            )
            .await
            .unwrap();
            if !["running", "committing"].contains(&state["state"].as_str().unwrap_or("")) {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    fd(
        core,
        token,
        file,
        json!({"action":"op.release","operationId":id}),
    )
    .await
    .unwrap();
    assert!(fd(core,token,file,json!({"action":"op.start","requestId":format!("{request}-stale"),"sequence":"1","operation":operation})).await.unwrap_err().contains("序号已退役"));
    result
}
async fn conflict_choice(core: &Arc<AppCore>, token: &str, id: &str, choice: &str) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let snapshot = core
                .ui_event("main", token, json!({"action":"readTasks"}))
                .unwrap();
            if snapshot["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["id"] == id && t["state"] == "awaitingConflict")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    core.ui_event(
        "main",
        token,
        json!({"action":"resolveConflict","batchId":id,"choice":choice,"applyAll":true}),
    )
    .unwrap();
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
    f.core.ui_action("main", &context, json!({"op":"open","key":"sodeck.files:files","requestId":"files-fixture"})).unwrap();
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
    // Changed destination guards and malformed local components must preserve originals.
    let protected = f.path.join("protected.bin");
    std::fs::write(&protected, b"keep-original").unwrap();
    let reference = f
        .core
        .files_register_input("main", token, id, vec![protected.clone()], true)
        .unwrap();
    fd(
        &f.core,
        token,
        id,
        json!({"action":"input.claim","inputId":reference["inputId"]}),
    )
    .await
    .unwrap();
    let target = json!({"side":"local","inputId":reference["inputId"],"rootId":"0","relative":[]});
    let rejected=file_operation(&f.core,token,id,json!({"kind":"copyFile","source":{"side":"remote","path":large.to_str().unwrap().replace('\\',"/")},"destination":target,"destinationGuard":{"kind":"absent"}})).await;
    assert_eq!(rejected["state"], "failed", "{rejected}");
    assert_eq!(rejected["error"]["code"], "targetExists");
    assert_eq!(std::fs::read(&protected).unwrap(), b"keep-original");
    let inspected = file_operation(
        &f.core,
        token,
        id,
        json!({"kind":"inspect","endpoint":target}),
    )
    .await;
    std::fs::write(&protected, b"external-writer-changed").unwrap();
    let rejected=file_operation(&f.core,token,id,json!({"kind":"copyFile","source":{"side":"remote","path":large.to_str().unwrap().replace('\\',"/")},"destination":target,"destinationGuard":{"kind":"match","stamp":inspected["result"]["attrs"]["stamp"]}})).await;
    assert_eq!(rejected["error"]["code"], "targetChanged", "{rejected}");
    assert_eq!(
        std::fs::read(&protected).unwrap(),
        b"external-writer-changed"
    );
    let traversal=file_operation(&f.core,token,id,json!({"kind":"inspect","endpoint":{"side":"local","inputId":reference["inputId"],"rootId":"0","relative":[".."]}})).await;
    assert_eq!(traversal["error"]["code"], "invalid", "{traversal}");
    // Real external-process batches: directory upload/download, empty folders and binary bytes.
    let upload = f.path.join("本地 上传");
    std::fs::create_dir_all(upload.join("空目录")).unwrap();
    std::fs::create_dir_all(upload.join("nested")).unwrap();
    std::fs::write(
        upload.join("nested").join("data.bin"),
        vec![173u8; 2 * 1024 * 1024],
    )
    .unwrap();
    std::fs::write(upload.join(".hidden"), b"hidden").unwrap();
    std::fs::write(upload.join("empty.txt"), []).unwrap();
    let input = f
        .core
        .files_register_input("main", token, id, vec![upload.clone()], false)
        .unwrap();
    let batch = f.core.ui_event("main", token, json!({"action":"startBatch","parameters":{"kind":"upload","inputId":input["inputId"],"targetPath":remote_path}})).unwrap();
    let batch = wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await;
    assert_eq!(batch["state"], "completed", "{batch}");
    assert_eq!(batch["failed"], "0", "{batch}");
    let uploaded = remote.join("本地 上传");
    assert!(uploaded.join("空目录").is_dir());
    assert_eq!(
        std::fs::read(uploaded.join("nested/data.bin")).unwrap(),
        vec![173u8; 2 * 1024 * 1024]
    );
    assert_eq!(
        std::fs::metadata(uploaded.join("empty.txt")).unwrap().len(),
        0
    );
    let output = f.path.join("download-directory");
    std::fs::create_dir(&output).unwrap();
    let input = f
        .core
        .files_register_input("main", token, id, vec![output.clone()], true)
        .unwrap();
    let batch = f.core.ui_event("main", token, json!({"action":"startBatch","parameters":{"kind":"download","inputId":input["inputId"],"single":false,"roots":[uploaded.to_str().unwrap().replace('\\',"/")]}})).unwrap();
    let batch = wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await;
    assert_eq!(batch["state"], "completed", "{batch}");
    assert!(output.join("本地 上传/空目录").is_dir());
    assert_eq!(
        std::fs::read(output.join("本地 上传/nested/data.bin")).unwrap(),
        vec![173u8; 2 * 1024 * 1024]
    );
    // The same destination requires an explicit conflict choice; skip keeps the original.
    std::fs::write(upload.join(".hidden"), b"changed-source").unwrap();
    let input = f
        .core
        .files_register_input("main", token, id, vec![upload.join(".hidden")], false)
        .unwrap();
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"upload","inputId":input["inputId"],"targetPath":uploaded.to_str().unwrap().replace('\\',"/")}})).unwrap();
    let batch_id = batch["batchId"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let snapshot = f
                .core
                .ui_event("main", token, json!({"action":"readTasks"}))
                .unwrap();
            let task = snapshot["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["id"] == batch_id)
                .unwrap();
            if task["state"] == "awaitingConflict" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    f.core
        .ui_event(
            "main",
            token,
            json!({"action":"resolveConflict","batchId":batch_id,"choice":"skip"}),
        )
        .unwrap();
    let batch = wait_file_batch(&f.core, token, batch_id).await;
    assert_eq!(batch["skipped"], "1", "{batch}");
    assert_eq!(std::fs::read(uploaded.join(".hidden")).unwrap(), b"hidden");
    let input = f
        .core
        .files_register_input("main", token, id, vec![upload.join(".hidden")], false)
        .unwrap();
    fd(
        &f.core,
        token,
        id,
        json!({"action":"input.claim","inputId":input["inputId"]}),
    )
    .await
    .unwrap();
    let overwrite_target = json!({"side":"remote","path":uploaded.join(".hidden").to_str().unwrap().replace('\\',"/")});
    let stamp = file_operation(
        &f.core,
        token,
        id,
        json!({"kind":"inspect","endpoint":overwrite_target}),
    )
    .await;
    let replaced=file_operation(&f.core,token,id,json!({"kind":"copyFile","source":{"side":"local","inputId":input["inputId"],"rootId":"0","relative":[]},"destination":overwrite_target,"destinationGuard":{"kind":"match","stamp":stamp["result"]["attrs"]["stamp"]}})).await;
    assert_eq!(replaced["state"], "completed", "{replaced}");
    assert_eq!(
        std::fs::read(uploaded.join(".hidden")).unwrap(),
        b"changed-source"
    );
    // Remote mutations use the same batch queue and explicit conflict choices.
    let ops = remote.join("operations");
    let ops_path = ops.to_str().unwrap().replace('\\', "/");
    let batch = f
        .core
        .ui_event(
            "main",
            token,
            json!({"action":"startBatch","parameters":{"kind":"mkdir","path":ops_path}}),
        )
        .unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await["state"],
        "completed"
    );
    let empty = ops.join("new.txt");
    let empty_path = empty.to_str().unwrap().replace('\\', "/");
    let batch = f
        .core
        .ui_event(
            "main",
            token,
            json!({"action":"startBatch","parameters":{"kind":"createEmpty","path":empty_path}}),
        )
        .unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await["state"],
        "completed"
    );
    let renamed = ops.join("renamed.txt");
    let renamed_path = renamed.to_str().unwrap().replace('\\', "/");
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"rename","path":empty_path,"destination":renamed_path}})).unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await["state"],
        "completed"
    );
    assert!(!empty.exists());
    assert!(renamed.exists());
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"copy","roots":[renamed_path],"targetPath":ops_path}})).unwrap();
    let copy_id = batch["batchId"].as_str().unwrap();
    conflict_choice(&f.core, token, copy_id, "keepBoth").await;
    assert_eq!(
        wait_file_batch(&f.core, token, copy_id).await["state"],
        "completed"
    );
    assert!(ops.join("renamed (1).txt").exists());
    let move_source = remote.join("move-replace.txt");
    let move_target = ops.join("move-replace.txt");
    std::fs::write(&move_source, b"new move content").unwrap();
    std::fs::write(&move_target, b"old target").unwrap();
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"move","roots":[move_source.to_str().unwrap().replace('\\',"/")],"targetPath":ops_path}})).unwrap();
    let move_id = batch["batchId"].as_str().unwrap();
    conflict_choice(&f.core, token, move_id, "replace").await;
    let moved = wait_file_batch(&f.core, token, move_id).await;
    assert_eq!(moved["state"], "completed", "{moved}");
    assert!(!move_source.exists());
    assert_eq!(std::fs::read(move_target).unwrap(), b"new move content");
    // A directory larger than one page must visit each child exactly once.
    let many = remote.join("many");
    std::fs::create_dir(&many).unwrap();
    for index in 0..215 {
        std::fs::write(many.join(format!("{index}.txt")), index.to_string()).unwrap();
    }
    let many_path = many.to_str().unwrap().replace('\\', "/");
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"copy","roots":[many_path],"targetPath":ops_path}})).unwrap();
    let result = wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await;
    assert_eq!(result["state"], "completed", "{result}");
    assert_eq!(std::fs::read_dir(ops.join("many")).unwrap().count(), 215);
    let moved = remote.join("moved");
    std::fs::create_dir(&moved).unwrap();
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"move","roots":[ops.join("many").to_str().unwrap().replace('\\',"/")],"targetPath":moved.to_str().unwrap().replace('\\',"/")}})).unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await["state"],
        "completed"
    );
    assert!(!ops.join("many").exists());
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"delete","roots":[moved.to_str().unwrap().replace('\\',"/")]}})).unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await["state"],
        "completed"
    );
    assert!(!moved.exists());
    // Failed-only retry leaves previously successful files alone.
    let missing = f.path.join("missing-local.txt");
    let good = f.path.join("good-local.txt");
    std::fs::write(&good, b"first-copy").unwrap();
    let input = f
        .core
        .files_register_input("main", token, id, vec![missing.clone(), good], false)
        .unwrap();
    let batch=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"upload","inputId":input["inputId"],"targetPath":ops_path}})).unwrap();
    let failed = wait_file_batch(&f.core, token, batch["batchId"].as_str().unwrap()).await;
    assert_eq!(failed["state"], "partial", "{failed}");
    std::fs::write(&missing, b"retry-success").unwrap();
    std::fs::write(ops.join("good-local.txt"), b"external-after-success").unwrap();
    let retried = f
        .core
        .ui_event(
            "main",
            token,
            json!({"action":"retryFailed","batchId":failed["id"]}),
        )
        .unwrap();
    let retried = wait_file_batch(&f.core, token, retried["batchId"].as_str().unwrap()).await;
    assert_eq!(retried["state"], "completed", "{retried}");
    assert_eq!(
        std::fs::read(ops.join("good-local.txt")).unwrap(),
        b"external-after-success"
    );
    assert_eq!(
        std::fs::read(ops.join("missing-local.txt")).unwrap(),
        b"retry-success"
    );
    // Discard-close must also wake a plugin waiting for a conflict decision.
    let pending=f.core.ui_event("main",token,json!({"action":"startBatch","parameters":{"kind":"copy","roots":[renamed_path],"targetPath":ops_path}})).unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let state = f
                .core
                .ui_event("main", token, json!({"action":"readTasks"}))
                .unwrap();
            if state["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["id"] == pending["batchId"] && b["state"] == "awaitingConflict")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let close = f
        .core
        .ui_close_prepare(
            "main",
            vec![context.workspace_id.clone()],
            Some(browser.into()),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let progress = f
                .core
                .ui_close_poll("main", close["token"].as_str().unwrap(), false)
                .unwrap();
            if progress["waiting"].as_array().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    f.core
        .ui_close_finish(
            close["token"].as_str().unwrap(),
            vec![browser.into()],
            "cancel",
        )
        .unwrap();
    assert_eq!(
        wait_file_batch(&f.core, token, pending["batchId"].as_str().unwrap()).await["state"],
        "cancelled"
    );
    // Cancel a real streamed overwrite after the first bytes; the original stays intact.
    let cancel_source = f.path.join("cancel-source.bin");
    std::fs::write(&cancel_source, vec![71u8; 32 * 1024 * 1024]).unwrap();
    let cancel_target = remote.join("cancel-original.bin");
    std::fs::write(&cancel_target, b"original-before-cancel").unwrap();
    let input = f
        .core
        .files_register_input("main", token, id, vec![cancel_source], false)
        .unwrap();
    fd(
        &f.core,
        token,
        id,
        json!({"action":"input.claim","inputId":input["inputId"]}),
    )
    .await
    .unwrap();
    let destination =
        json!({"side":"remote","path":cancel_target.to_str().unwrap().replace('\\',"/")});
    let stamp = file_operation(
        &f.core,
        token,
        id,
        json!({"kind":"inspect","endpoint":destination}),
    )
    .await;
    let operation=fd(&f.core,token,id,json!({"action":"op.start","requestId":"cancel-stream-test","operation":{"kind":"copyFile","source":{"side":"local","inputId":input["inputId"],"rootId":"0","relative":[]},"destination":destination,"destinationGuard":{"kind":"match","stamp":stamp["result"]["attrs"]["stamp"]}}})).await.unwrap();
    let operation_id = operation["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = fd(
                &f.core,
                token,
                id,
                json!({"action":"op.poll","operationId":operation_id}),
            )
            .await
            .unwrap();
            assert_eq!(status["state"], "running", "{status}");
            if status["received"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                > 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    fd(
        &f.core,
        token,
        id,
        json!({"action":"op.cancel","operationId":operation_id}),
    )
    .await
    .unwrap();
    let cancelled = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let status = fd(
                &f.core,
                token,
                id,
                json!({"action":"op.poll","operationId":operation_id}),
            )
            .await
            .unwrap();
            if status["state"] != "running" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    assert_eq!(
        std::fs::read(&cancel_target).unwrap(),
        b"original-before-cancel"
    );
    assert!(!std::fs::read_dir(&remote)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".sodeck-")));
    fd(
        &f.core,
        token,
        id,
        json!({"action":"op.release","operationId":operation_id}),
    )
    .await
    .unwrap();
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
