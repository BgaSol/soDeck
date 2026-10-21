use super::*;
use base64::Engine;
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
};

fn server(id: &str) -> ServerConfig {
    ServerConfig {
        id: id.into(),
        name: id.into(),
        host: "127.0.0.1".into(),
        port: 22,
        username: "ops".into(),
        tags: vec![],
        auth_type: "password".into(),
        private_key: String::new(),
        password: "fixture-password".into(),
        jump_server_id: None,
        direct_host: None,
        direct_port: 22,
    }
}
pub(crate) fn state() -> Arc<State> {
    Arc::new(State {
        port_forwarding: Arc::new(crate::port_forwarding::PortForwardManager::default()),
        traffic: crate::traffic::TrafficManager::new(),
        database: Mutex::new(crate::db::Database::open(Path::new(":memory:")).unwrap()),
        configuration: Mutex::new(()),
        stopping: std::sync::atomic::AtomicBool::new(false),
        moves: Default::default(),
        files: Default::default(),
        deletes: Default::default(),
        file_tasks: Default::default(),
        transfers: Default::default(),
        drags: Default::default(),
        logs: Default::default(),
        history: Default::default(),
        monitor: Default::default(),
        docker: Default::default(),
        netbird: Default::default(),
        terminals: Mutex::new(HashMap::new()),
        pending_windows: Mutex::new(HashMap::new()),
        container_windows: Mutex::new(HashMap::new()),
        closed_windows: Mutex::new(HashSet::new()),
        window_generations: Mutex::new(HashMap::new()),
    })
}
fn collector() -> (Channel<ConnectionEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let values = Arc::new(Mutex::new(Vec::new()));
    let received = values.clone();
    (
        Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                received.lock().unwrap().push(serde_json::from_str(&json)?);
            }
            Ok(())
        }),
        values,
    )
}
fn wait_for(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !check() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn route_validation_identity_and_secret_free_view() {
    assert!(ConnectionPlan::new(vec![]).is_err());
    assert!(ConnectionPlan::new(vec![server("a"), server("b"), server("c")]).is_err());
    assert!(ConnectionPlan::new(vec![server("a"), server("b")]).is_err());
    let mut b = server("b");
    b.jump_server_id = Some("a".into());
    assert!(ConnectionPlan::new(vec![server("a"), b.clone()]).is_ok());
    let mut hops = vec![VerifiedHop {
        server: server("a"),
        fingerprint: "key-a".into(),
        public_key: "ssh-ed25519 AAAA".into(),
        credentials: Credentials::default(),
    }];
    let original = route_identity(&hops);
    hops[0].server.name = "renamed".into();
    hops[0].server.password = "other-secret".into();
    assert_eq!(original, route_identity(&hops));
    hops[0].server.host = "another-network".into();
    assert_ne!(original, route_identity(&hops));
    let context = ConnectionContext {
        traffic: crate::traffic::TrafficManager::new(),
        identity: original,
        hops,
    };
    let view = serde_json::to_string(&context.view()).unwrap();
    assert!(!view.contains("other-secret"));
    assert!(!view.contains("credentials"));
    assert!(context.uses_server("a"));
    assert!(!context.uses_server("b"));
}

#[test]
fn challenge_replies_are_typed_bound_and_single_use() {
    let interaction = Interaction::default();
    let (sender, receiver) = mpsc::sync_channel(1);
    *interaction.pending.lock().unwrap() = Some(PendingChallenge {
        id: "current".into(),
        kind: ChallengeKind::Trust,
        sender,
    });
    assert!(interaction
        .answer("stale", ConnectionAnswer::Trust { accept: true })
        .is_err());
    assert!(interaction
        .answer(
            "current",
            ConnectionAnswer::Credentials {
                password: "x".into(),
                passphrase: String::new()
            }
        )
        .is_err());
    interaction
        .answer("current", ConnectionAnswer::Trust { accept: true })
        .unwrap();
    assert!(matches!(
        receiver.recv().unwrap(),
        ConnectionAnswer::Trust { accept: true }
    ));
    assert!(interaction
        .answer("current", ConnectionAnswer::Trust { accept: true })
        .is_err());
}

#[test]
fn cancelled_or_expired_owner_never_registers_ready_terminal() {
    let state = state();
    state
        .window_generations
        .lock()
        .unwrap()
        .insert("main".into(), 1);
    let plan = Arc::new(ConnectionPlan::new(vec![server("a")]).unwrap());
    let (output, _) = collector();
    assert!(register(
        &state,
        WindowOwner {
            label: "main".into(),
            generation: 0
        },
        plan.clone(),
        output
    )
    .is_err());
    let (output, events) = collector();
    let (id, _, cancel, _, _) = register(
        &state,
        WindowOwner {
            label: "main".into(),
            generation: 1,
        },
        plan,
        output,
    )
    .unwrap();
    cancel.cancel();
    finish(
        &state,
        &id,
        &cancel,
        Err(AppError::new("network", "late failure")),
    );
    finish(&state, &id, &cancel, Ok(()));
    assert_eq!(events.lock().unwrap().len(), 1);
    assert_eq!(events.lock().unwrap()[0]["reason"], "cancelled");
    assert!(state.terminals.lock().unwrap().is_empty());
    state
        .stopping
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (output, _) = collector();
    assert!(register(
        &state,
        WindowOwner {
            label: "main".into(),
            generation: 1
        },
        Arc::new(ConnectionPlan::new(vec![server("a")]).unwrap()),
        output
    )
    .is_err());
}

#[test]
fn cancellation_interrupts_a_stalled_ssh_handshake() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut target = server("stalled");
    target.port = listener.local_addr().unwrap().port();
    let cancel = Cancellation::default();
    let worker_cancel = cancel.clone();
    let worker = thread::spawn(move || {
        connect(
            &ConnectionPlan::new(vec![target]).unwrap(),
            &mut AutoProvider::default(),
            &worker_cancel,
        )
        .err()
        .unwrap()
    });
    let (_socket, _) = listener.accept().unwrap();
    let start = Instant::now();
    cancel.cancel();
    assert_eq!(worker.join().unwrap().kind, "cancelled");
    assert!(start.elapsed() < Duration::from_secs(2));
}

struct Fixtures {
    root: tempfile::TempDir,
    children: Vec<Child>,
    a: ServerConfig,
    b: ServerConfig,
    key: PathBuf,
    passphrase: String,
}
impl Fixtures {
    fn new(encrypted: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let python = std::env::var_os("SSH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join(".qa/venv/bin/python"));
        let key = root.path().join("client-key");
        let public = root.path().join("client-key.pub");
        let passphrase = if encrypted { "test-key-passphrase" } else { "" }.to_string();
        let status = Command::new(&python).arg("-c").arg("import paramiko,sys; k=paramiko.RSAKey.generate(2048); k.write_private_key_file(sys.argv[1],password=sys.argv[3] or None); open(sys.argv[2],'w').write(k.get_name()+' '+k.get_base64())")
            .args([key.as_os_str(), public.as_os_str(), std::ffi::OsStr::new(&passphrase)]).status().expect("install tests/requirements.txt into .qa/venv or set SSH_TEST_PYTHON");
        assert!(status.success());
        let ports: Vec<_> = (0..2)
            .map(|_| {
                let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
                listener.local_addr().unwrap().port()
            })
            .collect();
        let mut result = Self {
            root,
            children: Vec::new(),
            a: server("a"),
            b: server("b"),
            key,
            passphrase,
        };
        result.a.port = ports[0];
        result.a.username = "jump-user".into();
        result.a.password = "jump-password".into();
        result.b.port = ports[1];
        result.b.username = "target-user".into();
        result.b.password = "target-password".into();
        for (name, script, config) in [
            ("b", "ssh_fixture.py", &result.b),
            ("a", "jump_fixture.py", &result.a),
        ] {
            let directory = result.root.path().join(name);
            std::fs::create_dir_all(&directory).unwrap();
            let log = std::fs::File::create(directory.join("process.log")).unwrap();
            let mut child = Command::new(&python)
                .arg(repo.join("tests").join(script))
                .env("SSH_WORKSPACE_FIXTURE_DIR", &directory)
                .env("SSH_WORKSPACE_FIXTURE_PORT", config.port.to_string())
                .env("SSH_WORKSPACE_FIXTURE_USER", &config.username)
                .env("SSH_WORKSPACE_FIXTURE_PASSWORD", &config.password)
                .env("SSH_WORKSPACE_JUMP_TARGET_PORT", result.b.port.to_string())
                .env("SSH_WORKSPACE_FIXTURE_PUBLIC_KEY", &public)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if TcpStream::connect(("127.0.0.1", config.port)).is_ok() {
                    break;
                }
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "fixture exited: {}",
                    std::fs::read_to_string(directory.join("process.log")).unwrap()
                );
                assert!(Instant::now() < deadline, "fixture startup timed out");
                thread::sleep(Duration::from_millis(20));
            }
            result.children.push(child);
        }
        result
    }
    fn plan(&self) -> ConnectionPlan {
        let mut b = self.b.clone();
        b.host = "target.internal.invalid".into();
        b.port = 22;
        b.jump_server_id = Some("a".into());
        ConnectionPlan::new(vec![self.a.clone(), b]).unwrap()
    }
    fn events(&self, name: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.path().join(name).join("ssh-events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[derive(Default)]
struct AutoProvider {
    keys: Vec<String>,
    passphrase: String,
}
impl AuthProvider for AutoProvider {
    fn progress(&mut self, _: &HopView, _: &str) {}
    fn verify(
        &mut self,
        _: &HopView,
        _: &str,
        fingerprint: &str,
        _: &ssh2::Session,
        _: &Cancellation,
    ) -> AppResult<()> {
        self.keys.push(fingerprint.into());
        Ok(())
    }
    fn credentials(
        &mut self,
        _: &HopView,
        server: &ServerConfig,
        _: Option<&AppError>,
        _: &ssh2::Session,
        _: &Cancellation,
    ) -> AppResult<Credentials> {
        Ok(Credentials {
            password: server.password.clone(),
            passphrase: self.passphrase.clone(),
        })
    }
    fn authenticated(&mut self, _: &ServerConfig, _: &Credentials) -> AppResult<()> {
        Ok(())
    }
}

#[test]
#[ignore = "real SSH fixtures; requires Python paramiko; run explicitly"]
fn jump_real_ssh_sftp_docker_terminal_and_failure_paths() {
    let fixtures = Fixtures::new(true);
    let (direct, direct_context) = connect(
        &ConnectionPlan::new(vec![fixtures.b.clone()]).unwrap(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(direct_context.hops.len(), 1);
    assert!(direct
        .session()
        .sftp()
        .unwrap()
        .stat(Path::new("/readme.txt"))
        .is_ok());
    drop(direct);
    let plan = fixtures.plan();
    let cancel = Cancellation::default();
    let mut provider = AutoProvider::default();
    let (connection, context) = connect(&plan, &mut provider, &cancel).unwrap();
    assert_eq!(provider.keys.len(), 2);
    assert_ne!(provider.keys[0], provider.keys[1]);
    assert_eq!(context.target().host, "target.internal.invalid");
    assert!(context.uses_server("a") && context.uses_server("b"));
    let sftp = connection.session().sftp().unwrap();
    assert_eq!(sftp.realpath(Path::new(".")).unwrap(), Path::new("/"));
    // Exercise both bridge directions beyond the SSH channel window, then compare every byte.
    let bytes: Vec<u8> = (0..(5 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
    let mut file = sftp.create(Path::new("/roundtrip.bin")).unwrap();
    file.write_all(&bytes).unwrap();
    file.close().unwrap();
    let mut returned = Vec::new();
    sftp.open(Path::new("/roundtrip.bin"))
        .unwrap()
        .read_to_end(&mut returned)
        .unwrap();
    assert_eq!(returned, bytes);
    drop(sftp);
    let docker = crate::docker::detect(&context).unwrap();
    assert_eq!(docker.state, "running");
    assert_eq!(
        crate::docker::images(&context, &docker.endpoint)
            .unwrap()
            .len(),
        1
    );
    let containers = crate::docker::containers(&context, &docker.endpoint).unwrap();
    let target = crate::docker::ContainerTarget {
        id: containers[0].id.clone(),
        name: containers[0].name.clone(),
        endpoint: docker.endpoint,
    };
    let command = crate::docker::terminal_command_on(connection.session(), &target).unwrap();
    assert!(command.ends_with("/bin/sh"));
    let logs = ssh::exec(
        connection.session(),
        &format!("docker logs --tail all --timestamps {}", target.id),
    )
    .unwrap();
    assert!(logs.stdout.contains("fixture history stdout"));
    assert!(logs.stderr.contains("fixture history stderr"));

    // A real terminal worker uses the same path, emits CWD, and archives independently of view transfer.
    let history_dir = tempfile::tempdir().unwrap();
    let mut state = state();
    Arc::get_mut(&mut state).unwrap().history =
        crate::terminal_history::HistoryState::open(history_dir.path().into());
    let (output, events) = collector();
    let (id, receiver, terminal_cancel, _, _) = register(
        &state,
        WindowOwner {
            label: "main".into(),
            generation: 0,
        },
        Arc::new(fixtures.plan()),
        output,
    )
    .unwrap();
    let terminal_connection = connect_verified(
        &context,
        &terminal_cancel,
        crate::traffic::TrafficPurpose::Shared,
    )
    .unwrap();
    let worker_state = state.clone();
    let worker_context = context.clone();
    let worker_id = id.clone();
    let worker_cancel = terminal_cancel.clone();
    let history_source = crate::terminal_history::model::Source::new(&context, None);
    let worker = thread::spawn(move || {
        let result = terminal::run(
            terminal_connection,
            worker_context,
            &worker_id,
            None,
            &worker_state,
            receiver,
            &worker_cancel,
            Some(("test-workspace".into(), history_source)),
        );
        finish(&worker_state, &worker_id, &worker_cancel, result);
    });
    wait_for(|| events.lock().unwrap().iter().any(|e| e["type"] == "ready"));
    state.terminals.lock().unwrap()[&id]
        .sender
        .send(Input::Data(b"cd /projects\r".to_vec()))
        .unwrap();
    wait_for(|| {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "cwd" && e["path"] == "/projects")
    });
    let (destination, destination_events) = collector();
    {
        let handles = state.terminals.lock().unwrap();
        let mut stream = handles[&id].stream.lock().unwrap();
        stream.freeze("transfer".into()).unwrap();
        stream.attach(destination).unwrap();
        stream.commit().unwrap();
    }
    assert!(destination_events
        .lock()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "ready"));
    state.terminals.lock().unwrap()[&id]
        .sender
        .send(Input::Data(b"echo moved-marker\r".to_vec()))
        .unwrap();
    wait_for(|| {
        destination_events.lock().unwrap().iter().any(|e| {
            e["type"] == "data"
                && base64::engine::general_purpose::STANDARD
                    .decode(e["data"].as_str().unwrap())
                    .is_ok_and(|b| String::from_utf8_lossy(&b).contains("moved-marker"))
        })
    });
    state.terminals.lock().unwrap()[&id]
        .sender
        .send(Input::Close)
        .unwrap();
    worker.join().unwrap();
    assert!(state.terminals.lock().unwrap().is_empty());
    state.history.shutdown().unwrap();
    let archived =
        crate::terminal_history::store::read_events(history_dir.path(), &id, 0, 10000).unwrap();
    let archived_bytes = archived
        .iter()
        .flat_map(|e| match &e.kind {
            crate::terminal_history::model::EventKind::Output { bytes } => bytes.clone(),
            _ => vec![],
        })
        .collect::<Vec<_>>();
    let delivered = events
        .lock()
        .unwrap()
        .iter()
        .chain(destination_events.lock().unwrap().iter())
        .filter(|e| e["type"] == "data")
        .flat_map(|e| {
            base64::engine::general_purpose::STANDARD
                .decode(e["data"].as_str().unwrap())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        archived_bytes, delivered,
        "migration must neither omit nor duplicate archived PTY bytes"
    );
    assert_eq!(state.history.status(&id).unwrap().state, "closed");

    // Container exec uses an independently identified archive through the same terminal loop.
    let (container_output, container_events) = collector();
    let container_id = open_container(
        state.clone(),
        WindowOwner {
            label: "container-test".into(),
            generation: 0,
        },
        context.clone(),
        target.clone(),
        container_output,
        None,
    )
    .unwrap();
    wait_for(|| {
        container_events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "data")
    });
    state.terminals.lock().unwrap()[&container_id]
        .sender
        .send(Input::Close)
        .unwrap();
    wait_for(|| !state.terminals.lock().unwrap().contains_key(&container_id));
    state.history.shutdown().unwrap();
    assert_eq!(
        state
            .history
            .status(&container_id)
            .unwrap()
            .source
            .container_id,
        Some(target.id.clone())
    );
    assert_ne!(
        state.history.status(&container_id).unwrap().source.id,
        state.history.status(&id).unwrap().source.id
    );
    // The independently owned SFTP/exec connection remains valid after closing the terminal.
    assert!(connection
        .session()
        .sftp()
        .unwrap()
        .stat(Path::new("/roundtrip.bin"))
        .is_ok());
    drop(connection);

    let mut wrong_key = ConnectionContext {
        traffic: crate::traffic::TrafficManager::new(),
        identity: context.identity.clone(),
        hops: context
            .hops
            .iter()
            .map(|h| VerifiedHop {
                server: h.server.clone(),
                credentials: h.credentials.clone(),
                fingerprint: h.fingerprint.clone(),
                public_key: h.public_key.clone(),
            })
            .collect(),
    };
    wrong_key.hops[1].fingerprint = "wrong-key".into();
    let error = connect_verified(
        &wrong_key,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Shared,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, "hostKey");
    assert_eq!(error.hop, Some(1));
    wrong_key.hops[1]
        .fingerprint
        .clone_from(&context.hops[1].fingerprint);
    wrong_key.hops[0].fingerprint = "changed-jump-key".into();
    let error = connect_verified(
        &wrong_key,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Shared,
    )
    .err()
    .unwrap();
    assert_eq!((error.kind.as_str(), error.hop), ("hostKey", Some(0)));
    let mut wrong_password = fixtures.plan();
    wrong_password.hops[0].password = "wrong".into();
    let error = connect(
        &wrong_password,
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, "authentication");
    assert_eq!(error.hop, Some(0));
    let mut wrong_target_password = fixtures.plan();
    wrong_target_password.hops[1].password = "wrong-target-password".into();
    let error = connect(
        &wrong_target_password,
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .err()
    .unwrap();
    assert_eq!(
        (error.kind.as_str(), error.hop),
        ("authentication", Some(1))
    );
    std::fs::write(fixtures.root.path().join("a/deny-forward"), "deny").unwrap();
    assert_eq!(
        connect_verified(
            &context,
            &Cancellation::default(),
            crate::traffic::TrafficPurpose::Shared
        )
        .err()
        .unwrap()
        .kind,
        "forwarding"
    );
    std::fs::remove_file(fixtures.root.path().join("a/deny-forward")).unwrap();
    // Local DNS cannot resolve B: this attempted direct route fails before authentication.
    let mut direct_b = plan.hops[1].clone();
    direct_b.jump_server_id = None;
    assert!(connect(
        &ConnectionPlan::new(vec![direct_b]).unwrap(),
        &mut AutoProvider::default(),
        &Cancellation::default()
    )
    .is_err());
    assert!(fixtures
        .events("a")
        .iter()
        .any(|e| e["event"] == "forward-request"
            && e["host"] == "target.internal.invalid"
            && e["port"] == 22));
    assert!(fixtures
        .events("b")
        .iter()
        .any(|e| e["event"] == "authentication" && e["username"] == "target-user"));
    wait_for(|| {
        let events = fixtures.events("a");
        events.iter().filter(|e| e["event"] == "jump-open").count()
            == events.iter().filter(|e| e["event"] == "jump-close").count()
    });
}

#[test]
#[ignore = "real SSH fixtures; requires Python paramiko; run explicitly"]
fn jump_background_downloads_and_log_archive() {
    let fixtures = Fixtures::new(false);
    let (connection, context) = connect(
        &fixtures.plan(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    let state = state();
    // The verification connection is gone; background jobs must own their paths.
    drop(connection);
    crate::transfers::download::tests::exercise_jump_downloads(
        state,
        context.clone(),
        &fixtures.root.path().join("b"),
    );
    crate::logs::tests::exercise_jump_archive(&context, fixtures.root.path());
    wait_for(|| {
        let events = fixtures.events("a");
        events.iter().filter(|e| e["event"] == "jump-open").count()
            == events.iter().filter(|e| e["event"] == "jump-close").count()
    });
}

#[test]
#[ignore = "real SSH fixtures; requires Python paramiko; run explicitly"]
fn jump_private_keys_cancellation_and_jump_disconnect() {
    let fixtures = Fixtures::new(true);
    let mut plan = fixtures.plan();
    for hop in &mut plan.hops {
        hop.auth_type = "key".into();
        hop.private_key = fixtures.key.to_string_lossy().into_owned();
    }
    let cancel = Cancellation::default();
    let (connection, context) = connect(
        &plan,
        &mut AutoProvider {
            passphrase: fixtures.passphrase.clone(),
            ..Default::default()
        },
        &cancel,
    )
    .unwrap();
    let began = Instant::now();
    cancel.cancel();
    drop(connection);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(
        connect_verified(&context, &cancel, crate::traffic::TrafficPurpose::Shared)
            .err()
            .unwrap()
            .kind,
        "cancelled"
    );
    let active = connect_verified(
        &context,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Shared,
    )
    .unwrap();
    std::fs::write(fixtures.root.path().join("a/disconnect"), "disconnect").unwrap();
    wait_for(|| active.transport_error().is_some());
    assert!(active.session().sftp().is_err());
    drop(active);
}

#[test]
#[ignore = "real SSH fixtures; requires Python paramiko; run explicitly"]
fn jump_interactive_prompts_and_cancel_at_second_host() {
    let fixtures = Fixtures::new(false);
    let state = state();
    let mut plan = fixtures.plan();
    for hop in &mut plan.hops {
        hop.password.clear();
        state
            .database
            .lock()
            .unwrap()
            .save(crate::model::SaveServerRequest {
                server: hop.view(),
                password_change: crate::model::PasswordChange::Clear,
            })
            .unwrap();
    }
    let plan = Arc::new(plan);
    let (output, events) = collector();
    let (id, _, cancel, stream, interaction) = register(
        &state,
        WindowOwner {
            label: "main".into(),
            generation: 0,
        },
        plan.clone(),
        output,
    )
    .unwrap();
    let worker_state = state.clone();
    let worker_id = id.clone();
    let worker_cancel = cancel.clone();
    let responder = interaction.clone();
    let worker = thread::spawn(move || {
        let result = connect(
            &plan,
            &mut InteractiveProvider {
                state: &worker_state,
                events: Arc::new(TerminalConnectSink(stream)),
                interaction,
                pinned: None,
            },
            &worker_cancel,
        );
        finish(
            &worker_state,
            &worker_id,
            &worker_cancel,
            result.map(|_| ()),
        );
    });
    let mut handled = HashSet::new();
    let mut stages = Vec::new();
    loop {
        let challenge = {
            events
                .lock()
                .unwrap()
                .iter()
                .find(|e| {
                    e["challenge"]["id"]
                        .as_str()
                        .is_some_and(|id| !handled.contains(id))
                })
                .cloned()
        };
        if let Some(event) = challenge {
            let challenge = &event["challenge"];
            let challenge_id = challenge["id"].as_str().unwrap().to_string();
            handled.insert(challenge_id.clone());
            let hop = challenge["hop"]["index"].as_u64().unwrap();
            stages.push((event["type"].as_str().unwrap().to_owned(), hop));
            if hop == 1 {
                // Cancelling while B's fingerprint is displayed closes B's
                // transport bridge and A, without sending credentials to B.
                cancel.cancel();
                break;
            }
            let answer = if event["type"] == "hostKeyRequired" {
                ConnectionAnswer::Trust { accept: true }
            } else {
                ConnectionAnswer::Credentials {
                    password: "jump-password".into(),
                    passphrase: String::new(),
                }
            };
            responder.answer(&challenge_id, answer).unwrap();
        } else {
            assert!(
                !worker.is_finished(),
                "connection ended before prompts: {:?}",
                events.lock().unwrap()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    let began = Instant::now();
    worker.join().unwrap();
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(
        stages,
        vec![
            ("hostKeyRequired".into(), 0),
            ("authRequired".into(), 0),
            ("hostKeyRequired".into(), 1)
        ]
    );
    assert!(!events.lock().unwrap().iter().any(|e| e["type"] == "ready"));
    assert!(!fixtures
        .events("b")
        .iter()
        .any(|e| e["event"] == "authentication"));
    assert_eq!(
        state.database.lock().unwrap().get("a").unwrap().password,
        "jump-password"
    );
    assert!(state
        .database
        .lock()
        .unwrap()
        .get("b")
        .unwrap()
        .password
        .is_empty());
    assert!(state.terminals.lock().unwrap().is_empty());
}

#[test]
#[ignore = "run under an isolated ssh-agent with SSH_WORKSPACE_TEST_AGENT=1"]
fn jump_agent_authentication() {
    assert_eq!(
        std::env::var("SSH_WORKSPACE_TEST_AGENT").as_deref(),
        Ok("1"),
        "use an isolated test ssh-agent"
    );
    let fixtures = Fixtures::new(false);
    assert!(Command::new("ssh-add")
        .arg(&fixtures.key)
        .status()
        .unwrap()
        .success());
    let mut plan = fixtures.plan();
    for hop in &mut plan.hops {
        hop.auth_type = "agent".into();
    }
    let (connection, context) = connect(
        &plan,
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(connection
        .session()
        .sftp()
        .unwrap()
        .stat(Path::new("/readme.txt"))
        .is_ok());
    assert_eq!(context.hops.len(), 2);
    assert!(Command::new("ssh-add")
        .arg("-d")
        .arg(&fixtures.key)
        .status()
        .unwrap()
        .success());
}

// Integration-only construction uses a fixture's key once, then every worker
// connection follows the same pinned-key path as production.
pub(crate) fn transfer_context(id: &str, port: u16) -> SharedContext {
    let mut server = server(id);
    if std::env::var_os("SSH_RSYNC_CONTAINER").is_some() {
        server.host = format!("rsync-{id}");
        server.port = 22;
    } else {
        server.port = port;
    }
    connect(
        &ConnectionPlan::new(vec![server]).unwrap(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap()
    .1
}

// No network connection: only for testing immutable draft identity snapshots.
pub(crate) fn mock_context(server: ServerConfig) -> SharedContext {
    let hops = vec![VerifiedHop {
        server,
        credentials: Credentials::default(),
        fingerprint: "fixture".into(),
        public_key: "ssh-ed25519 fixture".into(),
    }];
    Arc::new(ConnectionContext {
        traffic: crate::traffic::TrafficManager::new(),
        identity: route_identity(&hops),
        hops,
    })
}

#[test]
#[ignore = "real SSH jump fixture; requires Python paramiko; run explicitly"]
fn jump_resource_monitor_shares_samples_and_isolates_slow_mounts() {
    let fixtures = Fixtures::new(false);
    let (connection, context) = connect(
        &fixtures.plan(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    drop(connection);
    let root = fixtures.root.path().join("b");
    std::fs::create_dir_all(root.join("remote/proc/self")).unwrap();
    std::fs::write(
        root.join("remote/proc/self/mountinfo"),
        "20 1 8:1 / / rw - ext4 /dev/a rw\n21 20 0:9 / /slow rw - nfs nas:/slow rw\n",
    )
    .unwrap();
    std::fs::write(root.join("monitor-slow-mount"), "slow").unwrap();
    let state = state();
    let mut sessions = Vec::new();
    for window in ["main", "workspace-test"] {
        let (id, _, _, _, _) = register(
            &state,
            state.owner(window),
            Arc::new(fixtures.plan()),
            Channel::new(|_| Ok(())),
        )
        .unwrap();
        state.terminals.lock().unwrap().get_mut(&id).unwrap().spec = Some(context.clone());
        sessions.push(id);
    }
    crate::monitor::tests::exercise(state, context, &sessions[0], &sessions[1], &root);
}

#[test]
#[ignore = "real SSH/SFTP fixture; requires Python paramiko; run explicitly"]
fn jump_remote_delete_protocol_and_failures() {
    let fixtures = Fixtures::new(false);
    let (connection, context) = connect(
        &fixtures.plan(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    drop(connection);
    crate::remote_delete::tests::exercise_protocol(context, &fixtures.root.path().join("b"));
}

fn connect(
    plan: &ConnectionPlan,
    provider: &mut impl AuthProvider,
    cancel: &Cancellation,
) -> AppResult<(SshConnection, SharedContext)> {
    connect_metered(
        plan,
        provider,
        cancel,
        &crate::traffic::TrafficManager::new(),
        crate::traffic::TrafficPurpose::Terminal,
    )
}

#[test]
#[ignore = "real SSH/SFTP byte accounting; requires Python paramiko"]
fn ssh_traffic_matches_server_bytes_for_direct_and_jump_connections() {
    for jump in [false, true] {
        let fixtures = Fixtures::new(false);
        let plan = if jump {
            fixtures.plan()
        } else {
            ConnectionPlan::new(vec![fixtures.b.clone()]).unwrap()
        };
        let manager = crate::traffic::TrafficManager::new();
        let (connection, context) = connect_metered(
            &plan,
            &mut AutoProvider::default(),
            &Cancellation::default(),
            &manager,
            crate::traffic::TrafficPurpose::Files,
        )
        .unwrap();
        let account = manager.account(context.identity());
        let sftp = connection.session().sftp().unwrap();
        let bytes: Vec<u8> = (0..(1024 * 1024)).map(|i| (i % 251) as u8).collect();
        let mut file = sftp.create(Path::new("/meter.bin")).unwrap();
        file.write_all(&bytes).unwrap();
        file.close().unwrap();
        drop(file);
        let mut actual = Vec::new();
        let mut file = sftp.open(Path::new("/meter.bin")).unwrap();
        file.read_to_end(&mut actual).unwrap();
        file.close().unwrap();
        drop(file);
        assert_eq!(actual, bytes);
        drop(sftp);
        assert_eq!(
            ssh::exec(connection.session(), "command -v docker >/dev/null 2>&1")
                .unwrap()
                .code,
            0
        );
        connection
            .session()
            .disconnect(None, "meter test complete", None)
            .unwrap();
        let root = fixtures.root.path().join("b");
        let events = || -> Vec<serde_json::Value> {
            std::fs::read_to_string(root.join("ssh-events.jsonl"))
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        };
        wait_for(|| {
            events()
                .iter()
                .any(|e| e["event"] == "connection-close" && e["username"] == "target-user")
        });
        drop(connection);
        std::thread::sleep(Duration::from_millis(5));
        manager.flush();
        let sample = account.snapshot();
        let closed = events()
            .into_iter()
            .find(|e| e["event"] == "connection-close" && e["username"] == "target-user")
            .unwrap();
        assert_eq!(
            sample.total.sent_bytes.parse::<u64>().unwrap(),
            closed["received_bytes"].as_u64().unwrap(),
            "target server receive bytes, jump={jump}"
        );
        assert_eq!(
            sample.total.received_bytes.parse::<u64>().unwrap(),
            closed["sent_bytes"].as_u64().unwrap(),
            "target server send bytes, jump={jump}"
        );
        assert_eq!(sample.total.sent_bytes, sample.files.sent_bytes);
        assert_eq!(sample.monitoring.sent_bytes, "0");
        assert_eq!(sample.active_connections, 0);
        assert!(sample.total.sent_bytes.parse::<u64>().unwrap() > bytes.len() as u64);
    }
}

#[test]
#[ignore = "real SSH Docker/traffic sharing; requires Python paramiko"]
fn jump_docker_monitor_and_background_traffic_are_shared() {
    let fixtures = Fixtures::new(false);
    let (connection, context) = connect(
        &fixtures.plan(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    drop(connection);
    let state = state();
    let mut sessions = Vec::new();
    for window in ["main", "workspace-test"] {
        let (id, _, _, _, _) = register(
            &state,
            state.owner(window),
            Arc::new(fixtures.plan()),
            Channel::new(|_| Ok(())),
        )
        .unwrap();
        state.terminals.lock().unwrap().get_mut(&id).unwrap().spec = Some(context.clone());
        sessions.push(id);
    }
    crate::monitor::tests::exercise_docker(
        state,
        context,
        &sessions[0],
        &sessions[1],
        &fixtures.root.path().join("b"),
    );
}

#[test]
#[ignore = "real SSH with stateful Docker simulation; requires Python paramiko"]
fn jump_docker_management_tasks_and_uncertain_results() {
    let fixtures = Fixtures::new(false);
    let root = fixtures.root.path().join("b");
    std::fs::write(root.join("docker-management.json"),serde_json::json!({"a".repeat(64):{"name":"web","state":"running","health":"healthy"},"b".repeat(64):{"name":"worker","state":"exited"}}).to_string()).unwrap();
    let (connection, context) = connect(
        &fixtures.plan(),
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    drop(connection);
    let state = state();
    for server in &fixtures.plan().hops {
        state
            .database
            .lock()
            .unwrap()
            .save(crate::model::SaveServerRequest {
                server: server.view(),
                password_change: crate::model::PasswordChange::Set {
                    value: server.password.clone(),
                },
            })
            .unwrap();
    }
    let mut sessions = Vec::new();
    for window in ["main", "workspace-test"] {
        let (id, _, _, _, _) = register(
            &state,
            state.owner(window),
            Arc::new(fixtures.plan()),
            Channel::new(|_| Ok(())),
        )
        .unwrap();
        state.terminals.lock().unwrap().get_mut(&id).unwrap().spec = Some(context.clone());
        sessions.push(id);
    }
    crate::monitor::tests::exercise_management(state, context, &sessions[0], &sessions[1], &root);
}

#[test]
#[ignore = "real loopback SSH with simulated NetBird; requires Python paramiko"]
fn netbird_client_operations_over_verified_ssh() {
    let fixtures = Fixtures::new(false);
    let plan = ConnectionPlan::new(vec![fixtures.b.clone()]).unwrap();
    let (transport, context) = connect(
        &plan,
        &mut AutoProvider::default(),
        &Cancellation::default(),
    )
    .unwrap();
    drop(transport);
    let state = state();
    state
        .database
        .lock()
        .unwrap()
        .save(crate::model::SaveServerRequest {
            server: fixtures.b.view(),
            password_change: crate::model::PasswordChange::Set {
                value: fixtures.b.password.clone(),
            },
        })
        .unwrap();
    let mut sessions = Vec::new();
    for window in ["main", "workspace-test"] {
        let (id, _, _, _, _) = register(
            &state,
            state.owner(window),
            Arc::new(plan.clone()),
            Channel::new(|_| Ok(())),
        )
        .unwrap();
        state.terminals.lock().unwrap().get_mut(&id).unwrap().spec = Some(context.clone());
        sessions.push(id);
    }
    crate::netbird::tests::exercise(
        state,
        context,
        &sessions[0],
        &sessions[1],
        &fixtures.root.path().join("b"),
    );
}

#[test]
fn closed_events_classify_recovery_and_cancellation_wins() {
    for (kind, recovery) in [
        ("network", "retry"),
        ("timeout", "retry"),
        ("io", "retry"),
        ("authentication", "userAction"),
        ("hostKey", "userAction"),
        ("validation", "none"),
    ] {
        let state = state();
        let (output, events) = collector();
        let (id, _, cancel, _, _) = register(
            &state,
            state.owner("main"),
            Arc::new(ConnectionPlan::new(vec![server("a")]).unwrap()),
            output,
        )
        .unwrap();
        finish(&state, &id, &cancel, Err(AppError::new(kind, "fixture")));
        let events = events.lock().unwrap();
        assert_eq!(events[0]["reason"], "failed");
        assert_eq!(events[0]["recovery"], recovery);
        assert!(state.terminals.lock().unwrap().is_empty());
    }
    for cancelled in [false, true] {
        let state = state();
        let (output, events) = collector();
        let (id, _, cancel, _, _) = register(
            &state,
            state.owner("main"),
            Arc::new(ConnectionPlan::new(vec![server("a")]).unwrap()),
            output,
        )
        .unwrap();
        if cancelled {
            cancel.cancel();
        }
        finish(
            &state,
            &id,
            &cancel,
            if cancelled {
                Err(AppError::new("network", "late"))
            } else {
                Ok(())
            },
        );
        let events = events.lock().unwrap();
        assert_eq!(
            events[0]["reason"],
            if cancelled { "cancelled" } else { "ended" }
        );
        assert_eq!(events[0]["recovery"], "none");
    }
}

#[test]
#[ignore = "real loopback SSH; requires Python paramiko"]
fn recovery_real_ssh_normal_exit_transport_loss_and_reconnect() {
    for through_jump in [false, true] {
        let mut fixtures = Fixtures::new(false);
        let plan = if through_jump {
            fixtures.plan()
        } else {
            ConnectionPlan::new(vec![fixtures.b.clone()]).unwrap()
        };
        // First prove that an ordinary shell exit is not a recoverable failure.
        // Then terminate the loopback SSH server to exercise real socket loss.
        for abrupt in [false, true] {
            let state = state();
            let (output, events) = collector();
            let (id, receiver, cancel, _, _) =
                register(&state, state.owner("main"), Arc::new(plan.clone()), output).unwrap();
            let (connection, context) =
                connect(&plan, &mut AutoProvider::default(), &cancel).unwrap();
            let worker_state = state.clone();
            let worker_id = id.clone();
            let worker_cancel = cancel.clone();
            let worker = thread::spawn(move || {
                let result = terminal::run(
                    connection,
                    context,
                    &worker_id,
                    None,
                    &worker_state,
                    receiver,
                    &worker_cancel,
                    None,
                );
                finish(&worker_state, &worker_id, &worker_cancel, result);
            });
            wait_for(|| {
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|event| event["type"] == "ready")
            });
            if abrupt {
                fixtures.children[0].kill().unwrap();
                fixtures.children[0].wait().unwrap();
            } else {
                state.terminals.lock().unwrap()[&id]
                    .sender
                    .send(Input::Data(b"exit\r".to_vec()))
                    .unwrap();
            }
            wait_for(|| {
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|event| event["type"] == "closed")
            });
            worker.join().unwrap();
            let events = events.lock().unwrap();
            let closed = events
                .iter()
                .find(|event| event["type"] == "closed")
                .unwrap();
            assert_eq!(closed["reason"], if abrupt { "failed" } else { "ended" });
            assert_eq!(closed["recovery"], if abrupt { "retry" } else { "none" });
            assert!(state.terminals.lock().unwrap().is_empty());
        }
        // Bring the same test endpoint back and prove a fresh connection works.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let python = std::env::var_os("SSH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join(".qa/venv/bin/python"));
        let directory = fixtures.root.path().join("b");
        let log = std::fs::File::create(directory.join("restart.log")).unwrap();
        let child = Command::new(python)
            .arg(repo.join("tests/ssh_fixture.py"))
            .env("SSH_WORKSPACE_FIXTURE_DIR", &directory)
            .env("SSH_WORKSPACE_FIXTURE_PORT", fixtures.b.port.to_string())
            .env("SSH_WORKSPACE_FIXTURE_USER", &fixtures.b.username)
            .env("SSH_WORKSPACE_FIXTURE_PASSWORD", &fixtures.b.password)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        fixtures.children.push(child);
        wait_for(|| TcpStream::connect(("127.0.0.1", fixtures.b.port)).is_ok());
        let (connection, _) = connect(
            &plan,
            &mut AutoProvider::default(),
            &Cancellation::default(),
        )
        .unwrap();
        assert!(connection.session().authenticated());
    }
}
