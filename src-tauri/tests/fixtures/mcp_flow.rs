async fn call(client:&reqwest::Client,url:&str,token:&str,method:&str,params:Value)->Value{
    let response=client.post(url).bearer_auth(token).header("Accept","application/json, text/event-stream").header("MCP-Protocol-Version","2025-11-25").json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).send().await.unwrap();let code=response.status();let text=response.text().await.unwrap();assert_eq!(code,200,"{text}");serde_json::from_str::<Value>(&text).unwrap()["result"].clone()
}
async fn tool(client:&reqwest::Client,url:&str,token:&str,name:&str,p:Value)->Value{let r=call(client,url,token,"tools/call",json!({"name":name,"arguments":p})).await;assert_ne!(r["isError"],true,"{r}");assert!(r.get("structuredContent").is_none());assert_eq!(r["content"].as_array().unwrap().len(),1);assert_eq!(r["content"][0]["type"],"text");assert!(serde_json::to_vec(&r).unwrap().len()<=256*1024);serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()}
async fn done(client:&reqwest::Client,url:&str,token:&str,id:&Value)->Value{let end=std::time::Instant::now()+Duration::from_secs(30);let mut revision=Value::Null;loop{let mut p=json!({"taskId":id,"outputMode":"none","waitMs":2000});if !revision.is_null(){p["sinceRevision"]=revision;}let result=tool(client,url,token,"sodeck_task_get",p).await;if result["done"]==true{return tool(client,url,token,"sodeck_task_get",json!({"taskId":id,"outputMode":"text"})).await;}revision=result["revision"].clone();assert!(std::time::Instant::now()<end,"{result}");}}


async fn interaction_until(client:&reqwest::Client,url:&str,token:&str,id:&Value,marker:&str,available:bool)->Value {
    let end=std::time::Instant::now()+Duration::from_secs(30);
    loop {
        let result=tool(client,url,token,"sodeck_task_get",json!({"taskId":id})).await;
        let output=result["output"]["terminal"].as_str().or_else(||result["output"]["stdout"].as_str()).unwrap_or("");
        if output.contains(marker)&&(!available||result["interaction"]["available"]==true){return result;}
        assert!(!result["done"].as_bool().unwrap_or(false),"{result}");
        assert!(std::time::Instant::now()<end,"{result}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn interaction_send(client:&reqwest::Client,url:&str,token:&str,id:&Value,sequence:u64,action:&str,extra:Value)->Value {
    let mut input=json!({"taskId":id,"sequence":sequence.to_string(),"action":action});
    input.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let result=tool(client,url,token,"sodeck_task_input",input.clone()).await;
    assert_eq!(result["taskId"],*id);
    let duplicate=tool(client,url,token,"sodeck_task_input",input).await;
    assert_eq!(duplicate["taskId"],*id);
    result
}
async fn interactive_remote_checks(_f:&Fixture,client:&reqwest::Client,url:&str,token:&str,target:&Value) {
    let command=if cfg!(windows) {
        "powershell.exe -NoLogo -NoProfile -Command \"[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); $r=[IO.StreamReader]::new([Console]::OpenStandardInput(),[Text.Encoding]::UTF8); [Console]::Write('first?'); $a=$r.ReadLine(); [Console]::Write('second?'); $b=$r.ReadLine(); [Console]::Write(('length:'+$a.Length+';answer:'+$b)); $tail=$r.ReadToEnd(); [Console]::Write(('tail:'+$tail)); exit 7\""
    } else {
        "printf 'first?'; IFS= read -r a; printf 'second?'; IFS= read -r b; printf 'length:%s;answer:%s' \"${#a}\" \"$b\"; cat; printf 'tail-done'; exit 7"
    };
    let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":"real-pipe","mode":"pipe","command":command,"waitMs":0})).await;
    let id=&started["taskId"];
    interaction_until(client,url,token,id,"first?",true).await;
    let data="x".repeat(9000);
    interaction_send(client,url,token,id,1,"write",json!({"text":data.clone()})).await;
    // Without a newline the same process still waits for the rest of its first line.
    interaction_until(client,url,token,id,"first?",true).await;
    interaction_send(client,url,token,id,2,"write",json!({"text":if cfg!(windows){"\r\n"}else{"\n"}})).await;
    interaction_until(client,url,token,id,"second?",true).await;
    interaction_send(client,url,token,id,3,"write",json!({"text":if cfg!(windows){"yes\r\n"}else{"yes\n"}})).await;
    interaction_until(client,url,token,id,"length:9000;answer:yes",true).await;
    interaction_send(client,url,token,id,4,"write",json!({"text":"tail-input"})).await;
    interaction_until(client,url,token,id,"answer:yes",true).await;
    interaction_send(client,url,token,id,5,"eof",json!({})).await;
    let result=done(client,url,token,id).await;
    assert_eq!(result["native"]["exitCode"],7,"{result}");
    assert_eq!(result["evidence"]["complete"],true);
    assert!(result["output"]["stdout"].as_str().unwrap().contains("tail-input"));
    assert_eq!(result["interaction"]["last"]["state"],"sent");
    if !cfg!(windows) {
        let command="stty -echo; printf 'confirm?'; IFS= read -r a; printf 'confirmed:%s;raw?' \"$a\"; stty raw -echo; dd bs=1 count=6 2>/dev/null | od -An -tx1; stty -raw -echo; printf 'size?'; IFS= read -r b; stty size; printf 'end'; exit 7";
        let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":"real-pty","mode":"pty","command":command})).await;
        let id=&started["taskId"];
        interaction_until(client,url,token,id,"confirm?",true).await;
        interaction_send(client,url,token,id,1,"write",json!({"text":"y\r"})).await;
        interaction_until(client,url,token,id,"confirmed:y;raw?",true).await;
        interaction_send(client,url,token,id,2,"write",json!({"dataBase64":"AAMEG1tB"})).await;
        interaction_until(client,url,token,id,"00 03 04 1b 5b 41",true).await;
        interaction_send(client,url,token,id,3,"resize",json!({"cols":140,"rows":35})).await;
        interaction_until(client,url,token,id,"size?",true).await;
        interaction_send(client,url,token,id,4,"write",json!({"text":"\r"})).await;
        let result=done(client,url,token,id).await;
        assert_eq!(result["native"]["exitCode"],7,"{result}");
        assert!(result["output"]["terminal"].as_str().unwrap().contains("35 140"));
        assert!(result["output"].get("stdout").is_none());
        assert_eq!(result["evidence"]["complete"],true);
        let full=tool(client,url,token,"sodeck_task_get",json!({"taskId":id,"outputMode":"full"})).await;
        assert!(full["output"]["chunks"].as_array().unwrap().iter().all(|c|c["v"]==2&&c["source"]=="terminal"));

        let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":"real-shell","mode":"pty","command":"sh -i"})).await;
        let id=&started["taskId"];
        let end=std::time::Instant::now()+Duration::from_secs(10);
        loop {let r=tool(client,url,token,"sodeck_task_get",json!({"taskId":id,"outputMode":"none"})).await;if r["interaction"]["available"]==true{break;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(20)).await;}
        interaction_send(client,url,token,id,1,"write",json!({"text":"printf 'round-one\\n'\r"})).await;
        interaction_until(client,url,token,id,"round-one",true).await;
        interaction_send(client,url,token,id,2,"write",json!({"text":"printf 'round-two\\n'; exit 9\r"})).await;
        let r=done(client,url,token,id).await;assert_eq!(r["native"]["exitCode"],9);assert!(r["output"]["terminal"].as_str().unwrap().contains("round-two"));
        for (name,byte,command) in [
            ("canonical-eof","BA==","stty -echo; printf 'eof-ready'; cat; printf 'eof-seen'"),
            ("canonical-interrupt","Aw==","stty -echo; trap 'printf interrupt-seen; exit 6' INT; printf 'interrupt-ready'; while :; do sleep 1; done"),
        ] {
            let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":name,"mode":"pty","command":command})).await;
            let id=&started["taskId"];interaction_until(client,url,token,id,if name=="canonical-eof"{"eof-ready"}else{"interrupt-ready"},true).await;
            interaction_send(client,url,token,id,1,"write",json!({"dataBase64":byte})).await;
            let r=done(client,url,token,id).await;assert!(r["output"]["terminal"].as_str().unwrap().contains(if name=="canonical-eof"{"eof-seen"}else{"interrupt-seen"}),"{r}");
        }
    } else {
        let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":"real-pty","mode":"pty","command":"powershell.exe -NoLogo -NoProfile -Command \"[Console]::Write('confirm?'); $line=[Console]::ReadLine(); [Console]::Write(('confirmed:'+$line)); exit 0\""})).await;
        let id=&started["taskId"];interaction_until(client,url,token,id,"confirm?",true).await;
        interaction_send(client,url,token,id,1,"resize",json!({"cols":120,"rows":30})).await;
        interaction_until(client,url,token,id,"confirm?",true).await;
        interaction_send(client,url,token,id,2,"write",json!({"text":"y\r"})).await;
        let result=done(client,url,token,id).await;assert!(result["output"]["terminal"].as_str().unwrap().contains("confirmed:y"));assert_eq!(result["evidence"]["complete"],true);
    }
    #[cfg(unix)]
    if let Ok(username)=std::env::var("SODECK_TEST_SUDO_USER") {
        let server=_f.server("127.0.0.1",std::env::var("SODECK_TEST_SSH_PORT").unwrap().parse().unwrap(),json!([]));
        let mut draft=_f.core.server_catalog()["servers"].as_array().unwrap().iter().find(|s|s["id"]==server).unwrap().clone();
        draft["username"]=json!(username);
        draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});
        _f.change(json!({"op":"saveServer","draft":draft}));
        let context=connected(_f,&server).await.context.unwrap();
        let end=std::time::Instant::now()+Duration::from_secs(10);
        let target=loop{let r=tool(client,url,token,"sodeck_targets_list",json!({"workspaceId":context.workspace_id})).await;if let Some(id)=r["targets"][0]["targetId"].as_str(){break id.to_owned();}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(30)).await;};
        let marker=std::env::var("SODECK_TEST_SUDO_MARKER").unwrap();
        let started=tool(client,url,token,"sodeck_exec_start",json!({"targetId":target,"requestId":"real-sudo","mode":"pty","command":format!("sudo -k -p 'sudo-password?' /usr/bin/touch '{marker}'")})).await;
        let id=&started["taskId"];interaction_until(client,url,token,id,"sudo-password?",true).await;
        interaction_send(client,url,token,id,1,"write",json!({"text":"incorrect-password\r"})).await;
        interaction_until(client,url,token,id,"Sorry, try again.",true).await;
        interaction_send(client,url,token,id,2,"write",json!({"text":format!("{}\r",std::env::var("SODECK_TEST_SUDO_PASSWORD").unwrap())})).await;
        let result=done(client,url,token,id).await;assert_eq!(result["native"]["exitCode"],0,"{result}");assert_eq!(result["evidence"]["complete"],true);
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(marker).unwrap().uid(),0);
        let tasks=tool(client,url,token,"sodeck_tasks_list",json!({"targetId":target,"requestId":"real-sudo"})).await;assert_eq!(tasks["tasks"].as_array().unwrap().len(),1);
    }
}

async fn codex_mcp_trace(codex:&str,directory:&std::path::Path,url:&str,token:&str,prompt:&str,name:&str)->(String,Vec<Value>) {
    let mut child=std::process::Command::new(codex).args(["exec","--ephemeral","--skip-git-repo-check","--json","-s","danger-full-access","-C",directory.to_str().unwrap(),"-c",&format!("mcp_servers={{sodeck={{url=\"{url}\",bearer_token_env_var=\"SODECK_MCP_TOKEN\"}}}}"),prompt]).env("SODECK_MCP_TOKEN",token).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let mut out=child.stdout.take().unwrap();let mut err=child.stderr.take().unwrap();
    let stdout_thread=std::thread::spawn(move||{use std::io::Read;let mut v=Vec::new();out.read_to_end(&mut v).unwrap();v});let stderr_thread=std::thread::spawn(move||{use std::io::Read;let mut v=Vec::new();err.read_to_end(&mut v).unwrap();v});
    let end=std::time::Instant::now()+Duration::from_secs(300);let status=loop{if let Some(status)=child.try_wait().unwrap(){break status;}if std::time::Instant::now()>end{let _=child.kill();break child.wait().unwrap();}tokio::time::sleep(Duration::from_millis(100)).await;};
    let stdout=String::from_utf8_lossy(&stdout_thread.join().unwrap()).replace(token,"[redacted]");let stderr=String::from_utf8_lossy(&stderr_thread.join().unwrap()).replace(token,"[redacted]");
    let output_directory=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_OUTPUT").unwrap_or_else(|_|directory.join("codex-output").to_string_lossy().into_owned()));std::fs::create_dir_all(&output_directory).unwrap();std::fs::write(output_directory.join(format!("{name}.jsonl")),&stdout).unwrap();
    assert!(status.success(),"Codex failed: {stderr}\n{stdout}");
    let calls=stdout.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).filter(|v|v["item"]["type"]=="mcp_tool_call"&&v["item"]["status"]=="completed").map(|v|v["item"].clone()).collect();(stdout,calls)
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires three isolated OpenSSH servers and actual MCP plugin; npm run mcp:test"]
async fn openssh_mcp_plugin(){
    use base64::{engine::general_purpose::STANDARD, Engine};
    let _ = rustls::crypto::ring::default_provider().install_default();
    let f=Fixture::new();
    let ports:Vec<u16>=serde_json::from_str(&std::env::var("SODECK_TEST_MCP_PORTS").unwrap()).unwrap();assert_eq!(ports.len(),3);
    let mut contexts=Vec::new();
    for port in ports{let server=f.server("127.0.0.1",port,json!([]));let mut draft=f.core.server_catalog()["servers"].as_array().unwrap().iter().find(|s|s["id"]==server).unwrap().clone();if contexts.is_empty(){draft["name"]=json!("187");}draft["username"]=json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});f.change(json!({"op":"saveServer","draft":draft}));contexts.push(connected(&f,&server).await.context.unwrap());}
    // Keep integration traffic separate from a running desktop MCP service.
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let mcp_port=listener.local_addr().unwrap().port();drop(listener);
    let plugin=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_PLUGIN").unwrap());f.core.add_plugin(&plugin).unwrap();let settings=f.core.settings_page("sodeck.mcp","connection").unwrap();assert_eq!(f.core.save_settings("sodeck.mcp","connection",json!({"requestId":"isolated-listener","definitionToken":settings["definitionToken"],"baseRevision":settings["revision"],"values":{"port":mcp_port}})).unwrap()["persisted"],true);
    let baseline=if let Ok(old)=std::env::var("SODECK_TEST_MCP_BASELINE") {
        f.core.load(std::path::Path::new(&old)).unwrap();
        let token=std::fs::read_to_string(f.path.join("plugin-data/sodeck.mcp/access-token")).unwrap();let client=reqwest::Client::new();let url=format!("http://127.0.0.1:{mcp_port}/mcp");
        let end=std::time::Instant::now()+Duration::from_secs(8);loop {let count=f.core.ui_state().workspaces.values().flat_map(|w|w.views.iter()).filter(|v|v.plugin_id=="sodeck.mcp").count();if count==3{break;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(25)).await;}
        call(&client,&url,&token,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"baseline","version":"1"}})).await;
        let (started,raw)=loop {let started=std::time::Instant::now();let raw=call(&client,&url,&token,"tools/call",json!({"name":"sodeck_targets_list","arguments":{}})).await;let value:Value=serde_json::from_str(raw["content"][0]["text"].as_str().unwrap()).unwrap();if value["targets"].as_array().unwrap().len()==3{break (started,raw);}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(25)).await;};
        let sample=json!({"version":"0.4.0","elapsedMs":started.elapsed().as_millis(),"bytes":serde_json::to_vec(&raw).unwrap().len(),"result":raw});f.core.unload("sodeck.mcp").unwrap();Some(sample)
    }else{None};
    f.core.load(&plugin).unwrap();
    let deadline=std::time::Instant::now()+Duration::from_secs(8);
    loop{let views=f.core.ui_state().workspaces.values().flat_map(|w|w.views.iter()).filter(|v|v.plugin_id=="sodeck.mcp").count();if views==3{break;}assert!(std::time::Instant::now()<deadline,"background views did not start");tokio::time::sleep(Duration::from_millis(25)).await;}
    let token=std::fs::read_to_string(f.path.join("plugin-data/sodeck.mcp/access-token")).unwrap();
    let client=reqwest::Client::new();let local_url=format!("http://127.0.0.1:{mcp_port}/mcp");let url=local_url.as_str();
    let end=std::time::Instant::now()+Duration::from_secs(5);loop{if client.post(url).send().await.is_ok(){break;}assert!(std::time::Instant::now()<end);tokio::time::sleep(Duration::from_millis(25)).await;}
    call(&client,url,&token,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"SoDeck integration","version":"1"}})).await;
    let end=std::time::Instant::now()+Duration::from_secs(8);let targets=loop{let targets=tool(&client,url,&token,"sodeck_targets_list",json!({})).await;if targets["targets"].as_array().unwrap().len()==3{break targets;}assert!(std::time::Instant::now()<end,"background registration failed: {targets}");tokio::time::sleep(Duration::from_millis(50)).await;};
    let query_started=std::time::Instant::now();let unique_raw=call(&client,url,&token,"tools/call",json!({"name":"sodeck_targets_list","arguments":{"query":"187"}})).await;let unique:Value=serde_json::from_str(unique_raw["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(unique["resolution"],"unique");assert_eq!(unique["targets"][0]["matchedBy"],json!(["nameExact"]));assert_eq!(unique["targets"][0]["workspaceId"],contexts[0].workspace_id);
    let metrics=json!({"scope":"actual core and plugin with three isolated OpenSSH sessions, not production 187","version":"0.5.0","baseline":baseline,"query":"187","elapsedMs":query_started.elapsed().as_millis(),"bytes":serde_json::to_vec(&unique_raw).unwrap().len(),"result":unique_raw});
    if let Ok(output)=std::env::var("SODECK_TEST_MCP_OUTPUT"){std::fs::create_dir_all(&output).unwrap();std::fs::write(std::path::Path::new(&output).join("c-live-discovery.json"),serde_json::to_vec_pretty(&metrics).unwrap()).unwrap();}
    for query in ["127.0.0.1".to_string(),"1".to_string()] {let all=tool(&client,url,&token,"sodeck_targets_list",json!({"query":query})).await;assert_eq!(all["resolution"],"ambiguous");assert_eq!(all["targets"].as_array().unwrap().len(),3);}
    for context in &contexts {let item=targets["targets"].as_array().unwrap().iter().find(|t|t["workspaceId"]==context.workspace_id).unwrap();let exact=tool(&client,url,&token,"sodeck_targets_list",json!({"query":item["target"]["endpoint"]})).await;assert_eq!(exact["resolution"],"unique");assert_eq!(exact["targets"][0]["targetId"],item["targetId"]);}
    assert!(!f.core.ui_state().definitions.contains_key("sodeck.mcp:operations"),"MCP has no central-page contribution");
    let central_before={let state=f.core.ui_state();let host=state.workspaces[&contexts[0].workspace_id].host("main").unwrap();json!({"center":host.center,"groups":host.groups,"focusedGroup":host.focused_group_id})};
    assert!(f.core.ui_action("main",&contexts[0],json!({"op":"open","key":"sodeck.mcp:operations","requestId":"old-central-page"})).is_err());
    let opened=f.core.ui_action("main",&contexts[0],json!({"op":"toggle","key":"sodeck.mcp:status","requestId":"show-sidebar"})).unwrap();let page=opened["viewId"].as_str().unwrap();
    let attached=f.core.ui_attach("main",page).unwrap();let surface=attached["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",surface).unwrap();
    let target=targets["targets"].as_array().unwrap().iter().find(|t|t["workspaceId"]==contexts[0].workspace_id).unwrap();
    assert_eq!(attached["data"]["targetId"],target["targetId"]);assert_eq!(attached["data"]["target"]["descriptor"],target["target"]);assert_eq!(attached["data"]["target"]["availability"],"ready");assert!(attached["data"].get("token").is_none());
    assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3,"showing the sidebar does not duplicate background targets");
    interactive_remote_checks(&f,&client,url,&token,&target["targetId"]).await;
    let job=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"page-lifecycle","waitMs":0,"command":if cfg!(windows){"powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 2; Write-Output sodeck-page-survived\""}else{"sleep 2; printf sodeck-page-survived"}})).await;assert!(job.get("wait").is_none());
    let detail=f.core.ui_event("main",surface,json!({"action":"task","taskId":job["taskId"],"cursor":"0"})).unwrap();assert_eq!(detail["taskId"],job["taskId"]);assert_eq!(detail["done"],false);
    f.core.ui_action("main",&contexts[0],json!({"op":"hideTool","viewId":page})).unwrap();
    let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-page-survived"));
    let reopened=f.core.ui_action("main",&contexts[0],json!({"op":"toggle","key":"sodeck.mcp:status","requestId":"show-sidebar-again"})).unwrap();assert_eq!(reopened["viewId"],page,"hiding the sidebar retains its background view");let attached=f.core.ui_attach("main",page).unwrap();
    assert!(attached["data"]["tasks"].as_array().unwrap().iter().any(|t|t["taskId"]==job["taskId"]));assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({})).await["targets"].as_array().unwrap().len(),3);
    let central_after={let state=f.core.ui_state();let host=state.workspaces[&contexts[0].workspace_id].host("main").unwrap();json!({"center":host.center,"groups":host.groups,"focusedGroup":host.focused_group_id})};assert_eq!(central_after,central_before,"MCP sidebar lifecycle leaves central groups and selection unchanged");
    let remote=f.path.join("mcp evidence 中文.bin");let bytes:Vec<u8>=(0..150000).map(|n|n as u8).collect();std::fs::write(&remote,&bytes).unwrap();
    let command=if cfg!(windows){"cmd /c echo sodeck-first-collection"}else{"printf 'sodeck-first-collection\\n'"};
    let mut jobs=Vec::new();for target in targets["targets"].as_array().unwrap(){let mut input=json!({"targetId":target["targetId"],"requestId":"first","command":command});let result=tool(&client,url,&token,"sodeck_exec_start",input.clone()).await;assert_eq!(result["done"],true,"short command must finish in the default window: {result}");assert_eq!(result["wait"]["reason"],"done");assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-first-collection"));input["waitMs"]=json!(0);let retry=tool(&client,url,&token,"sodeck_exec_start",input).await;assert_eq!(retry["taskId"],result["taskId"]);assert!(retry.get("wait").is_none());jobs.push((target.clone(),result));}
    for (target,job) in jobs{
        let result=done(&client,url,&token,&job["taskId"]).await;assert_eq!(result["native"]["exitCode"],0);assert_eq!(result["evidence"]["complete"],true);assert_eq!(result["resourceReleased"],true);assert!(result["output"]["stdout"].as_str().unwrap().contains("sodeck-first-collection"));
        assert!(result["output"].get("chunks").is_none());assert!(result.get("input").is_none());
        let diagnostic=tool(&client,url,&token,"sodeck_task_get",json!({"taskId":job["taskId"],"detail":true,"outputMode":"none"})).await;assert_eq!(diagnostic["input"]["command"],command);assert!(diagnostic.get("output").is_none());
        let full=tool(&client,url,&token,"sodeck_task_get",json!({"taskId":job["taskId"],"outputMode":"full"})).await;assert!(full["output"].get("stdout").is_none());let mut raw=Vec::new();for chunk in full["output"]["chunks"].as_array().unwrap(){raw.extend(STANDARD.decode(chunk["dataBase64"].as_str().unwrap()).unwrap());}assert_eq!(String::from_utf8_lossy(&raw),result["output"]["stdout"].as_str().unwrap());
        let found=tool(&client,url,&token,"sodeck_tasks_list",json!({"targetId":target["targetId"],"requestId":"first","status":"done"})).await;assert_eq!(found["tasks"].as_array().unwrap().len(),1);assert_eq!(found["tasks"][0]["taskId"],job["taskId"]);
        let none=tool(&client,url,&token,"sodeck_task_get",json!({"taskId":job["taskId"],"outputMode":"none","waitMs":2000})).await;assert!(none.get("output").is_none());assert_eq!(none["wait"]["reason"],"done");assert_eq!(none["revision"],result["revision"]);
        let follow=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":target["targetId"],"requestId":"follow","command":if cfg!(windows){"cmd /c echo sodeck-followup"}else{"printf sodeck-followup"}})).await;assert_eq!(done(&client,url,&token,&follow["taskId"]).await["evidence"]["complete"],true);
        let download=tool(&client,url,&token,"sodeck_file_download",json!({"targetId":target["targetId"],"requestId":"evidence","remotePath":remote.to_str().unwrap().replace('\\',"/")})).await;let result=done(&client,url,&token,&download["taskId"]).await;assert_eq!(result["native"]["state"],"completed","{result}");assert_eq!(result["evidence"]["complete"],true);assert_eq!(std::fs::read(result["file"]["localPath"].as_str().unwrap()).unwrap(),bytes);
    }
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX"){
        let short_command=if cfg!(windows){"cmd /c \"ver & echo sodeck-codex-short\""}else{"uname -s; printf sodeck-codex-short"};
        let command=if cfg!(windows){"powershell.exe -NoProfile -Command \"Start-Sleep -Seconds 2; Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors | Format-List; Write-Output ('page-line-' * 2400); Write-Output sodeck-codex-acceptance\""}else{"sleep 2; lscpu; printf '%024000d' 0; printf sodeck-codex-acceptance"};
        let (stdout,calls)=codex_mcp_trace(&codex,&f.path,url,&token,&format!("Validate the SoDeck MCP integration. Use only the sodeck MCP tools. List current targets. Choose one target explicitly. First start this short system query with requestId codex-short, omitting waitMs: {short_command}. Verify stable done, exitCode 0, evidence.complete and marker directly from exec_start; do not task_get this short task when its complete result is already returned. Then start this long harmless command with requestId codex-acceptance, omitting waitMs: {command}. Continue its same taskId. Read status with outputMode none and a positive waitMs until done. Read the first output page with the default parameters (omit detail and outputMode). Read the next page using nextCursor until the final marker is found. Read one page in full mode and verify original chunks without stdout. Read detail=true with outputMode none and verify the original command is present and no output. Verify exitCode 0, evidence.complete true and stdout containing sodeck-codex-acceptance. In your final answer report the short system query result, CPU name and whether paging, full and diagnostics worked, then answer MCP_ACCEPTANCE_OK. Do not use shell tools or read any local files."),"codex-acceptance").await;
        let reads:Vec<&Value>=calls.iter().filter(|v|v["tool"]=="sodeck_task_get").collect();
        let shorts:Vec<&Value>=calls.iter().filter(|v|v["tool"]=="sodeck_exec_start"&&v["arguments"]["requestId"]=="codex-short").collect();assert_eq!(shorts.len(),1,"short result must use one launch: {stdout}");assert!(shorts[0]["arguments"].get("waitMs").is_none());let short:Value=serde_json::from_str(shorts[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();assert_eq!(short["done"],true,"{short}");assert_eq!(short["wait"]["reason"],"done");assert!(short["output"]["stdout"].as_str().unwrap().contains("sodeck-codex-short"));assert!(!reads.iter().any(|v|v["arguments"]["taskId"]==short["taskId"]),"short query must not need another read: {stdout}");
        assert!(reads.iter().any(|v|v["arguments"]["outputMode"]=="none"&&v["arguments"]["waitMs"].as_u64().is_some_and(|n|n>0)),"missing status wait: {stdout}");
        assert!(reads.iter().any(|v|v["arguments"].get("outputMode").is_none()&&v["arguments"].get("detail").is_none()),"missing default read: {stdout}");
        assert!(reads.iter().any(|v|v["arguments"]["cursor"].as_str().is_some_and(|s|s!="0")),"missing paging: {stdout}");
        assert!(reads.iter().any(|v|v["arguments"]["outputMode"]=="full"),"missing full: {stdout}");
        assert!(reads.iter().any(|v|v["arguments"]["detail"]==true&&v["arguments"]["outputMode"]=="none"),"missing diagnostic: {stdout}");
        assert!(stdout.contains("MCP_ACCEPTANCE_OK"),"{stdout}");
    }
    let original_target=unique["targets"][0]["targetId"].clone();
    let recovery_command=if cfg!(windows){"cmd /c echo sodeck-original-operation"}else{"printf sodeck-original-operation"};
    let mut recovery=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":original_target,"requestId":"c-recover","command":recovery_command})).await;
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX") {
        let short_command=if cfg!(windows){"cmd /c \"ver & echo sodeck-c-unique\""}else{"uname -s"};
        let (stdout,calls)=codex_mcp_trace(&codex,&f.path,url,&token,&format!("Use only SoDeck MCP tools; do not use shell tools or local files. The user asks for the OS of 187. Query targets once with query 187. If resolution is unique and the target is ready, start this harmless command once with requestId c-codex-unique, omitting waitMs: {short_command}. Check done, exitCode 0 and evidence.complete from the returned result; if complete with one page, answer directly without reading the task again. Answer C_UNIQUE_OK with the observed OS."),"codex-c-unique").await;
        assert_eq!(calls.len(),2,"unique query and short command need two calls: {stdout}");assert_eq!(calls[0]["tool"],"sodeck_targets_list");assert_eq!(calls[0]["arguments"]["query"],"187");assert_eq!(calls[1]["tool"],"sodeck_exec_start");
        let short:Value=serde_json::from_str(calls[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();assert_eq!(short["done"],true);assert_eq!(short["native"]["exitCode"],0);assert_eq!(short["evidence"]["complete"],true);assert!(stdout.contains("C_UNIQUE_OK"));
        let (stdout,calls)=codex_mcp_trace(&codex,&f.path,url,&token,"Use only SoDeck MCP tools; do not use shell tools or local files. The user asks to check the CPU of 1. Query targets with query 1. If ambiguous, ask the user to choose an endpoint, retaining all candidates. Do not execute or download anything before the user chooses. End with C_AMBIGUOUS_OK.","codex-c-ambiguous").await;
        assert_eq!(calls.len(),1,"ambiguity must not start an operation: {stdout}");assert_eq!(calls[0]["tool"],"sodeck_targets_list");let r:Value=serde_json::from_str(calls[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();assert_eq!(r["resolution"],"ambiguous");assert_eq!(r["targets"].as_array().unwrap().len(),3);assert!(stdout.contains("C_AMBIGUOUS_OK"));
    }
    recovery=done(&client,url,&token,&recovery["taskId"]).await;
    f.core.disconnect_connection("main",&contexts[0]).unwrap();until(||f.core.connection_views().iter().any(|v|v.server_id==contexts[0].server_id&&v.state=="disconnected")).await;
    let offline=tool(&client,url,&token,"sodeck_targets_list",json!({"query":"187"})).await;assert_eq!(offline["resolution"],"unique");assert_eq!(offline["targets"][0]["availability"],"disconnected");assert_eq!(offline["targets"][0]["targetSource"],"lastConnected");assert_eq!(offline["targets"][0]["targetId"],original_target);
    let offline_sidebar=f.core.ui_attach("main",page).unwrap();assert_eq!(offline_sidebar["data"]["targetId"],original_target);assert_eq!(offline_sidebar["data"]["target"]["descriptor"],offline["targets"][0]["target"]);assert_eq!(offline_sidebar["data"]["target"]["targetSource"],"lastConnected");assert_eq!(offline_sidebar["data"]["target"]["availability"],"disconnected");
    let all_offline=tool(&client,url,&token,"sodeck_targets_list",json!({"query":"1"})).await;assert_eq!(all_offline["resolution"],"ambiguous");assert_eq!(all_offline["targets"].as_array().unwrap().len(),3);
    let old_context=contexts[0].clone();contexts[0]=connected(&f,&old_context.server_id).await.context.unwrap();assert_ne!(contexts[0].connection_id,old_context.connection_id);
    let end=std::time::Instant::now()+Duration::from_secs(8);loop {let next=tool(&client,url,&token,"sodeck_targets_list",json!({"workspaceId":old_context.workspace_id,"detail":true})).await;if next["targets"][0]["context"]==json!(contexts[0]){assert_ne!(next["targets"][0]["targetId"],original_target);break;}assert!(std::time::Instant::now()<end,"{next}");tokio::time::sleep(Duration::from_millis(25)).await;}
    assert_eq!(tool(&client,url,&token,"sodeck_targets_list",json!({"targetId":original_target})).await["resolution"],"none");
    let original=tool(&client,url,&token,"sodeck_exec_start",json!({"targetId":original_target,"requestId":"c-recover","command":recovery_command,"waitMs":0})).await;assert_eq!(original["taskId"],recovery["taskId"]);
    let diagnostic=tool(&client,url,&token,"sodeck_task_get",json!({"taskId":recovery["taskId"],"detail":true,"outputMode":"none"})).await;assert_eq!(diagnostic["context"],json!(old_context));
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX") {
        let (stdout,calls)=codex_mcp_trace(&codex,&f.path,url,&token,&format!("Use only SoDeck MCP tools, not shell tools or local files. A command with requestId c-recover on serverId {} was submitted before reconnect. Its original targetId is now absent. Find the original task with tasks_list using serverId and requestId, then read its original task once. Do not launch or download anything, and do not replace it with the new target. Report the original-operation marker and C_RECOVERY_OK.",old_context.server_id),"codex-c-recovery").await;
        assert_eq!(calls.len(),2,"recovery must query the original task without replay: {stdout}");assert_eq!(calls[0]["tool"],"sodeck_tasks_list");assert_eq!(calls[1]["tool"],"sodeck_task_get");assert_eq!(calls[1]["arguments"]["taskId"],recovery["taskId"]);assert!(stdout.contains("C_RECOVERY_OK"));
    }
    let plan=f.core.ui_close_prepare("main",contexts.iter().map(|c|c.workspace_id.clone()).collect(),None).unwrap();let closing=tool(&client,url,&token,"sodeck_targets_list",json!({"query":"187"})).await;assert_eq!(closing["targets"][0]["availability"],"closing");f.core.ui_close_finish(plan["token"].as_str().unwrap(),serde_json::from_value(plan["views"].clone()).unwrap(),"discard").unwrap();
    f.core.unload("sodeck.mcp").unwrap();assert!(client.post(url).send().await.is_err());f.core.shutdown().unwrap();
}
