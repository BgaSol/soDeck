#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires three isolated OpenSSH servers and actual MCP plugin; npm run mcp:test"]
async fn openssh_mcp_plugin(){
    let _ = rustls::crypto::ring::default_provider().install_default();
    let f=Fixture::new();
    let ports:Vec<u16>=serde_json::from_str(&std::env::var("SODECK_TEST_MCP_PORTS").unwrap()).unwrap();assert_eq!(ports.len(),3);
    let mut contexts=Vec::new();
    for port in ports{let server=f.server("127.0.0.1",port,json!([]));let mut draft=f.core.server_catalog()["servers"].as_array().unwrap().iter().find(|s|s["id"]==server).unwrap().clone();draft["username"]=json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});f.change(json!({"op":"saveServer","draft":draft}));contexts.push(connected(&f,&server).await.context.unwrap());}
    let plugin=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_PLUGIN").unwrap());f.core.load(&plugin).unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(8);
    loop{let views=f.core.ui_state().workspaces.values().flat_map(|w|w.views.iter()).filter(|v|v.plugin_id=="sodeck.mcp").count();if views==3{break;}assert!(std::time::Instant::now()<deadline,"background views did not start");tokio::time::sleep(Duration::from_millis(25)).await;}
    let token=std::fs::read_to_string(f.path.join("plugin-data/sodeck.mcp/access-token")).unwrap();
    let client=reqwest::Client::new();let url="http://127.0.0.1:19846/mcp";
    async fn call(client:&reqwest::Client,url:&str,token:&str,method:&str,params:Value)->Value{
        let response=client.post(url).bearer_auth(token).header("Accept","application/json, text/event-stream").header("MCP-Protocol-Version","2025-11-25").json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).send().await.unwrap();let code=response.status();let text=response.text().await.unwrap();assert_eq!(code,200,"{text}");serde_json::from_str::<Value>(&text).unwrap()["result"].clone()
    }
    async fn tool(client:&reqwest::Client,url:&str,token:&str,name:&str,p:Value)->Value{let r=call(client,url,token,"tools/call",json!({"name":name,"arguments":p})).await;assert_ne!(r["isError"],true,"{r}");r["structuredContent"].clone()}
    async fn done(client:&reqwest::Client,url:&str,token:&str,id:&Value)->Value{let end=std::time::Instant::now()+Duration::from_secs(30);loop{let result=tool(client,url,token,"sodeck_task_get",json!({"taskId":id})).await;if result["done"]==true{return result;}assert!(std::time::Instant::now()<end,"{result}");tokio::time::sleep(Duration::from_millis(50)).await;}}
    let end=std::time::Instant::now()+Duration::from_secs(5);loop{if client.post(url).send().await.is_ok(){break;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(25)).await;}
    call(&client,url,&token,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"SoDeck integration","version":"1"}})).await;
    let end=std::time::Instant::now()+Duration::from_secs(8);let targets=loop{let targets=tool(&client,url,&token,"sodeck_targets_list",json!({})).await;if targets["targets"].as_array().unwrap().len()==3{break targets;}assert!(std::time::Instant::now()<end,"background registration failed: {targets}");tokio::time::sleep(Duration::from_millis(50)).await;};
    let opened=f.core.ui_action("main",&contexts[0],json!({"op":"open","key":"sodeck.mcp:operations","requestId":"open-operations"})).unwrap();let page=opened["viewId"].as_str().unwrap();
    assert_eq!(f.core.ui_action("main",&contexts[0],json!({"op":"open","key":"sodeck.mcp:operations","requestId":"open-operations-again"})).unwrap()["viewId"],page,"operation page is a workspace singleton");
    let attached=f.core.ui_attach("main",page).unwrap();let surface=attached["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",surface).unwrap();
    let target=targets["targets"].as_array().unwrap().iter().find(|t|t["context"]["workspaceId"]==contexts[0].workspace_id).unwrap();
    assert_eq!(attached["data"]["target"]["targetId"],target["targetId"]);assert!(attached["data"].get("token").is_none());
    assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3,"opening a page does not duplicate background targets");
    let job=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"page-lifecycle","command":if cfg!(windows){"powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 2; Write-Output sodeck-page-survived\""}else{"sleep 2; printf sodeck-page-survived"}})).await;
    let detail=f.core.ui_event("main",surface,json!({"action":"task","taskId":job["taskId"],"cursor":"0"})).unwrap();assert_eq!(detail["taskId"],job["taskId"]);assert_eq!(detail["done"],false);
    let plan=f.core.ui_close_prepare("main",vec![],Some(page.into())).unwrap();f.core.ui_close_finish(plan["token"].as_str().unwrap(),serde_json::from_value(plan["views"].clone()).unwrap(),"discard").unwrap();
    let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-page-survived"));
    let reopened=f.core.ui_action("main",&contexts[0],json!({"op":"open","key":"sodeck.mcp:operations","requestId":"reopen-operations"})).unwrap();let attached=f.core.ui_attach("main",reopened["viewId"].as_str().unwrap()).unwrap();
    assert!(attached["data"]["tasks"].as_array().unwrap().iter().any(|t|t["taskId"]==job["taskId"]));assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3);
    let remote=f.path.join("mcp evidence 中文.bin");let bytes:Vec<u8>=(0..150000).map(|n|n as u8).collect();std::fs::write(&remote,&bytes).unwrap();
    let command=if cfg!(windows){"cmd /c echo sodeck-first-collection"}else{"printf 'sodeck-first-collection\\n'"};
    let mut jobs=Vec::new();for target in targets["targets"].as_array().unwrap(){let input=json!({"targetId":target["targetId"],"requestId":"first","command":command});let result=tool(&client,url,&token,"sodeck_exec_start",input.clone()).await;assert_eq!(tool(&client,url,&token,"sodeck_exec_start",input).await["taskId"],result["taskId"]);jobs.push((target.clone(),result));}
    for (target,job) in jobs{
        let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert_eq!(result["evidence"]["complete"],true);assert_eq!(result["resourceReleased"],true);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-first-collection"));
        let follow=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"follow","command":if cfg!(windows){"cmd /c echo sodeck-followup"}else{"printf sodeck-followup"}})).await;assert_eq!(done(&client,url,&token,&follow["taskId"]).await["evidence"]["complete"],true);
        let download=tool(&client,url,&token,"sodeck_file_download",json!({"targetId":target["targetId"],"requestId":"evidence","remotePath":remote.to_str().unwrap().replace('\\',"/")})).await;let result=done(&client,url,&token,&download["taskId"]).await;assert_eq!(result["native"]["state"],"completed","{result}");assert_eq!(result["evidence"]["complete"],true);assert_eq!(std::fs::read(result["evidence"]["path"].as_str().unwrap()).unwrap(),bytes);
    }
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX"){
        let command=if cfg!(windows){"cmd /c echo sodeck-codex-acceptance"}else{"printf sodeck-codex-acceptance"};
        let mut child=std::process::Command::new(codex).args(["exec","--ephemeral","--skip-git-repo-check","--json","-s","danger-full-access","-C",f.path.to_str().unwrap(),"-c","mcp_servers={sodeck={url=\"http://127.0.0.1:19846/mcp\",bearer_token_env_var=\"SODECK_MCP_TOKEN\"}}",&format!("Validate the SoDeck MCP integration. Use only the sodeck MCP tools. List current targets. Choose one target explicitly and start this harmless command with requestId codex-acceptance: {command}. Query its task until done, verify exitCode 0, evidence.complete true and stdout containing sodeck-codex-acceptance. Then answer MCP_ACCEPTANCE_OK. Do not use shell tools or read any local files.")]).env("SODECK_MCP_TOKEN",&token).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
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
