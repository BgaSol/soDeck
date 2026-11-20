#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn external_connection_requests_share_transport_and_keep_original_identity(){
    let f=Fixture::new();let first=SshServer::new().await;let second=SshServer::new().await;
    let first_id=f.server("127.0.0.1",first.addr.port(),json!([]));
    let original=connected(&f,&first_id).await.context.unwrap();
    let (_,token,_)=terminal_fixture(&f);
    let call=|method:&str,params:Value|f.core.ui_event("main",&token,json!({"rpc":method,"params":params})).unwrap();
    let second_id=f.server("127.0.0.1",second.addr.port(),json!([]));
    let page=call("servers.list",json!({"serverId":second_id}));
    let profile=page["servers"][0]["profileToken"].clone();
    assert!(page.to_string().find("secret").is_none());
    assert!(page["servers"][0]["connection"].is_null());
    assert_eq!(call("connections.cancel",json!({"requestId":"early-cancel"}))["receipt"]["state"],"cancelled");
    let input=|id:&str|json!({"requestId":id,"serverId":second_id,"expectedProfileToken":profile});
    let mut invalid_view=input("invalid-view");invalid_view["viewType"]=json!("missing");
    assert_eq!(call("connections.open",invalid_view)["rpcError"]["data"]["code"],"invalidParams");
    assert_eq!(call("servers.list",json!({"serverId":second_id,"viewType":"missing"}))["rpcError"]["data"]["code"],"invalidParams");
    assert_eq!(f.core.connection_views().len(),1,"invalid background projection must be rejected before network admission");
    assert_eq!(call("connections.open",input("early-cancel"))["receipt"]["state"],"cancelled");
    assert_eq!(f.core.connection_views().len(),1);
    let a=call("connections.open",input("open-a"));let b=call("connections.open",input("open-b"));
    assert_eq!(a["receipt"]["attemptId"],b["receipt"]["attemptId"]);
    assert_eq!(call("connections.open",input("open-b"))["receipt"]["attemptId"],b["receipt"]["attemptId"]);
    assert_eq!(call("connections.cancel",json!({"requestId":"open-a"}))["receipt"]["state"],"cancelled");
    let attempt=b["receipt"]["attemptId"].as_str().unwrap().to_owned();
    let mut answered=HashSet::new();
    until(||{let view=f.core.connection_views().into_iter().find(|v|v.attempt_id==attempt).unwrap();
        if let Some(Question::HostKey{id,..})=view.question{if answered.insert(id.clone()){f.core.answer_connection("main",&attempt,&id,Answer::HostKey{accept:true}).unwrap();}}
        assert_ne!(view.state,"cancelled");assert_ne!(view.state,"failed");view.state=="connected"}).await;
    let accepted=call("connections.get",json!({"requestId":"open-b"}));assert_eq!(accepted["receipt"]["state"],"accepted");
    assert_eq!(second.state.auth.load(Ordering::Relaxed),1);
    assert_eq!(call("connections.cancel",json!({"requestId":"open-b"}))["receipt"],accepted["receipt"]);
    f.password(&second_id,Some("new-password"));
    let changed=call("servers.list",json!({"serverId":second_id}));
    assert_eq!(changed["servers"][0]["connection"]["matchesSavedTarget"],true);
    assert_eq!(call("connections.open",json!({"requestId":"reuse","serverId":second_id,"expectedProfileToken":changed["servers"][0]["profileToken"]}))["receipt"],accepted["receipt"]);
    assert_eq!(second.state.auth.load(Ordering::Relaxed),1);
    let context: sodeck::workspaces::Context=serde_json::from_value(accepted["receipt"]["context"].clone()).unwrap();
    f.core.disconnect_connection("main",&context).unwrap();until(||f.core.connection_views().iter().any(|v|v.server_id==second_id&&v.state=="disconnected")).await;
    assert_eq!(call("connections.get",json!({"requestId":"open-b"}))["receipt"],accepted["receipt"]);
    assert_eq!(call("connections.get",json!({"requestId":"open-b"}))["current"]["state"],"disconnected");
    assert_eq!(f.core.connection_views().iter().find(|v|v.server_id==first_id).unwrap().context.as_ref(),Some(&original));
    let mut conflict=input("open-b");conflict["expectedProfileToken"]=json!("different");assert_eq!(call("connections.open",conflict)["rpcError"]["data"]["code"],"requestConflict");
    let paged=call("servers.list",json!({"limit":1}));assert_eq!(paged["hasMore"],true);
    f.password(&first_id,Some("edited"));assert_eq!(call("servers.list",json!({"limit":1,"cursor":paged["nextCursor"]}))["rpcError"]["data"]["code"],"catalogChanged");
    f.core.unload("test.terminal").unwrap();
}
