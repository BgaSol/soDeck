use super::*;
use crate::{Caller, Core};
use serde_json::{json, Value};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::time::{Duration, Instant};

#[test]
fn app_summary_feed_key_and_live_range_are_bounded() {
    assert_eq!(Core::communication_object_id(&json!({"target":{"kind":"server","configEpoch":"epoch","serverId":"server-a"}})).unwrap(),"64f8601ee329f7fbb34350d357605f32e878723bd3594e24ddd777d509b5d5e5");
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();
    let range=core.call(&Caller::shell(),"communication.history.query",json!({"range":{"kind":"live","minutes":60}})).unwrap();
    assert_eq!(range["points"].as_array().unwrap().len(),720);
    assert!(range["points"].as_array().unwrap().iter().all(|p|p["gap"]==true&&p["sendRate"].is_null()));
    assert_eq!(core.call(&Caller::shell(),"communication.history.query",json!({"range":{"kind":"live","minutes":61}})).unwrap_err().code,"INVALID_PARAMS");
}

#[test]
#[ignore = "requires dedicated loopback OpenSSH: SODECK_TEST_COMMUNICATION_CONFIG"]
fn trusted_pty_sampling_conserves_totals_and_final_bytes() {
    let config:Value=serde_json::from_slice(&std::fs::read(std::env::var("SODECK_TEST_COMMUNICATION_CONFIG").unwrap()).unwrap()).unwrap();
    let node=config["nodes"].as_array().unwrap().iter().find(|n|n["platform"]=="linux").unwrap();
    assert_eq!(node["host"],"127.0.0.1");
    assert!((22329..=22339).contains(&node["port"].as_u64().unwrap()));
    let root=tempfile::tempdir().unwrap();
    let core=Core::open(root.path()).unwrap();
    let shell=Caller::shell();
    let terminal=Caller {plugin_id:Some("sodeck.terminal".into()),instance_id:"meter-terminal".into(),source:"plugin".into(),..shell.clone()};
    let server=core.call(&shell,"servers.create",json!({"name":"traffic fixture","host":node["host"],"port":node["port"],"username":node["username"],"authType":"key"})).unwrap();
    core.call(&shell,"auth.ssh.set",json!({"serverId":server["id"],"privateKeyPath":node["key"]})).unwrap();
    let request=json!({"serverId":server["id"],"workspaceId":"traffic-fixture","operationId":"first"});
    let challenge=core.call(&terminal,"connections.acquire",request.clone()).unwrap_err();
    assert_eq!(challenge.code,"HOST_KEY_REQUIRED");
    core.call(&shell,"hosts.trust",json!({"serverId":server["id"],"fingerprint":challenge.details.unwrap()["fingerprint"]})).unwrap();
    let mut request=request;request["operationId"]=json!("trusted");
    let lease=core.call(&terminal,"connections.acquire",request).unwrap();
    let pty=core.call(&terminal,"pty.open",json!({"leaseId":lease["leaseId"],"cols":80,"rows":24})).unwrap();
    let input=b"printf 'meter ownership and final-byte test\\n'\n";
    core.call(&terminal,"pty.write",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"inputSeq":1,"bytes":STANDARD.encode(input)})).unwrap();
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        core.traffic.sample();
        let summary=core.traffic.summary(None,"channel");
        if summary["receivedBytes"].as_str().unwrap().parse::<u64>().unwrap()>0 && summary["sentBytes"]==input.len().to_string() {break;}
        assert!(Instant::now()<deadline,"PTY output never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    let all=core.traffic.summary(None,"channel");
    let target=core.traffic.summary(server["id"].as_str(),"channel");
    assert_eq!(all["sentBytes"],input.len().to_string());
    assert_eq!(all["sentBytes"],target["sentBytes"]);
    assert_eq!(all["receivedBytes"],target["receivedBytes"]);
    assert_eq!(all["coverage"],"complete");
    assert_eq!(all["unattributedSentBytes"],"0");
    let summary=core.call(&shell,"communication.summary",json!({})).unwrap();
    let sources=core.call(&shell,"communication.sources",json!({"target":summary["target"]})).unwrap();
    assert_eq!(sources["items"].as_array().unwrap().len(),1);
    assert_eq!(sources["items"][0]["pluginId"],"sodeck.terminal");
    assert_eq!(sources["items"][0]["sentBytes"],input.len().to_string());
    let resources=core.call(&shell,"communication.connections",json!({"target":summary["target"]})).unwrap();
    assert_eq!(resources["items"][0]["resource"]["resourceId"],pty["ptyId"]);
    assert_eq!(resources["items"][0]["resource"]["generation"],pty["generation"].as_u64().unwrap().to_string());
    core.shutdown();
    let final_summary=core.traffic.summary(None,"channel");
    assert_eq!(final_summary["active"],0);
    assert_eq!(final_summary["sentBytes"],all["sentBytes"]);
    assert_eq!(final_summary["coverage"],"complete");
    assert_eq!(core.traffic.summary(None,"transport")["active"],0);
}

#[test]
fn metric_contract_rejects_spoofed_fields_epochs_and_unsigned_overflow() {
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let caller=Caller::shell();
    let summary=core.call(&caller,"communication.summary",json!({})).unwrap();
    assert_eq!(summary["sentBytes"],"0");
    assert_eq!(core.call(&caller,"communication.summary",json!({"pluginId":"spoof"})).unwrap_err().code,"INVALID_PARAMS");
    assert_eq!(core.call(&caller,"communication.summary",json!({"target":{"kind":"all","configEpoch":"old"}})).unwrap_err().code,"STORE_EPOCH_CHANGED");
    let mut corrupt=summary;corrupt["sentBytes"]=json!("18446744073709551616");
    assert!(super::contract::validate("x-results","communication.summary",&corrupt).is_err());
}

#[test]
fn failed_unopened_registrations_release_all_slots() {
    let traffic=Traffic::new("fixture".into());
    for round in 0..3 {
        let registrations:Vec<_>=(0..4096).map(|n|traffic.register(Owner {access_mode:"direct".into(),
            server_id:"server".into(),config_epoch:"fixture".into(),workspace_id:None,workspace_generation:None,
            plugin_id:None,instance_id:"test".into(),resource_id:format!("r-{round}-{n}"),generation:"1".into(),kind:"exec".into(),route_run_id:"route".into(),layer:"channel".into(),
        })).collect();
        assert!(registrations.iter().all(|r|r.counter.is_some()));
        drop(registrations);traffic.sample();
    }
    let counters:Vec<_>=(0..8192).filter_map(|_|traffic.meter.reserve()).collect();
    assert_eq!(counters.len(),8192);
}
