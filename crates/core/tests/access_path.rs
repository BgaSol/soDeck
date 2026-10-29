mod support;
use sodeck_core::{Core,Caller};
use serde_json::{Value,json};
fn call(core:&Core,method:&str,p:Value)->Value{core.call(&Caller::shell(),method,p).unwrap()}
fn server(core:&Core,name:&str,path:Value)->Value{call(core,"servers.routes.create",json!({"name":name,"host":"target.internal","port":22,"username":"test-user","authType":"password","accessPath":path}))}
fn proxy(core:&Core,port:u16,auth:bool)->Value{call(core,"proxies.socks5.create",json!({"name":"Network service","host":"127.0.0.1","port":port,"authType":if auth{"usernamePassword"}else{"none"},"username":if auth{"proxy-user"}else{""}}))}
fn edit(core:&Core,server:&Value,path:Value)->Value {
    call(core,"servers.routes.update",json!({"serverId":server["id"],"expectedRevision":server["revision"],"name":server["name"],"host":server["host"],"port":server["port"],"username":server["username"],"authType":server["authType"],"accessPath":path}))
}

#[test]
fn explicit_paths_do_not_inherit_host_routes_and_proxy_refs_are_isolated(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let p=proxy(&core,1080,false);
    let a=server(&core,"A",support::proxy_path(p["id"].clone(),"proxy"));
    let b=server(&core,"B",support::ssh_steps(json!([a["id"]])));
    let t=server(&core,"T",support::ssh_steps(json!([b["id"]])));
    let status=call(&core,"connections.route.get",json!({"serverId":t["id"],"workspaceId":t["id"]}));
    assert_eq!(status["saved"]["stepCount"],2);assert_eq!(status["saved"]["proxyCount"],0);
    let nodes=call(&core,"connections.route.nodes",json!({"reference":status["saved"]["reference"]}));assert_eq!(nodes["nodes"][0]["serverId"],b["id"]);
    support::prefill(&core,"ssh",&t["id"],json!({"password":"target-secret","remember":true}));
    let status=call(&core,"connections.route.get",json!({"serverId":t["id"],"workspaceId":t["id"]}));
    edit(&core,&b,json!({"steps":[]}));
    assert_eq!(status["saved"]["routeRevision"],call(&core,"connections.route.get",json!({"serverId":t["id"],"workspaceId":t["id"]}))["saved"]["routeRevision"]);
    assert_eq!(call(&core,"servers.routes.get",json!({"serverId":t["id"]}))["hasSecret"],true);
    let mut preview=t.clone();preview.as_object_mut().unwrap().remove("accessPath");
    let preview=call(&core,"servers.routes.preview",json!({"server":preview,"accessPath":support::proxy_path(p["id"].clone(),"local")}));
    assert_eq!(preview["route"]["proxyCount"],1);assert_eq!(preview["nodes"][0]["proxyId"],p["id"]);
    for ids in [json!([t["id"]]),json!([a["id"],a["id"]]),json!(["missing"])] {
        let mut change=t.clone();let obj=change.as_object_mut().unwrap();obj.remove("hasSecret");obj.remove("id");obj.remove("revision");
        change["serverId"]=t["id"].clone();change["expectedRevision"]=t["revision"].clone();change["accessPath"]=support::ssh_steps(json!(ids));
        assert!(core.call(&Caller::shell(),"servers.routes.update",change).is_err());
    }
    assert_eq!(core.call(&Caller::shell(),"proxies.socks5.delete",json!({"proxyId":p["id"]})).unwrap_err().code,"PROXY_DEPENDENTS");
    assert_eq!(core.call(&Caller::shell(),"servers.routes.v2.list",json!({})).unwrap_err().code,"UNSUPPORTED_API");
    assert_eq!(call(&core,"servers.list",json!({}))["servers"].as_array().unwrap().len(),3);
}

#[test]
fn proxy_secret_namespace_and_endpoint_changes(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let p=proxy(&core,1080,true);
    support::prefill(&core,"socks5",&p["id"],json!({"password":"secret","remember":true}));
    assert_eq!(std::fs::read_dir(dir.path().join("secrets")).unwrap().count(),1);assert!(!dir.path().join("secrets/proxies").exists());
    let got=call(&core,"proxies.socks5.get",json!({"proxyId":p["id"]}));assert_eq!(got["hasPreset"],true);assert!(got.get("password").is_none());
    let changed=call(&core,"proxies.socks5.update",json!({"proxyId":p["id"],"expectedRevision":p["revision"],"name":"Changed","host":"127.0.0.1","port":1081,"authType":"usernamePassword","username":"proxy-user"}));assert_eq!(changed["hasPreset"],false);
}

#[test]
fn concurrent_proxy_edits_have_one_winner_and_preserve_other_targets(){
    let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let p=proxy(&core,1080,false);
    let a=server(&core,"A",support::proxy_path(p["id"].clone(),"proxy"));
    let b=server(&core,"B",support::proxy_path(p["id"].clone(),"proxy"));
    edit(&core,&a,support::proxy_path(p["id"].clone(),"local"));
    assert_eq!(call(&core,"servers.routes.get",json!({"serverId":b["id"]}))["accessPath"]["steps"][0]["resolve"],"proxy");
    std::thread::scope(|scope|{
        let save=|name|core.call(&Caller::shell(),"proxies.socks5.update",json!({"proxyId":p["id"],"expectedRevision":p["revision"],"name":name,"host":p["host"],"port":p["port"],"authType":"none"}));
        let one=scope.spawn(move ||save("One"));let two=scope.spawn(move ||save("Two"));
        let results=[one.join().unwrap(),two.join().unwrap()];assert_eq!(results.iter().filter(|r|r.is_ok()).count(),1);
        assert_eq!(results.into_iter().find_map(|r|r.err()).unwrap().code,"REVISION_CONFLICT");
    });
}

#[test]
#[ignore="Requires isolated Python/Paramiko wire fixture"]
fn socks_connection_challenges_target_ssh_and_releases_resources(){
    use std::{io::{BufRead,BufReader},process::{Command,Stdio},time::{Duration,Instant}};
    let fixture=tempfile::tempdir().unwrap();
    let mut child=Command::new(std::env::var("SODECK_TEST_PYTHON").unwrap()).args(["-u",concat!(env!("CARGO_MANIFEST_DIR"),"/../../tests/core/socks_route_fixture.py")]).arg(fixture.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
    struct Stop(std::process::Child);impl Drop for Stop{fn drop(&mut self){drop(self.0.stdin.take());let _=self.0.wait();}}
    let mut line=String::new();BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();let info:Value=serde_json::from_str(&line).unwrap();let _stop=Stop(child);
    for (port,auth) in [("port",false),("authPort",true)] {
        let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let p=proxy(&core,info[port].as_u64().unwrap() as u16,auth);let t=server(&core,"Target",support::proxy_path(p["id"].clone(),"proxy"));
        support::prefill(&core,"ssh",&t["id"],json!({"password":"isolated-test-password"}));
        let acquire=||core.call(&Caller::shell(),"connections.acquire",json!({"serverId":t["id"],"workspaceId":t["id"],"operationId":"connect"}));
        if auth {let error=acquire().unwrap_err();assert_eq!(error.code,"PROXY_AUTH_REQUIRED");let detail=error.details.unwrap();assert_eq!(detail["ownerKind"],"socks5");assert!(detail.get("serverId").is_none());
            let challenge=call(&core,"auth.challenge.get",json!({"challengeId":detail["challengeId"],"targetServerId":t["id"]}));assert!(challenge.get("server").is_none());assert_eq!(challenge["proxy"]["id"],p["id"]);
            support::prefill(&core,"socks5",&p["id"],json!({"challengeId":detail["challengeId"],"password":"proxy-password"}));
        }
        let error=acquire().unwrap_err();assert_eq!(error.code,"HOST_KEY_REQUIRED");let detail=error.details.unwrap();assert_eq!(detail["ownerKind"],"targetSsh");assert_eq!(detail["serverId"],t["id"]);call(&core,"hosts.trust",detail);
        let lease=acquire().unwrap();let another=acquire().unwrap();assert_eq!(lease["connectionId"],another["connectionId"]);
        let status=call(&core,"connections.route.get",json!({"serverId":t["id"],"workspaceId":t["id"]}));assert_eq!(status["running"][0]["resources"],json!({"sessions":1,"bridges":0}));assert_eq!(status["saved"]["accessMode"],"networkProxy");
        call(&core,"connections.release",json!({"leaseId":another["leaseId"]}));
        call(&core,"proxies.socks5.update",json!({"proxyId":p["id"],"expectedRevision":p["revision"],"name":"Renamed only","host":p["host"],"port":p["port"],"authType":p["authType"],"username":p["username"]}));
        let fresh=acquire().unwrap();assert_ne!(fresh["connectionId"],lease["connectionId"]);
        let status=call(&core,"connections.route.get",json!({"serverId":t["id"],"workspaceId":t["id"]}));assert_eq!(status["changedCount"],1);
        call(&core,"connections.release",json!({"leaseId":fresh["leaseId"]}));call(&core,"connections.release",json!({"leaseId":lease["leaseId"]}));
        if auth {
            support::prefill(&core,"socks5",&p["id"],json!({"password":"wrong-password"}));
            let error=acquire().unwrap_err();assert_eq!(error.code,"PROXY_AUTH_FAILED");let details=error.details.unwrap();
            call(&core,"operations.cancel",json!({"operationId":"connect"}));
            assert_eq!(core.call(&Caller::shell(),"auth.socks5.set",json!({"proxyId":p["id"],"password":"proxy-password","challengeId":details["challengeId"]})).unwrap_err().code,"HOST_CHALLENGE_EXPIRED");
            let error=core.call(&Caller::shell(),"connections.acquire",json!({"serverId":t["id"],"workspaceId":t["id"],"operationId":"changed-config"})).unwrap_err();assert_eq!(error.code,"PROXY_AUTH_FAILED");let details=error.details.unwrap();
            let current=call(&core,"proxies.socks5.get",json!({"proxyId":p["id"]}));
            call(&core,"proxies.socks5.update",json!({"proxyId":p["id"],"expectedRevision":current["revision"],"name":"Edited while waiting","host":p["host"],"port":p["port"],"authType":p["authType"],"username":p["username"]}));
            assert_eq!(core.call(&Caller::shell(),"auth.socks5.set",json!({"proxyId":p["id"],"password":"proxy-password","challengeId":details["challengeId"]})).unwrap_err().code,"HOST_CHALLENGE_EXPIRED");
            support::prefill(&core,"socks5",&p["id"],json!({"password":"proxy-password"}));
            for cycle in 0..50 {
                let marker=fixture.path().join("pause-connect");std::fs::write(&marker,b"1").unwrap();
                let before=std::fs::read_to_string(fixture.path().join("proxy-events.jsonl")).unwrap().matches("\"event\": \"connect\"").count();
                let operation=format!("cancel-{cycle}");
                std::thread::scope(|scope| {
                    let pending=scope.spawn(||core.call(&Caller::shell(),"connections.acquire",json!({"serverId":t["id"],"workspaceId":t["id"],"operationId":operation})));
                    let deadline=Instant::now()+Duration::from_secs(5);
                    while std::fs::read_to_string(fixture.path().join("proxy-events.jsonl")).unwrap().matches("\"event\": \"connect\"").count()<=before {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(5));}
                    let started=Instant::now();call(&core,"operations.cancel",json!({"operationId":operation}));
                    assert_eq!(pending.join().unwrap().unwrap_err().code,"CANCELED");assert!(started.elapsed()<Duration::from_secs(1));
                });
                std::fs::remove_file(marker).unwrap();
            }
        }
        for cycle in 0..50 {
            let lease=call(&core,"connections.acquire",json!({"serverId":t["id"],"workspaceId":t["id"],"operationId":format!("cycle-{cycle}")}));
            call(&core,"connections.release",json!({"leaseId":lease["leaseId"]}));
        }
        use std::net::ToSocketAddrs;
        let addresses=("localhost",22).to_socket_addrs().unwrap().map(|v|v.ip().to_string()).collect::<Vec<_>>();
        assert!(addresses.iter().any(|ip|ip!=&addresses[0]),"multi-IP fixture needs both localhost families");
        std::fs::write(fixture.path().join("deny-ip"),&addresses[0]).unwrap();
        let denied_before=std::fs::read_to_string(fixture.path().join("proxy-events.jsonl")).unwrap().matches("\"event\": \"denied-ip\"").count();
        let local=call(&core,"servers.routes.create",json!({"name":"Local DNS","host":"localhost","port":22,"username":"test-user","authType":"password","accessPath":support::proxy_path(p["id"].clone(),"local")}));
        support::prefill(&core,"ssh",&local["id"],json!({"password":"isolated-test-password"}));
        let params=json!({"serverId":local["id"],"workspaceId":local["id"],"operationId":"local-dns"});
        let error=core.call(&Caller::shell(),"connections.acquire",params.clone()).unwrap_err();assert_eq!(error.code,"HOST_KEY_REQUIRED");call(&core,"hosts.trust",error.details.unwrap());
        let lease=call(&core,"connections.acquire",params);call(&core,"connections.release",json!({"leaseId":lease["leaseId"]}));
        std::fs::remove_file(fixture.path().join("deny-ip")).unwrap();
        let events=std::fs::read_to_string(fixture.path().join("proxy-events.jsonl")).unwrap();
        assert!(events.matches("\"event\": \"denied-ip\"").count()>=denied_before+2,"each acquisition must retry after first IP is refused");
        core.shutdown();
    }
    let deadline=Instant::now()+Duration::from_secs(5);loop{let events=std::fs::read_to_string(fixture.path().join("proxy-events.jsonl")).unwrap();if events.matches("\"event\": \"open\"").count()==events.matches("\"event\": \"close\"").count(){break;}assert!(Instant::now()<deadline,"proxy socket leaked");std::thread::sleep(Duration::from_millis(20));}
}
