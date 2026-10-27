use serde_json::{json,Value};
use sodeck_core::{Core,Caller};
fn call(core:&Core,method:&str,p:Value)->Value {core.call(&Caller::shell(),method,p).unwrap()}
fn group(core:&Core,name:&str)->Value {call(core,"serverGroups.create",json!({"operationId":uuid::Uuid::new_v4().to_string(),"name":name}))}
fn configuration(name:&str)->Value {json!({"name":name,"host":"test.invalid","port":22,"username":"tester","authType":"password","connectionRoute":{"kind":"direct"}})}
fn save(core:&Core,group:&Value)->Value {call(core,"servers.catalog.save",json!({"operationId":uuid::Uuid::new_v4().to_string(),"configuration":configuration("测试服务器"),"groupId":group}))}

#[test]
fn atomic_membership_legacy_updates_and_deletion() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let group=group(&core,"生产");let server=save(&core,&group["group"]["id"]);
    call(&core,"auth.set",json!({"serverId":server["server"]["id"],"password":"isolated-test","remember":false}));
    let moved=call(&core,"servers.catalog.save",json!({"operationId":"move","serverId":server["server"]["id"],"expectedMembershipRevision":server["membershipRevision"],"groupId":null}));
    assert_eq!(moved["server"]["revision"],server["server"]["revision"]);
    let mut old=configuration("旧API改名");old["serverId"]=server["server"]["id"].clone();old["expectedRevision"]=server["server"]["revision"].clone();
    call(&core,"servers.routes.v2.update",old);
    let again=call(&core,"servers.catalog.save",json!({"operationId":"move-back","serverId":server["server"]["id"],"expectedMembershipRevision":moved["membershipRevision"],"groupId":group["group"]["id"]}));
    assert_eq!(again["server"]["name"],"旧API改名");
    let listed=call(&core,"serverGroups.list",json!({}));assert_eq!(listed["groups"][0]["serverCount"],1);
    let deleted=call(&core,"serverGroups.delete",json!({"operationId":"delete-group","groupId":group["group"]["id"],"expectedRevision":group["group"]["revision"],"expectedCatalogRevision":listed["catalogRevision"]}));assert_eq!(deleted["movedCount"],1);
    let current=call(&core,"servers.catalog.get",json!({"serverId":server["server"]["id"]}));assert_eq!(current["groupId"],Value::Null);assert_eq!(current["server"]["revision"],again["server"]["revision"]);
    assert!(core.resources()["connections"].as_array().unwrap().is_empty());
    drop(core);let core=Core::open(dir.path()).unwrap();assert_eq!(call(&core,"servers.catalog.list",json!({}))["items"].as_array().unwrap().len(),1);
}

#[test]
fn receipt_survives_restart_and_rejects_conflicting_replay() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let request=json!({"operationId":"save-once","configuration":configuration("一次"),"groupId":null});
    let saved=call(&core,"servers.catalog.save",request.clone());drop(core);
    let core=Core::open(dir.path()).unwrap();assert_eq!(call(&core,"servers.catalog.save",request.clone()),saved);
    assert_eq!(call(&core,"servers.catalog.operation",json!({"operationId":"save-once"}))["result"],saved);
    let mut conflict=request;conflict["configuration"]["name"]=json!("其他");
    assert_eq!(core.call(&Caller::shell(),"servers.catalog.save",conflict).unwrap_err().code,"OPERATION_CONFLICT");
    assert_eq!(call(&core,"servers.catalog.list",json!({}))["items"].as_array().unwrap().len(),1);
}

#[test]
fn deleted_group_rolls_back_configuration_and_stale_cursor_fails() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let g=group(&core,"开发");let saved=save(&core,&g["group"]["id"]);save(&core,&Value::Null);
    let page=call(&core,"servers.catalog.list",json!({"limit":1}));
    call(&core,"serverGroups.delete",json!({"operationId":"delete","groupId":g["group"]["id"],"expectedRevision":g["group"]["revision"],"expectedCatalogRevision":page["catalogRevision"]}));
    assert_eq!(core.call(&Caller::shell(),"servers.catalog.list",json!({"limit":1,"cursor":page["nextCursor"]})).unwrap_err().code,"STALE_CURSOR");
    let current=call(&core,"servers.catalog.get",json!({"serverId":saved["server"]["id"]}));
    let error=core.call(&Caller::shell(),"servers.catalog.save",json!({"operationId":"fail","serverId":saved["server"]["id"],"configuration":configuration("不应保存"),"expectedServerRevision":saved["server"]["revision"],"groupId":g["group"]["id"],"expectedMembershipRevision":current["membershipRevision"]})).unwrap_err();assert_eq!(error.code,"GROUP_NOT_FOUND");
    assert_eq!(call(&core,"servers.get",json!({"serverId":saved["server"]["id"]}))["name"],"测试服务器");
}

#[test]
fn names_normalize_and_unknown_fields_fail() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();group(&core," Café ");
    assert_eq!(core.call(&Caller::shell(),"serverGroups.create",json!({"operationId":"duplicate","name":"CAFE\u{301}"})).unwrap_err().code,"GROUP_NAME_CONFLICT");
    assert_eq!(core.call(&Caller::shell(),"serverGroups.list",json!({"unknown":true})).unwrap_err().code,"INVALID_PARAMS");
    assert_eq!(core.call(&Caller::shell(),"serverGroups.create",json!({"operationId":"control","name":"a\nb"})).unwrap_err().code,"INVALID_PARAMS");
}

#[test]
fn credential_cleanup_failure_retains_committed_identity_and_can_resume() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let saved=save(&core,&Value::Null);
    let id=saved["server"]["id"].as_str().unwrap();let blocked=dir.path().join("secrets").join(format!("{id}.dpapi"));
    // A directory at the encrypted-file path makes removal fail on every OS.
    std::fs::create_dir_all(&blocked).unwrap();
    let mut config=configuration("已提交");config["host"]=json!("changed.invalid");
    let request=json!({"operationId":"cleanup-once","serverId":id,"expectedServerRevision":1,"configuration":config});
    let failure=core.call(&Caller::shell(),"servers.catalog.save",request.clone()).unwrap_err();
    assert_eq!(failure.details.as_ref().unwrap()["stage"],"credentialCleanup");
    let pending=call(&core,"servers.catalog.operation",json!({"operationId":"cleanup-once"}));
    assert_eq!(pending["result"]["postCommitPending"],true);assert_eq!(pending["result"]["server"]["revision"],2);
    std::fs::remove_dir(&blocked).unwrap();
    let resumed=call(&core,"servers.catalog.save",request);assert_eq!(resumed["server"]["id"],id);assert_eq!(resumed["server"]["revision"],2);assert_eq!(resumed["postCommitPending"],false);assert_eq!(resumed["server"]["hasSecret"],false);
    assert_eq!(call(&core,"servers.catalog.list",json!({}))["items"].as_array().unwrap().len(),1);
}

#[test]
fn foreign_caller_and_pagination_queries_cannot_reuse_a_receipt_or_cursor() {
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let saved=save(&core,&Value::Null);save(&core,&Value::Null);
    let request=json!({"operationId":"owned","serverId":saved["server"]["id"],"expectedMembershipRevision":saved["membershipRevision"],"groupId":null});call(&core,"servers.catalog.save",request.clone());
    let mut other=Caller::shell();other.source="different-caller".into();
    assert_eq!(core.call(&other,"servers.catalog.save",request).unwrap_err().code,"OPERATION_CONFLICT");
    let page=call(&core,"servers.catalog.list",json!({"limit":1}));
    assert_eq!(core.call(&Caller::shell(),"servers.catalog.list",json!({"limit":1,"cursor":page["nextCursor"],"query":"changed"})).unwrap_err().code,"STALE_CURSOR");
}
