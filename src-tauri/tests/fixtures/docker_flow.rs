// Real core + actual native plugin + isolated OpenSSH/Linux Docker Engine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated Docker Engine/OpenSSH; npm run docker:test"]
async fn openssh_docker_plugin() {
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
            &std::env::var("SODECK_TEST_DOCKER_PLUGIN").unwrap(),
        ))
        .unwrap();
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
    let event = |token: &str, data: Value| f.core.ui_event("main", token, data.clone()).unwrap_or_else(|error|panic!("{data}: {error}"));
    async fn idle(f: &Fixture, token: &str) -> Value {
        let end = std::time::Instant::now() + Duration::from_secs(40);
        loop {
            let snapshot = f
                .core
                .ui_event("main", token, json!({"action":"snapshot"}))
                .unwrap();
            if snapshot["busy"] == false {
                return snapshot;
            }
            assert!(std::time::Instant::now() < end, "{snapshot}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let browser = idle(&f, token).await;
    assert_eq!(browser["state"], "ready", "{browser}");
    event(token,json!({"action":"pauseRefresh","paused":true}));
    let list = event(token, json!({"action":"list","status":"all"}));
    let id = std::env::var("SODECK_TEST_DOCKER_CONTAINER").unwrap();
    assert!(
        list["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == id && r["project"] == "sodeck-test"),
        "{list}; {browser}"
    );
    assert!(
        list["rows"][0]["health"].is_null(),
        "containers without HEALTHCHECK must remain visible"
    );
    let opened = event(
        token,
        json!({"action":"openContainer","id":id,"requestId":"docker-detail"}),
    );
    let detail_id = opened["viewId"].as_str().unwrap();
    let mounted = f.core.ui_attach("main", detail_id).unwrap();
    let detail = mounted["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", detail).unwrap();
    assert_eq!(idle(&f, detail).await["status"], "running");
    f.core.load(std::path::Path::new(&std::env::var("SODECK_TEST_TERMINAL_PLUGIN").unwrap())).unwrap();
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
    td(&f.core,"main",terminal_token,json!({"op":"write","id":stream,"sequence":1,"data":b"printf 'sodeck-source-closed:%s\\n' \"$SODECK_TOKEN\"\r".as_slice()})).await.unwrap();
    data_until(&f.core,"main",terminal_token,stream,&mut output,"sodeck-source-closed:a=b").await;
    let reopened=f.core.ui_action("main",&context,json!({"op":"open","key":"sodeck.docker:browser","requestId":"docker-browser-again"})).unwrap();
    let remounted=f.core.ui_attach("main",reopened["viewId"].as_str().unwrap()).unwrap();let token=remounted["surfaceId"].as_str().unwrap();f.core.ui_surface_ready("main",token).unwrap();idle(&f,token).await;
    event(token,json!({"action":"pauseRefresh","paused":true}));
    let env = event(detail, json!({"action":"section","section":"environment"}));
    assert!(
        env["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == "SODECK_TOKEN" && r["value"] == "a=b" && r["secret"] == true),
        "{env}"
    );
    event(detail, json!({"action":"stats"}));
    let stats = idle(&f, detail).await;
    assert!(stats["stats"]["CPUPerc"].is_string(), "{stats}");
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
    let lines = event(logs, json!({"action":"readLogs"}));
    assert!(
        lines["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["text"].as_str().unwrap().contains("sodeck-docker-log")),
        "{lines}"
    );
    let operate=|mut data:Value| {data["id"]=json!(id);data["engineId"]=browser["engine"]["id"].clone();data["endpoint"]=browser["endpoint"].clone();data["expectedContext"]=browser["context"].clone();event(token,data)};
    let status=||event(token,json!({"action":"list","status":"all"}))["rows"].as_array().unwrap().iter().find(|row|row["id"]==id).map(|row|row["status"].clone()).unwrap_or(Value::Null);
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
    assert_eq!(idle(&f,detail).await["state"],"gone","existing details reflect sidebar deletion");
    f.core.disconnect_connection("main", &context).unwrap();
    event(logs, json!({"action":"refresh"}));
    assert!(idle(&f, logs).await["error"].is_string());
    let retained = event(logs, json!({"action":"readLogs"}));
    assert_eq!(
        retained["rows"], lines["rows"],
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
    f.core.shutdown().unwrap();
}
