#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires three isolated OpenSSH servers and actual MCP plugin; npm run mcp:test"]
async fn openssh_mcp_plugin(){
    let _ = rustls::crypto::ring::default_provider().install_default();
    let f=Fixture::new();
    let ports:Vec<u16>=serde_json::from_str(&std::env::var("SODECK_TEST_MCP_PORTS").unwrap()).unwrap();assert_eq!(ports.len(),3);
    let mut contexts=Vec::new();
    for port in ports{let server=f.server("127.0.0.1",port,json!([]));let mut draft=f.core.server_catalog()["servers"].as_array().unwrap().iter().find(|s|s["id"]==server).unwrap().clone();draft["username"]=json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});f.change(json!({"op":"saveServer","draft":draft}));contexts.push(connected(&f,&server).await.context.unwrap());}
    // Keep integration traffic separate from a running desktop MCP service.
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let mcp_port=listener.local_addr().unwrap().port();drop(listener);
    let plugin=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_PLUGIN").unwrap());f.core.add_plugin(&plugin).unwrap();let settings=f.core.settings_page("sodeck.mcp","connection").unwrap();assert_eq!(f.core.save_settings("sodeck.mcp","connection",json!({"requestId":"isolated-listener","definitionToken":settings["definitionToken"],"baseRevision":settings["revision"],"values":{"port":mcp_port}})).unwrap()["persisted"],true);f.core.load(&plugin).unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(8);
    loop{let views=f.core.ui_state().workspaces.values().flat_map(|w|w.views.iter()).filter(|v|v.plugin_id=="sodeck.mcp").count();if views==3{break;}assert!(std::time::Instant::now()<deadline,"background views did not start");tokio::time::sleep(Duration::from_millis(25)).await;}
    let token=std::fs::read_to_string(f.path.join("plugin-data/sodeck.mcp/access-token")).unwrap();
    let client=reqwest::Client::new();let local_url=format!("http://127.0.0.1:{mcp_port}/mcp");let url=local_url.as_str();
    async fn call(client:&reqwest::Client,url:&str,token:&str,method:&str,params:Value)->Value{
        let response=client.post(url).bearer_auth(token).header("Accept","application/json, text/event-stream").header("MCP-Protocol-Version","2025-11-25").json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).send().await.unwrap();let code=response.status();let text=response.text().await.unwrap();assert_eq!(code,200,"{text}");serde_json::from_str::<Value>(&text).unwrap()["result"].clone()
    }
    async fn tool(client:&reqwest::Client,url:&str,token:&str,name:&str,p:Value)->Value{let r=call(client,url,token,"tools/call",json!({"name":name,"arguments":p})).await;assert_ne!(r["isError"],true,"{r}");r["structuredContent"].clone()}
    async fn done(client:&reqwest::Client,url:&str,token:&str,id:&Value)->Value{let end=std::time::Instant::now()+Duration::from_secs(30);let mut revision=Value::Null;loop{let mut p=json!({"taskId":id,"outputMode":"none","waitMs":2000});if !revision.is_null(){p["sinceRevision"]=revision;}let result=tool(client,url,token,"sodeck_task_get",p).await;if result["done"]==true{return tool(client,url,token,"sodeck_task_get",json!({"taskId":id,"outputMode":"text"})).await;}revision=result["revision"].clone();assert!(std::time::Instant::now()<end,"{result}");}}
    let end=std::time::Instant::now()+Duration::from_secs(5);loop{if client.post(url).send().await.is_ok(){break;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(25)).await;}
    call(&client,url,&token,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"SoDeck integration","version":"1"}})).await;
    let end=std::time::Instant::now()+Duration::from_secs(8);let targets=loop{let targets=tool(&client,url,&token,"sodeck_targets_list",json!({})).await;if targets["targets"].as_array().unwrap().len()==3{break targets;}assert!(std::time::Instant::now()<end,"background registration failed: {targets}");tokio::time::sleep(Duration::from_millis(50)).await;};
    assert!(!f.core.ui_state().definitions.contains_key("sodeck.mcp:operations"),"MCP has no central-page contribution");
    let central_before={let state=f.core.ui_state();let host=state.workspaces[&contexts[0].workspace_id].host("main").unwrap();json!({"center":host.center,"groups":host.groups,"focusedGroup":host.focused_group_id})};
    assert!(f.core.ui_action("main",&contexts[0],json!({"op":"open","key":"sodeck.mcp:operations","requestId":"old-central-page"})).is_err());
    let opened=f.core.ui_action("main",&contexts[0],json!({"op":"toggle","key":"sodeck.mcp:status","requestId":"show-sidebar"})).unwrap();let page=opened["viewId"].as_str().unwrap();
    let attached=f.core.ui_attach("main",page).unwrap();let surface=attached["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",surface).unwrap();
    let target=targets["targets"].as_array().unwrap().iter().find(|t|t["context"]["workspaceId"]==contexts[0].workspace_id).unwrap();
    assert_eq!(attached["data"]["target"]["targetId"],target["targetId"]);assert!(attached["data"].get("token").is_none());
    assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3,"showing the sidebar does not duplicate background targets");
    let job=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"page-lifecycle","command":if cfg!(windows){"powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 2; Write-Output sodeck-page-survived\""}else{"sleep 2; printf sodeck-page-survived"}})).await;
    let detail=f.core.ui_event("main",surface,json!({"action":"task","taskId":job["taskId"],"cursor":"0"})).unwrap();assert_eq!(detail["taskId"],job["taskId"]);assert_eq!(detail["done"],false);
    f.core.ui_action("main",&contexts[0],json!({"op":"hideTool","viewId":page})).unwrap();
    let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-page-survived"));
    let reopened=f.core.ui_action("main",&contexts[0],json!({"op":"toggle","key":"sodeck.mcp:status","requestId":"show-sidebar-again"})).unwrap();assert_eq!(reopened["viewId"],page,"hiding the sidebar retains its background view");let attached=f.core.ui_attach("main",page).unwrap();
    assert!(attached["data"]["tasks"].as_array().unwrap().iter().any(|t|t["taskId"]==job["taskId"]));assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3);
    let central_after={let state=f.core.ui_state();let host=state.workspaces[&contexts[0].workspace_id].host("main").unwrap();json!({"center":host.center,"groups":host.groups,"focusedGroup":host.focused_group_id})};assert_eq!(central_after,central_before,"MCP sidebar lifecycle leaves central groups and selection unchanged");
    let remote=f.path.join("mcp evidence 中文.bin");let bytes:Vec<u8>=(0..150000).map(|n|n as u8).collect();std::fs::write(&remote,&bytes).unwrap();
    let command=if cfg!(windows){"cmd /c echo sodeck-first-collection"}else{"printf 'sodeck-first-collection\\n'"};
    let mut jobs=Vec::new();for target in targets["targets"].as_array().unwrap(){let input=json!({"targetId":target["targetId"],"requestId":"first","command":command});let result=tool(&client,url,&token,"sodeck_exec_start",input.clone()).await;assert_eq!(tool(&client,url,&token,"sodeck_exec_start",input).await["taskId"],result["taskId"]);jobs.push((target.clone(),result));}
    for (target,job) in jobs{
        let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert_eq!(result["evidence"]["complete"],true);assert_eq!(result["resourceReleased"],true);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-first-collection"));
        assert!(result["output"].get("chunks").is_none());
        let found=tool(&client,url,&token,"sodeck_tasks_list",json!({"targetId":target["targetId"],"requestId":"first","status":"done"})).await;assert_eq!(found["tasks"].as_array().unwrap().len(),1);assert_eq!(found["tasks"][0]["taskId"],job["taskId"]);
        let none=tool(&client,url,&token,"sodeck_task_get",json!({"taskId":job["taskId"],"outputMode":"none","waitMs":2000})).await;assert!(none.get("output").is_none());assert_eq!(none["wait"]["reason"],"done");assert_eq!(none["revision"],result["revision"]);
        let follow=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"follow","command":if cfg!(windows){"cmd /c echo sodeck-followup"}else{"printf sodeck-followup"}})).await;assert_eq!(done(&client,url,&token,&follow["taskId"]).await["evidence"]["complete"],true);
        let download=tool(&client,url,&token,"sodeck_file_download",json!({"targetId":target["targetId"],"requestId":"evidence","remotePath":remote.to_str().unwrap().replace('\\',"/")})).await;let result=done(&client,url,&token,&download["taskId"]).await;assert_eq!(result["native"]["state"],"completed","{result}");assert_eq!(result["evidence"]["complete"],true);assert_eq!(std::fs::read(result["evidence"]["path"].as_str().unwrap()).unwrap(),bytes);
    }
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX"){
        let command=if cfg!(windows){"cmd /c echo sodeck-codex-acceptance"}else{"printf sodeck-codex-acceptance"};
        let mut child=std::process::Command::new(codex).args(["exec","--ephemeral","--skip-git-repo-check","--json","-s","danger-full-access","-C",f.path.to_str().unwrap(),"-c",&format!("mcp_servers={{sodeck={{url=\"{url}\",bearer_token_env_var=\"SODECK_MCP_TOKEN\"}}}}"),&format!("Validate the SoDeck MCP integration. Use only the sodeck MCP tools. List current targets. Choose one target explicitly and start this harmless command with requestId codex-acceptance: {command}. Query its task until done, verify exitCode 0, evidence.complete true and stdout containing sodeck-codex-acceptance. Then answer MCP_ACCEPTANCE_OK. Do not use shell tools or read any local files.")]).env("SODECK_MCP_TOKEN",&token).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        let mut out=child.stdout.take().unwrap();let mut err=child.stderr.take().unwrap();let stdout_thread=std::thread::spawn(move||{use std::io::Read;let mut v=Vec::new();out.read_to_end(&mut v).unwrap();v});let stderr_thread=std::thread::spawn(move||{use std::io::Read;let mut v=Vec::new();err.read_to_end(&mut v).unwrap();v});
        let end=std::time::Instant::now()+Duration::from_secs(180);let status=loop{if let Some(status)=child.try_wait().unwrap(){break status;}if std::time::Instant::now()>end{let _=child.kill();panic!("Codex acceptance timed out");}tokio::time::sleep(Duration::from_millis(100)).await;};let output=std::process::Output{status,stdout:stdout_thread.join().unwrap(),stderr:stderr_thread.join().unwrap()};
        let stdout=String::from_utf8_lossy(&output.stdout).replace(&token,"[redacted]");let stderr=String::from_utf8_lossy(&output.stderr).replace(&token,"[redacted]");
        let output_directory=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_OUTPUT").unwrap_or_else(|_|f.path.join("codex-output").to_string_lossy().into_owned()));std::fs::create_dir_all(&output_directory).unwrap();std::fs::write(output_directory.join("codex-acceptance.jsonl"),&stdout).unwrap();
        assert!(output.status.success(),"Codex failed: {stderr}\n{stdout}");
        for tool in ["sodeck_targets_list","sodeck_exec_start","sodeck_task_get"]{assert!(stdout.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).any(|v|v["item"]["type"]=="mcp_tool_call"&&v["item"]["tool"]==tool&&v["item"]["status"]=="completed"),"missing actual Codex call {tool}: {stdout}");}
        assert!(stdout.contains("MCP_ACCEPTANCE_OK"),"{stdout}");
    }
    let plan=f.core.ui_close_prepare("main",contexts.iter().map(|c|c.workspace_id.clone()).collect(),None).unwrap();f.core.ui_close_finish(plan["token"].as_str().unwrap(),serde_json::from_value(plan["views"].clone()).unwrap(),"discard").unwrap();
    f.core.unload("sodeck.mcp").unwrap();assert!(client.post(url).send().await.is_err());f.core.shutdown().unwrap();
}
