use serde_json::json;
use sodeck::{
    lifecycle::{Action, Operation, Phase, Target},
    AppCore,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
fn core() -> Arc<AppCore> {
    AppCore::open(
        std::env::temp_dir().join(format!(
            "sodeck-close-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )),
        "test".into(),
    )
    .unwrap()
}
fn wait(core: &AppCore, id: &str, phase: Phase) -> Operation {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let op = core.close_status("main", id).unwrap();
        if op.phase == phase {
            return op;
        }
        assert!(
            Instant::now() < deadline,
            "{id}: {:?} {:?}",
            op.phase,
            op.error
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn inputs(core: &Arc<AppCore>, op: &Operation) {
    for window in op.inputs.keys() {
        core.close_report_inputs(window, &op.id, op.input_round, json!({}))
            .unwrap();
    }
}
fn workspace(core: &Arc<AppCore>, name: &str) -> String {
    let catalog = core.server_catalog();
    let result=core.change_catalog(catalog["revision"].as_u64().unwrap(),serde_json::from_value(json!({"op":"saveServer","draft":{"name":name,"host":"invalid.test","port":22,"username":"ops","authentication":{"type":"password","secret":{"action":"clear"}},"route":[]}})).unwrap()).unwrap();
    core.accept_connected(sodeck::workspaces::ConnectedSession {
        server_id: result["selectedId"].as_str().unwrap().into(),
        connection_id: name.into(),
        endpoint: "ops@invalid.test:22".into(),
    })
    .unwrap()
    .context
    .workspace_id
}
#[test]
fn native_close_intents_coalesce_while_active_and_start_fresh_after_cancel() {
    let core = core();
    let target = Target::Application {
        action: Action::Exit,
    };
    let first = core.close_native_prepare("main", target.clone()).unwrap();
    assert_eq!(
        core.close_native_prepare("main", target.clone())
            .unwrap()
            .id,
        first.id
    );
    core.close_decide("main", &first.id, first.decision_revision, "cancel")
        .unwrap();
    wait(&core, &first.id, Phase::Aborted);
    let next = core.close_native_prepare("main", target.clone()).unwrap();
    assert_ne!(next.id, first.id);
    assert_eq!(
        core.close_native_prepare("main", target).unwrap().id,
        next.id
    );
    core.close_decide("main", &next.id, next.decision_revision, "cancel")
        .unwrap();
    wait(&core, &next.id, Phase::Aborted);
    let identified = core
        .close_prepare(
            "main",
            "identified",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert!(core
        .close_prepare(
            "main",
            "identified",
            Target::Application {
                action: Action::Restart
            }
        )
        .is_err());
    core.close_decide(
        "main",
        &identified.id,
        identified.decision_revision,
        "cancel",
    )
    .unwrap();
    wait(&core, &identified.id, Phase::Aborted);
}
#[test]
fn closing_last_secondary_workspace_reserves_and_finalizes_the_native_host() {
    let core = core();
    let id = workspace(&core, "last");
    let movement = core
        .prepare_workspace_move("main", &id, "secondary", 0, "last-move".into(), true)
        .unwrap();
    core.workspaces.lock().unwrap().ready("secondary").unwrap();
    core.commit_workspace_move(&movement).unwrap();
    core.register_close_host("secondary").unwrap();
    let finished = Arc::new(AtomicUsize::new(0));
    let observed = finished.clone();
    core.set_finalizer(Arc::new(move |target| {
        assert_eq!(
            target,
            Target::Window {
                id: "secondary".into()
            }
        );
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let plan = core
        .close_fixture_prepare("secondary", Target::Workspace { id: id.clone() })
        .unwrap();
    let operation = plan["token"].as_str().unwrap();
    assert_eq!(
        core.close_status("secondary", operation).unwrap().target,
        Target::Window {
            id: "secondary".into()
        }
    );
    let other = workspace(&core, "other");
    assert!(core
        .prepare_workspace_move("main", &other, "secondary", 0, "late-move".into(), false)
        .is_err());
    let server = core.workspace_projection("secondary").unwrap().workspaces[0]
        .context
        .server_id
        .clone();
    assert!(core.start_connection("secondary", &server).is_err());
    core.ui_close_finish(
        operation,
        serde_json::from_value(plan["views"].clone()).unwrap(),
        "discard",
    )
    .unwrap();
    assert!(core.workspace_projection("secondary").is_err());
    assert_eq!(finished.load(Ordering::SeqCst), 1);
    assert_eq!(
        core.workspace_projection("main").unwrap().workspaces[0]
            .context
            .workspace_id,
        other
    );
}
#[test]
fn global_scope_includes_secondary_workspaces_and_close_cancels_late_move_commit() {
    let core = core();
    let a = workspace(&core, "main");
    let b = workspace(&core, "second");
    let movement = core
        .prepare_workspace_move("main", &b, "secondary", 0, "move".into(), true)
        .unwrap();
    core.workspaces.lock().unwrap().ready("secondary").unwrap();
    core.commit_workspace_move(&movement).unwrap();
    core.workspace_changed();
    let pending = core
        .prepare_workspace_move("main", &a, "secondary", 1, "pending".into(), false)
        .unwrap();
    let op = core
        .close_prepare(
            "main",
            "exit",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert!(op.workspaces.contains(&a) && op.workspaces.contains(&b));
    assert!(core.commit_workspace_move(&pending).is_err());
    assert!(core
        .prepare_workspace_move("secondary", &b, "main", 0, "late".into(), false)
        .is_err());
    inputs(&core, &op);
    let deciding = wait(&core, &op.id, Phase::Deciding);
    core.close_decide("main", &op.id, deciding.decision_revision, "cancel")
        .unwrap();
    wait(&core, &op.id, Phase::Aborted);
    assert_eq!(
        core.workspaces.lock().unwrap().owner(&b).as_deref(),
        Some("secondary")
    );
    assert!(core
        .prepare_workspace_move("main", &a, "secondary", 1, "after".into(), false)
        .is_ok());
}
#[test]
fn lost_window_transfers_cleanup_to_main_without_waiting_for_missing_inputs() {
    let core = core();
    let id = workspace(&core, "lost");
    let m = core
        .prepare_workspace_move("main", &id, "secondary", 0, "move".into(), true)
        .unwrap();
    core.workspaces.lock().unwrap().ready("secondary").unwrap();
    core.commit_workspace_move(&m).unwrap();
    core.register_close_host("secondary").unwrap();
    let op = core
        .close_prepare(
            "secondary",
            "close",
            Target::Window {
                id: "secondary".into(),
            },
        )
        .unwrap();
    core.close_host_lost("secondary");
    wait(&core, &op.id, Phase::Completed);
    assert!(core.workspace_projection("secondary").is_err());
    assert!(core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    core.close_host_lost("secondary");
    assert!(core.close_operations("main").is_empty());
}
#[test]
fn losing_main_during_local_preparation_drains_every_window_as_an_application_close() {
    let core = core();
    let a = workspace(&core, "a");
    let b = workspace(&core, "b");
    let movement = core
        .prepare_workspace_move("main", &b, "secondary", 0, "move".into(), true)
        .unwrap();
    core.workspaces.lock().unwrap().ready("secondary").unwrap();
    core.commit_workspace_move(&movement).unwrap();
    core.register_close_host("main").unwrap();
    core.register_close_host("secondary").unwrap();
    let targets = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = targets.clone();
    core.set_finalizer(Arc::new(move |target| {
        seen.lock().unwrap().push(target);
        Ok(())
    }));
    let op = core
        .close_prepare("main", "local", Target::Workspace { id: a })
        .unwrap();
    core.close_host_lost("main");
    let completed = wait(&core, &op.id, Phase::Completed);
    assert_eq!(
        completed.target,
        Target::Application {
            action: Action::Exit
        }
    );
    assert!(completed.workspaces.contains(&b));
    assert!(core.workspaces.lock().unwrap().ids().is_empty());
    assert_eq!(
        *targets.lock().unwrap(),
        vec![Target::Application {
            action: Action::Exit
        }]
    );
    assert!(core.is_closing());
}
#[test]
fn unhosted_exit_reserves_startup_and_registered_hosts_prevent_automatic_discard() {
    let core = core();
    let op = core
        .close_prepare(
            "main",
            "exit",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert!(core.close_unhosted(&op.id));
    assert!(core.register_close_host("main").is_err());
    wait(&core, &op.id, Phase::Completed);
    let hosted = self::core();
    hosted.register_close_host("main").unwrap();
    let op = hosted
        .close_prepare(
            "main",
            "exit",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert!(!hosted.close_unhosted(&op.id));
    assert_eq!(
        hosted.close_status("main", &op.id).unwrap().phase,
        Phase::Preparing
    );
    hosted
        .close_decide("main", &op.id, op.decision_revision, "cancel")
        .unwrap();
    wait(&hosted, &op.id, Phase::Aborted);
}
#[test]
fn failed_empty_window_keeps_host_drafts_in_global_adoption_and_cancel_restores_failure() {
    let core = core();
    core.workspaces
        .lock()
        .unwrap()
        .register("secondary")
        .unwrap();
    core.register_close_host("secondary").unwrap();
    core.set_finalizer(Arc::new(|target| {
        if matches!(target, Target::Window { .. }) {
            Err("destroy failed".into())
        } else {
            Ok(())
        }
    }));
    let child = core
        .close_prepare(
            "secondary",
            "close",
            Target::Window {
                id: "secondary".into(),
            },
        )
        .unwrap();
    inputs(&core, &child);
    let deciding = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let op = core.close_status("secondary", &child.id).unwrap();
            if op.phase == Phase::Deciding {
                break op;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let decision = core
        .close_decide(
            "secondary",
            &child.id,
            deciding.decision_revision,
            "discard",
        )
        .unwrap();
    inputs(&core, &decision);
    let deadline = Instant::now() + Duration::from_secs(10);
    while core.close_status("secondary", &child.id).unwrap().phase != Phase::Failed {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let global = core
        .close_prepare(
            "main",
            "exit",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert!(global.inputs.contains_key("secondary"));
    assert_eq!(
        core.close_status("secondary", &child.id)
            .unwrap()
            .adopted_by
            .as_deref(),
        Some(global.id.as_str())
    );
    inputs(&core, &global);
    let deciding = wait(&core, &global.id, Phase::Deciding);
    core.close_decide("main", &global.id, deciding.decision_revision, "cancel")
        .unwrap();
    wait(&core, &global.id, Phase::Aborted);
    let restored = core.close_status("secondary", &child.id).unwrap();
    assert_eq!(restored.phase, Phase::Failed);
    assert!(restored.adopted_by.is_none());
}
#[test]
fn window_reports_are_independent_of_progress_revisions_and_cancel_restores_admission() {
    let core = core();
    core.workspaces
        .lock()
        .unwrap()
        .register("secondary")
        .unwrap();
    let op = core
        .close_prepare(
            "main",
            "request",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    assert_eq!(
        core.close_prepare("main", "duplicate", op.target.clone())
            .unwrap()
            .id,
        op.id
    );
    let first = core
        .close_report_inputs("secondary", &op.id, op.input_round, json!({"drafts":[]}))
        .unwrap();
    assert!(first.revision > op.revision);
    core.close_report_inputs("main", &op.id, op.input_round, json!({"drafts":[]}))
        .unwrap();
    let deciding = wait(&core, &op.id, Phase::Deciding);
    assert!(core.is_closing());
    core.close_decide("main", &op.id, deciding.decision_revision, "cancel")
        .unwrap();
    wait(&core, &op.id, Phase::Aborted);
    assert!(!core.is_closing());
}
#[test]
fn finalization_failure_keeps_stopped_state_and_retry_does_not_repeat_cleanup() {
    let core = core();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    core.set_finalizer(Arc::new(move |_| {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            Err("native failure".into())
        } else {
            Ok(())
        }
    }));
    let op = core
        .close_prepare(
            "main",
            "request",
            Target::Application {
                action: Action::Restart,
            },
        )
        .unwrap();
    inputs(&core, &op);
    let deciding = wait(&core, &op.id, Phase::Deciding);
    let decision = core
        .close_decide("main", &op.id, deciding.decision_revision, "discard")
        .unwrap();
    inputs(&core, &decision);
    let failed = wait(&core, &op.id, Phase::Failed);
    assert_eq!(failed.failed_stage, Some(Phase::Finalizing));
    assert!(core.is_closing());
    assert!(core
        .close_decide("main", &op.id, failed.decision_revision, "cancel")
        .is_err());
    core.close_retry("main", &op.id).unwrap();
    wait(&core, &op.id, Phase::Completed);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(core.close_retry("main", &op.id).is_err());
}
#[test]
fn host_save_failure_preserves_decision_and_allows_return_without_finalizing() {
    let core = core();
    let called = Arc::new(AtomicUsize::new(0));
    let count = called.clone();
    core.set_finalizer(Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let op = core
        .close_prepare(
            "main",
            "request",
            Target::Application {
                action: Action::Exit,
            },
        )
        .unwrap();
    core.close_report_inputs(
        "main",
        &op.id,
        op.input_round,
        json!({"drafts":[{"id":"settings:hidden","title":"Hidden settings","dirty":true}]}),
    )
    .unwrap();
    let deciding = wait(&core, &op.id, Phase::Deciding);
    assert_eq!(deciding.dirty.len(), 1);
    let decision = core
        .close_decide("main", &op.id, deciding.decision_revision, "save")
        .unwrap();
    let repeated = core
        .close_decide("main", &op.id, deciding.decision_revision, "save")
        .unwrap();
    assert_eq!(decision.input_round, repeated.input_round);
    core.close_report_inputs(
        "main",
        &op.id,
        decision.input_round,
        json!({"error":"settings conflict"}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let failed = loop {
        let op = core.close_status("main", &op.id).unwrap();
        if op.error.is_some() {
            break op;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(failed.phase, Phase::Deciding);
    assert_eq!(called.load(Ordering::SeqCst), 0);
    core.close_decide("main", &op.id, failed.decision_revision, "cancel")
        .unwrap();
    wait(&core, &op.id, Phase::Aborted);
}
