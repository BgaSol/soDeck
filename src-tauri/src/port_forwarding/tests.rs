use super::*;
use crate::{
    connection::{ConnectionAnswer, ConnectionEvent},
    model::{PasswordChange, SaveServerRequest, ServerView},
    ssh_transport::forwarding::tests::Fixture,
};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    thread,
};

fn rule(server: &str) -> Rule {
    Rule {
        id: String::new(),
        revision: 0,
        name: "测试内网转发".into(),
        web_scheme: None,
        settings: Settings {
            server_id: server.into(),
            listen_address: "127.0.0.1".into(),
            listen_port: None,
            target_host: "echo.internal.invalid".into(),
            target_port: 7,
            auto_reconnect: true,
        },
    }
}
fn configure(state: &State, port: u16) -> Rule {
    let server = ServerView {
        id: "forward-server".into(),
        name: "仅转发 SSH".into(),
        host: "127.0.0.1".into(),
        port,
        username: "forward".into(),
        tags: vec![],
        auth_type: "password".into(),
        private_key: String::new(),
        has_saved_password: true,
        jump_server_id: None,
        direct_host: None,
        direct_port: 22,
    };
    state
        .database
        .lock()
        .unwrap()
        .save(SaveServerRequest {
            server,
            password_change: PasswordChange::Set {
                value: "test-only".into(),
            },
        })
        .unwrap();
    state
        .port_forwarding
        .with_store(|s| s.save(rule("forward-server")))
        .unwrap()
}
fn start(state: &Arc<State>, rule: &Rule) -> Arc<Run> {
    let _gate = state.configuration.lock().unwrap();
    let plan = Arc::new(
        ConnectionPlan::new(
            state
                .database
                .lock()
                .unwrap()
                .resolve_connection(&rule.settings.server_id)
                .unwrap(),
        )
        .unwrap(),
    );
    let id = state
        .port_forwarding
        .start(
            state.clone(),
            rule.clone(),
            plan,
            state.owner("port-forwarding"),
        )
        .unwrap();
    state.port_forwarding.run(&rule.id, &id).unwrap()
}
fn until(mut condition: impl FnMut() -> bool) {
    let began = Instant::now();
    while !condition() {
        assert!(
            began.elapsed() < Duration::from_secs(20),
            "condition timed out"
        );
        thread::sleep(Duration::from_millis(15));
    }
}
fn trust(run: &Run) {
    until(|| run.view.lock().unwrap().prompt.is_some() || run.done.load(Ordering::SeqCst));
    let prompt = run
        .view
        .lock()
        .unwrap()
        .prompt
        .clone()
        .expect("host key challenge");
    let ConnectionEvent::HostKeyRequired { challenge } = prompt.event else {
        panic!("expected host key");
    };
    run.interaction
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .answer(&challenge.id, ConnectionAnswer::Trust { accept: true })
        .unwrap();
}
fn echo(address: &str) {
    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let bytes: Vec<_> = (0..131_071).map(|n| (n % 251) as u8).collect();
    client.write_all(&bytes).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut answer = Vec::new();
    client.read_to_end(&mut answer).unwrap();
    assert_eq!(answer, bytes);
}
struct Stop(Arc<State>);
impl Drop for Stop {
    fn drop(&mut self) {
        let _ = self.0.port_forwarding.shutdown();
    }
}

#[test]
#[ignore = "isolated real SSH; requires Python paramiko"]
fn forward_worker_lifecycle_reconnect_pinned_identity_and_configuration_snapshot() {
    let mut fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let _stop = Stop(state.clone());
    let rule = configure(&state, fixture.port);
    let run = start(&state, &rule);
    assert_eq!(
        start(&state, &rule).id,
        run.id,
        "duplicate start must return same run"
    );
    trust(&run);
    until(|| run.view.lock().unwrap().phase == "ready");
    let address = run.view.lock().unwrap().actual_address.clone().unwrap();
    echo(&address);
    assert!(
        state.terminals.lock().unwrap().is_empty(),
        "forward-only account needs no terminal"
    );
    assert!(state.delete_server("forward-server").is_err());
    // Closing the observer leaves the ready path running.
    state
        .port_forwarding
        .window_closed(&state.owner("port-forwarding"));
    echo(&address);
    // Saved settings must never retarget a live run or its automatic reconnect.
    let mut edited = rule.clone();
    edited.settings.target_host = "denied.invalid".into();
    state
        .port_forwarding
        .with_store(|s| s.save(edited))
        .unwrap();
    assert!(state.port_forwarding.snapshot(&state).unwrap()[0].pending_changes);
    fixture.restart(false);
    until(|| run.view.lock().unwrap().phase == "reconnecting");
    assert_eq!(
        run.view.lock().unwrap().actual_address.as_deref(),
        Some(address.as_str())
    );
    assert!(
        TcpListener::bind(&address).is_err(),
        "local port remains reserved"
    );
    until(|| run.view.lock().unwrap().phase == "ready");
    echo(&address);
    // A changed key pauses retries and retains the same local port.
    fixture.restart(true);
    until(|| run.view.lock().unwrap().phase == "attention");
    assert_eq!(
        run.view.lock().unwrap().error.as_ref().unwrap().kind,
        "hostKey"
    );
    let changed_key = run
        .view
        .lock()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .fingerprint
        .clone()
        .unwrap();
    // Simulate another window trusting it: this run must still challenge against its old key.
    state
        .database
        .lock()
        .unwrap()
        .trust("direct", "127.0.0.1", fixture.port, &changed_key)
        .unwrap();
    run.control(Control::Resume(state.owner("port-forwarding")))
        .unwrap();
    until(|| run.view.lock().unwrap().prompt.is_some());
    let prompt = run.view.lock().unwrap().prompt.clone().unwrap();
    let ConnectionEvent::HostKeyRequired { challenge } = &prompt.event else {
        panic!()
    };
    assert_ne!(challenge.previous.as_ref(), Some(&changed_key));
    // Cancelling an authentication retry must keep the listener and go back to attention.
    run.control(Control::CancelAuth(prompt.attempt_id)).unwrap();
    until(|| run.view.lock().unwrap().phase == "attention");
    assert!(TcpListener::bind(&address).is_err());
    run.control(Control::Resume(state.owner("port-forwarding")))
        .unwrap();
    trust(&run);
    until(|| run.view.lock().unwrap().phase == "ready");
    echo(&address);
    run.stop();
    run.wait().unwrap();
    assert!(
        run.context.lock().unwrap().is_none(),
        "stop releases credentials"
    );
    assert_eq!(
        state
            .port_forwarding
            .total_connections
            .load(Ordering::SeqCst),
        0
    );
    assert!(
        TcpListener::bind(&address).is_ok(),
        "stop releases listener before done"
    );
    assert!(run.view.lock().unwrap().sent_bytes.parse::<u64>().unwrap() >= 4 * 131_071);
}

#[test]
fn forward_cancel_start_window_generation_port_conflict_and_ipv6() {
    let state = crate::connection::tests::state();
    let _stop = Stop(state.clone());
    let ssh_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let rule = configure(&state, ssh_listener.local_addr().unwrap().port());
    let run = start(&state, &rule);
    let (_stalled_handshake, _) = ssh_listener.accept().unwrap();
    until(|| run.view.lock().unwrap().actual_address.is_some());
    let address = run.view.lock().unwrap().actual_address.clone().unwrap();
    state.port_forwarding.window_closed(&WindowOwner {
        label: "port-forwarding".into(),
        generation: 99,
    });
    assert!(
        !run.stopped.load(Ordering::SeqCst),
        "old window cannot stop newer startup"
    );
    state
        .port_forwarding
        .window_closed(&state.owner("port-forwarding"));
    run.wait().unwrap();
    assert!(TcpListener::bind(&address).is_ok());

    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut conflict = rule.clone();
    conflict.settings.listen_port = Some(occupied.local_addr().unwrap().port());
    let run = start(&state, &conflict);
    run.wait().unwrap();
    assert_eq!(
        run.view.lock().unwrap().error.as_ref().unwrap().kind,
        "listen"
    );
    assert!(run.view.lock().unwrap().actual_address.is_none());
    drop(occupied);
    let mut ipv6 = rule.clone();
    ipv6.settings.listen_address = "::1".into();
    let run = start(&state, &ipv6);
    until(|| run.view.lock().unwrap().actual_address.is_some() || run.done.load(Ordering::SeqCst));
    assert!(run
        .view
        .lock()
        .unwrap()
        .actual_address
        .as_deref()
        .unwrap()
        .starts_with("[::1]:"));
    run.stop();
    run.wait().unwrap();
}

#[test]
fn forward_invalid_listener_and_unknown_schema_do_not_mutate_files() {
    let mut invalid = rule("server");
    invalid.settings.listen_address = "0.0.0.0".into();
    assert!(invalid.validate().is_err());
    invalid.settings.listen_address = "::1".into();
    invalid.settings.target_host = "[2001:db8::1]".into();
    invalid.validate().unwrap();
    assert_eq!(invalid.settings.target_host, "2001:db8::1");
    invalid.settings.target_host = "http://internal/path".into();
    assert!(invalid.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("port-forwards.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE future_data(value TEXT); INSERT INTO future_data VALUES ('preserved'); PRAGMA user_version=19;").unwrap();
    drop(db);
    assert!(store::Store::open(&path).is_err());
    let db = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM future_data", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserved"
    );
}

#[test]
fn forward_observer_requires_ack_and_expires_with_its_window_generation() {
    let state = crate::connection::tests::state();
    let frames = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let output = frames.clone();
    let channel = tauri::ipc::Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body {
            output.lock().unwrap().push(serde_json::from_str(&json)?);
        }
        Ok(())
    });
    let owner = state.owner("port-forwarding");
    let id = state
        .port_forwarding
        .subscribe(state.clone(), owner.clone(), channel)
        .unwrap();
    until(|| frames.lock().unwrap().len() == 1);
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        frames.lock().unwrap().len(),
        1,
        "unacknowledged observer cannot queue frames"
    );
    let wrong = WindowOwner {
        label: owner.label.clone(),
        generation: owner.generation + 1,
    };
    assert!(state.port_forwarding.ack(&id, 1, &wrong).is_err());
    assert!(state.port_forwarding.ack(&id, 2, &owner).is_err());
    state.port_forwarding.ack(&id, 1, &owner).unwrap();
    until(|| frames.lock().unwrap().len() == 2);
    state.port_forwarding.window_closed(&owner);
    assert!(state.port_forwarding.ack(&id, 2, &owner).is_err());
}

#[test]
#[ignore = "isolated OpenSSH containers; tests/port-forwarding/environment.py"]
fn forward_openssh_restricted_accounts() {
    assert_eq!(std::env::var("SSH_FORWARD_OPENSSH").as_deref(), Ok("1"));
    let state = crate::connection::tests::state();
    let _stop = Stop(state.clone());
    for side in ["a", "b"] {
        let server = ServerView {
            id: side.into(),
            name: side.into(),
            host: format!("forward-{side}"),
            port: 22,
            username: "ops".into(),
            tags: vec![],
            auth_type: "password".into(),
            private_key: String::new(),
            has_saved_password: true,
            jump_server_id: None,
            direct_host: None,
            direct_port: 22,
        };
        state
            .database
            .lock()
            .unwrap()
            .save(SaveServerRequest {
                server,
                password_change: PasswordChange::Set {
                    value: "fixture-password".into(),
                },
            })
            .unwrap();
    }
    fn ready(run: &Run) {
        until(|| {
            let view = run.view.lock().unwrap().clone();
            assert!(
                !run.done.load(Ordering::SeqCst),
                "OpenSSH route failed: {:?}",
                view.error
            );
            if let Some(Prompt {
                event: ConnectionEvent::HostKeyRequired { challenge },
                ..
            }) = view.prompt
            {
                let _ = run
                    .interaction
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .1
                    .answer(&challenge.id, ConnectionAnswer::Trust { accept: true });
            }
            view.phase == "ready"
        });
    }
    let direct_rule = state
        .port_forwarding
        .with_store(|s| s.save(rule("b")))
        .unwrap();
    let direct = start(&state, &direct_rule);
    ready(&direct);
    let context = direct.context.lock().unwrap().clone().unwrap();
    let extra = connection::connect_verified(
        &context,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::PortForward,
    )
    .unwrap();
    assert!(
        extra.session().channel_session().is_err(),
        "fixture must reject shell and SFTP channels"
    );
    drop(extra);
    let direct_address = direct.view.lock().unwrap().actual_address.clone().unwrap();
    echo(&direct_address);
    let mut server = state.database.lock().unwrap().get("b").unwrap().view();
    server.jump_server_id = Some("a".into());
    state
        .database
        .lock()
        .unwrap()
        .save(SaveServerRequest {
            server,
            password_change: PasswordChange::Keep,
        })
        .unwrap();
    let jump_rule = state
        .port_forwarding
        .with_store(|s| s.save(rule("b")))
        .unwrap();
    let jump = start(&state, &jump_rule);
    ready(&jump);
    assert_eq!(
        jump.view
            .lock()
            .unwrap()
            .connection
            .as_ref()
            .unwrap()
            .hops
            .len(),
        2
    );
    let address = jump.view.lock().unwrap().actual_address.clone().unwrap();
    let clients: Vec<_> = (0..4)
        .map(|_| {
            let address = address.clone();
            thread::spawn(move || echo(&address))
        })
        .collect();
    for client in clients {
        client.join().unwrap();
    }
    direct.stop();
    direct.wait().unwrap();
    echo(&address); // Rule paths must remain independent.
    let mut denied_rule = rule("b");
    denied_rule.settings.target_host = "forbidden.internal.invalid".into();
    let denied_rule = state
        .port_forwarding
        .with_store(|s| s.save(denied_rule))
        .unwrap();
    let denied = start(&state, &denied_rule);
    ready(&denied);
    denied.control(Control::Probe("probe".into())).unwrap();
    until(|| denied.view.lock().unwrap().target_result.is_some());
    assert_eq!(
        denied.view.lock().unwrap().phase,
        "ready",
        "target denial must not stop SSH route"
    );
    let result = denied
        .view
        .lock()
        .unwrap()
        .target_result
        .as_ref()
        .unwrap()
        .message
        .clone();
    assert!(
        result
            .to_lowercase()
            .contains("administratively prohibited"),
        "{result}"
    );
    echo(&address);
}
