mod support;
use serde_json::{json,Value};
use sodeck_core::{Core,Caller};
fn call(c:&Core,m:&str,p:Value)->Value{c.call(&Caller::shell(),m,p).unwrap()}
fn config()->Value{json!({"name":"测试代理","host":"proxy.invalid","port":1080,"authType":"usernamePassword","username":"test"})}
fn server(c:&Core,path:Value,deps:Value,op:&str)->Value{call(c,"servers.catalog.save",json!({"operationId":op,"configuration":{"name":"测试","host":"test.invalid","port":22,"username":"tester","authType":"password","accessPath":path},"groupId":null,"expectedDependencies":deps}))}

#[test]
fn proxy_receipts_restart_conflict_dependencies_and_v7(){
    let dir=tempfile::tempdir().unwrap();let c=Core::open(dir.path()).unwrap();
    let request=json!({"operationId":"create","configuration":config()});let saved=call(&c,"proxies.catalog.save",request.clone());
    let revision=c.catalog_revision().unwrap();assert_eq!(call(&c,"proxies.catalog.save",request.clone()),saved);assert_eq!(c.catalog_revision().unwrap(),revision);
    let mut wrong=request.clone();wrong["configuration"]["host"]=json!("changed.invalid");assert_eq!(c.call(&Caller::shell(),"proxies.catalog.save",wrong).unwrap_err().code,"OPERATION_CONFLICT");
    let proxy=&saved["proxy"];let deps=json!([{"stepId":"00000000-0000-4000-8000-000000000001","kind":"socks5","id":proxy["id"],"host":proxy["host"],"port":1080,"username":"test","authType":"usernamePassword"}]);
    server(&c,json!({"steps":[{"stepId":"00000000-0000-4000-8000-000000000001","kind":"socks5","proxyId":proxy["id"],"resolve":"proxy"}]}),deps,"target");
    assert_eq!(call(&c,"proxies.catalog.list",json!({}))["items"][0]["referenceCount"],1);
    assert_eq!(c.call(&Caller::shell(),"proxies.catalog.remove",json!({"operationId":"remove","proxyId":proxy["id"],"expectedRevision":1})).unwrap_err().code,"PROXY_DEPENDENTS");
    drop(c);let c=Core::open(dir.path()).unwrap();assert_eq!(call(&c,"proxies.catalog.save",request),saved);
    assert_eq!(call(&c,"proxies.catalog.operation",json!({"operationId":"create"}))["result"],saved);
    assert_eq!(sodeck_core::store_bootstrap::SCHEMA_VERSION,7);
}

#[test]
fn credential_conditions_idempotency_and_no_persistent_secret_receipt(){
    let dir=tempfile::tempdir().unwrap();let c=Core::open(dir.path()).unwrap();let s=server(&c,json!({"steps":[]}),json!([]),"server");
    let prepare=json!({"target":{"kind":"ssh","id":s["server"]["id"]},"expectedOwnerRevision":1,"expectedDependencies":[]});
    let first=call(&c,"credentials.configuration.prepare",prepare.clone());
    support::prefill(&c,"ssh",&s["server"]["id"],json!({"password":"isolated-challenge","remember":false}));
    assert_eq!(c.call(&Caller::shell(),"credentials.configuration.set",json!({"operationId":"stale","contextToken":first["contextToken"],"password":"old-secret","remember":false})).unwrap_err().code,"REVISION_CONFLICT");
    let second=call(&c,"credentials.configuration.prepare",prepare);let request=json!({"operationId":"secret-once","contextToken":second["contextToken"],"password":"new-test-secret","remember":false});
    let stored=call(&c,"credentials.configuration.set",request.clone());let revision=c.catalog_revision().unwrap();
    assert_eq!(call(&c,"credentials.configuration.set",request.clone()),stored);assert_eq!(c.catalog_revision().unwrap(),revision);
    assert_eq!(call(&c,"credentials.configuration.operation",json!({"operationId":"secret-once"}))["status"],"stored");
    let mut wrong=request;wrong["password"]=json!("different");assert_eq!(c.call(&Caller::shell(),"credentials.configuration.set",wrong).unwrap_err().code,"OPERATION_CONFLICT");
    drop(c);let c=Core::open(dir.path()).unwrap();assert_eq!(call(&c,"credentials.configuration.operation",json!({"operationId":"secret-once"}))["status"],"unknown");
}

#[test]
fn dependency_identity_ignores_name_but_rejects_account_change_before_commit(){
    let dir=tempfile::tempdir().unwrap();let c=Core::open(dir.path()).unwrap();let jump=server(&c,json!({"steps":[]}),json!([]),"jump");let id=&jump["server"]["id"];
    let deps=json!([{"stepId":"00000000-0000-4000-8000-000000000001","kind":"ssh","id":id,"host":"test.invalid","port":22,"username":"tester","authType":"password"}]);
    let request=json!({"operationId":"target","configuration":{"name":"target","host":"target.invalid","port":22,"username":"tester","authType":"password","accessPath":json!({"steps":[{"stepId":"00000000-0000-4000-8000-000000000001","kind":"ssh","serverId":id}]})},"groupId":null,"expectedDependencies":deps});
    let update=json!({"operationId":"rename","serverId":id,"expectedServerRevision":1,"configuration":{"name":"新名称","host":"test.invalid","port":22,"username":"tester","authType":"password","accessPath":{"steps":[]}}});call(&c,"servers.catalog.save",update.clone());
    let saved=call(&c,"servers.catalog.save",request.clone());
    let prepared=call(&c,"credentials.configuration.prepare",json!({"target":{"kind":"ssh","id":saved["server"]["id"]},"expectedOwnerRevision":1,"expectedDependencies":deps}));
    let mut update=update;update["operationId"]=json!("account");update["expectedServerRevision"]=json!(2);update["configuration"]["username"]=json!("other");call(&c,"servers.catalog.save",update);
    assert_eq!(call(&c,"servers.catalog.save",request.clone())["server"]["id"],saved["server"]["id"]);
    let mut fresh=request;fresh["operationId"]=json!("fresh");assert_eq!(c.call(&Caller::shell(),"servers.catalog.save",fresh).unwrap_err().code,"REVISION_CONFLICT");
    assert_eq!(c.call(&Caller::shell(),"credentials.configuration.set",json!({"operationId":"stale-target-secret","contextToken":prepared["contextToken"],"password":"isolated","remember":false})).unwrap_err().code,"REVISION_CONFLICT");
}

#[test]
fn credential_directory_failure_is_pending_and_retried_without_secret_rotation(){
    let dir=tempfile::tempdir().unwrap();let c=Core::open(dir.path()).unwrap();let s=server(&c,json!({"steps":[]}),json!([]),"server");
    let prepare=json!({"target":{"kind":"ssh","id":s["server"]["id"]},"expectedOwnerRevision":1,"expectedDependencies":[]});
    let token=call(&c,"credentials.configuration.prepare",prepare.clone());
    let db=rusqlite::Connection::open(dir.path().join("core.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER catalog_failure BEFORE UPDATE ON settings WHEN NEW.key='serverCatalogRevision' BEGIN SELECT RAISE(FAIL,'isolated failure'); END;").unwrap();
    let request=json!({"operationId":"pending-secret","contextToken":token["contextToken"],"password":"isolated","remember":false});
    let stored=call(&c,"credentials.configuration.set",request.clone());assert_eq!(stored["postCommitPending"],true);
    db.execute_batch("DROP TRIGGER catalog_failure;").unwrap();
    let before=call(&c,"credentials.configuration.prepare",prepare);
    assert_eq!(call(&c,"credentials.configuration.set",request)["stored"],true);
    assert!(call(&c,"credentials.configuration.operation",json!({"operationId":"pending-secret"}))["result"]["postCommitPending"].is_null());
    // A context captured after the first write remains valid after replay/notification recovery.
    call(&c,"credentials.configuration.set",json!({"operationId":"next-secret","contextToken":before["contextToken"],"password":"next-isolated","remember":false}));
}

#[test]
fn proxy_db_failure_preserves_old_configuration_and_credentials(){
    let dir=tempfile::tempdir().unwrap();let c=Core::open(dir.path()).unwrap();let proxy=call(&c,"proxies.catalog.save",json!({"operationId":"proxy","configuration":config()}))["proxy"].clone();
    support::prefill(&c,"socks5",&proxy["id"],json!({"password":"isolated","remember":true}));
    let db=rusqlite::Connection::open(dir.path().join("core.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER proxy_failure BEFORE UPDATE ON socks_proxies BEGIN SELECT RAISE(FAIL,'isolated failure'); END;").unwrap();
    let mut configuration=config();configuration["username"]=json!("changed");
    let error=c.call(&Caller::shell(),"proxies.catalog.save",json!({"operationId":"failed","proxyId":proxy["id"],"expectedRevision":1,"configuration":configuration})).unwrap_err();
    assert_eq!(error.code,"STORAGE_ERROR");
    assert_eq!(call(&c,"proxies.catalog.list",json!({}))["items"][0]["proxy"]["hasPreset"],true);
    assert_eq!(call(&c,"proxies.catalog.list",json!({}))["items"][0]["proxy"]["username"],"test");
    assert_eq!(call(&c,"proxies.catalog.operation",json!({"operationId":"failed"}))["status"],"unknown");
}
