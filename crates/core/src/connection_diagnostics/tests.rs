use super::*;
use std::{io::{Read,Write},net::{TcpListener,Shutdown}};

fn call(core:&Core,method:&str,p:Value)->Value{core.call(&Caller::shell(),method,p).unwrap()}
fn target(core:&Core,steps:Value)->Value{call(core,"servers.routes.create",json!({"name":"diagnostic fixture","host":"127.0.0.1","port":1,"username":"PRIVATE_ACCOUNT_MARKER","authType":"password","accessPath":{"steps":steps}}))}
fn request(server:&Value)->Value{json!({"serverId":server["id"],"workspaceId":"diagnostic-test","operationId":id()})}
fn get(core:&Core,reference:&Value)->Value{call(core,"connections.diagnostics.get",json!({"reference":reference}))}
fn phases(core:&Core,record:&Value)->Vec<Value>{call(core,"connections.diagnostics.phases",json!({"reference":record["reference"],"revision":record["revision"]}))["phases"].as_array().unwrap().clone()}

#[test]
fn eof_is_at_method_reply_after_tcp_success_and_never_runs_target(){
    for received in [0,1]{
        let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
        let listener=TcpListener::bind(("127.0.0.1",0)).unwrap();let port=listener.local_addr().unwrap().port();
        let worker=std::thread::spawn(move||{let(mut socket,_)=listener.accept().unwrap();let mut greeting=[0;3];socket.read_exact(&mut greeting).unwrap();if received==1{socket.write_all(&[5]).unwrap();}socket.shutdown(Shutdown::Write).unwrap();});
        let proxy=call(&core,"proxies.socks5.create",json!({"name":"EOF fixture","host":"127.0.0.1","port":port,"username":"","authType":"none"}));
        let target=target(&core,json!([{"stepId":id(),"kind":"socks5","proxyId":proxy["id"],"resolve":"proxy"}]));
        let error=core.call(&Caller::shell(),"connections.acquire",request(&target)).unwrap_err();worker.join().unwrap();
        assert_eq!(error.code,"PROXY_DISCONNECTED");let record=get(&core,&error.details.unwrap()["diagnosticRef"]);
        assert_eq!(record["state"],"failed");assert_eq!(record["error"]["receivedBytes"],received);assert_eq!(record["error"]["expectedBytes"],2);
        let phases=phases(&core,&record);
        assert!(phases.iter().any(|p|p["stage"]=="connect"&&p["state"]=="succeeded"));
        assert!(phases.iter().any(|p|p["stage"]=="proxyNegotiate"&&p["state"]=="failed"));
        assert!(!phases.iter().any(|p|p["stage"]=="proxyConnect"||p["stage"]=="handshake"||p["stepIndex"]==1));
        assert!(!record.to_string().contains("PRIVATE_ACCOUNT_MARKER"));
        assert_eq!(call(&core,"resources.list",json!({}))["leases"].as_array().unwrap().len(),0);
    }
}

#[test]
fn exact_reference_snapshot_pagination_and_preparation_failures(){
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let server=target(&core,json!([]));let p=request(&server);
    let mut guard=AttemptGuard::new(&core,&Caller::shell(),&p,"session");let plan=core.route_plan(server["id"].as_str().unwrap()).unwrap();core.diagnostic_plan(&guard.token,&plan);
    core.diagnostic_phase(&guard.token,None,PhaseEvent::success("queue"));
    for _ in 0..70{core.diagnostic_phase(&guard.token,Some(0),PhaseEvent::start("handshake"));core.diagnostic_phase(&guard.token,Some(0),PhaseEvent::success("handshake"));}
    core.diagnostic_phase(&guard.token,None,PhaseEvent::start("deliver"));let mut result=Err(CoreError::new("CANCELED","private raw error"));guard.finish(&mut result);
    let record=get(&core,&guard.reference);assert_eq!(record["state"],"canceled");
    let first=call(&core,"connections.diagnostics.phases",json!({"reference":guard.reference,"revision":record["revision"]}));assert_eq!(first["phases"].as_array().unwrap().len(),64);assert!(first["nextCursor"].is_string());
    let second=call(&core,"connections.diagnostics.phases",json!({"reference":guard.reference,"revision":record["revision"],"cursor":first["nextCursor"]}));assert_eq!(second["phases"].as_array().unwrap().last().unwrap()["state"],"canceled");
    let mut wrong=guard.reference.clone();wrong["attemptGeneration"]=json!(0);assert_eq!(core.call(&Caller::shell(),"connections.diagnostics.get",json!({"reference":wrong})).unwrap_err().code,"DIAGNOSTIC_EXPIRED");
    call(&core,"servers.routes.update",json!({"serverId":server["id"],"expectedRevision":1,"name":"new","host":"changed.invalid","username":"different","accessPath":{"steps":[]}}));
    let nodes=call(&core,"connections.route.nodes",json!({"reference":record["route"]["reference"]}));assert_eq!(nodes["nodes"][0]["host"],"127.0.0.1");
    let error=core.call(&Caller::shell(),"connections.acquire",json!({"serverId":"missing","workspaceId":"diagnostic-test","operationId":id()})).unwrap_err();let prepared=get(&core,&error.details.unwrap()["diagnosticRef"]);assert_eq!(prepared["state"],"failed");assert!(prepared["route"].is_null());assert_eq!(phases(&core,&prepared)[0]["stage"],"prepare");
}

#[test]
fn starts_are_not_success_and_eviction_never_removes_authentication(){
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let s=target(&core,json!([]));let p=request(&s);
    let guard=AttemptGuard::new(&core,&Caller::shell(),&p,"session");let first=get(&core,&guard.reference);assert_eq!(phases(&core,&first)[0]["state"],"running");
    core.host_challenges.lock().unwrap().insert("fixture".into(),json!({"valid":"independent"}));
    for _ in 0..530{core.start_diagnostic(&Caller::shell(),&request(&s),"session");}
    {let cache=core.connection_diagnostics.lock().unwrap();assert!(cache.records.len()<=512);assert!(cache.records.values().map(Record::bytes).sum::<usize>()<=16*1024*1024);}
    assert!(core.host_challenges.lock().unwrap().contains_key("fixture"));
    let mut guard=guard;let mut result=Err(CoreError::new("CANCELED","canceled"));guard.finish(&mut result);assert!(result.unwrap_err().details.unwrap()["diagnosticRef"].is_object());
    let secret=CoreError::new("PROXY_NETWORK_ERROR","SECRET_MESSAGE").details(json!({"stage":"proxyNegotiate","password":"SECRET_PASSWORD","operation":"SECRET_OPERATION","receivedBytes":0,"expectedBytes":2,"osErrorCode":10054}));
    let safe=safe_error(&secret).to_string();assert!(!safe.contains("SECRET"));assert!(safe.contains("10054"));
}

#[test]
fn waiting_attempts_end_with_their_owner_or_deadline_and_continue_without_a_challenge() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let server = target(&core, json!([]));
    let wait = |p: &Value| {
        let mut guard = AttemptGuard::new(&core, &Caller::shell(), p, "session");
        let mut result = Err(CoreError::new("HOST_KEY_REQUIRED", "fixture"));
        guard.finish(&mut result);
        guard.reference.clone()
    };
    let p = request(&server);
    let first = wait(&p);
    // A successful confirmation removes its challenge. That is not cancellation.
    assert!(core.host_challenges.lock().unwrap().is_empty());
    assert_eq!(get(&core, &first)["state"], "waiting");
    let second = wait(&p);
    assert_eq!(get(&core, &first)["state"], "superseded");
    assert_eq!(get(&core, &second)["previous"], first);
    assert_eq!(get(&core, &second)["continuation"], true);

    call(&core, "operations.cancel", json!({"operationId":p["operationId"]}));
    let canceled = get(&core, &second);
    assert_eq!(canceled["state"], "canceled");
    assert_eq!(phases(&core, &canceled)[0]["state"], "canceled");
    assert_eq!(canceled["error"]["code"], "CANCELED");
    assert_eq!(call(&core, "connections.diagnostics.list", json!({"workspaceId":"diagnostic-test","source":"shell"}))["total"], 2);
    assert_eq!(call(&core, "connections.diagnostics.list", json!({"workspaceId":"diagnostic-test","source":"sodeck.monitor"}))["total"], 0);
    assert_eq!(get(&core, &first)["state"], "superseded");

    let expired = wait(&request(&server));
    core.connection_diagnostics.lock().unwrap().records.get_mut(expired["attemptId"].as_str().unwrap()).unwrap().updated -= 300_001;
    let ended = get(&core, &expired);
    assert_eq!(ended["state"], "failed");
    assert_eq!(ended["error"]["code"], "HOST_CHALLENGE_EXPIRED");
    assert_eq!(ended["error"]["stage"], "prepare");
    assert_eq!(phases(&core, &ended)[0]["state"], "failed");

    core.register_workspace("diagnostic-test", "first");
    let revoked = wait(&request(&server));
    core.revoke_workspace("diagnostic-test", "first");
    assert_eq!(get(&core, &revoked)["state"], "canceled");
    core.connection_diagnostics.lock().unwrap().records.get_mut(revoked["attemptId"].as_str().unwrap()).unwrap().updated -= 1_800_001;
    assert_eq!(core.call(&Caller::shell(), "connections.diagnostics.get", json!({"reference":revoked})).unwrap_err().code, "DIAGNOSTIC_EXPIRED");
}
