use super::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct MemoryFs {
    entries: RefCell<HashMap<String, Stamp>>,
    removed: RefCell<Vec<String>>,
    errors: RefCell<HashMap<String, &'static str>>,
}
impl MemoryFs {
    fn add(&self, path: &str, kind: &'static str) {
        self.entries.borrow_mut().insert(
            path.into(),
            Stamp {
                kind,
                size: Some(4),
                modified: Some(10),
                mode: Some(0o755),
            },
        );
    }
}
impl DeleteFs for MemoryFs {
    fn stat(&self, path: &str) -> AppResult<Option<Stamp>> {
        Ok(self.entries.borrow().get(path).cloned())
    }
    fn children(&self, path: &str) -> AppResult<Vec<String>> {
        let mut children: Vec<_> = self
            .entries
            .borrow()
            .keys()
            .filter(|p| remote_path::parent(p) == path)
            .cloned()
            .collect();
        children.sort();
        Ok(children)
    }
    fn remove(&self, path: &str, directory: bool) -> AppResult<()> {
        if let Some(kind) = self.errors.borrow().get(path) {
            return Err(AppError::new(kind, "injected failure"));
        }
        if directory && !self.children(path)?.is_empty() {
            return Err(AppError::new("remote", "directory not empty"));
        }
        self.entries.borrow_mut().remove(path);
        self.removed.borrow_mut().push(path.into());
        Ok(())
    }
}

#[test]
fn deletion_selection_rejects_root_aliases_and_deduplicates_subtrees() {
    for path in ["/", "//./", "relative", "/a/../b", "/a\0b"] {
        assert!(normalize_paths(&[path.into()]).is_err(), "{path:?}");
    }
    assert_eq!(
        normalize_paths(&["/a/b".into(), "/a/".into(), "/ab".into(), "/a".into()]).unwrap(),
        vec!["/a", "/ab"]
    );
}
#[test]
fn recursive_delete_is_postorder_and_does_not_follow_symlinks() {
    let fs = MemoryFs::default();
    for (path, kind) in [
        ("/a", "directory"),
        ("/a/empty", "directory"),
        ("/a/.hidden", "file"),
        ("/a/中文\n'$(x)", "file"),
        ("/a/link", "symlink"),
        ("/target", "file"),
    ] {
        fs.add(path, kind);
    }
    let mut plan = vec![];
    scan(&fs, "/a", &mut plan, 0, &|| Ok(())).unwrap();
    assert_eq!(plan.last().unwrap().path, "/a");
    for item in &plan {
        assert!(remove_planned(&fs, item).unwrap());
    }
    assert!(fs.stat("/a").unwrap().is_none());
    assert!(fs.stat("/target").unwrap().is_some());
    assert_eq!(fs.removed.borrow().len(), 5);
}
#[test]
fn changed_ancestors_and_files_are_retained() {
    let fs = MemoryFs::default();
    fs.add("/a", "directory");
    fs.add("/a/file", "file");
    let mut plan = vec![];
    scan(&fs, "/a", &mut plan, 0, &|| Ok(())).unwrap();
    fs.add("/a", "symlink");
    assert_eq!(remove_planned(&fs, &plan[0]).unwrap_err().kind, "path");
    fs.add("/a", "directory");
    fs.entries.borrow_mut().get_mut("/a/file").unwrap().modified = Some(20);
    assert_eq!(remove_planned(&fs, &plan[0]).unwrap_err().kind, "changed");
    assert!(fs.removed.borrow().is_empty());
}
#[test]
fn new_children_and_failed_children_prevent_parent_removal() {
    let fs = MemoryFs::default();
    fs.add("/a", "directory");
    fs.add("/a/file", "file");
    let mut plan = vec![];
    scan(&fs, "/a", &mut plan, 0, &|| Ok(())).unwrap();
    fs.add("/a/new", "file");
    fs.errors
        .borrow_mut()
        .insert("/a/file".into(), "permission");
    assert_eq!(
        remove_planned(&fs, &plan[0]).unwrap_err().kind,
        "permission"
    );
    assert!(remove_planned(&fs, plan.last().unwrap()).is_err());
    assert!(fs.stat("/a/new").unwrap().is_some());
    assert!(fs.removed.borrow().is_empty());
}
#[test]
fn missing_is_separate_from_success_and_uncertain_mutations_are_never_repeated() {
    let fs = MemoryFs::default();
    fs.add("/file", "file");
    let mut plan = vec![];
    scan(&fs, "/file", &mut plan, 0, &|| Ok(())).unwrap();
    fs.errors.borrow_mut().insert("/file".into(), "unknown");
    assert_eq!(remove_planned(&fs, &plan[0]).unwrap_err().kind, "unknown");
    fs.entries.borrow_mut().remove("/file");
    assert!(!remove_planned(&fs, &plan[0]).unwrap());
    assert!(fs.removed.borrow().is_empty());
    let mut item = entry("/file", None);
    item.status = "failed".into();
    item.failures.push(DeleteFailure {
        path: "/file".into(),
        message: "lost reply".into(),
        uncertain: true,
        current_state: None,
    });
    assert_eq!(final_status(&[item.clone()]), "unknown");
    item.failures[0].current_state = Some("exists".into());
    assert_eq!(final_status(&[item]), "failed");
}
#[test]
fn scan_cancellation_does_not_mutate_and_mount_boundaries_include_descendants() {
    let fs = MemoryFs::default();
    fs.add("/a", "directory");
    fs.add("/a/file", "file");
    let calls = Cell::new(0);
    let mut plan = vec![];
    assert!(scan(&fs, "/a", &mut plan, 0, &|| {
        calls.set(calls.get() + 1);
        if calls.get() > 1 {
            Err(AppError::new("cancelled", "stop"))
        } else {
            Ok(())
        }
    })
    .is_err());
    assert!(fs.removed.borrow().is_empty());
    let mounts = vec!["/".into(), "/data/mount".into()];
    assert!(protect_mounts("/data", &mounts).is_err());
    assert!(protect_mounts("/data/mount", &mounts).is_err());
    assert!(protect_mounts("/database", &mounts).is_ok());
    assert!(protect_mounts("/data/mount/child", &mounts).is_ok());
}

pub(crate) fn exercise_protocol(context: SharedContext, fixture: &Path) {
    let local = fixture.join("remote");
    std::fs::create_dir_all(local.join("proc/self")).unwrap();
    let table =
        "20 1 8:1 / / rw - ext4 /dev/fixture rw\n21 20 8:2 / /mounted rw - ext4 /dev/other rw\n";
    std::fs::write(local.join("proc/self/mountinfo"), table).unwrap();
    std::fs::create_dir_all(local.join("delete/tree/empty")).unwrap();
    std::fs::write(local.join("delete/tree/.hidden"), "keep no hidden files").unwrap();
    std::fs::write(
        local.join("delete/tree/中文 '$(x)\n.txt"),
        "names are literal",
    )
    .unwrap();
    std::fs::write(local.join("delete/keep"), "untouched").unwrap();
    let execute = |paths: &[&str]| {
        let c = connection::connect_verified(
            &context,
            &Cancellation::default(),
            crate::traffic::TrafficPurpose::Shared,
        )
        .unwrap();
        let sftp = c.session().sftp().unwrap();
        let entries: Vec<_> = paths
            .iter()
            .map(|p| (p.to_string(), SftpFs(&sftp).stat(p).unwrap()))
            .collect();
        let handle = Arc::new(Handle {
            context: context.clone(),
            view: Mutex::new(DeleteTask {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: "fixture".into(),
                server_name: "fixture".into(),
                status: "checking".into(),
                revision: 0,
                current_path: None,
                deleted_count: 0,
                missing_count: 0,
                entries: entries.iter().map(|(p, s)| entry(p, s.as_ref())).collect(),
                affected_directories: vec![],
                error: None,
                entry_count: entries.len(),
                server_identity: context.identity().into(),
                ..DeleteTask::default()
            }),
            stop: AtomicBool::new(false),
            reconciling: AtomicBool::new(false),
            cancel: Cancellation::default(),
            last_emit: Mutex::new(Instant::now()),
        });
        (handle, entries)
    };
    let state = crate::connection::tests::state();
    let (task, entries) = execute(&["/delete/tree", "/download/file-link"]);
    run(&state, None, &task, &entries).unwrap();
    let completed = task.view.lock().unwrap().clone();
    assert_eq!(
        completed.status,
        "completed",
        "{}",
        serde_json::to_string(&completed).unwrap()
    );
    assert!(!local.join("delete/tree").exists());
    assert!(local.join("delete/keep").exists());
    assert!(local.join("download/readme.txt").exists());

    std::fs::create_dir_all(local.join("mounted")).unwrap();
    std::fs::write(local.join("mounted/keep"), "mounted").unwrap();
    let (task, entries) = execute(&["/mounted"]);
    run(&state, None, &task, &entries).unwrap();
    assert_eq!(task.view.lock().unwrap().status, "failed");
    assert!(local.join("mounted/keep").exists());

    let (task, entries) = execute(&["/delete/keep"]);
    let read_claim = [Claim {
        key: context.target_public_key().into(),
        path: "/delete/keep".into(),
        write: false,
    }];
    let reader = state
        .transfers
        .paths
        .acquire_many(&read_claim, "fixture-download", |_| Ok(()))
        .unwrap();
    assert_eq!(run(&state, None, &task, &entries).unwrap_err().kind, "busy");
    assert!(local.join("delete/keep").exists());
    drop(reader);

    let (task, entries) = execute(&["/tree-link/log/app.log"]);
    run(&state, None, &task, &entries).unwrap();
    assert_eq!(task.view.lock().unwrap().status, "failed");
    assert!(local.join("var/log/app.log").exists());

    std::fs::create_dir_all(local.join("delete/no-mount-data")).unwrap();
    std::fs::write(local.join("delete/no-mount-data/keep"), "preserve").unwrap();
    std::fs::remove_file(local.join("proc/self/mountinfo")).unwrap();
    let (task, entries) = execute(&["/delete/no-mount-data"]);
    run(&state, None, &task, &entries).unwrap();
    assert_eq!(task.view.lock().unwrap().status, "failed");
    assert!(local.join("delete/no-mount-data/keep").exists());
    std::fs::write(local.join("proc/self/mountinfo"), table).unwrap();

    std::fs::write(local.join("delete/deny"), "permission test").unwrap();
    std::fs::write(local.join("delete/ok"), "success test").unwrap();
    std::fs::write(fixture.join("sftp-delete-deny"), "/delete/deny").unwrap();
    let (task, entries) = execute(&["/delete/deny", "/delete/ok"]);
    run(&state, None, &task, &entries).unwrap();
    assert_eq!(task.view.lock().unwrap().status, "partialFailed");
    assert!(local.join("delete/deny").exists());
    assert!(!local.join("delete/ok").exists());
    std::fs::remove_file(fixture.join("sftp-delete-deny")).unwrap();

    std::fs::write(local.join("delete/lost"), "lost reply").unwrap();
    std::fs::write(fixture.join("sftp-delete-lost-reply"), "/delete/lost").unwrap();
    let (task, entries) = execute(&["/delete/lost", "/delete/keep"]);
    run(&state, None, &task, &entries).unwrap();
    assert_eq!(task.view.lock().unwrap().status, "unknown");
    assert_eq!(task.view.lock().unwrap().entries[1].status, "notExecuted");
    assert!(!local.join("delete/lost").exists());
    assert!(local.join("delete/keep").exists());
    std::fs::remove_file(fixture.join("sftp-delete-lost-reply")).unwrap();
    let connection = connection::connect_verified(
        &context,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Shared,
    )
    .unwrap();
    let sftp = connection.session().sftp().unwrap();
    let mut observation = task.view.lock().unwrap().clone();
    assert_eq!(
        reconcile_view(&sftp, &mut observation, &|| Ok(())).unwrap(),
        vec!["/delete/lost"]
    );
    assert_eq!(observation.entries[0].status, "missing");
    assert_eq!(observation.missing_count, 1);
    reconcile_view(&sftp, &mut observation, &|| Ok(())).unwrap();
    assert_eq!(observation.missing_count, 1);
    std::fs::write(local.join("delete/lost"), "new same-named file").unwrap();
    assert!(reconcile_view(&sftp, &mut observation, &|| Ok(()))
        .unwrap()
        .is_empty());
    assert_eq!(observation.entries[0].status, "failed");
    assert_eq!(observation.missing_count, 0);
    assert_eq!(
        std::fs::read_to_string(local.join("delete/lost")).unwrap(),
        "new same-named file"
    );

    std::fs::create_dir_all(local.join("delete/slow")).unwrap();
    for n in 0..20 {
        std::fs::write(local.join(format!("delete/slow/{n}")), "slow").unwrap();
    }
    std::fs::write(fixture.join("sftp-delete-delay"), "0.05").unwrap();
    let (task, entries) = execute(&["/delete/slow"]);
    let worker_task = task.clone();
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || run(&worker_state, None, &worker_task, &entries));
    let deadline = Instant::now() + Duration::from_secs(12);
    state
        .deletes
        .0
        .lock()
        .unwrap()
        .tasks
        .insert("background".into(), task.clone());
    state.deletes.0.lock().unwrap().drafts.insert(
        "unconfirmed".into(),
        Draft {
            owner: "closed-window".into(),
            generation: 0,
            session_id: "fixture".into(),
            context: context.clone(),
            entries: vec![],
            expires: Instant::now() + DRAFT_TTL,
            started: None,
            parent_task_id: None,
            require_session: true,
        },
    );
    state.deletes.discard_window_drafts("closed-window");
    assert!(state.deletes.0.lock().unwrap().drafts.is_empty());
    assert!(!task.stop.load(Ordering::SeqCst));
    assert!(state.deletes.contains("background"));
    while task.view.lock().unwrap().deleted_count == 0 {
        assert!(Instant::now() < deadline, "deletion did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    task.request_stop();
    worker.join().unwrap().unwrap();
    let view = task.view.lock().unwrap();
    assert_eq!(view.status, "cancelled");
    assert!(view.deleted_count > 0 && view.deleted_count < 20);
    assert!(local.join("delete/slow").exists());
    std::fs::remove_file(fixture.join("sftp-delete-delay")).unwrap();
}

fn stored_task() -> DeleteTask {
    DeleteTask {
        id: "stored-delete".into(),
        status: "running".into(),
        entry_count: 2,
        entries: vec![entry("/data", None), entry("/untouched", None)],
        ..DeleteTask::default()
    }
}

#[test]
fn recovery_preserves_confirmed_results_and_only_marks_inflight_path_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.sqlite");
    {
        let mut store = crate::file_tasks::store::Store::open(&path).unwrap();
        let task = stored_task();
        store.save("delete", &task.id, &task).unwrap();
        store.begin_delete(&task.id, 0, "/data/a").unwrap();
        store
            .finish_delete(
                &task.id,
                &crate::file_tasks::store::DeleteStep {
                    root: 0,
                    path: "/data/a".into(),
                    outcome: "deleted".into(),
                    message: None,
                },
            )
            .unwrap();
        store.begin_delete(&task.id, 0, "/data/b").unwrap();
    }
    for _ in 0..2 {
        let mut store = crate::file_tasks::store::Store::open(&path).unwrap();
        let recovered = DeleteState::restore(&mut store)
            .unwrap()
            .snapshot()
            .pop()
            .unwrap();
        assert_eq!(recovered.status, "unknown");
        assert_eq!(recovered.deleted_count, 1);
        assert_eq!(recovered.entries[0].status, "partial");
        assert_eq!(recovered.entries[0].failures.len(), 1);
        assert_eq!(recovered.entries[0].failures[0].path, "/data/b");
        assert_eq!(recovered.entries[1].status, "notExecuted");
        assert_eq!(recovered.entries[1].deleted_count, 0);
    }
}

#[test]
fn recovery_without_dispatched_request_is_interrupted_not_unknown() {
    let mut store = crate::file_tasks::store::Store::open(Path::new(":memory:")).unwrap();
    let task = stored_task();
    store.save("delete", &task.id, &task).unwrap();
    let recovered = DeleteState::restore(&mut store)
        .unwrap()
        .snapshot()
        .pop()
        .unwrap();
    assert_eq!(recovered.status, "interrupted");
    assert!(recovered
        .entries
        .iter()
        .all(|e| e.status == "notExecuted" && e.failures.is_empty()));
}

#[test]
fn disk_failure_before_dispatch_never_deletes_and_after_ack_requires_reconciliation() {
    for before in [true, false] {
        let state = crate::connection::tests::state();
        let fs = MemoryFs::default();
        fs.add("/a", "file");
        let planned = Planned {
            path: "/a".into(),
            stamp: fs.stat("/a").unwrap().unwrap(),
        };
        state
            .file_tasks
            .store
            .lock()
            .unwrap()
            .fail_insert(if before {
                "delete_pending"
            } else {
                "delete_steps"
            });
        let error = remove_recorded(&state, "task", 0, &fs, &planned).unwrap_err();
        if before {
            assert_eq!(error.kind, "taskStore");
            assert!(fs.removed.borrow().is_empty());
        } else {
            assert_eq!(error.kind, "unknown");
            assert_eq!(fs.removed.borrow().len(), 1);
            assert_eq!(
                state
                    .file_tasks
                    .store
                    .lock()
                    .unwrap()
                    .pending_delete("task")
                    .unwrap(),
                Some((0, "/a".into()))
            );
        }
    }
}

#[test]
fn complete_acknowledged_journal_recovers_as_complete_without_replay() {
    let mut store = crate::file_tasks::store::Store::open(Path::new(":memory:")).unwrap();
    let task = stored_task();
    store.save("delete", &task.id, &task).unwrap();
    for (root, path) in [(0, "/data"), (1, "/untouched")] {
        store.begin_delete(&task.id, root, path).unwrap();
        store
            .finish_delete(
                &task.id,
                &crate::file_tasks::store::DeleteStep {
                    root,
                    path: path.into(),
                    outcome: "deleted".into(),
                    message: None,
                },
            )
            .unwrap();
    }
    let recovered = DeleteState::restore(&mut store)
        .unwrap()
        .snapshot()
        .pop()
        .unwrap();
    assert_eq!(recovered.status, "completed");
    assert_eq!(recovered.deleted_count, 2);
    assert!(recovered.error.is_none());
}
