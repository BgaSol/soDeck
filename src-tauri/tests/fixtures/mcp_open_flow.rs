#[tokio::test(flavor="multi_thread",worker_threads=4)]
#[ignore="requires isolated OpenSSH servers and MCP package; npm run mcp:test"]
async fn openssh_mcp_connections(){
    async fn write_error(client:&reqwest::Client,url:&str,token:&str,name:&str,input:Value)->Value {let r=call(client,url,token,"tools/call",json!({"name":name,"arguments":input})).await;assert_eq!(r["isError"],true,"{r}");serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()}
    let _=rustls::crypto::ring::default_provider().install_default();
    let f=Fixture::new();let ports:Vec<u16>=serde_json::from_str(&std::env::var("SODECK_TEST_MCP_PORTS").unwrap()).unwrap();
    let mut ids=Vec::new();
    for (index,port) in ports.iter().enumerate(){let id=f.server("127.0.0.1",*port,json!([]));let mut draft=f.core.server_catalog()["servers"].as_array().unwrap().iter().find(|s|s["id"]==id).unwrap().clone();if index==0{draft["name"]=json!("187");}draft["username"]=json!(std::env::var("SODECK_TEST_SSH_USER").unwrap());draft["authentication"]=json!({"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}});f.change(json!({"op":"saveServer","draft":draft}));ids.push(id);}
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let port=listener.local_addr().unwrap().port();drop(listener);
    let plugin=std::path::PathBuf::from(std::env::var("SODECK_TEST_MCP_PLUGIN").unwrap());f.core.add_plugin(&plugin).unwrap();let settings=f.core.settings_page("sodeck.mcp","connection").unwrap();f.core.save_settings("sodeck.mcp","connection",json!({"requestId":"opening-http","definitionToken":settings["definitionToken"],"baseRevision":settings["revision"],"values":{"port":port}})).unwrap();f.core.load(&plugin).unwrap();
    let token=std::fs::read_to_string(f.path.join("plugin-data/sodeck.mcp/access-token")).unwrap();let client=reqwest::Client::new();let url=format!("http://127.0.0.1:{port}/mcp");
    let listening=std::time::Instant::now()+Duration::from_secs(5);loop{if client.post(&url).send().await.is_ok(){break;}assert!(std::time::Instant::now()<listening);tokio::time::sleep(Duration::from_millis(25)).await;}
    call(&client,&url,&token,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"opening-ssh","version":"1"}})).await;
    assert!(f.core.workspace_projection("main").unwrap().workspaces.is_empty());
    let catalog=tool(&client,&url,&token,"sodeck_servers_list",json!({})).await;assert_eq!(catalog["matchCount"],3);assert!(catalog["servers"].as_array().unwrap().iter().all(|s|s["target"].is_null()));
    assert_eq!(tool(&client,&url,&token,"sodeck_servers_list",json!({"query":"127.0.0.1","limit":1})).await["resolution"],"ambiguous");
    assert_eq!(tool(&client,&url,&token,"sodeck_servers_list",json!({"query":"187"})).await["resolution"],"unique");
    let create_input=json!({"catalogRevision":catalog["catalogRevision"],"server":{"name":"managed-through-mcp","host":"127.0.0.1","port":1,"username":std::env::var("SODECK_TEST_SSH_USER").unwrap(),"authentication":{"type":"privateKey","privateKeyPath":std::env::var("SODECK_TEST_SSH_KEY").unwrap(),"secret":{"action":"clear"}}}});
    let managed=tool(&client,&url,&token,"sodeck_server_create",create_input.clone()).await;assert!(managed["serverId"].is_string(),"{managed}");
    assert_eq!(write_error(&client,&url,&token,"sodeck_server_create",create_input).await["error"]["details"]["code"],"conflict");
    let changed=tool(&client,&url,&token,"sodeck_server_update",json!({"catalogRevision":managed["catalogRevision"],"serverId":managed["serverId"],"patch":{"port":ports[0],"name":"managed-through-mcp-updated"}})).await;assert!(changed["catalogRevision"].is_string(),"{changed}");
    let selected=tool(&client,&url,&token,"sodeck_servers_list",json!({"serverId":managed["serverId"]})).await;assert_eq!(selected["servers"][0]["port"],ports[0]);
    let invalid=write_error(&client,&url,&token,"sodeck_server_update",json!({"catalogRevision":changed["catalogRevision"],"serverId":managed["serverId"],"patch":{"authentication":{"type":"secret-redaction-marker","secret":{"action":"replace","remember":true,"value":"secret-redaction-marker"}}}})).await;assert!(!invalid.to_string().contains("secret-redaction-marker"));
    let mut opened_managed=tool(&client,&url,&token,"sodeck_connection_open",json!({"serverId":managed["serverId"],"expectedProfileToken":selected["servers"][0]["profileToken"],"requestId":"managed-open","waitMs":0})).await;
    let end=std::time::Instant::now()+Duration::from_secs(30);
    while opened_managed["connection"]["state"]!="ready" {assert!(std::time::Instant::now()<end,"{opened_managed}");assert_ne!(opened_managed["connection"]["state"],"failed","{opened_managed}");for v in f.core.connection_views(){if v.server_id==managed["serverId"].as_str().unwrap(){if let Some(Question::HostKey{id,..})=v.question {f.core.answer_connection("main",&v.attempt_id,&id,Answer::HostKey{accept:true}).unwrap();}}}opened_managed=tool(&client,&url,&token,"sodeck_connection_get",json!({"connectionRequestId":opened_managed["connection"]["connectionRequestId"],"waitMs":1000})).await;}
    let rev=f.core.server_catalog()["revision"].as_u64().unwrap().to_string();
    assert_eq!(write_error(&client,&url,&token,"sodeck_server_delete",json!({"catalogRevision":rev,"serverId":managed["serverId"]})).await["error"]["details"]["code"],"inUse");
    let context=f.core.workspace_projection("main").unwrap().workspaces.iter().find(|w|w.context.server_id==managed["serverId"].as_str().unwrap()).unwrap().context.clone();
    let plan=f.core.ui_close_prepare("main",vec![context.workspace_id.clone()],None).unwrap();f.core.ui_close_finish(plan["token"].as_str().unwrap(),serde_json::from_value(plan["views"].clone()).unwrap(),"discard").unwrap();
    let deleted=tool(&client,&url,&token,"sodeck_server_delete",json!({"catalogRevision":rev,"serverId":managed["serverId"]})).await;assert_eq!(deleted["serverId"],managed["serverId"],"{deleted}");assert_eq!(tool(&client,&url,&token,"sodeck_servers_list",json!({"serverId":managed["serverId"]})).await["matchCount"],0);
    assert!(!std::fs::read_to_string(f.path.join("servers.json")).unwrap().contains("managed-through-mcp"));
    if let Ok(codex)=std::env::var("SODECK_TEST_CODEX_OPEN") {
        let command=if cfg!(windows){"cmd /c echo codex-opened-server"}else{"printf codex-opened-server"};
        let prompt=format!("Validate the SoDeck MCP connection flow using only sodeck MCP tools, no shell tools or local files. The user wants a harmless query on server 187. Discover saved servers with query 187, choose only if unique, open that exact saved server using requestId codex-open-zero and its discovered profileToken. If pending or needsUser, the test user will answer the SoDeck prompt; wait with connection_get on the same receipt until ready. Use the ready targetId directly to execute once with requestId codex-open-query: {command}. Check stable done, exitCode 0, evidence.complete and the output marker. Reply CODEX_OPEN_OK. Never guess a targetId or reconnect an unrelated server.");
        let trace=codex_mcp_trace(&codex,&f.path,&url,&token,&prompt,"codex-opening");tokio::pin!(trace);
        let mut answered=HashSet::new();
        let (stdout,calls)=loop{tokio::select!{result=&mut trace=>break result,_=tokio::time::sleep(Duration::from_millis(100))=>{
            for view in f.core.connection_views(){if view.server_id==ids[0]{if let Some(Question::HostKey{id,..})=view.question{if answered.insert(id.clone()){f.core.answer_connection("main",&view.attempt_id,&id,Answer::HostKey{accept:true}).unwrap();}}}}
        }}};
        assert!(calls.iter().any(|c|c["tool"]=="sodeck_servers_list"&&c["arguments"]["query"]=="187"),"{stdout}");
        assert!(calls.iter().any(|c|c["tool"]=="sodeck_connection_open"&&c["arguments"]["serverId"]==ids[0]),"{stdout}");
        let executions:Vec<_>=calls.iter().filter(|c|c["tool"]=="sodeck_exec_start").collect();assert_eq!(executions.len(),1,"{stdout}");
        let result:Value=serde_json::from_str(executions[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["done"],true);assert_eq!(result["native"]["exitCode"],0);assert_eq!(result["evidence"]["complete"],true);assert!(result["output"]["stdout"].as_str().unwrap().contains("codex-opened-server"));assert!(stdout.contains("CODEX_OPEN_OK"));
    }
    let mut opened=Vec::new();
    for (index,id) in ids.iter().enumerate(){let row=catalog["servers"].as_array().unwrap().iter().find(|s|s["serverId"]==*id).unwrap();let input=json!({"serverId":id,"expectedProfileToken":row["profileToken"],"requestId":format!("opening-{index}")});
        let mut result=tool(&client,&url,&token,"sodeck_connection_open",input.clone()).await;let end=std::time::Instant::now()+Duration::from_secs(30);let mut answered=HashSet::new();
        loop{assert!(std::time::Instant::now()<end,"opening did not settle: {result}");assert_ne!(result["connection"]["state"],"failed","{result}");
            if result["connection"]["state"]=="ready"{break;}
            for view in f.core.connection_views(){if view.server_id==*id{if let Some(Question::HostKey{id:q,..})=view.question{if answered.insert(q.clone()){f.core.answer_connection("main",&view.attempt_id,&q,Answer::HostKey{accept:true}).unwrap();}}}}
            result=tool(&client,&url,&token,"sodeck_connection_get",json!({"connectionRequestId":result["connection"]["connectionRequestId"],"waitMs":20000,"sinceRevision":result["connection"]["revision"]})).await;
        }
        assert_eq!(result["target"]["availability"],"ready");assert!(result["target"]["capabilities"].as_array().unwrap().iter().any(|c|c=="exec"));
        let retry=tool(&client,&url,&token,"sodeck_connection_open",input.clone()).await;assert_eq!(retry["connection"]["connectionRequestId"],result["connection"]["connectionRequestId"]);assert_eq!(retry["target"]["targetId"],result["target"]["targetId"]);opened.push((input,result));
    }
    let command=if cfg!(windows){"cmd /c echo sodeck-opening-path"}else{"printf sodeck-opening-path"};
    let job=tool(&client,&url,&token,"sodeck_exec_start",json!({"targetId":opened[0].1["target"]["targetId"],"requestId":"opened-command","command":command})).await;
    let job=done(&client,&url,&token,&job["taskId"]).await;assert_eq!(job["native"]["exitCode"],0);assert!(job["output"]["stdout"].as_str().unwrap().contains("sodeck-opening-path"));
    let remote=f.path.join("opening-download.bin");std::fs::write(&remote,b"opening-download").unwrap();
    let download=tool(&client,&url,&token,"sodeck_file_download",json!({"targetId":opened[1].1["target"]["targetId"],"requestId":"opened-download","remotePath":remote.to_str().unwrap().replace('\\',"/")})).await;let file=done(&client,&url,&token,&download["taskId"]).await;assert_eq!(std::fs::read(file["file"]["localPath"].as_str().unwrap()).unwrap(),b"opening-download");
    let context=f.core.workspace_projection("main").unwrap().workspaces.iter().find(|w|w.context.server_id==ids[0]).unwrap().context.clone();f.core.disconnect_connection("main",&context).unwrap();until(||f.core.connection_views().iter().any(|v|v.server_id==ids[0]&&v.state=="disconnected")).await;
    assert_eq!(tool(&client,&url,&token,"sodeck_connection_get",json!({"connectionRequestId":opened[0].1["connection"]["connectionRequestId"]})).await["target"],opened[0].1["target"]);
    let mut new=opened[0].0.clone();new["requestId"]=json!("reconnect-opening");let result=tool(&client,&url,&token,"sodeck_connection_open",new).await;
    let result=if result["connection"]["state"]=="ready"{result}else{tool(&client,&url,&token,"sodeck_connection_get",json!({"connectionRequestId":result["connection"]["connectionRequestId"],"waitMs":20000})).await};assert_eq!(result["connection"]["state"],"ready");assert_ne!(result["target"]["targetId"],opened[0].1["target"]["targetId"]);
    assert_eq!(tool(&client,&url,&token,"sodeck_task_get",json!({"taskId":job["taskId"],"outputMode":"none"})).await["done"],true);
    f.core.unload("sodeck.mcp").unwrap();assert!(f.core.connection_views().iter().any(|v|v.state=="connected"),"MCP stopping must not disconnect accepted sessions");
}
