use super::*;
use serde_json::json;
use std::path::Path;
fn fixture_detail(state: &str) -> ContainerDetail {
    inspect::parse(&json!({"Id":"a".repeat(64),"Name":"/web","Image":"app:latest","State":{"Status":state,"ExitCode":137,"OOMKilled":true},"AutoRemove":false,"Mounts":[],"Ports":{"80/tcp":[{"HostIp":"::","HostPort":"8080"}],"53/udp":null}}).to_string(),&"a".repeat(64)).unwrap()
}
#[test]
fn inspect_preserves_unknown_health_and_unpublished_ports() {
    let detail = fixture_detail("exited");
    assert_eq!(detail.health, None);
    assert_eq!(detail.exit_code, Some(137));
    assert!(detail.oom_killed);
    assert_eq!(detail.ports.len(), 2);
    assert!(detail.ports.iter().any(|p| p.host_port.is_none()));
    let serialized = serde_json::to_string(&detail).unwrap();
    assert!(!serialized.contains("Env"));
    assert!(inspect::parse("{}", &"a".repeat(64)).is_err());
    assert!(inspect::parse(
        &json!({"Id":"b".repeat(64),"State":{"Status":"running"}}).to_string(),
        &"a".repeat(64)
    )
    .is_err());
}
#[test]
fn actions_are_checked_against_observed_state() {
    let running = fixture_detail("running");
    assert!(!Action::Start.check(&running).unwrap());
    assert!(Action::Stop.check(&running).unwrap());
    assert!(Action::Remove.check(&running).is_err());
    let stopped = fixture_detail("exited");
    assert!(!Action::Stop.check(&stopped).unwrap());
    assert!(Action::Start.check(&stopped).unwrap());
    assert!(Action::Remove.needs_confirmation(&stopped));
    assert!(Action::Restart.check(&stopped).is_err());
    assert!(Action::Stop.check(&fixture_detail("paused")).is_err());
    let mut auto = running.clone();
    auto.auto_remove = true;
    assert!(Action::Stop.needs_confirmation(&auto));
    assert!(Action::Restart.needs_confirmation(&auto));
    assert_ne!(inspect::fingerprint(&running), inspect::fingerprint(&auto));
    assert!(validate_id("a;touch /tmp/no").is_err());
    assert!(validate_id(&"a".repeat(64)).is_ok());
}
fn wait(message: &str, check: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(25);
    while !check() {
        assert!(Instant::now() < until, "{message}");
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn task(state: &State, id: &str) -> Operation {
    state
        .docker
        .core
        .lock()
        .unwrap()
        .records
        .iter()
        .find(|r| r.view.id == id)
        .unwrap()
        .view
        .clone()
}

pub(crate) fn exercise(
    state: Arc<State>,
    context: SharedContext,
    first: &str,
    second: &str,
    root: &Path,
) {
    let first_owner = state.owner("main");
    let second_owner = state.owner("workspace-test");
    let binding = state.monitor.docker_binding(context.identity()).unwrap();
    let make = |action: Action, id: &str| OperationRequest {
        request_id: uuid(),
        engine_token: binding.token.clone(),
        target_id: id.into(),
        action,
        preview_id: None,
    };
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let values = Arc::new(Mutex::new(Vec::<Frame>::new()));
    let subscribe = |session: &str, owner: WindowOwner| {
        let values = values.clone();
        let weak = Arc::downgrade(&state);
        let ack_owner = owner.clone();
        state
            .docker
            .subscribe(
                &state,
                session.into(),
                owner,
                Channel::new(move |body| {
                    if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                        let f: serde_json::Value = serde_json::from_str(&s)?;
                        // Capture through serde to exercise the real IPC spelling.
                        if let Some(state) = weak.upgrade() {
                            let _ = state.docker.ack(
                                &state,
                                f["subscriptionId"].as_str().unwrap(),
                                &ack_owner,
                                f["revision"].as_u64().unwrap(),
                            );
                        }
                        // Keep a lightweight receipt for the assertions below.
                        values.lock().unwrap().push(Frame {
                            subscription_id: f["subscriptionId"].as_str().unwrap().into(),
                            revision: f["revision"].as_u64().unwrap(),
                            catalog_revision: 0,
                            engine: None,
                            operations: vec![],
                            detail: None,
                            error: None,
                        });
                    }
                    Ok(())
                }),
            )
            .unwrap()
    };
    std::fs::write(root.join("docker-stats-delay"), "2").unwrap();
    let one = subscribe(first, first_owner.clone());
    let two = subscribe(second, second_owner.clone());
    state
        .docker
        .interest(&state, &one, &first_owner, Some(a.clone()))
        .unwrap();
    state
        .docker
        .interest(&state, &two, &second_owner, Some(a.clone()))
        .unwrap();
    wait("both windows receive snapshots", || {
        values.lock().unwrap().len() >= 2
    });
    wait(
        "inspect is visible before the slow stats request completes",
        || {
            state
                .docker
                .core
                .lock()
                .unwrap()
                .details
                .values()
                .any(|cache| {
                    cache.running
                        && !cache.state.loading
                        && !cache.state.stale
                        && cache
                            .state
                            .value
                            .as_ref()
                            .is_some_and(|d| d.id == a && d.stats.is_none())
                })
        },
    );
    std::fs::remove_file(root.join("docker-stats-delay")).unwrap();
    std::fs::write(root.join("docker-action-delay"), "6.5").unwrap();
    let request = make(Action::Stop, &a);
    let id = state
        .docker
        .submit(&state, first, first_owner.clone(), request.clone())
        .unwrap();
    assert_eq!(
        state
            .docker
            .submit(&state, second, second_owner.clone(), request.clone())
            .unwrap(),
        id
    );
    assert_eq!(
        state
            .docker
            .submit(
                &state,
                second,
                second_owner.clone(),
                make(Action::Restart, &a)
            )
            .unwrap_err()
            .kind,
        "busy"
    );
    assert!(state.docker.has_server("a") && state.docker.has_server("b"));
    wait(
        "long stop completes outside six-second query timeout",
        || !task(&state, &id).active(),
    );
    assert_eq!(task(&state, &id).command_result, "succeeded");
    assert_eq!(task(&state, &id).observed_state.as_deref(), Some("exited"));
    std::fs::remove_file(root.join("docker-action-delay")).unwrap();
    // Supplementary actions use the same locks and verified identity.
    for action in [
        Action::Start,
        Action::Pause,
        Action::Unpause,
        Action::Rename {
            name: "renamed-web".into(),
        },
        Action::RestartPolicy {
            policy: "on-failure".into(),
            maximum_retry_count: 3,
        },
        Action::Stop,
    ] {
        let id = state
            .docker
            .submit(&state, first, first_owner.clone(), make(action, &a))
            .unwrap();
        wait("maintenance action finishes", || {
            !task(&state, &id).active()
        });
        assert_eq!(
            task(&state, &id).command_result,
            "succeeded",
            "{}",
            task(&state, &id).message
        );
    }
    let image_id = format!("sha256:{}", "c".repeat(64));
    let image_file = root.join("docker-images.json");
    let original = json!({image_id.clone(): {"Id":image_id,"RepoTags":["app:v1","app:latest"],"RepoDigests":[],"Created":"2026-10-01T00:00:00Z","Os":"linux","Architecture":"amd64","Size":1234}});
    std::fs::write(&image_file, original.to_string()).unwrap();
    let image_action = Action::ImageRemove {
        reference: Some("app:v1".into()),
    };
    let preview = state
        .docker
        .prepare(
            &state,
            first,
            &first_owner,
            &binding.token,
            &image_id,
            image_action.clone(),
        )
        .unwrap();
    assert!(preview.consequences.iter().any(|s| s.contains("2 个容器")));
    // An externally moved tag invalidates the exact reference preview.
    let moved_id = format!("sha256:{}", "e".repeat(64));
    let moved = json!({image_id.clone(): {"Id":image_id,"RepoTags":["app:latest"],"RepoDigests":[],"Created":"2026-10-01T00:00:00Z","Os":"linux","Architecture":"amd64","Size":1234},moved_id.clone(): {"Id":moved_id,"RepoTags":["app:v1"],"RepoDigests":[],"Created":"2026-10-01T00:00:00Z","Os":"linux","Architecture":"amd64","Size":1234}});
    std::fs::write(&image_file, moved.to_string()).unwrap();
    let mut request = make(image_action.clone(), &image_id);
    request.preview_id = Some(preview.id);
    let id = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("moved reference rejected", || !task(&state, &id).active());
    assert_eq!(task(&state, &id).command_result, "notSubmitted");
    assert_eq!(
        std::fs::read_to_string(&image_file).unwrap(),
        moved.to_string()
    );
    std::fs::write(&image_file, original.to_string()).unwrap();
    let preview = state
        .docker
        .prepare(
            &state,
            first,
            &first_owner,
            &binding.token,
            &image_id,
            image_action.clone(),
        )
        .unwrap();
    let mut request = make(image_action, &image_id);
    request.preview_id = Some(preview.id);
    let id = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("remove one tag", || !task(&state, &id).active());
    assert_eq!(task(&state, &id).command_result, "succeeded");
    let remaining: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&image_file).unwrap()).unwrap();
    assert_eq!(remaining[&image_id]["RepoTags"], json!(["app:latest"]));
    // Binary channels preserve bytes while stderr is busy and publish only on
    // a complete receipt; no local archive extraction occurs.
    let output = root.join("image-output.tar");
    let input = super::super::transfer::selection(
        super::super::transfer::Kind::ImageSave,
        image_id.clone(),
        String::new(),
        output.clone(),
        "rename".into(),
    )
    .unwrap();
    let preview = state
        .docker
        .prepare_transfer(&state, first, &first_owner, &binding.token, input)
        .unwrap();
    let mut request = make(preview.action.clone(), &image_id);
    request.preview_id = Some(preview.id);
    let transferred = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("binary save finishes", || {
        !task(&state, &transferred).active()
    });
    assert_eq!(
        task(&state, &transferred).command_result,
        "succeeded",
        "{}",
        task(&state, &transferred).message
    );
    let expected: Vec<u8> = (0..=255).cycle().take(2097152).collect();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    assert_eq!(
        task(&state, &transferred)
            .transfer
            .as_ref()
            .unwrap()
            .bytes_done,
        2097152
    );
    assert!(state
        .docker
        .transfer_snapshot()
        .iter()
        .any(|t| t.id == transferred && t.status == "completed"));
    let input = super::super::transfer::selection(
        super::super::transfer::Kind::ImageLoad,
        "image-import".into(),
        String::new(),
        output,
        "overwrite".into(),
    )
    .unwrap();
    let preview = state
        .docker
        .prepare_transfer(&state, first, &first_owner, &binding.token, input)
        .unwrap();
    let mut request = make(preview.action.clone(), "image-import");
    request.preview_id = Some(preview.id);
    let transferred = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("binary upload finishes", || {
        !task(&state, &transferred).active()
    });
    assert_eq!(
        task(&state, &transferred).command_result,
        "succeeded",
        "{}",
        task(&state, &transferred).message
    );
    assert_eq!(
        std::fs::read(root.join("docker-binary-upload")).unwrap(),
        expected
    );
    std::fs::write(root.join("docker-binary-mode"), "no-receipt").unwrap();
    let partial = root.join("missing-receipt.tar");
    let input = super::super::transfer::selection(
        super::super::transfer::Kind::ImageSave,
        image_id.clone(),
        String::new(),
        partial.clone(),
        "rename".into(),
    )
    .unwrap();
    let preview = state
        .docker
        .prepare_transfer(&state, first, &first_owner, &binding.token, input)
        .unwrap();
    let mut request = make(preview.action.clone(), &image_id);
    request.preview_id = Some(preview.id);
    let transferred = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("incomplete download fails", || {
        !task(&state, &transferred).active()
    });
    assert_eq!(task(&state, &transferred).phase, "failed");
    assert!(!partial.exists());
    std::fs::remove_file(root.join("docker-binary-mode")).unwrap();
    // Private environment files never appear in previews or shared task frames.
    std::fs::create_dir_all(root.join("remote/tmp")).unwrap();
    let input = super::super::create::Spec {
        image_id: image_id.clone(),
        name: "created-by-test".into(),
        ports: vec![],
        environment: vec![super::super::create::Environment {
            key: "PASSWORD".into(),
            value: "secret-canary-value".into(),
        }],
        mounts: vec![],
        network: None,
        restart_policy: "no".into(),
        maximum_retry_count: 0,
        command: vec![],
        working_directory: String::new(),
        start: true,
    };
    let preview = state
        .docker
        .prepare_create(&state, first, &first_owner, &binding.token, input)
        .unwrap();
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("secret-canary-value"));
    let mut request = make(preview.action.clone(), "created-by-test");
    request.preview_id = Some(preview.id);
    let created = state
        .docker
        .submit(&state, first, first_owner.clone(), request)
        .unwrap();
    wait("create and start finish", || {
        !task(&state, &created).active()
    });
    assert_eq!(
        task(&state, &created).command_result,
        "succeeded",
        "{}",
        task(&state, &created).message
    );
    assert!(task(&state, &created).created_container_id.is_some());
    assert!(!serde_json::to_string(&task(&state, &created))
        .unwrap()
        .contains("secret-canary-value"));
    assert_eq!(
        std::fs::read_to_string(root.join("create-env-receipt")).unwrap(),
        "PASSWORD=secret-canary-value\n"
    );
    // Windows filesystems cannot report POSIX 0600. Verify the actual SFTP
    // creation mode on every platform, and the resulting mode on Unix too.
    let events = std::fs::read_to_string(root.join("ssh-events.jsonl")).unwrap();
    let opens: Vec<serde_json::Value> = events
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|event: &serde_json::Value| {
            event["event"] == "sftp-open"
                && event["path"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("/tmp/.sodeck-env-"))
        })
        .collect();
    assert!(!opens.is_empty());
    assert!(opens.iter().all(|event| event["mode"]
        .as_u64()
        .is_some_and(|mode| mode & 0o777 == 0o600)));
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_to_string(root.join("create-env-mode")).unwrap(),
        "384"
    );
    assert!(std::fs::read_dir(root.join("remote/tmp"))
        .unwrap()
        .all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".sodeck-env-")));
    // A delete preview is single use and cannot be transplanted to another window.
    let preview = state
        .docker
        .prepare(
            &state,
            first,
            &first_owner,
            &binding.token,
            &b,
            Action::Remove,
        )
        .unwrap();
    let mut remove = make(Action::Remove, &b);
    remove.preview_id = Some(preview.id);
    assert_eq!(
        state
            .docker
            .submit(&state, second, second_owner.clone(), remove.clone())
            .unwrap_err()
            .kind,
        "conflict"
    );
    let removed = state
        .docker
        .submit(&state, first, first_owner.clone(), remove)
        .unwrap();
    wait("delete finishes", || !task(&state, &removed).active());
    assert_eq!(
        task(&state, &removed).observed_state.as_deref(),
        Some("removed")
    );
    // Lost command completion must never be interpreted as success or replayed.
    std::fs::write(root.join("docker-operation-mode"), "no-receipt").unwrap();
    let unknown = state
        .docker
        .submit(&state, first, first_owner.clone(), make(Action::Start, &a))
        .unwrap();
    wait("missing receipt becomes unknown", || {
        task(&state, &unknown).phase == "unknown"
    });
    assert!(task(&state, &unknown).observed_state.is_none());
    assert!(state
        .docker
        .acknowledge(&state, first, &first_owner, &unknown)
        .is_err());
    let before = std::fs::read_to_string(root.join("docker-management.json")).unwrap();
    state
        .docker
        .reconcile(&state, second, &second_owner, &unknown)
        .unwrap();
    wait("read-only reconciliation finishes", || {
        !task(&state, &unknown).reconciling
    });
    assert_eq!(
        std::fs::read_to_string(root.join("docker-management.json")).unwrap(),
        before
    );
    assert_eq!(task(&state, &unknown).phase, "unknown");
    assert_eq!(
        task(&state, &unknown).observed_state.as_deref(),
        Some("running")
    );
    state
        .docker
        .acknowledge(&state, first, &first_owner, &unknown)
        .unwrap();
    std::fs::write(root.join("docker-operation-mode"), "verify-fail").unwrap();
    let file = root.join("docker-management.json");
    let mut fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    fixture[&a]["mutated"] = false.into();
    std::fs::write(&file, fixture.to_string()).unwrap();
    let completed = state
        .docker
        .submit(&state, first, first_owner.clone(), make(Action::Stop, &a))
        .unwrap();
    wait(
        "successful command with failed verification stays successful",
        || !task(&state, &completed).active(),
    );
    assert_eq!(task(&state, &completed).command_result, "succeeded");
    assert!(task(&state, &completed).message.contains("当前状态待刷新"));
    std::fs::remove_file(root.join("docker-operation-mode")).unwrap();
    // History stays readable beyond the live frame's 200-record bound. A cursor
    // is scoped to this connection; paging never repeats or loses an operation.
    let expected_history = {
        let mut core = state.docker.core.lock().unwrap();
        let template = core.records[0].view.clone();
        for _ in 0..205 {
            let mut view = template.clone();
            view.id = uuid();
            view.request_id = uuid();
            core.records.push(Record {
                view,
                identity: context.identity().into(),
                servers: vec![],
                request: make(Action::Stop, &a),
                cancel: Cancellation::default(),
            });
        }
        core.records
            .iter()
            .filter(|r| r.identity == context.identity())
            .count()
    };
    let mut cursor = None;
    let mut history_ids = std::collections::HashSet::new();
    loop {
        let page = state
            .docker
            .history(&state, second, &second_owner, cursor.as_deref())
            .unwrap();
        assert!(page.operations.len() <= 100);
        for operation in page.operations {
            assert!(history_ids.insert(operation.id));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(history_ids.len(), expected_history);
    assert!(state
        .docker
        .history(&state, first, &first_owner, Some("foreign-cursor"))
        .is_err());
    // Accepted jobs keep running when the last owning terminal disappears.
    std::fs::write(root.join("docker-action-delay"), "1").unwrap();
    let background = state
        .docker
        .submit(&state, first, first_owner.clone(), make(Action::Start, &a))
        .unwrap();
    wait("restart accepted by remote", || {
        task(&state, &background).phase == "running"
    });
    state.docker.window_closed("main");
    state.docker.window_closed("workspace-test");
    state.terminals.lock().unwrap().clear();
    wait("background task survives closing workspaces", || {
        !task(&state, &background).active()
    });
    assert_eq!(task(&state, &background).command_result, "succeeded");
    state.stopping.store(true, Ordering::SeqCst);
    state.docker.shutdown().unwrap();
    state.monitor.shutdown().unwrap();
}
