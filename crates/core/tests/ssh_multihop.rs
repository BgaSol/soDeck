mod support;
use serde_json::{json, Value};
use sodeck_core::{Caller, Core};
use std::sync::{Arc, Barrier};
use std::{io::{BufRead,BufReader},process::{Child,Command,Stdio},time::{Duration,Instant}};
use base64::{Engine,engine::general_purpose::STANDARD};

fn create(core:&Core,name:&str,jump:Option<&Value>)->Value {
    core.call(&Caller::shell(),"servers.routes.create",json!({"name":name,"host":format!("{name}.internal"),"username":"user","accessPath":jump.map_or(json!({"steps":[]}),|s|{let mut ids=s["accessPath"]["steps"].as_array().unwrap().iter().map(|s|s["serverId"].clone()).collect::<Vec<_>>();ids.push(s["id"].clone());support::ssh_steps(json!(ids))})})).unwrap()
}
fn edit(core:&Core,server:&Value,changes:Value)->sodeck_core::Result<Value>{
    let mut p=server.clone();for key in ["id","revision","hasSecret"]{p.as_object_mut().unwrap().remove(key);}
    p["serverId"]=server["id"].clone();p["expectedRevision"]=server["revision"].clone();
    for(k,v)in changes.as_object().unwrap(){p[k]=v.clone();}
    core.call(&Caller::shell(),"servers.routes.update",p)
}
fn status(core:&Core,server:&Value)->Value {
    core.call(&Caller::shell(),"connections.route.get",json!({"serverId":server["id"],"workspaceId":"test"})).unwrap()
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
    let snapshot=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();
    assert_eq!(snapshot["runningCount"],64);assert_eq!(snapshot["currentCount"],1);assert_eq!(snapshot["changedCount"],63);
    assert!(snapshot["running"].as_array().unwrap().len()<64);assert!(serde_json::to_vec(&snapshot).unwrap().len()<=28*1024);
    let mut total=0;let mut cursor=Value::Null;
    loop{let page=core.call(&Caller::shell(),"connections.route.list",json!({"cursor":cursor})).unwrap();assert_eq!(page["usage"]["sessions"],256);total+=page["connections"].as_array().unwrap().len();cursor=page["nextCursor"].clone();if cursor.is_null(){break;}}
    assert_eq!(total,64);core.shutdown();drained(&core,&f);
    println!("64 live revisions, bounded state, all paged resources and full cleanup: {:?}",started.elapsed());
}

#[test]
fn explicit_paths_invalidate_only_referenced_host_configuration(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();
    let a=create(&core,"a",None);let b=create(&core,"b",Some(&a));let c=create(&core,"c",Some(&b));let d=create(&core,"d",Some(&c));
    assert_eq!(status(&core,&d)["saved"]["stepCount"],4);
    let page=core.call(&Caller::shell(),"connections.route.nodes",json!({"reference":status(&core,&d)["saved"]["reference"]})).unwrap();
    assert_eq!(page["nodes"].as_array().unwrap().iter().map(|n|n["name"].as_str().unwrap()).collect::<Vec<_>>(),["a","b","c","d"]);
    for server in [&b,&c,&d]{support::prefill(&core,"ssh",&server["id"],json!({"password":"test-only","remember":true}));}
    let before=status(&core,&d);
    let b=edit(&core,&b,json!({"name":"renamed"})).unwrap();
    assert_eq!(before["saved"]["routeRevision"],status(&core,&d)["saved"]["routeRevision"]);
    assert_eq!(core.call(&Caller::shell(),"servers.routes.get",json!({"serverId":d["id"]})).unwrap()["hasSecret"],true);
    edit(&core,&b,json!({"username":"new-user"})).unwrap();
    assert_ne!(before["saved"]["routeRevision"],status(&core,&d)["saved"]["routeRevision"]);
    assert_eq!(core.call(&Caller::shell(),"servers.routes.get",json!({"serverId":b["id"]})).unwrap()["hasSecret"],false);
    for server in [&c,&d]{assert_eq!(core.call(&Caller::shell(),"servers.routes.get",json!({"serverId":server["id"]})).unwrap()["hasSecret"],true);}
    let deps=core.call(&Caller::shell(),"servers.routes.dependencies",json!({"serverId":a["id"]})).unwrap();
    assert_eq!(deps["dependencies"].as_array().unwrap().len(),3);
    assert_eq!(deps["dependencies"].as_array().unwrap().iter().filter(|r|r["direct"]==true).count(),3);
    assert_eq!(core.call(&Caller::shell(),"servers.delete",json!({"serverId":a["id"],"stopSessions":true})).unwrap_err().code,"SERVER_DEPENDENTS");

}

#[test]
fn concurrent_explicit_paths_are_independent_and_reject_self_references(){
    let dir=tempfile::tempdir().unwrap();let core=Arc::new(Core::open(dir.path()).unwrap());let a=create(&core,"a",None);let b=create(&core,"b",None);
    let barrier=Arc::new(Barrier::new(2));let jobs=[(a.clone(),b.clone()),(b.clone(),a.clone())].into_iter().map(|(from,to)|{let core=core.clone();let barrier=barrier.clone();std::thread::spawn(move||{barrier.wait();edit(&core,&from,json!({"accessPath":support::ssh_steps(json!([to["id"]]))}))})}).collect::<Vec<_>>();
    for job in jobs{assert!(job.join().unwrap().is_ok());}
    // Neither path recursively expands the referenced host's own path.
    assert_eq!(status(&core,&a)["saved"]["stepCount"],2);assert_eq!(status(&core,&b)["saved"]["stepCount"],2);
    let a=core.call(&Caller::shell(),"servers.routes.get",json!({"serverId":a["id"]})).unwrap();
    assert_eq!(edit(&core,&a,json!({"accessPath":support::ssh_steps(json!([a["id"]]))})).unwrap_err().code,"INVALID_ROUTE");
}
#[test]
fn full_budget_path_pages_are_bounded_and_revision_bound(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let mut hosts=vec![];
    for index in 0..255{hosts.push(create(&core,&format!("node-{index}"),None));}
    let target=core.call(&Caller::shell(),"servers.routes.create",json!({"name":"Target","host":"target.internal","username":"user","accessPath":support::ssh_steps(json!(hosts.iter().map(|s|s["id"].clone()).collect::<Vec<_>>()))})).unwrap();
    let summary=status(&core,&target)["saved"].clone();assert_eq!(summary["stepCount"],256);let mut cursor=Value::Null;let mut count=0;let mut first=Value::Null;
    loop{let page=core.call(&Caller::shell(),"connections.route.nodes",json!({"reference":summary["reference"],"cursor":cursor})).unwrap();assert_eq!(page["offset"],count);let nodes=page["nodes"].as_array().unwrap();assert!(nodes.len()<=64);assert!(page.to_string().len()<=256*1024);count+=nodes.len();cursor=page["nextCursor"].clone();if first.is_null(){first=cursor.clone();}if cursor.is_null(){break;}}
    assert_eq!(count,256);let caller=Caller{instance_id:"other".into(),..Caller::shell()};assert_eq!(core.call(&caller,"connections.route.nodes",json!({"reference":summary["reference"],"cursor":first})).unwrap_err().code,"ROUTE_SNAPSHOT_EXPIRED");
    edit(&core,&hosts[100],json!({"username":"changed"})).unwrap();assert_eq!(core.call(&Caller::shell(),"connections.route.nodes",json!({"reference":summary["reference"],"cursor":first})).unwrap_err().code,"ROUTE_SNAPSHOT_EXPIRED");
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
            let node=core.call(&Caller::shell(),"servers.routes.create",json!({"name":format!("hop-{index}"),"host":if index==0{"127.0.0.1".into()}else if last{"target.internal".into()}else{format!("hop-{index}.internal")},"port":if index==0{self.port}else{22},"username":if last{"test-user"}else{"jump-user"},"accessPath":if nodes.is_empty(){json!({"steps":[]})}else{support::ssh_steps(json!(nodes.iter().map(|n|n["id"].clone()).collect::<Vec<_>>()))}})).unwrap();
            support::prefill(&core,"ssh",&node["id"],json!({"password":if last{"isolated-test-password"}else{"jump-test-password"}}));nodes.push(node);
        }
        nodes
    }
}
impl Drop for Fixture{fn drop(&mut self){drop(self.child.stdin.take());let _=self.child.wait();}}
fn acquire(core:&Core,target:&Value,op:&str)->sodeck_core::Result<Value>{core.call(&Caller::shell(),"connections.acquire",json!({"serverId":target["id"],"workspaceId":"protocol","operationId":op}))}
fn connect(core:&Core,target:&Value,count:usize,op:&str)->Value{
    for _ in 0..=count*2 {match acquire(core,target,op){Ok(lease)=>return lease,Err(error)=>{
        let details=error.details.unwrap();
        if error.code=="CREDENTIAL_CONFIRM_REQUIRED"{core.call(&Caller::shell(),"auth.credentials.usePreset",json!({"challengeId":details["challengeId"],"operationId":op,"remember":false})).unwrap();continue;}
        assert_eq!(error.code,"HOST_KEY_REQUIRED");
        let challenge=core.call(&Caller::shell(),"auth.challenge.get",json!({"challengeId":details["challengeId"],"targetServerId":target["id"]})).unwrap();
        assert_eq!(challenge["stepCount"],count);assert!(challenge["stepIndex"].as_u64().unwrap()<(count as u64));
        core.call(&Caller::shell(),"hosts.trust",json!({"serverId":details["serverId"],"challengeId":details["challengeId"],"fingerprint":details["fingerprint"]})).unwrap();
    }}}
    panic!("path did not complete")
}
fn drained(core:&Core,f:&Fixture){
    let until=Instant::now()+Duration::from_secs(5);
    loop {
        let budget=core.call(&Caller::shell(),"connections.route.list",json!({})).unwrap();let events=f.events();
        if budget["usage"]["sessions"]==0&&budget["usage"]["bridges"]==0&&events.matches("connection-open").count()==events.matches("connection-close").count(){break}
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
        let segments=core.call(&Caller::shell(),"connections.route.segments",json!({"connectionId":lease["connectionId"],"generation":lease["generation"]})).unwrap();
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
        loop{let s=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if s["attempt"]["hopIndex"]==index&&s["attempt"]["stage"]=="handshake"{break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
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
    let usage=core.call(&Caller::shell(),"connections.route.list",json!({})).unwrap();assert_eq!(usage["usage"]["sessions"],256);assert_eq!(usage["usage"]["bridges"],240);
    let before=f.events().matches("connection-open").count();
    let error=acquire(&core,target,"over-capacity").unwrap_err();assert_eq!(error.code,"CONNECTION_RESOURCES_EXHAUSTED");assert_eq!(error.details.unwrap()["available"]["sessions"],0);assert_eq!(before,f.events().matches("connection-open").count());
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
        loop{let s=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if s["attempt"]["hopIndex"]==1&&s["attempt"]["stage"]=="handshake"{break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
        let start=Instant::now();core.call(&Caller::shell(),"operations.cancel",json!({"operationId":op})).unwrap();assert_eq!(worker.join().unwrap().unwrap_err().code,"CANCELED");assert!(start.elapsed()<Duration::from_secs(1));f.marker(1,"pause-handshake",false);drained(&core,&f);
    }
    let lease=acquire(&core,target,"active-drop").unwrap();echo(&core,&lease);f.marker(1,"drop",true);
    let until=Instant::now()+Duration::from_secs(5);
    loop{let state=core.call(&Caller::shell(),"connections.route.get",json!({"serverId":target["id"],"workspaceId":"protocol"})).unwrap();if state["running"][0]["state"]=="closed"{assert!(state["running"][0]["failureHop"].is_number());break}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(20));}
    core.shutdown();drained(&core,&f);
}
