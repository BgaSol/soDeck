use serde_json::{json,Value};
use sodeck_core::{Caller,Core};

fn call(core:&Core,method:&str,p:Value)->Value{core.call(&Caller::shell(),method,p).unwrap()}
fn ssh(core:&Core,name:&str,steps:Value)->Value {
    call(core,"servers.routes.create",json!({"name":name,"host":"fixture.invalid","port":22,"username":"fixture","authType":"password","accessPath":{"steps":steps}}))
}
fn proxy(core:&Core)->Value {call(core,"proxies.socks5.create",json!({"name":"loopback","host":"127.0.0.1","port":12021,"authType":"none","username":""}))}
fn hop(server:&Value)->Value{json!({"stepId":uuid::Uuid::new_v4().to_string(),"kind":"ssh","serverId":server["id"]})}
fn via(proxy:&Value)->Value{json!({"stepId":uuid::Uuid::new_v4().to_string(),"kind":"socks5","proxyId":proxy["id"],"resolve":"proxy"})}

#[test]
fn typed_steps_roundtrip_and_single_relational_source() {
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
    let a=ssh(&core,"A",json!([]));let b=ssh(&core,"B",json!([]));let p=proxy(&core);
    let steps=json!([hop(&a),hop(&b),via(&p),via(&p)]);
    let target=ssh(&core,"target",steps.clone());assert_eq!(target["accessPath"],json!({"steps":steps}));
    let status=call(&core,"connections.route.get",json!({"serverId":target["id"],"workspaceId":"test"}));
    assert_eq!(status["saved"]["stepCount"],5);assert_eq!(status["saved"]["sshCount"],3);assert_eq!(status["saved"]["proxyCount"],2);
    assert_eq!(status["saved"]["attemptTimeoutMs"],255000);
    let page=call(&core,"connections.route.nodes",json!({"reference":status["saved"]["reference"]}));
    assert_eq!(page["nodes"][2]["kind"],"socks5");assert_eq!(page["nodes"][4]["stepId"],"target");
    assert_eq!(page["nodes"][3]["fromStepId"],steps[2]["stepId"]);
    let db=rusqlite::Connection::open(root.path().join("core.sqlite")).unwrap();
    assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),7);
    assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('connection_paths','connection_path_refs')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(db.query_row("SELECT count(*) FROM server_route_steps WHERE target_server_id=?",[target["id"].as_str().unwrap()],|r|r.get::<_,i64>(0)).unwrap(),4);
    let dependencies=call(&core,"proxies.catalog.list",json!({}));assert_eq!(dependencies["items"][0]["referenceCount"],1);
    assert_eq!(core.call(&Caller::shell(),"servers.delete",json!({"serverId":a["id"]})).unwrap_err().code,"SERVER_DEPENDENTS");
    assert_eq!(core.call(&Caller::shell(),"proxies.socks5.delete",json!({"proxyId":p["id"]})).unwrap_err().code,"PROXY_DEPENDENTS");
}

#[test]
fn rejects_old_shapes_duplicate_steps_and_self_without_partial_save() {
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
    let a=ssh(&core,"A",json!([]));
    for path in [json!({"kind":"direct"}),json!({"steps":[hop(&a),hop(&a)]}),json!({"steps":[{"kind":"ssh","serverId":a["id"],"stepId":"target"}]})] {
        let error=core.call(&Caller::shell(),"servers.routes.create",json!({"name":"bad","host":"fixture.invalid","username":"fixture","accessPath":path})).unwrap_err();
        assert!(matches!(error.code.as_str(),"INVALID_ROUTE"|"INVALID_PARAMS"));
    }
    let p=proxy(&core);let duplicate=via(&p);
    assert_eq!(core.call(&Caller::shell(),"servers.routes.update",json!({"serverId":a["id"],"expectedRevision":1,"name":"bad","host":"fixture.invalid","username":"fixture","accessPath":{"steps":[duplicate,duplicate]}})).unwrap_err().code,"INVALID_ROUTE");
    assert_eq!(call(&core,"servers.routes.get",json!({"serverId":a["id"]}))["name"],"A");
    assert!(core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":a["id"],"password":"unused","remember":false})).is_err());
    assert!(core.call(&Caller::shell(),"hosts.trust",json!({"serverId":a["id"],"fingerprint":"unused"})).is_err());
}

#[test]
fn long_finite_proxy_path_saves_and_paginates_without_hop_limit() {
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let p=proxy(&core);
    let steps=(0..300).map(|_|via(&p)).collect::<Vec<_>>();let target=ssh(&core,"long",json!(steps));
    let status=call(&core,"connections.route.get",json!({"serverId":target["id"],"workspaceId":"test"}));
    assert_eq!(status["saved"]["stepCount"],301);assert_eq!(status["saved"]["attemptTimeoutMs"],900000);
    let mut cursor=Value::Null;let mut count=0;
    loop {let page=call(&core,"connections.route.nodes",json!({"reference":status["saved"]["reference"],"cursor":cursor}));assert_eq!(page["offset"],count);count+=page["nodes"].as_array().unwrap().len();cursor=page["nextCursor"].clone();if cursor.is_null(){break;}}
    assert_eq!(count,301);
}

#[test]
#[ignore="Dedicated T-027 OpenSSH VM required"]
fn public_api_authenticates_each_typed_step_and_binds_presets() {
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
    let config:Value=serde_json::from_slice(&std::fs::read(std::env::var("SODECK_MIXED_OPENSSH_CONFIG").unwrap()).unwrap()).unwrap();
    let entries=config["paths"][0].as_array().unwrap();let mut steps=Vec::new();let mut dependencies=Vec::new();let mut target=Value::Null;
    for (index,entry) in entries.iter().enumerate() {
        if entry["kind"]=="socks" {
            let p=call(&core,"proxies.socks5.create",json!({"name":"remote-loopback","host":entry["host"],"port":entry["port"],"authType":"none","username":""}));
            let step=via(&p);dependencies.push(json!({"stepId":step["stepId"],"kind":"socks5","id":p["id"],"host":p["host"],"port":p["port"],"username":"","authType":"none"}));steps.push(step);continue;
        }
        let last=index+1==entries.len();
        let server=call(&core,"servers.routes.create",json!({"name":entry["name"],"host":entry["host"],"port":entry["port"],"username":entry["username"],"authType":"key","accessPath":{"steps":if last{json!(steps)}else{json!([])}}}));
        let prepared=call(&core,"credentials.configuration.prepare",json!({"target":{"kind":"ssh","id":server["id"]},"expectedOwnerRevision":1,"expectedDependencies":if last{json!(dependencies)}else{json!([])}}));
        call(&core,"credentials.configuration.set",json!({"operationId":uuid::Uuid::new_v4().to_string(),"contextToken":prepared["contextToken"],"privateKeyPath":entry["key"],"remember":true}));
        if last {target=server;}else{
            let step=hop(&server);dependencies.push(json!({"stepId":step["stepId"],"kind":"ssh","id":server["id"],"host":server["host"],"port":server["port"],"username":server["username"],"authType":"key"}));steps.push(step);
        }
    }
    let operation=uuid::Uuid::new_v4().to_string();let mut trusted=Vec::new();let mut confirmed=Vec::new();let mut lease=None;
    for _ in 0..10 {
        match core.call(&Caller::shell(),"connections.acquire",json!({"serverId":target["id"],"workspaceId":"test","operationId":operation})) {
            Ok(value)=>{lease=Some(value);break;},
            Err(error)=>{
                let details=error.details.unwrap();assert!(details.get("_contextKey").is_none());
                let index=details["stepIndex"].as_u64().unwrap();
                let challenge=call(&core,"auth.challenge.get",json!({"challengeId":details["challengeId"],"targetServerId":target["id"]}));
                assert_eq!(challenge["stepIndex"],index);
                if error.code=="HOST_KEY_REQUIRED" {
                    trusted.push(index);call(&core,"hosts.trust",json!({"serverId":details["serverId"],"fingerprint":details["fingerprint"],"challengeId":details["challengeId"]}));
                }else{
                    assert_eq!(error.code,"CREDENTIAL_CONFIRM_REQUIRED");confirmed.push(index);
                    let request=json!({"challengeId":details["challengeId"],"operationId":operation,"remember":true});
                    let first=call(&core,"auth.credentials.usePreset",request.clone());
                    assert_eq!(call(&core,"auth.credentials.usePreset",request.clone()),first);
                    let mut conflicting=request;conflicting["remember"]=json!(false);
                    assert_eq!(core.call(&Caller::shell(),"auth.credentials.usePreset",conflicting).unwrap_err().code,"OPERATION_CONFLICT");
                }
            }
        }
    }
    assert!(lease.is_some());assert_eq!(trusted,vec![0,1,3]);assert_eq!(confirmed,vec![0,1,3]);
    let db=rusqlite::Connection::open(root.path().join("core.sqlite")).unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM credential_bindings",[],|r|r.get::<_,i64>(0)).unwrap(),6);
    assert_eq!(std::fs::read_dir(root.path().join("secrets")).unwrap().count(),6);
    core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease.unwrap()["leaseId"]})).unwrap();
}
