#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn target_facts_follow_authenticated_context_without_catalog_fallback() {
    let f = Fixture::new();
    let ssh = SshServer::new().await;
    let replacement = SshServer::new().await;
    let server = f.server("127.0.0.1", ssh.addr.port(), json!([]));
    let first = connected(&f, &server).await.context.unwrap();
    let (view, token, _) = terminal_fixture(&f);
    let invoke = |method: &str, p: Value| {
        f.core
            .ui_event("main", &token, json!({"rpc":method,"params":p}))
            .unwrap()
    };
    let summary = invoke("session.target.describe", json!({}));
    assert_eq!(summary["context"], json!(first));
    assert_eq!(summary["targetSource"], "current");
    assert_eq!(summary["target"]["host"], "127.0.0.1");
    assert_eq!(summary["target"]["port"], ssh.addr.port());
    assert_eq!(summary["target"]["username"], "ops");
    let legacy = invoke("session.state", json!({}));
    assert!(legacy["target"].get("host").is_none());
    assert!(legacy.get("targetSource").is_none());
    assert_eq!(legacy["target"]["endpoint"], summary["target"]["endpoint"]);
    let foreign = invoke(
        "session.target.describe",
        json!({"viewId":"foreign","expectedContext":first}),
    );
    assert_eq!(foreign["rpcError"]["data"]["code"], "staleContext");
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["name"] = json!("renamed");
    draft["port"] = json!(replacement.addr.port());
    draft["authentication"] = json!({"type":"password","secret":{"action":"keep","remember":true}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let edited = invoke("session.target.describe", json!({}));
    assert_eq!(edited["target"]["name"], "renamed");
    assert_eq!(edited["target"]["port"], ssh.addr.port());
    f.core.disconnect_connection("main", &first).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    let last = invoke("session.target.describe", json!({}));
    assert_eq!(last["targetSource"], "lastConnected");
    assert_eq!(last["target"], edited["target"]);
    assert_eq!(last["state"], "disconnected");
    assert_eq!(last["capabilities"], json!([]));
    assert!(invoke("session.state", json!({}))["target"].is_null());
    for accept in [false, true] {
        let attempt = f.core.start_connection("main", &server).unwrap();
        until(|| f.core.connection_views()[0].question.is_some()).await;
        let question = f.core.connection_views()[0].question.clone().unwrap();
        let Question::TargetChange { id, .. } = question else {
            panic!("changed target must be confirmed")
        };
        f.core
            .answer_connection(
                "main",
                &attempt.attempt_id,
                &id,
                Answer::TargetChange { accept },
            )
            .unwrap();
        if !accept {
            until(|| f.core.connection_views()[0].state == "cancelled").await;
            assert_eq!(
                invoke("session.target.describe", json!({})),
                last,
                "failed reconnect keeps prior facts"
            );
        }
    }
    let mut answered = HashSet::new();
    until(|| {
        let v = f.core.connection_views()[0].clone();
        if let Some(Question::HostKey { id, .. }) = &v.question {
            if answered.insert(id.clone()) {
                f.core
                    .answer_connection("main", &v.attempt_id, id, Answer::HostKey { accept: true })
                    .unwrap();
            }
        } else if let Some(Question::Credentials { id, .. }) = &v.question {
            if answered.insert(id.clone()) {
                submit_credentials(&f, &v, "secret", true);
            }
        } else if v
            .question
            .as_ref()
            .is_some_and(|q| !matches!(q, Question::TargetChange { .. }))
        {
            panic!("unexpected reconnect question: {v:?}");
        }
        assert!(
            v.state != "failed" && v.state != "cancelled",
            "reconnect failed: {v:?}"
        );
        v.state == "connected"
    })
    .await;
    let second = f.core.connection_views()[0].context.clone().unwrap();
    assert_eq!(second.generation, 2);
    assert_ne!(first.connection_id, second.connection_id);
    // Rebinding revokes the old surface; read the retained view through its new surface.
    let mounted = f.core.ui_attach("main", &view).unwrap();
    let next_token = mounted["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", next_token).unwrap();
    let read = |p: Value| {
        f.core
            .ui_event(
                "main",
                next_token,
                json!({"rpc":"session.target.describe","params":p}),
            )
            .unwrap()
    };
    assert_eq!(
        read(json!({"expectedContext":first}))["rpcError"]["data"]["code"],
        "staleContext"
    );
    let current = read(json!({}));
    assert_eq!(current["context"], json!(second));
    assert_eq!(current["target"]["port"], replacement.addr.port());
    assert!(
        f.core
            .change_catalog(
                f.core.server_catalog()["revision"].as_u64().unwrap(),
                serde_json::from_value(json!({"op":"deleteServer","id":server})).unwrap()
            )
            .is_err(),
        "active workspace prevents catalog deletion"
    );
    assert_eq!(read(json!({}))["target"], current["target"]);
    close_terminal_views(&f);
    f.core.shutdown().unwrap();
}
