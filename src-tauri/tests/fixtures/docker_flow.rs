// Real core + actual native plugin + isolated OpenSSH/Linux Docker Engine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated Docker Engine/OpenSSH; npm run docker:test"]
async fn openssh_docker_plugin() {
    let f = Fixture::new();
    let load = |name: &str| {
        let path = std::env::var(format!("SODECK_TEST_{name}_PLUGIN")).unwrap();
        f.core.load(std::path::Path::new(&path)).unwrap_or_else(|error| {
            panic!("{name} initialization: {error}; state: {:?}; logs: {:?}",
                f.core.snapshot(), f.core.logs(&Default::default()))
        });
    };
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
    load("DOCKER");
    let opened = f
        .core
        .ui_action(
            "main",
            &context,
            json!({"op":"open","key":"sodeck.docker:browser","requestId":"docker-browser"}),
        )
        .unwrap();
    let view = opened["viewId"].as_str().unwrap();
    let mount = f.core.ui_attach("main", view).unwrap();
    let token = mount["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    let event = |token: &str, mut data: Value| {
        if matches!(data["action"].as_str(),Some("openLogs"|"openTerminal"|"operate"|"tunnel.create"|"tunnel.reveal"|"browser.stats")) {
            let current=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();
            data["expectedContext"]=current["context"].clone();data["scopeEpoch"]=current["scopeEpoch"].clone();data["engineId"]=current["engine"]["id"].clone();data["endpoint"]=current["endpoint"].clone();
        }
        f.core.ui_event("main", token, data.clone()).unwrap_or_else(|error|panic!("{data}: {error}")) };
    async fn idle(f: &Fixture, token: &str) -> Value {
        let end = std::time::Instant::now() + Duration::from_secs(40);
        loop {
            let snapshot = f
                .core
                .ui_event("main", token, json!({"action":"snapshot"}))
                .unwrap();
            if snapshot["busy"] == false && (snapshot["kind"]!="browser" || snapshot["state"]!="ready" || !matches!(snapshot["indexes"]["container"]["phase"].as_str(),Some("loading"|"unloaded"))) {
                return snapshot;
            }
            assert!(std::time::Instant::now() < end, "{snapshot}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let browser = idle(&f, token).await;
    assert_eq!(browser["state"], "ready", "{browser}");
    assert_eq!(browser["engine"]["dockerRootDir"],std::env::var("SODECK_TEST_DOCKER_ROOT").unwrap_or_else(|_|"/var/lib/sodeck-test-docker".into()));
    if std::env::var("SODECK_TEST_DOCKER_WINDOWS").is_ok(){assert_eq!(browser["engine"]["clientOs"],"windows");assert_eq!(browser["engine"]["os"],"linux");assert!(browser["endpoint"].as_str().unwrap().starts_with("npipe:"));}
    event(token,json!({"action":"pauseRefresh","paused":true}));
    let id = std::env::var("SODECK_TEST_DOCKER_CONTAINER").unwrap();
    let list=event(token,json!({"action":"browser.page","query":id,"status":"all","group":false}));
    assert!(list["nodes"].as_array().unwrap().iter().any(|n|n["resource"]["id"]==id&&n["health"].is_null()),"containers without HEALTHCHECK must remain visible: {list}");
    async fn observed(f:&Fixture,token:&str,id:&str)->Value {
        f.core.ui_event("main",token,json!({"action":"browser.load","resource":{"kind":"container","id":id}})).unwrap();
        let end=std::time::Instant::now()+Duration::from_secs(40);
        loop{let o=f.core.ui_event("main",token,json!({"action":"browser.object","resource":{"kind":"container","id":id}})).unwrap();if o["relation"]["phase"]=="ready"{return o;}assert!(std::time::Instant::now()<end,"{o}");tokio::time::sleep(Duration::from_millis(50)).await;}
    }
    async fn loaded(f:&Fixture,token:&str,kind:&str)->Value {
        let end=std::time::Instant::now()+Duration::from_secs(60);
        loop {let snapshot=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();if snapshot["indexes"][kind]["phase"]=="unloaded"{f.core.ui_event("main",token,json!({"action":"browser.load","branchKind":kind,"scopeEpoch":snapshot["scopeEpoch"]})).unwrap();}else if snapshot["indexes"][kind]["phase"]!="loading"{assert_eq!(snapshot["indexes"][kind]["phase"],"ready","{snapshot}");return snapshot;}assert!(std::time::Instant::now()<end,"{snapshot}");tokio::time::sleep(Duration::from_millis(50)).await;}
    }
    for kind in ["image","network","volume"] {event(token,json!({"action":"browser.load","branchKind":kind,"scopeEpoch":browser["scopeEpoch"]}));loaded(&f,token,kind).await;}
    let observation=observed(&f,token,&id).await;
    let image_id=observation["info"]["imageId"].as_str().unwrap().to_string();assert!(image_id.starts_with("sha256:"));
    let root_tree=event(token,json!({"action":"browser.page","query":"sodeck-test","status":"all"}));assert!(root_tree["nodes"].as_array().unwrap().iter().any(|n|n["type"]=="root"));
    let detail=token;
    assert!(!f.core.ui_snapshot("main").unwrap().to_string().contains("sodeck.docker:container"));
    async fn tunnel_http(f:&Fixture,address:SocketAddr){
        let mut socket=TcpStream::connect(address).await.unwrap();
        socket.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut body=Vec::new();tokio::time::timeout(Duration::from_secs(20),socket.read_to_end(&mut body)).await.unwrap().unwrap_or_else(|e|panic!("tunnel HTTP {e}; logs: {:?}",f.core.logs(&Default::default())));
        assert!(String::from_utf8_lossy(&body).contains("sodeck-port-tunnel"),"{}",String::from_utf8_lossy(&body));
    }
    let tunnel_address=if std::env::var("SODECK_TEST_DOCKER_TUNNELS").is_ok(){
        load("NETWORK");
        let deadline=std::time::Instant::now()+Duration::from_secs(20);
        loop{
            let shared=f.core.state_surface_request("main",detail,&json!({"op":"list","expectedContext":context,"filter":{"topics":["network.tunnelService.v1"]}})).unwrap();
            if shared["records"].as_array().unwrap().iter().any(|r|r["value"]["ready"]==true){break;}
            assert!(std::time::Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let ports=event(detail,json!({"action":"browser.collection","resource":{"kind":"container","id":id},"collection":"ports","observationRevision":observation["observationRevision"],"limit":20,"offset":0}));
        let binding=ports["rows"].as_array().unwrap().iter().find(|r|r["binding"]["containerPort"]==8080&&r["binding"]["protocol"]=="tcp").expect("actual HTTP publish binding")["binding"].clone();
        let input=json!({"action":"tunnel.create","requestId":"docker-port-once","binding":binding,"selectedRule":null,"id":id,"expectedContext":context,"engineId":browser["engine"]["id"],"endpoint":browser["endpoint"]});
        event(detail,input.clone());event(detail,input.clone());
        let deadline=std::time::Instant::now()+Duration::from_secs(30);
        let receipt=loop{let s=event(detail,json!({"action":"snapshot"}));if s["tunnel"]["status"]!="pending"{break s["tunnel"].clone();}assert!(std::time::Instant::now()<deadline,"{s}");tokio::time::sleep(Duration::from_millis(50)).await;};
        assert_eq!(receipt["status"],"completed","{receipt}");assert_eq!(receipt["result"]["outcome"],"accepted","{receipt}");
        let rule_id=receipt["result"]["ruleId"].as_str().unwrap();
        let address=loop{
            let shared=f.core.state_surface_request("main",detail,&json!({"op":"list","expectedContext":context,"filter":{"topics":["network.tunnelRules.v1"]}})).unwrap();
            let rows=shared["records"].as_array().unwrap();assert!(rows.len()<=1,"Duplicate creation must not save another rule");
            if let Some(value)=rows.iter().find(|r|r["key"]==rule_id&&r["value"]["run"]["snapshot"]["state"]=="running"){
                let address=&value["value"]["run"]["snapshot"]["address"];break format!("{}:{}",address["host"].as_str().unwrap(),address["port"].as_u64().unwrap()).parse::<SocketAddr>().unwrap();
            }
            assert!(std::time::Instant::now()<deadline,"{shared}");tokio::time::sleep(Duration::from_millis(50)).await;
        };
        tunnel_http(&f,address).await;
        let before=f.core.ui_snapshot("main").unwrap();
        let manager=before["workspaces"].as_object().unwrap().values().flat_map(|w|w["views"].as_array().unwrap()).find(|v|v["definitionKey"]=="sodeck.network-proxy:manager").unwrap();assert_eq!(manager["uiRequested"],false,"Creating a tunnel must not display its sidebar");
        event(detail,json!({"action":"tunnel.create","requestId":"docker-port-reuse","binding":binding,"selectedRule":null,"id":id,"expectedContext":context,"engineId":browser["engine"]["id"],"endpoint":browser["endpoint"]}));
        let deadline=std::time::Instant::now()+Duration::from_secs(20);
        loop{let s=event(detail,json!({"action":"snapshot"}));if s["tunnel"]["status"]=="completed"{assert_eq!(s["tunnel"]["result"]["ruleId"],rule_id);assert_eq!(s["tunnel"]["result"]["disposition"],"reused");break;}assert!(std::time::Instant::now()<deadline,"{s}");tokio::time::sleep(Duration::from_millis(50)).await;}
        tunnel_http(&f,address).await;
        Some(address)
    }else{None};
    load("TERMINAL");
    event(token,json!({"action":"openTerminal","requestId":"docker-terminal","shell":"/bin/sh","id":id,"engineId":browser["engine"]["id"],"endpoint":browser["endpoint"],"expectedContext":browser["context"]}));
    let deadline=std::time::Instant::now()+Duration::from_secs(40);
    let receipt=loop {
        let snapshot=event(token,json!({"action":"snapshot"}));
        if snapshot["terminal"]["status"]!="pending" {break snapshot["terminal"].clone();}
        assert!(std::time::Instant::now()<deadline,"{snapshot}");tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(receipt["status"],"completed","{receipt}");
    let terminal_view=receipt["viewId"].as_str().unwrap();let terminal=f.core.ui_attach("main",terminal_view).unwrap();
    let terminal_token=terminal["surfaceId"].as_str().unwrap();let stream=terminal["data"]["streamId"].as_str().unwrap();
    f.core.ui_surface_ready("main",terminal_token).unwrap();let mut output=subscribe(&f.core,"main",terminal_token,stream).await;
    td(&f.core,"main",terminal_token,json!({"op":"write","id":stream,"sequence":0,"data":b"printf 'sodeck-container-terminal:%s\\n' \"$SODECK_TOKEN\"\r".as_slice()})).await.unwrap();
    data_until(&f.core,"main",terminal_token,stream,&mut output,"sodeck-container-terminal:a=b").await;
    assert_eq!(terminal["data"]["integration"]["mode"],"off");
    let close=f.core.ui_close_prepare("main",vec![],Some(view.into())).unwrap();
    f.core.ui_close_finish(close["token"].as_str().unwrap(),serde_json::from_value(close["views"].clone()).unwrap(),"discard").unwrap();
    if let Some(address)=tunnel_address {tunnel_http(&f,address).await;}
    td(&f.core,"main",terminal_token,json!({"op":"write","id":stream,"sequence":1,"data":b"printf 'sodeck-source-closed:%s\\n' \"$SODECK_TOKEN\"\r".as_slice()})).await.unwrap();
    data_until(&f.core,"main",terminal_token,stream,&mut output,"sodeck-source-closed:a=b").await;
    let reopened=f.core.ui_action("main",&context,json!({"op":"open","key":"sodeck.docker:browser","requestId":"docker-browser-again"})).unwrap();
    let remounted=f.core.ui_attach("main",reopened["viewId"].as_str().unwrap()).unwrap();let token=remounted["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",token).unwrap();idle(&f,token).await;
    event(token,json!({"action":"pauseRefresh","paused":true}));
    let detail=token;let observation=observed(&f,detail,&id).await;
    assert!(f.core.ui_event("main",detail,json!({"action":"section","section":"environment"})).is_err());
    event(detail,json!({"action":"browser.stats","resource":{"kind":"container","id":id}}));idle(&f,detail).await;
    let stats=event(detail,json!({"action":"browser.object","resource":{"kind":"container","id":id}}));assert!(stats["stats"]["CPUPerc"].is_string(),"{stats}; {observation}");
    let opened = event(
        detail,
        json!({"action":"openLogs","id":id,"requestId":"docker-logs"}),
    );
    let logs_id = opened["viewId"].as_str().unwrap();
    let mount = f.core.ui_attach("main", logs_id).unwrap();
    let logs = mount["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", logs).unwrap();
    let snapshot = idle(&f, logs).await;
    assert_eq!(snapshot["state"], "ready", "{snapshot}");
    event(logs,json!({"action":"logs.start","requestId":"native-logs","intentSequence":"1","windowSeconds":900,"live":true}));
    let deadline=std::time::Instant::now()+Duration::from_secs(40);
    let capture=loop{let state=event(logs,json!({"action":"logs.status"}));if state["resources"].as_array().is_some_and(|r|r.iter().any(|r|!r["end"].is_null()&&r["status"]["finished"]==true)){break state;}assert!(std::time::Instant::now()<deadline,"{state}");tokio::time::sleep(Duration::from_millis(50)).await;};
    let resource=capture["resources"].as_array().unwrap().iter().find(|r|!r["end"].is_null()).unwrap();
    assert_eq!(resource["status"]["state"],"completed","{resource}");
    let resource_id=resource["id"].as_str().unwrap();let watermark=resource["status"]["producedBytes"].as_str().unwrap();
    async fn captured(core:&Arc<AppCore>,token:&str,id:&str,watermark:&str)->Vec<u8>{let end=watermark.parse::<u64>().unwrap();let mut offset=0u64;let mut bytes=vec![];while offset<end{let value=core.process_output_data("main",token,json!({"id":id,"action":"read","offset":offset.to_string(),"watermark":watermark}),None).await.unwrap();let part:Vec<u8>=serde_json::from_value(value["data"].clone()).unwrap();offset+=part.len() as u64;bytes.extend(part);}bytes}
    let lines=captured(&f.core,logs,resource_id,watermark).await;
    use base64::{engine::general_purpose::STANDARD, Engine};
    let raw=lines.split(|b|*b==b'\n').filter(|l|!l.is_empty()).flat_map(|line|{let envelope:Value=serde_json::from_slice(line).unwrap();STANDARD.decode(envelope["dataBase64"].as_str().unwrap()).unwrap()}).collect::<Vec<_>>();
    assert!(String::from_utf8_lossy(&raw).contains("sodeck-docker-log 中文"));
    assert_eq!(raw.iter().filter(|b|**b==b'\n').count(),2001,"million-line container only returns its finite initial tail");
    assert!(String::from_utf8_lossy(&raw).contains("INFO historical 0999999 中文"));
    assert!(!String::from_utf8_lossy(&raw).contains("INFO historical 0000000 中文"));
    assert!(watermark.parse::<u64>().unwrap()<=4*1024*1024);
    let operate=|mut data:Value| {data["id"]=json!(id);data["engineId"]=browser["engine"]["id"].clone();data["endpoint"]=browser["endpoint"].clone();data["expectedContext"]=browser["context"].clone();event(token,data)};
    let status=||event(token,json!({"action":"browser.page","query":"","group":false,"expandedKeys":["engine","category:container"]}))["nodes"].as_array().unwrap().iter().find(|row|row["resource"]["id"]==id).map(|row|row["status"].clone()).unwrap_or(Value::Null);
    operate(
        json!({"action":"operate","requestId":"docker-pause","intentSequence":"1","operation":"pause"}),
    );
    idle(&f,token).await;assert_eq!(status(),"paused");
    operate(
        json!({"action":"operate","requestId":"docker-unpause","intentSequence":"2","operation":"unpause"}),
    );
    idle(&f,token).await;assert_eq!(status(),"running");
    let stop = json!({"action":"operate","requestId":"docker-stop-once","intentSequence":"3","operation":"stop"});
    operate(stop.clone());
    operate(stop);
    let stopped = idle(&f, token).await;
    assert_eq!(status(), "exited", "{stopped}");
    assert_eq!(stopped["operation"]["state"], "succeeded");
    operate(
        json!({"action":"operate","requestId":"docker-start","intentSequence":"4","operation":"start"}),
    );
    idle(&f,token).await;assert_eq!(status(),"running");
    operate(
        json!({"action":"operate","requestId":"docker-restart","intentSequence":"5","operation":"restart"}),
    );
    assert_eq!(idle(&f, token).await["operation"]["state"], "succeeded");
    operate(
        json!({"action":"operate","requestId":"docker-stop-again","intentSequence":"6","operation":"stop"}),
    );
    idle(&f, token).await;
    operate(
        json!({"action":"operate","requestId":"docker-remove","intentSequence":"7","operation":"remove"}),
    );
    assert_eq!(idle(&f, token).await["operation"]["state"], "succeeded");
    assert!(status().is_null(),"removed container must leave the browser list");
    assert_eq!(idle(&f,logs).await["state"],"gone","existing logs reflect sidebar deletion");
    async fn proof(f:&Fixture,token:&str,resource:&Value)->Value {
        let snapshot=f.core.ui_event("main",token,json!({"action":"snapshot"})).unwrap();f.core.ui_event("main",token,json!({"action":"browser.load","resource":resource,"scopeEpoch":snapshot["scopeEpoch"]})).unwrap();
        let end=std::time::Instant::now()+Duration::from_secs(60);loop {let result=f.core.ui_event("main",token,json!({"action":"browser.object","resource":resource})).unwrap();if result["relation"]["phase"]!="loading" {assert_eq!(result["relation"]["complete"],true,"{result}");return result;}assert!(std::time::Instant::now()<end,"{result}");tokio::time::sleep(Duration::from_millis(50)).await;}
    }
    for kind in ["image","network","volume"] {loaded(&f,token,kind).await;}
    for (kind,target) in [("volume",std::env::var("SODECK_TEST_DOCKER_FREE_VOLUME").unwrap()),("network",std::env::var("SODECK_TEST_DOCKER_NETWORK").unwrap()),("image",image_id)] {
        let resource=json!({"kind":kind,"id":target});let observation=proof(&f,token,&resource).await;assert_eq!(observation["relation"]["count"],0);
        let snapshot=event(token,json!({"action":"snapshot"}));let input=json!({"action":"resource.operate","operation":"remove","resource":resource,"observationRevision":observation["revision"],"requestId":format!("delete-{kind}"),"intentSequence":snapshot["nextIntent"],"scopeEpoch":snapshot["scopeEpoch"],"engineId":snapshot["engine"]["id"],"endpoint":snapshot["endpoint"],"expectedContext":context});
        event(token,input.clone());event(token,input);let result=idle(&f,token).await;
        assert_eq!(result["operation"]["state"],if kind=="image"{"failed"}else{"succeeded"},"{result}");
        if kind=="image" {assert!(result["operation"]["message"].as_str().unwrap().contains("conflict"),"{result}");event(token,json!({"action":"refresh"}));idle(&f,token).await;}
        for kind in ["image","network","volume"] {loaded(&f,token,kind).await;}
    }
    let snapshot=event(token,json!({"action":"snapshot"}));let pull=json!({"action":"pull.start","reference":std::env::var("SODECK_TEST_DOCKER_PULL").unwrap(),"requestId":"real-registry-pull","intentSequence":snapshot["nextIntent"],"scopeEpoch":snapshot["scopeEpoch"],"engineId":snapshot["engine"]["id"],"endpoint":snapshot["endpoint"],"expectedContext":context});
    event(token,pull.clone());event(token,pull);let end=std::time::Instant::now()+Duration::from_secs(90);
    loop {let snapshot=event(token,json!({"action":"snapshot"}));if !matches!(snapshot["pull"]["state"].as_str(),Some("opening"|"running")){assert_eq!(snapshot["pull"]["state"],"succeeded","{snapshot}");break;}assert!(std::time::Instant::now()<end,"{snapshot}");tokio::time::sleep(Duration::from_millis(100)).await;}
    f.core.disconnect_connection("main", &context).unwrap();
    event(logs, json!({"action":"refresh"}));
    assert!(idle(&f, logs).await["error"].is_string());
    let retained = captured(&f.core,logs,resource_id,watermark).await;
    assert_eq!(
        retained, lines,
        "disconnect must retain log snapshot"
    );
    let close = f
        .core
        .ui_close_prepare("main", vec![context.workspace_id.clone()], None)
        .unwrap();
    f.core
        .ui_close_finish(
            close["token"].as_str().unwrap(),
            serde_json::from_value(close["views"].clone()).unwrap(),
            "discard",
        )
        .unwrap();
    if let Some(address)=tunnel_address {
        let deadline=std::time::Instant::now()+Duration::from_secs(10);loop {if let Ok(listener)=TcpListener::bind(address).await{drop(listener);break;}assert!(std::time::Instant::now()<deadline,"tunnel port was not released");tokio::time::sleep(Duration::from_millis(50)).await;}
    }
    f.core.shutdown().unwrap();
}
