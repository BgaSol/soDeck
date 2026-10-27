use serde_json::{json, Value};
use sodeck_core::{Caller, Core};
use std::sync::{Arc, Barrier};
use std::{io::{BufRead,BufReader},process::{Child,Command,Stdio},time::{Duration,Instant}};
use base64::{Engine,engine::general_purpose::STANDARD};

fn create(core:&Core,name:&str,jump:Option<&Value>)->Value {
    core.call(&Caller::shell(),"servers.routes.v2.create",json!({"name":name,"host":format!("{name}.internal"),"username":"user","connectionRoute":jump.map_or(json!({"kind":"direct"}),|s|json!({"kind":"sshJump","jumpServerId":s["id"]}))})).unwrap()
}
fn edit(core:&Core,server:&Value,changes:Value)->sodeck_core::Result<Value>{
    let mut p=server.clone();for key in ["id","revision","hasSecret"]{p.as_object_mut().unwrap().remove(key);}
    p["serverId"]=server["id"].clone();p["expectedRevision"]=server["revision"].clone();
    for(k,v)in changes.as_object().unwrap(){p[k]=v.clone();}
    core.call(&Caller::shell(),"servers.routes.v2.update",p)
}
fn status(core:&Core,server:&Value)->Value {
    core.call(&Caller::shell(),"connections.route.v2.get",json!({"serverId":server["id"],"workspaceId":"test"})).unwrap()
}

#[test]
#[ignore="Requires isolated four-node fixture; verifies bounded state under 64 live route revisions"]
fn route_state_stays_bounded_while_resource_pages_keep_all_connections(){
    let started=Instant::now();
    let f=Fixture::start(3);let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let nodes=f.setup(&core);let mut target=nodes.last().unwrap().clone();
    let initial=connect(&core,&target,4,"trust-summary");core.call(&Caller::shell(),"connections.release",json!({"leaseId":initial["leaseId"]})).unwrap();drained(&core,&f);
    for index in 0..64 {
        target=edit(&core,&target,json!({"name":format!("{index:02}{}","x".repeat(190))})).unwrap();
        acquire(&core,&target,&format!("summary-{index}")).unwrap();
    }
    let snapshot=core.call(&Caller::shell(),"connections.route.v2.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();
    assert_eq!(snapshot["runningCount"],64);assert_eq!(snapshot["currentCount"],1);assert_eq!(snapshot["changedCount"],63);
    assert!(snapshot["running"].as_array().unwrap().len()<64);assert!(serde_json::to_vec(&snapshot).unwrap().len()<=28*1024);
    let mut total=0;let mut cursor=Value::Null;
    loop{let page=core.call(&Caller::shell(),"connections.route.v2.list",json!({"cursor":cursor})).unwrap();assert_eq!(page["usage"]["sessions"],256);total+=page["connections"].as_array().unwrap().len();cursor=page["nextCursor"].clone();if cursor.is_null(){break;}}
    assert_eq!(total,64);core.shutdown();drained(&core,&f);
    println!("64 live revisions, bounded state, all paged resources and full cleanup: {:?}",started.elapsed());
}

#[test]
fn nested_references_preserve_legacy_contracts_and_invalidate_all_descendants(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let a=create(&core,"a",None);let b=create(&core,"b",Some(&a));let c=create(&core,"c",Some(&b));let d=create(&core,"d",Some(&c));
    assert_eq!(status(&core,&d)["saved"]["nodeCount"],4);
    let page=core.call(&Caller::shell(),"connections.route.v2.nodes",json!({"reference":status(&core,&d)["saved"]["reference"]})).unwrap();
    assert_eq!(page["nodes"].as_array().unwrap().iter().map(|n|n["name"].as_str().unwrap()).collect::<Vec<_>>(),["a","b","c","d"]);
    for (method,params) in [("servers.routes.list",json!({})),("connections.route.get",json!({"serverId":d["id"],"workspaceId":"test"})),("servers.routes.get",json!({"serverId":d["id"]}))] {
        assert_eq!(core.call(&Caller::shell(),method,params).unwrap_err().code,"ROUTE_VERSION_REQUIRED");
    }
    for server in [&b,&c,&d]{core.call(&Caller::shell(),"auth.set",json!({"serverId":server["id"],"password":"test-only","remember":true})).unwrap();}
    let before=status(&core,&d);
    let b=edit(&core,&b,json!({"name":"renamed"})).unwrap();
    assert_ne!(before["saved"]["routeRevision"],status(&core,&d)["saved"]["routeRevision"]);
    assert_eq!(core.call(&Caller::shell(),"servers.routes.v2.get",json!({"serverId":d["id"]})).unwrap()["hasSecret"],true);
    edit(&core,&b,json!({"username":"new-user"})).unwrap();
    for server in [&b,&c,&d]{assert_eq!(core.call(&Caller::shell(),"servers.routes.v2.get",json!({"serverId":server["id"]})).unwrap()["hasSecret"],false);}
    let deps=core.call(&Caller::shell(),"servers.routes.v2.dependencies",json!({"serverId":a["id"]})).unwrap();
    assert_eq!(deps["dependencies"].as_array().unwrap().len(),3);
    assert_eq!(deps["dependencies"].as_array().unwrap().iter().filter(|r|r["direct"]==true).count(),1);
    assert_eq!(core.call(&Caller::shell(),"servers.delete",json!({"serverId":a["id"],"stopSessions":true})).unwrap_err().code,"SERVER_DEPENDENTS");
    let mut legacy=d.clone();legacy.as_object_mut().unwrap().remove("connectionRoute");legacy["serverId"]=d["id"].clone();legacy["expectedRevision"]=d["revision"].clone();
    core.call(&Caller::shell(),"servers.update",legacy).unwrap();
    assert_eq!(status(&core,&d)["saved"]["nodeCount"],4);
}

#[test]
fn atomic_cycle_validation_and_old_writes_cannot_create_nested_paths(){
    let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());
    let a=create(&core,"a",None);let b=create(&core,"b",None);
    let barrier=Arc::new(Barrier::new(2));
    let jobs=[(a.clone(),b.clone()),(b,a)].into_iter().map(|(from,to)|{let core=core.clone();let barrier=barrier.clone();std::thread::spawn(move||{barrier.wait();edit(&core,&from,json!({"connectionRoute":{"kind":"sshJump","jumpServerId":to["id"]}}))})}).collect::<Vec<_>>();
    let results=jobs.into_iter().map(|j|j.join().unwrap()).collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r|r.is_ok()).count(),1);
    assert_eq!(results.iter().find_map(|r|r.as_ref().err()).unwrap().code,"INVALID_ROUTE");
    let a=create(&core,"entry",None);let b=create(&core,"middle",Some(&a));
    let before=core.call(&Caller::shell(),"servers.list",json!({})).unwrap();
    assert_eq!(core.call(&Caller::shell(),"servers.routes.create",json!({"name":"invalid-v1","host":"host","username":"user","connectionRoute":{"kind":"sshJump","jumpServerId":b["id"]}})).unwrap_err().code,"ROUTE_VERSION_REQUIRED");
    assert_eq!(before,core.call(&Caller::shell(),"servers.list",json!({})).unwrap());
    assert_eq!(edit(&core,&a,json!({"connectionRoute":{"kind":"sshJump","jumpServerId":"missing"}})).unwrap_err().code,"INVALID_ROUTE");
}

#[test]
fn thousand_hop_storage_and_pages_have_no_business_depth_limit(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let mut db=rusqlite::Connection::open(dir.path().join("core.sqlite")).unwrap();let tx=db.transaction().unwrap();
    for index in 0..1001 {
        let id=format!("node-{index}");
        let server=json!({"id":id,"name":id,"host":"private.internal","port":22,"username":"user","authType":"password","revision":1,"hasSecret":false});
        let jump=(index>0).then(||format!("node-{}",index-1));let route=jump.as_ref().map_or(json!({"kind":"direct"}),|s|json!({"kind":"sshJump","jumpServerId":s}));
        tx.execute("INSERT INTO servers(id,data) VALUES(?,?)",rusqlite::params![id,server.to_string()]).unwrap();
        tx.execute("INSERT INTO server_routes(server_id,data,jump_server_id) VALUES(?,?,?)",rusqlite::params![id,route.to_string(),jump]).unwrap();
    }
    tx.commit().unwrap();drop(db);
    let end=core.call(&Caller::shell(),"servers.routes.v2.get",json!({"serverId":"node-1000"})).unwrap();
    let end=edit(&core,&end,json!({"name":"deep-target"})).unwrap();
    let summary=status(&core,&end)["saved"].clone();assert_eq!(summary["nodeCount"],1001);
    let mut cursor=Value::Null;let mut count=0;let mut first_cursor=Value::Null;
    loop {
        let page=core.call(&Caller::shell(),"connections.route.v2.nodes",json!({"reference":summary["reference"],"cursor":cursor})).unwrap();
        assert_eq!(page["offset"],count);let nodes=page["nodes"].as_array().unwrap();assert!(nodes.len()<=64);
        assert!(page.to_string().len()<=256*1024);assert_eq!(nodes[0]["serverId"],format!("node-{count}"));
        count+=nodes.len();cursor=page["nextCursor"].clone();if first_cursor.is_null(){first_cursor=cursor.clone();}
        if cursor.is_null(){break}
    }
    assert_eq!(count,1001);
    let caller=Caller{instance_id:"another-reader".into(),..Caller::shell()};
    assert_eq!(core.call(&caller,"connections.route.v2.nodes",json!({"reference":summary["reference"],"cursor":first_cursor})).unwrap_err().code,"ROUTE_SNAPSHOT_EXPIRED");
    let middle=core.call(&Caller::shell(),"servers.routes.v2.get",json!({"serverId":"node-500"})).unwrap();
    edit(&core,&middle,json!({"name":"changed-middle"})).unwrap();
    assert_eq!(core.call(&Caller::shell(),"connections.route.v2.nodes",json!({"reference":summary["reference"],"cursor":first_cursor})).unwrap_err().code,"ROUTE_SNAPSHOT_EXPIRED");
    assert_eq!(core.call(&Caller::shell(),"connections.acquire",json!({"serverId":end["id"],"workspaceId":"deep","operationId":"capacity"})).unwrap_err().code,"CONNECTION_RESOURCES_EXHAUSTED");
    assert_eq!(core.call(&Caller::shell(),"connections.route.v2.list",json!({})).unwrap()["usage"],json!({"sessions":0,"bridges":0}));
    drop(core);let core=Core::open(dir.path()).unwrap();assert_eq!(status(&core,&end)["saved"]["nodeCount"],1001);
}

struct Fixture{child:Child,root:tempfile::TempDir,port:u16,jumps:usize}
impl Fixture {
    fn start(jumps:usize)->Self {
        let root=tempfile::tempdir().unwrap();
        let mut child=Command::new(std::env::var("SODECK_TEST_PYTHON").expect("Python/Paramiko required")).args(["-u",concat!(env!("CARGO_MANIFEST_DIR"),"/../../tests/core/ssh_multihop_fixture.py")]).arg(root.path()).arg(jumps.to_string()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut line=String::new();BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let info:Value=serde_json::from_str(&line).unwrap();Self{child,root,port:info["port"].as_u64().unwrap() as u16,jumps}
    }
    fn events(&self)->String{std::fs::read_to_string(self.root.path().join("events.jsonl")).unwrap_or_default()}
    fn marker(&self,index:usize,name:&str,enabled:bool){let p=self.root.path().join(format!("{index}-{name}"));if enabled{std::fs::write(p,b"1").unwrap();}else{std::fs::remove_file(p).unwrap();}}
    fn setup(&self,core:&Core)->Vec<Value>{
        let mut nodes:Vec<Value>=Vec::new();
        for index in 0..=self.jumps {
            let last=index==self.jumps;
            let node=core.call(&Caller::shell(),"servers.routes.v2.create",json!({"name":format!("hop-{index}"),"host":if index==0{"127.0.0.1".into()}else if last{"target.internal".into()}else{format!("hop-{index}.internal")},"port":if index==0{self.port}else{22},"username":if last{"test-user"}else{"jump-user"},"connectionRoute":nodes.last().map_or(json!({"kind":"direct"}),|n|json!({"kind":"sshJump","jumpServerId":n["id"]}))})).unwrap();
            core.call(&Caller::shell(),"auth.set",json!({"serverId":node["id"],"password":if last{"isolated-test-password"}else{"jump-test-password"}})).unwrap();nodes.push(node);
        }
        nodes
    }
}
impl Drop for Fixture{fn drop(&mut self){drop(self.child.stdin.take());let _=self.child.wait();}}
fn acquire(core:&Core,target:&Value,op:&str)->sodeck_core::Result<Value>{core.call(&Caller::shell(),"connections.acquire",json!({"serverId":target["id"],"workspaceId":"protocol","operationId":op}))}
fn connect(core:&Core,target:&Value,count:usize,op:&str)->Value{
    for _ in 0..=count {match acquire(core,target,op){Ok(lease)=>return lease,Err(error)=>{
        assert_eq!(error.code,"HOST_KEY_REQUIRED","{error:?}");let details=error.details.unwrap();
        let challenge=core.call(&Caller::shell(),"auth.challenge.v2.get",json!({"challengeId":details["challengeId"],"targetServerId":target["id"]})).unwrap();
        assert_eq!(challenge["hopCount"],count);assert!(challenge["hopIndex"].as_u64().unwrap()<(count as u64));
        core.call(&Caller::shell(),"hosts.trust",details).unwrap();
    }}}
    panic!("path did not complete")
}
fn drained(core:&Core,f:&Fixture){
    let until=Instant::now()+Duration::from_secs(5);
    loop {
        let budget=core.call(&Caller::shell(),"connections.route.v2.list",json!({})).unwrap();let events=f.events();
        if budget["usage"]==json!({"sessions":0,"bridges":0})&&events.matches("connection-open").count()==events.matches("connection-close").count(){break}
        assert!(Instant::now()<until,"resource cleanup exceeded 5s: {}",budget["usage"]);std::thread::sleep(Duration::from_millis(20));
    }
}
fn echo(core:&Core,lease:&Value){
    let pty=core.call(&Caller::shell(),"pty.open",json!({"leaseId":lease["leaseId"]})).unwrap();
    let text="multihop-中文-echo";
    core.call(&Caller::shell(),"pty.write",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"inputSeq":1,"bytes":STANDARD.encode(text)})).unwrap();
    let until=Instant::now()+Duration::from_secs(10);
    loop{let read=core.call(&Caller::shell(),"pty.read",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":0})).unwrap();let bytes=read["frames"].as_array().unwrap().iter().flat_map(|f|STANDARD.decode(f["bytes"].as_str().unwrap()).unwrap()).collect::<Vec<_>>();if String::from_utf8_lossy(&bytes).contains(text){break}assert!(Instant::now()<until,"PTY echo failed");std::thread::sleep(Duration::from_millis(20));}
    core.call(&Caller::shell(),"pty.close",pty).unwrap();
}

#[test]
#[ignore="Requires isolated Python/Paramiko multihop protocol fixture"]
fn two_three_and_fifteen_jump_protocol_paths_release_every_segment(){
    for jumps in [2,3,15] {
        let f=Fixture::start(jumps);let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let nodes=f.setup(&core);let target=nodes.last().unwrap();
        let lease=connect(&core,target,nodes.len(),"connect");echo(&core,&lease);
        let segments=core.call(&Caller::shell(),"connections.route.v2.segments",json!({"connectionId":lease["connectionId"],"generation":lease["generation"]})).unwrap();
        assert_eq!(segments["segments"].as_array().unwrap().len(),jumps);
        assert!(segments["segments"].as_array().unwrap().iter().all(|s|s["metrics"]["upstreamBytes"].as_u64().unwrap()>0));
        let shared=acquire(&core,target,"shared").unwrap();assert_eq!(lease["connectionId"],shared["connectionId"]);
        core.call(&Caller::shell(),"connections.release",json!({"leaseId":shared["leaseId"]})).unwrap();
        core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();drained(&core,&f);
        assert!(!f.events().contains("session-denied"));
        println!("{} nodes: real protocol PTY, {} bridge metrics, shared lease and full cleanup passed",nodes.len(),jumps);
    }
}

#[test]
#[ignore="Requires isolated multihop protocol fixture and per-hop fault markers"]
fn middle_hop_failures_cancellation_and_key_rotation_are_scoped(){
    let f=Fixture::start(3);let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let nodes=f.setup(&core);let target=nodes.last().unwrap();
    let lease=connect(&core,target,4,"initial");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();drained(&core,&f);
    for index in 0..3 {
        f.marker(index,"deny-forward",true);let error=acquire(&core,target,"denied").unwrap_err();assert_eq!(error.code,"JUMP_FORWARD_FAILED");assert_eq!(error.details.unwrap()["hopIndex"],index+1);f.marker(index,"deny-forward",false);drained(&core,&f);
        f.marker(index,"pause-handshake",true);let c=core.clone();let t=target.clone();let op=format!("cancel-{index}");let child_op=op.clone();let worker=std::thread::spawn(move||acquire(&c,&t,&child_op));
        let until=Instant::now()+Duration::from_secs(10);
        loop{let s=core.call(&Caller::shell(),"connections.route.v2.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if s["attempt"]["hopIndex"]==index&&s["attempt"]["stage"]=="handshake"{break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
        let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":op})).unwrap();assert_eq!(worker.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));f.marker(index,"pause-handshake",false);drained(&core,&f);
    }
    f.marker(1,"rotate-host-key",true);let error=acquire(&core,target,"rotated").unwrap_err();assert_eq!(error.code,"HOST_KEY_CHANGED");assert_eq!(error.details.as_ref().unwrap()["serverId"],nodes[1]["id"]);
    core.call(&Caller::shell(),"hosts.trust",error.details.unwrap()).unwrap();
    let error=acquire(&core,target,"downstream").unwrap_err();assert_eq!(error.code,"HOST_KEY_REQUIRED");assert_eq!(error.details.as_ref().unwrap()["serverId"],nodes[2]["id"]);
    core.call(&Caller::shell(),"hosts.trust",error.details.unwrap()).unwrap();
    assert_eq!(acquire(&core,target,"downstream-secret").unwrap_err().code,"AUTH_REQUIRED");
    core.shutdown();drained(&core,&f);
}

#[test]
#[ignore="Requires isolated 16-node protocol fixture; tests actual global transport capacity"]
fn transport_capacity_reserves_full_paths_and_releases_after_worker_exit(){
    let f=Fixture::start(15);let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let nodes=f.setup(&core);let target=nodes.last().unwrap();
    let initial=connect(&core,target,16,"trust-capacity");core.call(&Caller::shell(),"connections.release",json!({"leaseId":initial["leaseId"]})).unwrap();drained(&core,&f);
    let mut leases=Vec::new();
    for index in 0..16 {
        leases.push(core.call(&Caller::shell(),"connections.acquire",json!({"serverId":target["id"],"workspaceId":format!("capacity-{index}"),"operationId":format!("capacity-{index}")})).unwrap());
    }
    let usage=core.call(&Caller::shell(),"connections.route.v2.list",json!({})).unwrap();assert_eq!(usage["usage"],json!({"sessions":256,"bridges":240}));
    let before=f.events().matches("connection-open").count();
    let error=acquire(&core,target,"over-capacity").unwrap_err();assert_eq!(error.code,"CONNECTION_RESOURCES_EXHAUSTED");assert_eq!(error.details.unwrap()["availableSessions"],0);assert_eq!(before,f.events().matches("connection-open").count());
    let shared=core.call(&Caller::shell(),"connections.acquire",json!({"serverId":target["id"],"workspaceId":"capacity-0","operationId":"reuse"})).unwrap();assert_eq!(shared["connectionId"],leases[0]["connectionId"]);
    echo(&core,&shared);
    core.shutdown();drained(&core,&f);
}

#[test]
#[ignore="Requires isolated multihop fixture; repeated cancellation and active disconnect"]
fn fifty_multihop_cycles_and_cancellations_leave_no_workers(){
    let f=Fixture::start(3);let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let nodes=f.setup(&core);let target=nodes.last().unwrap();
    let lease=connect(&core,target,4,"trust-cycles");core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();drained(&core,&f);
    for index in 0..50 {
        let lease=acquire(&core,target,&format!("cycle-{index}")).unwrap();core.call(&Caller::shell(),"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();drained(&core,&f);
        f.marker(1,"pause-handshake",true);let c=core.clone();let t=target.clone();let op=format!("cancel-cycle-{index}");let child_op=op.clone();let worker=std::thread::spawn(move||acquire(&c,&t,&child_op));
        let until=Instant::now()+Duration::from_secs(5);
        loop{let s=core.call(&Caller::shell(),"connections.route.v2.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if s["attempt"]["hopIndex"]==1&&s["attempt"]["stage"]=="handshake"{break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
        let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":op})).unwrap();assert_eq!(worker.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));f.marker(1,"pause-handshake",false);drained(&core,&f);
    }
    let lease=acquire(&core,target,"active-drop").unwrap();echo(&core,&lease);f.marker(1,"drop",true);
    let until=Instant::now()+Duration::from_secs(5);
    loop{let state=core.call(&Caller::shell(),"connections.route.v2.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if state["running"][0]["state"]=="closed"{assert!(state["running"][0]["failureHop"].is_number());break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(20));}
    core.shutdown();drained(&core,&f);
}
