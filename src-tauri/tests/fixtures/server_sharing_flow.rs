#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_mixed_route_authenticates_independently_after_source_key_is_removed() {
    let source = Fixture::new();
    let destination = Fixture::new();
    let gateway = SshServer::new().await;
    let target = SshServer::new().await;
    let proxy = Socks::independent(gateway.addr, true).await;
    gateway
        .state
        .routes
        .lock()
        .unwrap()
        .insert("127.0.0.1".into(), target.addr);
    let proxy_id=source.change(json!({"op":"saveProxy","draft":{"name":"shared proxy","host":"127.0.0.1","port":proxy.addr.port(),"authentication":{"type":"usernamePassword","username":"proxy","secret":{"action":"replace","value":"proxy-secret","remember":true}}}}))["selectedId"].as_str().unwrap().to_string();
    let hop = source.server("127.0.0.1", gateway.addr.port(), json!([]));
    let id = source.server(
        "127.0.0.1",
        target.addr.port(),
        json!([{"kind":"socks5","id":"","proxyId":proxy_id},{"kind":"ssh","id":"","serverId":hop}]),
    );
    let path = source.path.join("share-key");
    let encrypted = target.key.encrypt(&mut rand::rng(), "key-phrase").unwrap();
    std::fs::write(
        &path,
        encrypted
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let mut draft = source.core.server_catalog()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
        .clone();
    draft["authentication"] = json!({"type":"privateKey","privateKeyPath":path,"secret":{"action":"replace","value":"key-phrase","remember":true}});
    source.change(json!({"op":"saveServer","draft":draft}));
    let revision = source.core.server_catalog()["revision"].as_u64().unwrap();
    let exported = source.core.export_server_share(revision, Some(id)).unwrap();
    let code = exported["code"].as_str().unwrap();
    let preview = destination.core.preview_server_share(code).unwrap();
    let result = destination
        .core
        .import_server_share(preview["catalogRevision"].as_u64().unwrap(), code)
        .unwrap();
    std::fs::remove_file(path).unwrap();
    let imported = result["targetIds"][0].as_str().unwrap();
    let context = connected(&destination, imported).await.context.unwrap();
    assert_eq!(
        destination
            .core
            .connection_probe(&context, "shared-route")
            .await
            .unwrap()
            .0,
        b"shared-route"
    );
    destination
        .core
        .disconnect_connection("main", &context)
        .unwrap();
}
