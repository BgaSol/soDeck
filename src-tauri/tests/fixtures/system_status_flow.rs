#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn environment_json_limit_fails_without_another_probe(){
    use base64::{engine::general_purpose::STANDARD, Engine};
    let f=Fixture::new();let target=SshServer::new().await;
    let id=f.server("127.0.0.1",target.addr.port(),json!([]));
    let context=connected(&f,&id).await.context.unwrap();let (view,token,_)=terminal_fixture(&f);
    let response=format!("SODECK_ENV_1\nwindows\n{}\n{}\n",STANDARD.encode("powershell.exe"),STANDARD.encode("\u{1}".repeat(2800)));
    assert!(response.len()<16*1024);
    *target.state.environment_response.lock().unwrap()=Some(response.into_bytes());
    let invoke=|method:&str,p:Value|f.core.ui_event("main",&token,json!({"rpc":method,"params":p})).unwrap();
    let started=invoke("session.environment.start",json!({"viewId":view,"expectedContext":context,"operationId":"environment","sequence":"1","keys":["DOCKER_HOST"],"timeoutMs":3000}));
    let task=started["taskId"].as_str().unwrap();let end=std::time::Instant::now()+Duration::from_secs(4);
    let output=loop{let output=invoke("session.exec.poll",json!({"taskId":task,"stdoutOffset":"0","stderrOffset":"0"}));
        if output["state"]!="running"{break output;}assert!(std::time::Instant::now()<end,"{output}");tokio::time::sleep(Duration::from_millis(25)).await;};
    assert_eq!(output["state"],"failed","{output}");assert_eq!(output["error"]["code"],"outputLimit","{output}");
    assert_eq!(output["stdout"]["nextOffset"],"0");assert_eq!(output["stderr"]["nextOffset"],"0");
    assert_eq!(output["outputComplete"],true);assert_eq!(target.state.environment_probes.load(Ordering::Relaxed),1);
    invoke("session.exec.release",json!({"taskId":task}));close_terminal_views(&f);f.core.shutdown().unwrap();
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
#[ignore="requires isolated OpenSSH and actual status package; npm run system-status:test"]
async fn openssh_system_status_plugin(){
    let f=Fixture::new();let port=std::env::var("SODECK_TEST_SSH_PORT").unwrap().parse().unwrap();
    let server=f.server("127.0.0.1",port,json!([]));let mut draft=f.core.server_catalog()["servers"][0].clone();draft["username"]=json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});f.change(json!({"op":"saveServer","draft":draft}));
    let package=std::path::PathBuf::from(std::env::var("SODECK_TEST_SYSTEM_STATUS_PLUGIN").unwrap());f.core.load(&package).unwrap();
    let context=connected(&f,&server).await.context.unwrap();
    // The coordinator must create and mount the logical view without ui_snapshot or attach.
    let end=std::time::Instant::now()+Duration::from_secs(8);let view=loop {let ui=f.core.ui_state();let found=ui.workspaces.get(&context.workspace_id).and_then(|w|w.views.first()).map(|v|v.id.clone());drop(ui);if let Some(v)=found{break v;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(50)).await;};
    let state=f.core.ui_snapshot("main").unwrap();assert_eq!(state["workspaces"][&context.workspace_id]["right"],view);
    let mount=f.core.ui_attach("main",&view).unwrap();let token=mount["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",token).unwrap();
    let end=std::time::Instant::now()+Duration::from_secs(45);
    let first=loop {let s=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();
        let observed=s["groups"].as_array().is_some_and(|groups|groups.iter().all(|g|g["observedAt"].is_number()||g["error"]=="此系统暂未提供可靠的数据源"));
        let healthy=s["data"]["sourceErrors"].as_object().is_some_and(|groups|groups.values().all(|g|g.as_object().is_some_and(|g|g.is_empty())));
        if observed&&healthy&&s["data"]["cpu"]["state"]=="ok" {break s;}
        assert!(std::time::Instant::now()<end,"expected each status source to recover: {s}");tokio::time::sleep(Duration::from_millis(100)).await;};
    assert!(first["data"]["identity"]["hostname"].is_string(),"{first}");
    for source in ["interfaces","network","disk"]{assert_eq!(first["data"]["sources"][source]["state"],"ok","{source}: {first}");}
    if first["family"]=="Linux" {let interfaces=f.core.ui_event("main",token,json!({"action":"list","kind":"interfaces"})).unwrap();assert!(interfaces["items"].as_array().unwrap().iter().any(|i|i["addresses"].as_array().is_some_and(|a|!a.is_empty())),"{interfaces}");}
    let before=first["groups"][1]["observedAt"].as_u64().expect("dynamic sample");
    let raw=f.core.ui_event("main",token,json!({"action":"list","kind":"filesystems"})).unwrap();
    let overview=f.core.ui_event("main",token,json!({"action":"list","kind":"filesystems","group":"overview"})).unwrap();
    assert!(raw["total"].as_u64().unwrap()>0,"capacity sample missing: {raw}");
    assert!(first["filesystemSummary"]["primary"].is_number(),"{first}");
    assert!(overview["total"].as_u64().unwrap()<=raw["total"].as_u64().unwrap());
    for row in overview["items"].as_array().unwrap(){assert_eq!(row["displayGroup"],"primary");assert!(row["mountKey"].is_string());assert!(row["spaceWarning"].is_boolean());}
    f.core.ui_action("main",&context,json!({"op":"collapse","side":"right"})).unwrap();tokio::time::sleep(Duration::from_secs(7)).await;
    let later=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();assert!(later["groups"][1]["observedAt"].as_u64().unwrap()>before,"hidden panel stopped sampling");
    f.core.ui_event("main",token,json!({"action":"pause"})).unwrap();tokio::time::sleep(Duration::from_secs(1)).await;let paused=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();assert_eq!(paused["paused"],true);
    f.core.ui_event("main",token,json!({"action":"resume"})).unwrap();tokio::time::sleep(Duration::from_secs(7)).await;let resumed=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();assert_eq!(resumed["paused"],false);assert!(resumed["groups"][1]["observedAt"].as_u64().unwrap()>later["groups"][1]["observedAt"].as_u64().unwrap(),"resume did not restart sampling");
    let plan=f.core.ui_close_prepare("main",vec![context.workspace_id.clone()],None).unwrap();assert!(plan["dirty"].as_array().unwrap().iter().all(|v|v["dirty"]!=true && v["busy"]!=true));f.core.ui_close_finish(plan["token"].as_str().unwrap(),serde_json::from_value(plan["views"].clone()).unwrap(),"discard").unwrap();f.core.cancel_workspace_connection(&context.workspace_id);f.core.shutdown().unwrap();
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn session_exec_retries_do_not_replay_and_poll_is_bounded(){
    let f=Fixture::new();let target=SshServer::new().await;let id=f.server("127.0.0.1",target.addr.port(),json!([]));let context=connected(&f,&id).await.context.unwrap();let (_,token,terminal)=terminal_fixture(&f);
    let params=json!({"expectedContext":context,"operationId":"first","sequence":"1","command":"sample","timeoutMs":1000,"maxOutputBytes":256});
    let invoke=|method:&str,p:Value|f.core.ui_event("main",&token,json!({"rpc":method,"params":p})).unwrap();
    let first=invoke("session.exec.start",params.clone());let repeat=invoke("session.exec.start",params.clone());assert_eq!(first["taskId"],repeat["taskId"]);
    let task=first["taskId"].as_str().unwrap();tokio::time::sleep(Duration::from_millis(100)).await;
    let poll=json!({"taskId":task,"stdoutOffset":"0","stderrOffset":"0"});let output=invoke("session.exec.poll",poll.clone());assert_eq!(output["stdout"]["dataBase64"],"c2FtcGxl");assert_eq!(output["state"],"completed");assert_eq!(output["outputComplete"],true);assert_eq!(output,invoke("session.exec.poll",poll));
    invoke("session.exec.release",json!({"taskId":task}));assert_eq!(invoke("session.exec.start",params)["rpcError"]["data"]["code"],"operationExpired");
    assert_eq!(invoke("session.exec.start",json!({"expectedContext":context,"operationId":"bad","sequence":"2","command":"sample","timeoutMs":0}))["rpcError"]["data"]["code"],"invalidParams");
    let _=terminal;close_terminal_views(&f);f.core.shutdown().unwrap();
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn session_exec_bounds_streams_and_cancels_without_losing_shared_connection(){
    let f=Fixture::new();let target=SshServer::new().await;let id=f.server("127.0.0.1",target.addr.port(),json!([]));let context=connected(&f,&id).await.context.unwrap();let (_,token,_)=terminal_fixture(&f);
    let invoke=|method:&str,p:Value|f.core.ui_event("main",&token,json!({"rpc":method,"params":p})).unwrap();
    let start=|sequence:u64,command:&str,budget:usize,timeout:u64|invoke("session.exec.start",json!({"expectedContext":context,"operationId":sequence.to_string(),"sequence":sequence.to_string(),"command":command,"timeoutMs":timeout,"maxOutputBytes":budget}))["taskId"].as_str().unwrap().to_string();
    let poll=|task:&str,a:&str,b:&str|invoke("session.exec.poll",json!({"taskId":task,"stdoutOffset":a,"stderrOffset":b}));
    let terminal=|task:String|{let poll=&poll;async move {let end=std::time::Instant::now()+Duration::from_secs(4);loop{let r=poll(&task,"0","0");if r["state"]!="running"{break r;}assert!(std::time::Instant::now()<end,"{r}");tokio::time::sleep(Duration::from_millis(25)).await;}}};
    let task=start(1,"status-test:mixed",20000,1000);let output=terminal(task.clone()).await;
    assert_eq!(output["state"],"completed");assert_eq!(output["exitCode"],7);assert_eq!(output["stdout"]["nextOffset"],"8192");assert_eq!(output["stderr"]["dataBase64"],"ZXJyCg==");assert_eq!(output["outputComplete"],false);
    let rest=poll(&task,"8192","4");assert_eq!(rest["stdout"]["nextOffset"],"9000");assert_eq!(rest["outputComplete"],true);assert_eq!(poll(&task,"9001","4")["rpcError"]["data"]["code"],"invalidOffset");invoke("session.exec.release",json!({"taskId":task}));
    for (n,command,budget,state,code) in [(2,"status-test:missing-exit",1000,"failed","missingExitStatus"),(3,"status-test:limit",256,"failed","outputLimit"),(4,"status-test:hang",256,"timedOut","timedOut")] {
        let task=start(n,command,budget,1000);let output=terminal(task.clone()).await;assert_eq!(output["state"],state,"{output}");assert_eq!(output["error"]["code"],code,"{output}");invoke("session.exec.release",json!({"taskId":task}));
    }
    let task=start(5,"status-test:hang",256,30000);invoke("session.exec.cancel",json!({"taskId":task}));assert_eq!(terminal(task.clone()).await["state"],"cancelled");invoke("session.exec.release",json!({"taskId":task}));
    let task=start(6,"sample",256,1000);assert_eq!(terminal(task.clone()).await["state"],"completed");assert_eq!(target.state.auth.load(Ordering::Relaxed),1,"cancel caused a new authentication");invoke("session.exec.release",json!({"taskId":task}));
    let task=start(7,"status-test:binary",30000,1000);let output=terminal(task.clone()).await;
    assert_eq!(output["state"],"completed");
    let (mut a,mut b)=(0usize,0usize);let (mut stdout,mut stderr)=(vec![],vec![]);
    loop {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let page=poll(&task,&a.to_string(),&b.to_string());
        assert_eq!(page,poll(&task,&a.to_string(),&b.to_string()));
        assert!(serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":page})).unwrap().len()<64*1024);
        let out=STANDARD.decode(page["stdout"]["dataBase64"].as_str().unwrap()).unwrap();
        let err=STANDARD.decode(page["stderr"]["dataBase64"].as_str().unwrap()).unwrap();
        assert!(out.len()<=8192 && err.len()<=8192);
        a+=out.len();b+=err.len();stdout.extend(out);stderr.extend(err);
        assert_eq!(page["stdout"]["nextOffset"],a.to_string());assert_eq!(page["stderr"]["nextOffset"],b.to_string());
        assert_eq!(page["outputComplete"],a==16385 && b==8193);
        if page["outputComplete"]==true { break; }
    }
    assert_eq!(stdout,(0..16385).map(|i|i as u8).collect::<Vec<_>>());
    assert_eq!(stderr,(0..8193).map(|i|(255-i%256) as u8).collect::<Vec<_>>());
    invoke("session.exec.release",json!({"taskId":task}));
    close_terminal_views(&f);f.core.shutdown().unwrap();
}
