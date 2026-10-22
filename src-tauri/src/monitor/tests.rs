use super::*;
use std::path::Path;

fn collector() -> (Channel<MonitorEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
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
fn wait(message: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(Instant::now() < deadline, "{message}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn exercise_docker(
    state: Arc<State>,
    context: SharedContext,
    first: &str,
    second: &str,
    root: &Path,
) {
    let interest = Interest {
        visible: true,
        client_traffic: true,
        docker: DockerInterest {
            summary: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let (a, received) = collector();
    let (b, _) = collector();
    let one = state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            first.into(),
            "main".into(),
            a,
            interest.clone(),
        )
        .unwrap();
    let two = state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            second.into(),
            "workspace-test".into(),
            b,
            interest,
        )
        .unwrap();
    let entry = state.monitor.owned(&state, &one, "main").unwrap();
    assert!(Arc::ptr_eq(
        &entry,
        &state.monitor.owned(&state, &two, "workspace-test").unwrap()
    ));
    wait("Docker summary and independent traffic arrive", || {
        let c = entry.core.lock().unwrap();
        c.docker
            .summary
            .value
            .as_ref()
            .is_some_and(|s| s.total == 3 && s.unhealthy == Some(0))
            && received
                .lock()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "traffic")
    });
    let events = || -> Vec<serde_json::Value> {
        std::fs::read_to_string(root.join("ssh-events.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    };
    let queries = |text: &str| {
        events()
            .iter()
            .filter(|e| {
                e["event"] == "exec" && e["command"].as_str().is_some_and(|c| c.contains(text))
            })
            .count()
    };
    assert_eq!(
        queries("command -v docker"),
        1,
        "two subscribers share discovery"
    );
    assert_eq!(
        queries(" image ls "),
        0,
        "hidden image lists are not fetched"
    );
    assert_eq!(
        queries(" ps -a --no-trunc --format "),
        1,
        "summary and detail share the list"
    );
    {
        let mut c = entry.core.lock().unwrap();
        c.subscribers.get_mut(&one).unwrap().interest.docker.images = true;
    }
    wait("images load on demand", || {
        entry.core.lock().unwrap().docker.images.status == "ready"
    });
    assert_eq!(queries(" image ls "), 1);
    std::fs::write(root.join("docker-images-delay"), "1.5").unwrap();
    entry.core.lock().unwrap().docker_refresh += 1;
    wait("old-engine image request is in flight", || {
        queries(" image ls ") == 2
    });
    std::fs::write(root.join("docker-endpoint"), "unix:///engine-two.sock\n").unwrap();
    entry.core.lock().unwrap().docker_refresh += 1;
    wait(
        "engine switches while the old image request is pending",
        || {
            entry
                .core
                .lock()
                .unwrap()
                .docker
                .engine
                .value
                .as_ref()
                .is_some_and(|s| s.endpoint == "unix:///engine-two.sock")
        },
    );
    wait("only the new engine's image list is published", || {
        entry
            .core
            .lock()
            .unwrap()
            .docker
            .images
            .value
            .as_ref()
            .is_some_and(|images| {
                images
                    .first()
                    .is_some_and(|i| i.name == "engine-two:alpine")
            })
    });
    assert!(!received
        .lock()
        .unwrap()
        .iter()
        .any(
            |e| e["docker"]["engine"]["value"]["endpoint"] == "unix:///engine-two.sock"
                && e["docker"]["images"]["value"][0]["name"] == "nginx:alpine"
        ));
    std::fs::remove_file(root.join("docker-images-delay")).unwrap();
    // A real remote read timeout retains the binding and recovers automatically.
    let binding = state.monitor.docker_binding(context.identity()).unwrap();
    std::fs::write(root.join("docker-mode"), "timeout").unwrap();
    entry.core.lock().unwrap().docker_refresh += 1;
    wait(
        "engine observation times out without revoking its target",
        || {
            let core = entry.core.lock().unwrap();
            core.docker.engine.status == "stale"
                && core.docker.engine_error_kind.as_deref() == Some("timeout")
        },
    );
    assert_eq!(
        state.monitor.docker_binding(context.identity()).unwrap(),
        binding
    );
    assert!(entry.core.lock().unwrap().docker.containers.value.is_some());
    std::fs::write(root.join("docker-mode"), "running").unwrap();
    wait("automatic retry recovers without a manual refresh", || {
        entry.core.lock().unwrap().docker.engine.status == "ready"
    });
    assert_eq!(
        state.monitor.docker_binding(context.identity()).unwrap(),
        binding
    );
    std::fs::write(root.join("docker-mode"), "permission").unwrap();
    entry.core.lock().unwrap().docker_refresh += 1;
    wait("Docker permission error is isolated", || {
        entry
            .core
            .lock()
            .unwrap()
            .docker
            .engine
            .value
            .as_ref()
            .is_some_and(|s| s.state == "permission")
    });
    assert_eq!(entry.core.lock().unwrap().metrics.cpu.status, "ready");
    assert!(
        entry.core.lock().unwrap().docker.summary.value.is_some(),
        "stale counts retained explicitly"
    );
    std::fs::write(root.join("docker-mode"), "unhealthy").unwrap();
    entry.core.lock().unwrap().docker_refresh += 1;
    wait("Docker recovers with unhealthy summary", || {
        entry
            .core
            .lock()
            .unwrap()
            .docker
            .summary
            .value
            .as_ref()
            .is_some_and(|s| s.unhealthy == Some(1))
    });
    let background = connection::connect_verified(
        &context,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Files,
    )
    .unwrap();
    state.monitor.close_session(first);
    state.monitor.close_session(second);
    state.monitor.shutdown().unwrap();
    let account = context.traffic.account(context.identity());
    let before = account
        .snapshot()
        .files
        .received_bytes
        .parse::<u64>()
        .unwrap();
    let sftp = background.session().sftp().unwrap();
    for _ in 0..10 {
        assert!(sftp.stat(Path::new("/readme.txt")).is_ok());
    }
    wait("traffic continues after all monitor sessions close", || {
        account
            .snapshot()
            .files
            .received_bytes
            .parse::<u64>()
            .unwrap()
            > before
    });
    assert!(
        account
            .snapshot()
            .monitoring
            .received_bytes
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(account.snapshot().active_connections, 1);
    drop(sftp);
    drop(background);
    wait(
        "closing the last background connection settles its counter",
        || account.snapshot().active_connections == 0,
    );
}
pub(crate) fn exercise(
    state: Arc<State>,
    context: SharedContext,
    first: &str,
    second: &str,
    root: &Path,
) {
    let (a, received_a) = collector();
    let (b, received_b) = collector();
    let interest = Interest {
        visible: true,
        mount_paths: vec!["/".into(), "/slow".into()],
        ..Interest::default()
    };
    let one = state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            first.into(),
            "main".into(),
            a,
            interest.clone(),
        )
        .unwrap();
    let two = state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            second.into(),
            "workspace-test".into(),
            b,
            interest.clone(),
        )
        .unwrap();
    assert_eq!(state.monitor.entries.lock().unwrap().len(), 1);
    assert!(state.monitor.owned(&state, &one, "workspace-test").is_err());
    let entry = state.monitor.owned(&state, &one, "main").unwrap();
    wait(
        "three real samples arrive while the slow mount is still running",
        || entry.core.lock().unwrap().metrics_revision >= 3,
    );
    {
        let core = entry.core.lock().unwrap();
        assert!(core.metrics.cpu.value.as_ref().unwrap().percent.is_some());
        assert!(
            core.metrics.network.value.as_ref().unwrap()[0]
                .rx_per_second
                .unwrap()
                > 0.0
        );
        assert_eq!(
            core.metrics.memory.value.as_ref().unwrap().used_percent,
            50.0
        );
        assert!(core.history.since(0).len() >= 3);
    }
    wait("both subscribers see shared samples", || {
        received_a
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "metrics" && v["revision"] == 3)
            && received_b
                .lock()
                .unwrap()
                .iter()
                .any(|v| v["type"] == "metrics" && v["revision"] == 3)
    });
    wait("slow mount fails independently", || {
        entry
            .core
            .lock()
            .unwrap()
            .mounts
            .usage
            .get("21")
            .is_some_and(|u| u.error.is_some())
    });
    assert_eq!(entry.core.lock().unwrap().metrics.cpu.status, "ready");
    assert_eq!(
        entry.core.lock().unwrap().mounts.usage["20"]
            .value
            .as_ref()
            .unwrap()
            .used_percent,
        Some(67)
    );
    let log = std::fs::read_to_string(root.join("ssh-events.jsonl")).unwrap();
    let events: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let transports: std::collections::HashSet<_> = events
        .iter()
        .filter(|v| v["event"] == "monitor-sample")
        .map(|v| v["transport"].as_u64().unwrap())
        .collect();
    assert_eq!(
        transports.len(),
        1,
        "sampling reuses one persistent transport"
    );
    assert_eq!(
        events
            .iter()
            .filter(|v| v["event"] == "exec"
                && v["command"]
                    .as_str()
                    .is_some_and(|s| s.contains("stat -f -c") && s.ends_with("'/'")))
            .count(),
        1,
        "duplicate subscribers must not duplicate disk queries"
    );
    // A moving workspace keeps the same backend entry/history but revokes the old window.
    state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .entries
        .values_mut()
        .find(|w| w.view.tools.context.as_ref().is_some_and(|c| c.id == first))
        .unwrap()
        .view
        .owner
        .label = "workspace-moved".into();
    assert!(state.monitor.owned(&state, &one, "main").is_err());
    let old_count = received_a.lock().unwrap().len();
    entry.publish_mounts(&state);
    assert_eq!(
        received_a.lock().unwrap().len(),
        old_count,
        "old owner must not receive data"
    );
    let (moved, moved_events) = collector();
    let moved_id = state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            first.into(),
            "workspace-moved".into(),
            moved,
            interest,
        )
        .unwrap();
    assert_eq!(
        moved_events.lock().unwrap()[0]["generation"],
        entry.generation
    );
    assert!(Arc::ptr_eq(
        &entry,
        &state
            .monitor
            .owned(&state, &moved_id, "workspace-moved")
            .unwrap()
    ));
    state
        .window_generations
        .lock()
        .unwrap()
        .insert("workspace-moved".into(), 1);
    assert!(state
        .monitor
        .owned(&state, &moved_id, "workspace-moved")
        .is_err());
    state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .entries
        .values_mut()
        .find(|w| w.view.tools.context.as_ref().is_some_and(|c| c.id == first))
        .unwrap()
        .view
        .owner
        .label = "main".into();
    assert!(
        state.monitor.owned(&state, &one, "main").is_ok(),
        "move rollback restores the original subscriber"
    );
    entry.publish_mounts(&state);
    assert!(received_a.lock().unwrap().len() > old_count);
    // A temporary sampling failure retains the last values and records a real gap.
    std::fs::write(root.join("monitor-mode"), "fail").unwrap();
    entry.core.lock().unwrap().refresh = true;
    wait("sampling failure is visible", || {
        entry.core.lock().unwrap().metrics.cpu.status == "stale"
    });
    assert!(
        entry
            .core
            .lock()
            .unwrap()
            .history
            .since(0)
            .last()
            .unwrap()
            .gap
    );
    assert!(entry.core.lock().unwrap().metrics.cpu.value.is_some());
    std::fs::remove_file(root.join("monitor-mode")).unwrap();
    entry.core.lock().unwrap().refresh = true;
    wait("sampling recovers", || {
        entry.core.lock().unwrap().metrics.cpu.status == "ready"
    });
    assert!(entry
        .core
        .lock()
        .unwrap()
        .metrics
        .cpu
        .value
        .as_ref()
        .unwrap()
        .percent
        .is_none());
    state.monitor.close_session(first);
    assert!(!entry.cancel.is_cancelled());
    assert!(state.monitor.owned(&state, &two, "workspace-test").is_ok());
    state.monitor.close_session(second);
    assert!(entry.cancel.is_cancelled());
    assert!(state.monitor.entries.lock().unwrap().is_empty());
    state.monitor.shutdown().unwrap();
    assert_eq!(state.monitor.jobs.load(Ordering::SeqCst), 0);
}

pub(crate) fn exercise_management(
    state: Arc<State>,
    context: SharedContext,
    first: &str,
    second: &str,
    root: &Path,
) {
    state
        .monitor
        .subscribe(
            &state,
            context.clone(),
            first.into(),
            "main".into(),
            Channel::new(|_| Ok(())),
            Interest {
                visible: true,
                docker: DockerInterest {
                    summary: true,
                    containers: true,
                    images: false,
                },
                ..Default::default()
            },
        )
        .unwrap();
    wait("Docker engine is available for management", || {
        state.monitor.docker_binding(context.identity()).is_ok()
    });
    // Exercise real independent SSH preflight and mutations while the observation is stale.
    let previous = state.monitor.docker_binding(context.identity()).unwrap();
    {
        let entries = state.monitor.entries.lock().unwrap();
        let entry = entries.get(context.identity()).unwrap();
        let mut core = entry.core.lock().unwrap();
        core.docker.engine.fail("stale", "远端资源查询超时");
        core.docker.engine_error_kind = Some("timeout".into());
    }
    assert_eq!(
        state.monitor.docker_binding(context.identity()).unwrap(),
        previous
    );
    crate::docker::tests::exercise(state, context, first, second, root);
}

#[test]
fn timeout_keeps_binding_but_permissions_and_closed_entry_do_not() {
    let state = crate::connection::tests::state();
    let entry = Arc::new(Entry::new(
        state.monitor.jobs.clone(),
        state.traffic.account("docker-test"),
    ));
    state
        .monitor
        .entries
        .lock()
        .unwrap()
        .insert("docker-test".into(), entry.clone());
    {
        let mut core = entry.core.lock().unwrap();
        core.docker.engine_generation = 7;
        core.docker.engine = Metric::success(
            crate::docker::DockerStatus {
                state: "running".into(),
                message: "ok".into(),
                endpoint: "unix:///docker.sock".into(),
                daemon_id: Some("engine".into()),
            },
            1,
        );
    }
    let binding = state.monitor.docker_binding("docker-test").unwrap();
    {
        let mut core = entry.core.lock().unwrap();
        core.docker.engine.fail("stale", "timeout");
        core.docker.engine_error_kind = Some("timeout".into());
    }
    assert_eq!(
        state.monitor.docker_binding("docker-test").unwrap(),
        binding
    );
    entry.core.lock().unwrap().docker.engine_error_kind = Some("permission".into());
    assert!(state.monitor.docker_binding("docker-test").is_err());
    state.monitor.retry_docker("docker-test");
    {
        let mut core = entry.core.lock().unwrap();
        assert_eq!(
            core.docker_refresh, 1,
            "manual recovery must not need a ready engine"
        );
        core.docker.refreshing = true;
        core.docker.engine_error_kind = Some("timeout".into());
    }
    state.monitor.retry_docker("docker-test");
    assert_eq!(
        entry.core.lock().unwrap().docker_refresh,
        1,
        "in-flight refreshes are coalesced"
    );
    entry.cancel.cancel();
    assert!(state.monitor.docker_binding("docker-test").is_err());
}
