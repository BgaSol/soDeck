mod support;
use serde_json::{json,Value};
use sodeck_core::{Caller,Core};
use base64::{Engine,engine::general_purpose::STANDARD};
use std::{io::{BufRead,BufReader},process::{Child,Command,Stdio},time::{Duration,Instant},sync::Arc};

fn create(core:&Core,name:&str,host:&str,port:u16,user:&str,jump:Option<&Value>)->Value {
    core.call(&Caller::shell(),"servers.routes.create",json!({"name":name,"host":host,"port":port,"username":user,"authType":"password","accessPath":jump.map_or(json!({"steps":[]}),|id|support::ssh_steps(json!([id])))})).unwrap()
}
fn edit(core:&Core,server:&Value,changes:Value)->sodeck_core::Result<Value>{
    let mut p=server.clone();p.as_object_mut().unwrap().remove("id");p.as_object_mut().unwrap().remove("hasSecret");p.as_object_mut().unwrap().remove("revision");p["serverId"]=server["id"].clone();p["expectedRevision"]=server["revision"].clone();
    for(k,v)in changes.as_object().unwrap(){p[k]=v.clone();}
    core.call(&Caller::shell(),"servers.routes.update",p)
}
#[test]
fn explicit_dependencies_and_same_schema_reopen(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let a=create(&core,"A","gateway",22,"jump",None);let b=create(&core,"B","private",22,"target",Some(&a["id"]));
    assert_eq!(edit(&core,&b,json!({"accessPath":support::ssh_steps(json!([b["id"]]))})).unwrap_err().code,"INVALID_ROUTE");
    assert_eq!(core.call(&Caller::shell(),"servers.delete",json!({"serverId":a["id"],"stopSessions":true})).unwrap_err().code,"SERVER_DEPENDENTS");
    let b=edit(&core,&b,json!({"accessPath":{"steps":[]}})).unwrap();core.call(&Caller::shell(),"servers.delete",json!({"serverId":a["id"]})).unwrap();drop(core);
    let reopened=Core::open(dir.path()).unwrap();assert_eq!(reopened.call(&Caller::shell(),"servers.routes.get",json!({"serverId":b["id"]})).unwrap()["accessPath"]["steps"],json!([]));assert!(!dir.path().join("backups").exists());
}

struct Fixture{child:Child,root:tempfile::TempDir,port:u16}
impl Fixture{
    fn start()->Self{
        let root=tempfile::tempdir().unwrap();let python=std::env::var("SODECK_TEST_PYTHON").expect("Python/Paramiko required");
        let mut child=Command::new(python).args(["-u",concat!(env!("CARGO_MANIFEST_DIR"),"/../../tests/core/ssh_jump_fixture.py")]).arg(root.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut line=String::new();BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();let info:Value=serde_json::from_str(&line).unwrap();
        Self{child,root,port:info["port"].as_u64().unwrap() as u16}
    }
    fn events(&self)->String{std::fs::read_to_string(self.root.path().join("gateway-events.jsonl")).unwrap_or_default()}
}
impl Drop for Fixture{fn drop(&mut self){drop(self.child.stdin.take());let _=self.child.wait();}}
fn setup(core:&Core,fixture:&Fixture)->(Value,Value){
    let a=create(core,"Gateway","127.0.0.1",fixture.port,"jump-user",None);
    let b=create(core,"Target","target.internal",22,"test-user",Some(&a["id"]));
    for (server,password) in [(&a,"jump-test-password"),(&b,"isolated-test-password")]{core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":server["id"],"password":password})).unwrap();}
    (a,b)
}
fn acquire(core:&Core,b:&Value,operation:&str)->sodeck_core::Result<Value>{core.call(&Caller::shell(),"connections.acquire",json!({"serverId":b["id"],"workspaceId":b["id"],"operationId":operation}))}
fn trust_connect(core:&Core,b:&Value,operation:&str)->Value{
    for _ in 0..3{match acquire(core,b,operation){Ok(v)=>return v,Err(e)=>{assert_eq!(e.code,"HOST_KEY_REQUIRED","{e:?}");core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();}}}
    panic!("route did not finish")
}
fn output(core:&Core,p:&Value,expected:&str){
    let deadline=Instant::now()+Duration::from_secs(5);let mut cursor=0;let mut text=Vec::new();
    while Instant::now()<deadline {
        let value=core.call(&Caller::shell(),"pty.read",json!({"ptyId":p["ptyId"],"generation":p["generation"],"after":cursor})).unwrap();
        for f in value["frames"].as_array().unwrap(){text.extend(STANDARD.decode(f["bytes"].as_str().unwrap()).unwrap());cursor=f["sequence"].as_u64().unwrap();}
        if String::from_utf8_lossy(&text).contains(expected){return}
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("target echo missing: {}",String::from_utf8_lossy(&text));
}
#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture; Linux/OpenSSH verified separately"]
fn nested_ssh_shares_target_and_preserves_route_identity(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(a,b)=setup(&core,&f);
    let first=trust_connect(&core,&b,"first");let second=acquire(&core,&b,"second").unwrap();assert_eq!(first["connectionId"],second["connectionId"]);
    let pty=core.call(&Caller::shell(),"pty.open",json!({"leaseId":first["leaseId"]})).unwrap();output(&core,&pty,"fixture-ready");
    core.call(&Caller::shell(),"connections.release",json!({"leaseId":second["leaseId"]})).unwrap();
    core.call(&Caller::shell(),"pty.write",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"inputSeq":1,"bytes":STANDARD.encode("jump-中文".as_bytes())})).unwrap();output(&core,&pty,"jump-中文");
    let scope=json!({"workspaceId":b["id"],"workspaceGeneration":"scope-one"});core.register_workspace(b["id"].as_str().unwrap(),"scope-one");
    let observed=core.call(&Caller::shell(),"connections.observe",json!({"scope":scope,"serverId":b["id"],"configRevision":b["revision"],"waitMs":0})).unwrap();assert_eq!(observed["state"],"active");
    let a=edit(&core,&a,json!({"name":"renamed gateway"})).unwrap();
    let changed=core.call(&Caller::shell(),"connections.observe",json!({"scope":scope,"serverId":b["id"],"configRevision":b["revision"],"waitMs":0})).unwrap();assert_eq!(changed["state"],"idle");assert_ne!(changed["revision"],observed["revision"]);
    assert_eq!(core.call(&Caller::shell(),"connections.acquireExisting",json!({"scope":scope,"serverId":b["id"],"configRevision":b["revision"],"operationId":"old-not-reusable"})).unwrap_err().code,"CONNECTION_NOT_READY");
    let new=acquire(&core,&b,"new-config").unwrap();assert_ne!(first["connectionId"],new["connectionId"]);
    let status=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":b["id"],"workspaceId":b["id"]})).unwrap();assert_eq!(status["running"].as_array().unwrap().len(),2);
    assert!(status["running"].as_array().unwrap().iter().any(|r|r["configurationChanged"]==true));
    let deps=core.call(&Caller::shell(),"servers.routes.dependencies",json!({"serverId":a["id"]})).unwrap();assert_eq!(deps["dependencies"].as_array().unwrap().iter().filter(|d|d["kind"]=="running").count(),2);
    let resources=core.call(&Caller::shell(),"connections.route.list",json!({})).unwrap();assert_eq!(resources["connections"].as_array().unwrap().len(),2);
    edit(&core,&a,json!({"username":"different-account"})).unwrap();
    assert_eq!(core.call(&Caller::shell(),"servers.routes.get",json!({"serverId":b["id"]})).unwrap()["hasSecret"],false);
    assert!(!f.events().contains("session-denied"));assert!(f.events().contains("target.internal"));
    core.shutdown();let deadline=Instant::now()+Duration::from_secs(5);
    while f.events().matches("connection-open").count()!=f.events().matches("connection-close").count(){assert!(Instant::now()<deadline,"gateway resource leaked");std::thread::sleep(Duration::from_millis(20));}
}

#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture"]
fn hop_challenges_cancel_and_forward_failures_are_scoped(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let(a,b)=setup(&core,&f);
    let e=acquire(&core,&b,"cancel-trust").unwrap_err();assert_eq!(e.details.as_ref().unwrap()["serverId"],a["id"]);
    core.call(&Caller::shell(),"operations.cancel",json!({"operationId":"cancel-trust"})).unwrap();
    assert_eq!(core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap_err().code,"HOST_CHALLENGE_EXPIRED");
    let lease=trust_connect(&core,&b,"ready");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    std::fs::write(f.root.path().join("deny-forward"),b"1").unwrap();
    let e=acquire(&core,&b,"denied").unwrap_err();assert_eq!(e.code,"JUMP_FORWARD_FAILED");assert_eq!(e.details.unwrap()["stage"],"forward");
    std::fs::remove_file(f.root.path().join("deny-forward")).unwrap();
    std::fs::write(f.root.path().join("pause-forward"),b"1").unwrap();let before=f.events().matches("\"forward\"").count();
    let c=core.clone();let bb=b.clone();let running=std::thread::spawn(move||acquire(&c,&bb,"slow"));let deadline=Instant::now()+Duration::from_secs(5);
    while f.events().matches("\"forward\"").count()==before {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
    let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":"slow"})).unwrap();assert_eq!(running.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));
    std::fs::remove_file(f.root.path().join("pause-forward")).unwrap();
    let lease=acquire(&core,&b,"again").unwrap();core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    std::fs::write(f.root.path().join("rotate-host-key"),b"1").unwrap();let e=acquire(&core,&b,"rotated").unwrap_err();assert_eq!(e.code,"HOST_KEY_CHANGED");assert_eq!(e.details.unwrap()["serverId"],a["id"]);
    core.shutdown();
}

#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture"]
fn waiters_revision_races_and_fifty_path_cycles(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let(a,b)=setup(&core,&f);
    let lease=trust_connect(&core,&b,"initial");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    for round in 0..50 {
        let lease=acquire(&core,&b,&format!("cycle-{round}")).unwrap();
        let status=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":b["id"],"workspaceId":b["id"]})).unwrap();
        assert!(status["running"][0]["resources"]["bridges"].as_u64().unwrap()==1);
        core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    }
    let pause=f.root.path().join("pause-forward");std::fs::write(&pause,b"1").unwrap();
    let before=f.events().matches("\"forward\"").count();let c=core.clone();let bb=b.clone();let first=std::thread::spawn(move||acquire(&c,&bb,"shared-first"));
    let deadline=Instant::now()+Duration::from_secs(5);while f.events().matches("\"forward\"").count()==before{assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
    let c=core.clone();let bb=b.clone();let second=std::thread::spawn(move||acquire(&c,&bb,"shared-second"));
    core.call(&Caller::shell(),"operations.cancel",json!({"operationId":"shared-second"})).unwrap();assert_eq!(second.join().unwrap().unwrap_err().code,"CANCELED");
    std::fs::remove_file(&pause).unwrap();let lease=first.join().unwrap().unwrap();core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    std::fs::write(&pause,b"1").unwrap();let before=f.events().matches("\"forward\"").count();let c=core.clone();let bb=b.clone();let racing=std::thread::spawn(move||acquire(&c,&bb,"racing"));
    let deadline=Instant::now()+Duration::from_secs(5);while f.events().matches("\"forward\"").count()==before{assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
    edit(&core,&a,json!({"name":"configuration changed before handoff"})).unwrap();std::fs::remove_file(&pause).unwrap();assert_eq!(racing.join().unwrap().unwrap_err().code,"REVISION_CONFLICT");
    core.shutdown();let deadline=Instant::now()+Duration::from_secs(5);while f.events().matches("connection-open").count()!=f.events().matches("connection-close").count(){assert!(Instant::now()<deadline,"gateway resource leaked");std::thread::sleep(Duration::from_millis(20));}
    assert!(core.resources()["connections"].as_array().unwrap().is_empty());
}

#[test]
fn mismatched_schema_requires_visible_startup_reset(){
    let dir=tempfile::tempdir().unwrap();let root=dir.path().join("core");let core=Core::open(&root).unwrap();
    let server=create(&core,"Old direct","example.invalid",22,"tester",None);
    support::prefill(&core,"ssh",&server["id"],json!({"password":"migration-secret","remember":true}));drop(core);
    let db=rusqlite::Connection::open(root.join("core.sqlite")).unwrap();db.execute_batch("DROP TABLE server_route_steps; PRAGMA user_version=0").unwrap();drop(db);
    assert_eq!(Core::open(&root).err().unwrap().code,"UNSUPPORTED_DATA_VERSION");
    let backup=sodeck_core::store_bootstrap::prepare_database(dir.path(),|_|Ok(())).unwrap().unwrap();
    let db=rusqlite::Connection::open(backup.join("core/core.sqlite")).unwrap();assert_eq!(db.query_row("PRAGMA user_version",[],|r|r.get::<_,u32>(0)).unwrap(),0);
    assert_eq!(db.query_row("SELECT count(*) FROM servers",[],|r|r.get::<_,u32>(0)).unwrap(),1);
    let secret_ref:String=db.query_row("SELECT secret_id FROM credential_bindings WHERE owner_id=?",[server["id"].as_str().unwrap()],|r|r.get(0)).unwrap();
    assert!(backup.join("core/secrets").join(format!("{secret_ref}.dpapi")).exists());
    let fresh=Core::open(&root).unwrap();assert!(fresh.call(&Caller::shell(),"servers.list",json!({})).unwrap()["servers"].as_array().unwrap().is_empty());drop(fresh);
    let db=rusqlite::Connection::open(root.join("core.sqlite")).unwrap();db.execute_batch("PRAGMA user_version=999").unwrap();drop(db);
    assert_eq!(Core::open(&root).err().unwrap().code,"UNSUPPORTED_DATA_VERSION");
}

#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture"]
fn host_changes_network_isolation_and_disconnect_have_no_fallback(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(a,b)=setup(&core,&f);
    let lease=trust_connect(&core,&b,"known");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    std::fs::write(f.root.path().join("target/rotate-host-key"),b"1").unwrap();
    let e=acquire(&core,&b,"target-rotated").unwrap_err();assert_eq!(e.code,"HOST_KEY_CHANGED");assert_eq!(e.details.as_ref().unwrap()["role"],"target");
    core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();
    let lease=acquire(&core,&b,"target-trusted").unwrap();core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    std::fs::write(f.root.path().join("rotate-host-key"),b"1").unwrap();
    let e=acquire(&core,&b,"jump-rotated").unwrap_err();assert_eq!(e.code,"HOST_KEY_CHANGED");assert_eq!(e.details.as_ref().unwrap()["serverId"],a["id"]);
    core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();
    let e=acquire(&core,&b,"new-network").unwrap_err();assert_eq!(e.code,"HOST_KEY_REQUIRED");assert_eq!(e.details.as_ref().unwrap()["role"],"target");core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();
    assert_eq!(acquire(&core,&b,"new-network-auth").unwrap_err().code,"AUTH_REQUIRED");
    core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":b["id"],"password":"isolated-test-password"})).unwrap();
    let lease=acquire(&core,&b,"after-key-change").unwrap();let pty=core.call(&Caller::shell(),"pty.open",json!({"leaseId":lease["leaseId"]})).unwrap();output(&core,&pty,"fixture-ready");
    std::fs::write(f.root.path().join("drop-gateway"),b"1").unwrap();
    let deadline=Instant::now()+Duration::from_secs(5);loop {
        let resources=core.resources();if resources["connections"][0]["state"]=="closed"{break}
        assert!(Instant::now()<deadline,"gateway death did not close target path");std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(core.resources()["ptys"][0]["state"],"disconnected");core.shutdown();
}

#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture"]
fn authentication_failures_and_handshake_cancellation_identify_the_real_node(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let(a,b)=setup(&core,&f);
    let challenge=acquire(&core,&b,"first-hop").unwrap_err();assert!(!f.events().contains("\"auth\""));core.call(&Caller::shell(),"hosts.trust",challenge.details.unwrap()).unwrap();
    let challenge=acquire(&core,&b,"second-hop").unwrap_err();assert_eq!(challenge.details.as_ref().unwrap()["role"],"target");
    assert!(!std::fs::read_to_string(f.root.path().join("target/events.jsonl")).unwrap().contains("password-auth"));core.call(&Caller::shell(),"hosts.trust",challenge.details.unwrap()).unwrap();
    for (server,role,password) in [(&a,"jump","jump-test-password"),(&b,"target","isolated-test-password")] {
        core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":server["id"],"password":"wrong-fixture-password"})).unwrap();
        let wrong=format!("wrong-{role}");let error=acquire(&core,&b,&wrong).unwrap_err();assert_eq!(error.code,"AUTH_FAILED");assert_eq!(error.details.as_ref().unwrap()["role"],role);
        core.call(&Caller::shell(),"operations.cancel",json!({"operationId":wrong})).unwrap();
        assert_eq!(core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":server["id"],"challengeId":error.details.unwrap()["challengeId"],"password":password})).unwrap_err().code,"HOST_CHALLENGE_EXPIRED");
        core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":server["id"],"password":password})).unwrap();
        // Cancellation is deliberately durable; a new operation is needed.
        let lease=acquire(&core,&b,&format!("correct-{role}")).unwrap();core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    }
    for (index,marker) in [(0,"pause-handshake"),(1,"target/pause-handshake")] {
        let marker=f.root.path().join(marker);std::fs::write(&marker,b"1").unwrap();
        let c=core.clone();let bb=b.clone();let op=format!("cancel-handshake-{index}");let child_op=op.clone();let worker=std::thread::spawn(move||acquire(&c,&bb,&child_op));
        let deadline=Instant::now()+Duration::from_secs(5);
        loop{let status=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":b["id"],"workspaceId":b["id"]})).unwrap();if status["attempt"]["hopIndex"]==index&&status["attempt"]["stage"]=="handshake"{break}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
        let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":op})).unwrap();assert_eq!(worker.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));std::fs::remove_file(marker).unwrap();
    }
    core.shutdown();
}

#[test]
#[ignore="Requires isolated Python/Paramiko protocol fixture"]
fn sixty_four_routes_include_internal_transports_in_the_target_budget(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(_,b)=setup(&core,&f);
    let first=trust_connect(&core,&b,"capacity-initial");core.call(&Caller::shell(),"connections.release",json!({"leaseId":first["leaseId"]})).unwrap();
    for n in 0..64 {core.call(&Caller::shell(),"connections.acquire",json!({"serverId":b["id"],"workspaceId":format!("capacity-{n}"),"operationId":format!("capacity-{n}")})).unwrap();}
    assert_eq!(core.resources()["connections"].as_array().unwrap().len(),64);
    assert_eq!(core.call(&Caller::shell(),"connections.acquire",json!({"serverId":b["id"],"workspaceId":"overflow","operationId":"overflow"})).unwrap_err().code,"LIMIT_EXCEEDED");
    core.shutdown();let deadline=Instant::now()+Duration::from_secs(5);while f.events().matches("connection-open").count()!=f.events().matches("connection-close").count(){assert!(Instant::now()<deadline,"gateway resource leaked");std::thread::sleep(Duration::from_millis(20));}
}

#[test]
#[ignore="Requires two isolated gateway networks with the same private target name"]
fn separate_target_dependencies_and_same_address_networks_stay_isolated(){
    let f=Fixture::start();let other=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(a,b)=setup(&core,&f);
    let first=trust_connect(&core,&b,"b");let gateway=trust_connect(&core,&a,"gateway-own");
    let c=create(&core,"C","target.internal",22,"test-user",Some(&a["id"]));core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":c["id"],"password":"isolated-test-password"})).unwrap();let third=trust_connect(&core,&c,"c");assert_ne!(first["connectionId"],third["connectionId"]);
    core.call(&Caller::shell(),"connections.release",json!({"leaseId":gateway["leaseId"]})).unwrap();assert_eq!(core.resources()["connections"].as_array().unwrap().len(),2);
    core.call(&Caller::shell(),"connections.release",json!({"leaseId":first["leaseId"]})).unwrap();let pty=core.call(&Caller::shell(),"pty.open",json!({"leaseId":third["leaseId"]})).unwrap();output(&core,&pty,"fixture-ready");
    let a2=create(&core,"other network","127.0.0.1",other.port,"jump-user",None);core.call(&Caller::shell(),"auth.ssh.set",json!({"serverId":a2["id"],"password":"jump-test-password"})).unwrap();
    let b=edit(&core,&b,json!({"accessPath":{"kind":"hostTunnel","jumpServerIds":[a2["id"]]}})).unwrap();assert_eq!(b["hasSecret"],false);
    let e=acquire(&core,&b,"network-a").unwrap_err();assert_eq!(e.code,"HOST_KEY_REQUIRED");core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();
    let e=acquire(&core,&b,"network-b").unwrap_err();assert_eq!(e.code,"HOST_KEY_REQUIRED");assert_eq!(e.details.as_ref().unwrap()["role"],"target");core.call(&Caller::shell(),"hosts.trust",e.details.unwrap()).unwrap();assert_eq!(acquire(&core,&b,"network-auth").unwrap_err().code,"AUTH_REQUIRED");
    core.shutdown();
}

#[test]
fn failed_route_transaction_leaves_no_profile_and_unknown_stored_routes_never_connect(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let db=rusqlite::Connection::open(dir.path().join("core.sqlite")).unwrap();
    let gateway=create(&core,"gateway","localhost",22,"test",None);
    db.execute_batch("CREATE TRIGGER fail_route BEFORE INSERT ON server_route_steps BEGIN SELECT RAISE(ABORT,'fixture transaction failure'); END;").unwrap();
    assert!(core.call(&Caller::shell(),"servers.routes.create",json!({"name":"fail","host":"localhost","username":"test","accessPath":support::ssh_steps(json!([gateway["id"]]))})).is_err());
    let servers=core.call(&Caller::shell(),"servers.list",json!({})).unwrap();
    assert_eq!(servers["servers"].as_array().unwrap().len(),1);assert_eq!(servers["servers"][0]["id"],gateway["id"]);
    db.execute_batch("DROP TRIGGER fail_route").unwrap();let server=create(&core,"test","localhost",22,"test",Some(&gateway["id"]));
    db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE server_route_steps SET kind='unsupported'").unwrap();
    assert_eq!(acquire(&core,&server,"unsupported").unwrap_err().code,"INVALID_ROUTE");
}

#[test]
#[ignore="Requires isolated protocol fixture; cancellation/cleanup under audit failure"]
fn fifty_cancellations_leave_no_gateway_and_audit_failure_does_not_block_cleanup(){
    let f=Fixture::start();let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let(_,b)=setup(&core,&f);
    let lease=trust_connect(&core,&b,"initial");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    let pause=f.root.path().join("pause-forward");std::fs::write(&pause,b"1").unwrap();
    for n in 0..50{
        let before=f.events().matches("\"forward\"").count();let operation=format!("cancel-{n}");let c=core.clone();let bb=b.clone();let op=operation.clone();
        let worker=std::thread::spawn(move||acquire(&c,&bb,&op));let deadline=Instant::now()+Duration::from_secs(5);
        while f.events().matches("\"forward\"").count()==before {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(5));}
        let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":operation})).unwrap();assert_eq!(worker.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));
    }
    std::fs::remove_file(pause).unwrap();let lease=acquire(&core,&b,"before-log-failure").unwrap();
    rusqlite::Connection::open(dir.path().join("observability/facts.sqlite")).unwrap().execute_batch("DROP TABLE calls").unwrap();
    assert_eq!(acquire(&core,&b,"log-unavailable").unwrap_err().code,"LOG_UNAVAILABLE");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
    assert!(core.resources()["connections"].as_array().unwrap().is_empty());core.shutdown();
    let deadline=Instant::now()+Duration::from_secs(5);while f.events().matches("connection-open").count()!=f.events().matches("connection-close").count(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));}
}
