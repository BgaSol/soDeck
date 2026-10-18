use super::model::*;
use super::*;
use crate::{
    connection::{SharedContext, WindowOwner},
    model::AppResult,
    State,
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::ipc::Channel;
const STATUS: &str = include_str!("fixtures/status-connected.json");
#[test]
fn status_preserves_units_missing_values_and_ipv6() {
    let s = parser::parse(STATUS, 100).unwrap();
    assert_eq!(s.connection, "connected");
    assert!(s.peers_complete);
    assert_eq!(s.peers[0].latency_ms, Some(18.));
    assert_eq!(
        s.peers[0].received_bytes.as_deref(),
        Some("9007199254740993")
    );
    assert_eq!(s.peers[0].sent_bytes.as_deref(), Some("0"));
    assert_eq!(s.peers[1].connection_mode, "relayed");
    assert_eq!(s.peers[2].state, "idle");
    assert_eq!(s.peers[2].ipv6.as_deref(), Some("fd00::4"));
    assert!(s.peers[2].latency_ms.is_none());
    assert!(s.peers[2].last_handshake_at.is_none());
    assert_eq!(s.dns.state, "notConfigured");
}
#[test]
fn partial_and_unknown_status_never_become_success_or_zero() {
    let mut v: serde_json::Value = serde_json::from_str(STATUS).unwrap();
    v.as_object_mut().unwrap().remove("daemonStatus");
    v["peers"]["total"] = json!(8);
    v["peers"]["details"][0]["status"] = json!("FutureState");
    v["dnsServers"] = json!([{ "enabled":true,"error":"lookup failed" }]);
    let s = parser::parse(&v.to_string(), 100).unwrap();
    assert_eq!(s.connection, "unknown");
    assert_eq!(s.authentication, "unknown");
    assert_eq!(s.peers[0].state, "unknown");
    assert_eq!(s.peers[0].raw_state.as_deref(), Some("FutureState"));
    assert!(!s.peers_complete);
    assert_eq!(s.dns.state, "error");
    assert!(parser::parse("{}", 0).is_err());
    assert!(parser::parse("not JSON", 0).is_err());
    for (daemon, expected) in [
        ("Idle", "disconnected"),
        ("Connecting", "connecting"),
        ("NeedsLogin", "needsLogin"),
        ("SessionExpired", "needsLogin"),
    ] {
        v["daemonStatus"] = json!(daemon);
        assert_eq!(
            parser::parse(&v.to_string(), 0).unwrap().connection,
            expected
        );
    }
}
#[test]
fn peer_list_is_bounded_and_duplicates_not_reported_as_complete() {
    let mut v: serde_json::Value = serde_json::from_str(STATUS).unwrap();
    v["peers"] = json!({"total":10001,"details":(0..10001).map(|i|json!({"publicKey":i.to_string()})).collect::<Vec<_>>()});
    let s = parser::parse(&v.to_string(), 0).unwrap();
    assert_eq!(s.peers.len(), 10000);
    assert!(!s.peers_complete);
    v["peers"] = json!({"total":2,"details":[{"publicKey":"same"},{"publicKey":"same"}]});
    let s = parser::parse(&v.to_string(), 0).unwrap();
    assert_eq!(s.peers.len(), 1);
    assert!(!s.peers_complete);
}
#[test]
fn commands_pin_daemon_clear_environment_and_quote_path() {
    let command = query::cli("/opt/a'$(touch nope)/netbird", "status --json");
    assert!(command.starts_with("test -S /var/run/netbird.sock && env -i "));
    assert!(command.contains("'unix:///var/run/netbird.sock'"));
    assert!(command.contains(&crate::ssh::quote("/opt/a'$(touch nope)/netbird")));
    assert!(query::validate_path("netbird").is_err());
    assert!(query::validate_path("/x\nexit").is_err());
    assert!(query::validate_path("/opt/NetBird App/netbird").is_ok());
}
#[test]
fn receipt_requires_strict_final_completion_marker() {
    assert_eq!(
        runner::receipt("hello\nTOKEN0\n", "TOKEN").unwrap(),
        ("hello".into(), 0)
    );
    for s in [
        "TOKEN0\ntrailing",
        "TOKEN+0\n",
        "TOKEN256\n",
        "TOKEN\n",
        "TOKEN-1\n",
        "hello",
    ] {
        assert!(runner::receipt(s, "TOKEN").is_err(), "{s}");
    }
}
fn snapshot() -> Snapshot {
    parser::parse(STATUS, 100).unwrap()
}
#[test]
fn safety_requires_address_interface_and_route_evidence() {
    let s = snapshot();
    let interfaces =
        json!([{"ifname":"mesh42","addr_info":[{"local":"100.80.0.1"},{"local":"fd00::1"}]}]);
    let route = json!([{"dev":"eth0","prefsrc":"127.0.0.1"}]);
    assert_eq!(
        safety::assess_data(
            &s,
            "127.0.0.1",
            false,
            "192.0.2.1 55000 127.0.0.1 22",
            &interfaces,
            &route
        )
        .result,
        "independent"
    );
    assert_eq!(
        safety::assess_data(
            &s,
            "100.80.0.1",
            false,
            "192.0.2.1 55000 100.80.0.1 22",
            &interfaces,
            &route
        )
        .result,
        "dependent"
    );
    assert_eq!(
        safety::assess_data(
            &s,
            "fd00::1",
            false,
            "2001:db8::1 55000 fd00::1 22",
            &interfaces,
            &route
        )
        .result,
        "dependent"
    );
    let mesh = json!([{"dev":"mesh42"}]);
    assert_eq!(
        safety::assess_data(
            &s,
            "127.0.0.1",
            false,
            "192.0.2.1 55000 127.0.0.1 22",
            &interfaces,
            &mesh
        )
        .result,
        "dependent"
    );
    for (host, jump, ifs, routes) in [
        ("host.invalid", false, interfaces.clone(), route.clone()),
        ("127.0.0.1", true, interfaces.clone(), route.clone()),
        ("127.0.0.1", false, json!([]), route.clone()),
        ("127.0.0.1", false, interfaces.clone(), json!([])),
    ] {
        assert_eq!(
            safety::assess_data(
                &s,
                host,
                jump,
                "192.0.2.1 55000 127.0.0.1 22",
                &ifs,
                &routes
            )
            .result,
            "unknown"
        );
    }
}
fn receipt() -> store::Receipt {
    store::Receipt {
        identity: "identity".into(),
        target: "host".into(),
        servers: vec!["server".into()],
        signature: "signature".into(),
        binding_token: "binding".into(),
        preview_id: "preview".into(),
        invocation: "before".into(),
        returned: false,
        operation: Operation {
            id: "operation".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            action: Action::Down,
            phase: "running".into(),
            message: String::new(),
            created_at: 1,
            finished_at: None,
            acknowledged: false,
        },
    }
}
#[test]
fn persistent_receipts_recover_as_uncertain_and_deduplicate() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("netbird.sqlite");
    let r = receipt();
    {
        let s = store::Store::open(&file).unwrap();
        s.save(&r).unwrap();
        s.set_path("server", Some("/opt/netbird")).unwrap();
        let mut collision = r.clone();
        collision.operation.id = "different".into();
        assert!(s.save(&collision).is_err());
    }
    let s = store::Store::open(&file).unwrap();
    let recovered = s
        .request("identity", &r.operation.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.operation.phase, "uncertain");
    assert_eq!(recovered.operation.id, r.operation.id);
    assert_eq!(s.path("server").unwrap().as_deref(), Some("/opt/netbird"));
    assert!(s
        .request("other", &r.operation.request_id)
        .unwrap()
        .is_none());
    s.set_path("server", None).unwrap();
    assert!(s.path("server").unwrap().is_none());
}
#[test]
fn previews_bind_owner_generation_action_target_and_expiry() {
    let owner = WindowOwner {
        label: "main".into(),
        generation: 1,
    };
    let mut draft = manager::Draft {
        owner: owner.clone(),
        identity: "identity".into(),
        signature: "signature".into(),
        preview: Preview {
            id: "preview".into(),
            binding_token: "binding".into(),
            action: Action::Down,
            expires_at: crate::traffic::now() + 30000,
            allowed: true,
            path: PathAssessment {
                result: "independent".into(),
                checked_at: 0,
                reasons: vec![],
            },
            consequences: vec![],
        },
    };
    let req = OperationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        binding_token: "binding".into(),
        action: Action::Down,
        preview_id: "preview".into(),
    };
    assert!(operations::preview_matches(
        &draft, &owner, "identity", &req
    ));
    let mut another = owner.clone();
    another.generation += 1;
    assert!(!operations::preview_matches(
        &draft, &another, "identity", &req
    ));
    another.label = "other".into();
    assert!(!operations::preview_matches(
        &draft, &another, "identity", &req
    ));
    let mut request = req.clone();
    request.binding_token = "other".into();
    assert!(!operations::preview_matches(
        &draft, &owner, "identity", &request
    ));
    draft.preview.expires_at = 0;
    assert!(!operations::preview_matches(
        &draft, &owner, "identity", &req
    ));
}
#[test]
fn state_probe_does_not_prove_restart_without_receipt_and_new_invocation() {
    let mut r = receipt();
    let mut s = snapshot();
    r.operation.action = Action::ServiceRestart;
    assert!(!operations::achieved(&r, &s));
    s.binding = Some(Binding {
        token: "x".into(),
        cli_path: "/usr/bin/netbird".into(),
        daemon_address: query::DAEMON.into(),
        client_version: None,
        daemon_version: None,
        profile: None,
        capabilities: Capabilities::default(),
        signature: "signature".into(),
        service: query::Service {
            invocation: "after".into(),
            ..Default::default()
        },
    });
    assert!(!operations::achieved(&r, &s));
    r.returned = true;
    assert!(operations::achieved(&r, &s));
    s.binding.as_mut().unwrap().service.invocation = "before".into();
    assert!(!operations::achieved(&r, &s));
}
fn wait(check: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < until, "NetBird fixture timed out");
        std::thread::sleep(Duration::from_millis(25));
    }
}
pub(crate) fn exercise(
    state: Arc<State>,
    context: SharedContext,
    first: &str,
    second: &str,
    root: &Path,
) {
    let owner = state.owner("main");
    let second_owner = state.owner("workspace-test");
    let values = Arc::new(Mutex::new(Vec::<Frame>::new()));
    let subscribe = |session: &str, owner: WindowOwner| -> AppResult<String> {
        let values = values.clone();
        state.netbird.subscribe(
            &state,
            session.into(),
            owner,
            Interest {
                level: "visible".into(),
            },
            Channel::new(move |body| {
                if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                    values.lock().unwrap().push(serde_json::from_str(&json)?);
                }
                Ok(())
            }),
        )
    };
    let sub = subscribe(first, owner.clone()).unwrap();
    let sub2 = subscribe(second, second_owner.clone()).unwrap();
    let entry = state.netbird.entry(context.identity()).unwrap();
    wait(|| entry.core.lock().unwrap().snapshot.collection_status == "ready");
    let token = entry
        .core
        .lock()
        .unwrap()
        .snapshot
        .binding
        .as_ref()
        .unwrap()
        .token
        .clone();
    assert_eq!(entry.core.lock().unwrap().snapshot.peers.len(), 3);
    // Ack backpressure: until acknowledged, only the initial frame reached each subscriber.
    assert_eq!(values.lock().unwrap().len(), 2);
    let revision = values.lock().unwrap()[0].revision;
    assert!(state.netbird.ack(&sub, &second_owner, revision).is_err());
    state.netbird.ack(&sub, &owner, revision).unwrap();
    assert_eq!(values.lock().unwrap().len(), 3);
    let result = state
        .netbird
        .read(
            &state,
            first,
            &owner,
            &token,
            |ssh, spec, snapshot, check| {
                diagnostics::probe(
                    ssh,
                    snapshot,
                    ProbeRequest {
                        peer_id: snapshot.peers[0].id.clone(),
                        kind: "icmp".into(),
                        port: None,
                    },
                    &spec.target().name,
                    check,
                )
            },
        )
        .unwrap();
    assert_eq!(result.state, "reachable");
    std::fs::write(root.join("netbird-permission"), "1").unwrap();
    state.netbird.refresh(&sub, &owner).unwrap();
    wait(|| entry.core.lock().unwrap().snapshot.collection_status == "stale");
    assert_eq!(entry.core.lock().unwrap().snapshot.peers.len(), 3);
    std::fs::remove_file(root.join("netbird-permission")).unwrap();
    state.netbird.refresh(&sub, &owner).unwrap();
    wait(|| entry.core.lock().unwrap().snapshot.collection_status == "ready");
    let preview = state
        .netbird
        .prepare(&state, first, &owner, &token, Action::Down)
        .unwrap();
    assert!(preview.allowed);
    let request = OperationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        binding_token: token.clone(),
        action: Action::Down,
        preview_id: preview.id,
    };
    assert!(state
        .netbird
        .submit(&state, second, &second_owner, request.clone())
        .is_err());
    let id = state
        .netbird
        .submit(&state, first, &owner, request.clone())
        .unwrap();
    assert_eq!(
        state
            .netbird
            .submit(&state, first, &owner, request.clone())
            .unwrap(),
        id
    );
    wait(|| !state.netbird.active());
    let r = state.netbird.with_store(|s| s.get(&id)).unwrap();
    assert_eq!(r.operation.phase, "succeeded", "{}", r.operation.message);
    assert_eq!(
        entry.core.lock().unwrap().snapshot.connection,
        "disconnected"
    );
    let preview = state
        .netbird
        .prepare(&state, first, &owner, &token, Action::Up)
        .unwrap();
    assert!(preview.allowed);
    std::fs::write(root.join("netbird-drop-receipt"), "1").unwrap();
    let id = state
        .netbird
        .submit(
            &state,
            first,
            &owner,
            OperationRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                binding_token: token.clone(),
                action: Action::Up,
                preview_id: preview.id,
            },
        )
        .unwrap();
    wait(|| !state.netbird.active());
    assert_eq!(
        state
            .netbird
            .with_store(|s| s.get(&id))
            .unwrap()
            .operation
            .phase,
        "uncertain"
    );
    std::fs::remove_file(root.join("netbird-drop-receipt")).unwrap();
    state.netbird.reconcile(&state, first, &owner, &id).unwrap();
    assert_eq!(
        state
            .netbird
            .with_store(|s| s.get(&id))
            .unwrap()
            .operation
            .phase,
        "succeeded"
    );
    for action in [Action::ServiceRestart, Action::ServiceStart] {
        if action == Action::ServiceStart {
            let file = root.join("netbird-state.json");
            let mut v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            v["active"] = json!("inactive");
            std::fs::write(&file, v.to_string()).unwrap();
        }
        let preview = state
            .netbird
            .prepare(&state, first, &owner, &token, action.clone())
            .unwrap();
        assert!(preview.allowed, "{:?}", preview.consequences);
        let id = state
            .netbird
            .submit(
                &state,
                first,
                &owner,
                OperationRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    binding_token: token.clone(),
                    action,
                    preview_id: preview.id,
                },
            )
            .unwrap();
        wait(|| !state.netbird.active());
        let r = state.netbird.with_store(|s| s.get(&id)).unwrap();
        assert_eq!(r.operation.phase, "succeeded", "{}", r.operation.message);
    }
    std::fs::write(root.join("netbird-other-service"), "1").unwrap();
    assert!(
        !state
            .netbird
            .prepare(&state, first, &owner, &token, Action::ServiceRestart)
            .unwrap()
            .allowed
    );
    std::fs::remove_file(root.join("netbird-other-service")).unwrap();
    std::fs::write(root.join("netbird-dependent"), "1").unwrap();
    assert!(
        !state
            .netbird
            .prepare(&state, first, &owner, &token, Action::Down)
            .unwrap()
            .allowed
    );
    std::fs::remove_file(root.join("netbird-dependent")).unwrap();
    state.netbird.unsubscribe(&sub, &owner).unwrap();
    state.netbird.unsubscribe(&sub2, &second_owner).unwrap();
    state.netbird.shutdown().unwrap();
    let events = std::fs::read_to_string(root.join("netbird-actions.jsonl")).unwrap();
    assert_eq!(
        events.lines().count(),
        4,
        "duplicate commands must never be replayed"
    );
}
