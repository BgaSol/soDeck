//! Real OpenSSH: startup idempotency and observer independence from display ack.
use base64::{engine::general_purpose::STANDARD,Engine};
use serde_json::{json,Value};
use sodeck_core::{Core,Caller};
use std::time::{Instant,Duration};
#[test]
#[ignore="requires isolated SODECK_TEST_REMOTE_CONFIG"]
fn startup_receipt_and_eight_independent_observers(){
    let config=std::env::var("SODECK_TEST_REMOTE_CONFIG").unwrap();
    let cfg:Value=serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    assert_eq!(cfg["host"],"127.0.0.1");assert_eq!(cfg["port"],22281);
    let temp=tempfile::tempdir().unwrap();let core=Core::open(temp.path()).unwrap();let caller=Caller::shell();
    let call=|method:&str,p:Value|core.call(&caller,method,p).unwrap_or_else(|e|panic!("{method}: {e}"));
    let server=call("servers.create",json!({"name":"observer fixture","host":cfg["host"],"port":cfg["port"],"username":cfg["username"],"authType":"key"}));
    call("auth.set",json!({"serverId":server["id"],"privateKeyPath":cfg["privateKeyPath"]}));
    let params=json!({"serverId":server["id"],"workspaceId":"observer"});
    let challenge=core.call(&caller,"connections.acquire",params.clone()).unwrap_err();assert_eq!(challenge.code,"HOST_KEY_REQUIRED");
    call("hosts.trust",json!({"serverId":server["id"],"fingerprint":challenge.details.unwrap()["fingerprint"]}));
    let lease=call("connections.acquire",params);let mut ptys=Vec::new();
    for n in 0..8 {
        let p=json!({"operationId":format!("start-{n}"),"leaseId":lease["leaseId"],"cols":80,"rows":24,"command":"printf 'FIRST_FRAME'; sleep 2; printf 'LAST_FRAME'; sleep 1","observe":true});
        let pty=call("pty.openCommand",p.clone());assert_eq!(pty,call("pty.openCommand",p.clone()));
        let receipt=call("pty.start.get",json!({"operationId":format!("start-{n}")}));assert_eq!(receipt["result"],pty);
        let mut conflict=p;conflict["command"]=json!("echo replay");assert_eq!(core.call(&caller,"pty.openCommand",conflict).unwrap_err().code,"OPERATION_CONFLICT");
        ptys.push(pty);
    }
    assert_eq!(call("resources.list",json!({}))["ptys"].as_array().unwrap().len(),8);
    for pty in &ptys {
        let mut p=json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"observerId":pty["observer"]["observerId"],"after":"0"});
        let deadline=Instant::now()+Duration::from_secs(15);let mut bytes=Vec::new();let first=call("pty.observe.read",p.clone());
        assert_eq!(first,call("pty.observe.read",p.clone()),"observer reads are retryable");
        bytes.extend(STANDARD.decode(first["bytes"].as_str().unwrap()).unwrap());p["after"]=first["nextCursor"].clone();
        // Display acknowledgement never consumes the observer's independent data.
        let display=call("pty.read",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"after":0}));
        if let Some(seq)=display["lastSequence"].as_u64(){call("pty.ack",json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"sequence":seq}));}
        loop {
            let r=call("pty.observe.read",p.clone());assert_eq!(r["gap"],false);bytes.extend(STANDARD.decode(r["bytes"].as_str().unwrap()).unwrap());p["after"]=r["nextCursor"].clone();
            if r["closed"]==true{break;}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));
        }
        let text=String::from_utf8_lossy(&bytes);assert!(text.contains("FIRST_FRAME")&&text.contains("LAST_FRAME"));
        let mut foreign=caller.clone();foreign.instance_id="another".into();assert_eq!(core.call(&foreign,"pty.observe.read",p.clone()).unwrap_err().code,"SOURCE_INVALID");
        let close=json!({"ptyId":pty["ptyId"],"generation":pty["generation"],"observerId":pty["observer"]["observerId"]});
        call("pty.observe.close",close.clone());call("pty.observe.close",close.clone());call("pty.close",json!({"ptyId":pty["ptyId"],"generation":pty["generation"]}));call("pty.observe.close",close);
    }
    call("connections.release",json!({"leaseId":lease["leaseId"]}));assert!(call("resources.list",json!({}))["ptys"].as_array().unwrap().is_empty());
}
