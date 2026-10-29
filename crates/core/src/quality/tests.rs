use super::*;
use crate::{Core,Caller};
use base64::{engine::general_purpose::STANDARD,Engine};

#[test]
fn policies_are_persisted_revision_checked_and_do_not_connect() {
    let root=tempfile::tempdir().unwrap();let caller=Caller::shell();let server;
    {
        let core=Core::open(root.path()).unwrap();
        server=core.call(&caller,"servers.create",json!({"name":"offline","host":"127.0.0.1","port":22339,"username":"test","authType":"key"})).unwrap();
        let p=json!({"serverId":server["id"],"configEpoch":core.traffic.config_epoch});
        assert_eq!(core.call(&caller,"quality.policy.get",p.clone()).unwrap()["enabled"],false);
        let update=json!({"serverId":server["id"],"configEpoch":core.traffic.config_epoch,"expectedRevision":"0","patch":{"enabled":true}});
        assert_eq!(core.call(&caller,"quality.policy.set",update.clone()).unwrap()["revision"],"1");
        assert_eq!(core.call(&caller,"quality.policy.set",update).unwrap_err().code,"CONFLICT");
        assert_eq!(core.call(&caller,"quality.policy.set",json!({"serverId":server["id"],"configEpoch":core.traffic.config_epoch,"expectedRevision":"1","patch":{"slowMs":100,"recoveryMs":100}})).unwrap_err().code,"INVALID_PARAMS");
        assert_eq!(core.call(&caller,"quality.check.start",json!({"serverId":server["id"],"configEpoch":core.traffic.config_epoch,"operationId":"offline"})).unwrap_err().code,"DISCONNECTED");
        assert_eq!(core.call(&caller,"quality.summary",p).unwrap()["activeProbes"],0);assert!(core.registry.lock().unwrap().connections.is_empty());
        assert_eq!(core.call(&caller,"quality.summary",json!({"configEpoch":"old"})).unwrap_err().code,"STORE_EPOCH_CHANGED");
        assert_eq!(core.call(&caller,"quality.summary",json!({"configEpoch":core.traffic.config_epoch,"source":"forged"})).unwrap_err().code,"INVALID_PARAMS");
    }
    let core=Core::open(root.path()).unwrap();assert!(core.quality.policy(server["id"].as_str().unwrap()).enabled);assert!(core.registry.lock().unwrap().connections.is_empty());
}

#[test]
#[ignore="requires dedicated loopback OpenSSH: SODECK_TEST_COMMUNICATION_CONFIG"]
fn real_checks_share_existing_connections_and_preserve_pty_after_cancel() {
    let config:Value=serde_json::from_slice(&std::fs::read(std::env::var("SODECK_TEST_COMMUNICATION_CONFIG").unwrap()).unwrap()).unwrap();
    for node in config["nodes"].as_array().unwrap() {
        assert_eq!(node["host"],"127.0.0.1");assert!((22329..=22339).contains(&node["port"].as_u64().unwrap()));
        let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
        let caller=Caller{instance_id:"quality-window-a".into(),..Caller::shell()};
        let other=Caller{instance_id:"quality-window-b".into(),..caller.clone()};
        let server=core.call(&caller,"servers.create",json!({"name":"quality fixture","host":node["host"],"port":node["port"],"username":node["username"],"authType":"key"})).unwrap();
        core.call(&caller,"auth.ssh.set",json!({"serverId":server["id"],"privateKeyPath":node["key"]})).unwrap();
        let acquire=json!({"serverId":server["id"],"workspaceId":"quality-fixture","operationId":"untrusted"});
        let error=core.call(&caller,"connections.acquire",acquire.clone()).unwrap_err();assert_eq!(error.code,"HOST_KEY_REQUIRED");
        core.call(&caller,"hosts.trust",json!({"serverId":server["id"],"fingerprint":error.details.unwrap()["fingerprint"]})).unwrap();
        let mut acquire=acquire;acquire["operationId"]=json!("trusted");let lease=core.call(&caller,"connections.acquire",acquire).unwrap();
        let pty=core.call(&caller,"pty.open",json!({"leaseId":lease["leaseId"],"cols":80,"rows":24})).unwrap();
        let target=json!({"serverId":server["id"],"configEpoch":core.traffic.config_epoch});
        assert_eq!(core.call(&caller,"quality.summary",target.clone()).unwrap()["activeProbes"],0);
        let mut first_request=target.clone();first_request["operationId"]=json!("once");
        let first=core.call(&caller,"quality.check.start",first_request.clone()).unwrap();
        assert_eq!(core.call(&caller,"quality.check.start",first_request.clone()).unwrap()["taskId"],first["taskId"]);
        assert_eq!(core.call(&other,"quality.check.get",json!({"taskId":first["taskId"]})).unwrap_err().code,"TARGET_ENDED");
        let wait=|id:&Value| {let deadline=Instant::now()+Duration::from_secs(5);loop {
            let value=core.call(&caller,"quality.check.get",json!({"taskId":id})).unwrap();
            if value["state"]=="finished"&&!value["cleanupPending"].as_bool().unwrap(){break value;}
            assert!(Instant::now()<deadline,"probe did not retire: {value}");std::thread::sleep(Duration::from_millis(20));
        }};
        let first=wait(&first["taskId"]);assert_eq!(first["result"]["outcome"],"succeeded");assert_eq!(first["result"]["sent"],true);assert!(first["result"]["durationMs"].as_f64().unwrap()<3000.);
        let mut cancellation=target.clone();cancellation["operationId"]=json!("cancel");
        let cancel=core.call(&caller,"quality.check.start",cancellation).unwrap();core.release_communication_owner(&caller.instance_id);
        let canceled=wait(&cancel["taskId"]);assert!(matches!(canceled["result"]["outcome"].as_str(),Some("CANCELED"|"succeeded")));
        core.call(&caller,"pty.write",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"inputSeq":1,"bytes":STANDARD.encode(b"echo T019-quality-business-still-alive\r\n")})).unwrap();
        let deadline=Instant::now()+Duration::from_secs(5);
        loop {let result=core.call(&caller,"pty.read",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":0})).unwrap();
            let bytes:Vec<u8>=result["frames"].as_array().unwrap().iter().flat_map(|v|STANDARD.decode(v["bytes"].as_str().unwrap()).unwrap()).collect();
            if String::from_utf8_lossy(&bytes).contains("T019-quality-business-still-alive"){break;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));}
        assert_eq!(core.registry.lock().unwrap().connections.len(),1,"diagnostics cannot create connections");
        let mut update=target.clone();update["expectedRevision"]=json!("0");update["patch"]=json!({"enabled":true});core.call(&caller,"quality.policy.set",update).unwrap();
        core.quality_tick();std::thread::sleep(Duration::from_millis(400));
        let summary=core.call(&caller,"quality.summary",target.clone()).unwrap();assert_eq!(summary["segments"][0]["connectionId"],first["connectionId"]);
        assert!(summary["segments"][0]["statistics"]["p95Ms"].is_null());
        core.call(&caller,"pty.close",json!({"ptyId":pty["ptyId"],"generation":pty["generation"]})).unwrap();core.call(&caller,"connections.release",json!({"leaseId":lease["leaseId"]})).unwrap();
        let deadline=Instant::now()+Duration::from_secs(5);loop {core.quality_tick();if core.quality.budget.active()==0{break;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));}
        assert!(core.registry.lock().unwrap().connections.is_empty());
        assert_eq!(core.call(&caller,"quality.check.start",first_request).unwrap()["taskId"],first["taskId"],"completed retry must not reconnect");
        assert_eq!(core.call(&caller,"quality.summary",target).unwrap()["segments"][0]["state"],"ended");
        core.shutdown();println!("{}: matched response, receipt identity, cancel, PTY survival, continuous probe and final lease release passed",node["platform"]);
    }
}
