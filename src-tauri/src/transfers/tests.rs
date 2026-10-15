use super::*;
use crate::{connection, remote_write};
use std::{fs, io::Read, path::Path};

// OpenSSH + rsync, not a shell/SFTP emulation. Start tests/rsync/environment.py first.
struct Fixture {
    a: SharedContext,
    b: SharedContext,
    root: String,
}
impl Fixture {
    fn new() -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let env: serde_json::Value = serde_json::from_slice(
            &fs::read(repo.join(".qa/rsync-environment.json"))
                .expect("start tests/rsync/environment.py"),
        )
        .unwrap();
        let f = Self {
            a: connection::tests::transfer_context("a", env["portA"].as_u64().unwrap() as u16),
            b: connection::tests::transfer_context("b", env["portB"].as_u64().unwrap() as u16),
            root: format!("/home/ops/test-{}", uuid::Uuid::new_v4()),
        };
        for c in [&f.a, &f.b] {
            f.mkdir(c, "");
        }
        f
    }
    fn path(&self, relative: &str) -> String {
        if relative.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{relative}", self.root)
        }
    }
    fn mkdir(&self, context: &SharedContext, relative: &str) {
        let c = connection::connect_verified(context, &Cancellation::default()).unwrap();
        c.session()
            .sftp()
            .unwrap()
            .mkdir(Path::new(&self.path(relative)), 0o755)
            .unwrap();
    }
    fn put(&self, context: &SharedContext, relative: &str, bytes: &[u8]) {
        let c = connection::connect_verified(context, &Cancellation::default()).unwrap();
        let s = c.session().sftp().unwrap();
        let p = self.path(relative);
        if remote_write::metadata(&s, &p).unwrap().is_some() {
            s.unlink(Path::new(&p)).unwrap();
        }
        direct::write_new(&s, &p, bytes).unwrap();
    }
    fn read(&self, relative: &str) -> Vec<u8> {
        let c = connection::connect_verified(&self.b, &Cancellation::default()).unwrap();
        let mut out = vec![];
        c.session()
            .sftp()
            .unwrap()
            .open(Path::new(&self.path(relative)))
            .unwrap()
            .read_to_end(&mut out)
            .unwrap();
        out
    }
    fn exists(&self, relative: &str) -> bool {
        let c = connection::connect_verified(&self.b, &Cancellation::default()).unwrap();
        remote_write::metadata(&c.session().sftp().unwrap(), &self.path(relative))
            .unwrap()
            .is_some()
    }
    fn item(&self, source: Source, policy: ConflictPolicy) -> WriteHandle {
        let remote = matches!(source, Source::Remote { .. });
        let endpoint = remote.then(|| DirectEndpoint {
            host: "rsync-b".into(),
            port: 22,
            username: "ops".into(),
            public_key: self.b.target_public_key().into(),
        });
        let receipt = Receipt {
            format_version: 3,
            target_server_id: "b".into(),
            target_identity: self.b.identity().into(),
            target_write_key: self.b.target_public_key().into(),
            endpoint: endpoint.clone(),
            source_identity: if remote {
                self.a.identity().into()
            } else {
                String::new()
            },
            source_server_id: if remote { "a".into() } else { String::new() },
            verify_sha256: true,
            ..Receipt::default()
        };
        WriteHandle {
            lifecycle: Arc::new(Mutex::new(())),
            source,
            target: self.b.clone(),
            endpoint,
            task: Arc::new(Mutex::new(view(&self.root, policy))),
            cancel: Cancellation::default(),
            stop: Arc::new(AtomicBool::new(false)),
            plan: Arc::new(Mutex::new(None)),
            receipt: Arc::new(Mutex::new(receipt)),
            auth: Arc::new(Mutex::new(
                remote.then(|| model::DirectAuth::Password("fixture-password".into())),
            )),
        }
    }
    fn remote(&self, relative: &str, policy: ConflictPolicy) -> WriteHandle {
        self.item(
            Source::Remote {
                context: self.a.clone(),
                paths: vec![self.path(relative)],
            },
            policy,
        )
    }
    fn run(&self, item: &WriteHandle) {
        let state = connection::tests::state();
        pipeline::run(&state, None, item).unwrap();
        cleanup(item).unwrap();
        assert!(!item.receipt.lock().unwrap().has_remote_resources());
    }
    fn exec(&self, context: &SharedContext, command: &str) -> String {
        let c = connection::connect_verified(context, &Cancellation::default()).unwrap();
        let (code, out, err) =
            direct::capabilities::exec(&c, &Cancellation::default(), command).unwrap();
        assert_eq!(code, 0, "{err}");
        out
    }
    fn mode(&self, context: &SharedContext, mode: &str) {
        self.exec(
            context,
            &format!(
                "printf '%s' {} > /home/ops/rsync-test-mode",
                crate::ssh::quote(mode)
            ),
        );
    }
}
fn view(root: &str, conflict: ConflictPolicy) -> TaskView {
    TaskView {
        id: uuid::Uuid::new_v4().to_string(),
        run_id: uuid::Uuid::new_v4().to_string(),
        presentation: model::TaskPresentation::default(),
        kind: "direct".into(),
        source_name: "A".into(),
        source_path: String::new(),
        target_name: "B".into(),
        target_path: root.into(),
        conflict,
        status: "queued".into(),
        bytes_done: 0,
        bytes_total: 0,
        current_file: None,
        current_bytes: 0,
        committed_bytes: 0,
        waiting_reason: None,
        files_done: 0,
        files_total: 0,
        skipped: 0,
        error: None,
        warnings: vec![],
        cleanup_pending: false,
        items: vec![],
        direct_address: None,
        temporary_paths: vec![],
        can_resume: false,
    }
}

#[test]
fn task_metadata_mixed_tasks_follow_attempt_order() {
    let mut older = view("/older", ConflictPolicy::Rename);
    older.presentation = model::TaskPresentation::new(1, vec!["/old".into()]);
    older.status = "running".into();
    let mut newer = view("/newer", ConflictPolicy::Rename);
    newer.kind = "download".into();
    newer.presentation = model::TaskPresentation::new(2, vec!["/new".into()]);
    newer.presentation.last_started_at = Some(1);
    newer.status = "completed".into();
    let newer_id = newer.id.clone();
    let mut tasks = vec![older, newer];
    model::sort_tasks(&mut tasks);
    assert_eq!(tasks[0].id, newer_id);
    let task = &mut tasks[1];
    task.presentation.restart(
        3,
        task.run_id.clone(),
        task.status.clone(),
        Some("previous failure".into()),
    );
    let retried_id = task.id.clone();
    model::sort_tasks(&mut tasks);
    assert_eq!(tasks[0].id, retried_id);
    assert_eq!(
        tasks[0]
            .presentation
            .last_attempt
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("previous failure")
    );
}

#[test]
fn task_metadata_survives_storage_and_seeds_next_order() {
    let directory = tempfile::tempdir().unwrap();
    let store = task_store::TaskStore::open(&directory.path().join("tasks-v3.sqlite")).unwrap();
    let mut task = view("/data", ConflictPolicy::Rename);
    task.status = "completed".into();
    task.presentation = model::TaskPresentation::new(40, vec!["/a\nb".into()]);
    task.presentation.restart(
        41,
        "previous-run".into(),
        "failed".into(),
        Some("断开连接".into()),
    );
    let created = task.presentation.created_at;
    let receipt = Receipt {
        view: Some(task),
        ..Receipt::default()
    };
    store.save(&receipt).unwrap();
    let state = TransferState::open(directory.path().to_path_buf()).unwrap();
    assert_eq!(state.next_order(), 42);
    let saved = state.recovered.lock().unwrap()[0].view.clone().unwrap();
    assert_eq!(saved.presentation.created_at, created);
    assert_eq!(saved.presentation.source_paths, vec!["/a\nb"]);
    assert_eq!(
        saved.presentation.last_attempt.unwrap().run_id,
        "previous-run"
    );
}

#[test]
fn task_metadata_old_receipts_have_no_invented_timestamp() {
    let mut value = serde_json::to_value(view("/old", ConflictPolicy::Rename)).unwrap();
    for key in [
        "createdAt",
        "lastStartedAt",
        "sortOrder",
        "sourcePaths",
        "lastAttempt",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    let restored: TaskView = serde_json::from_value(value).unwrap();
    assert_eq!(restored.presentation.sort_order, 0);
    assert_eq!(restored.presentation.created_at, None);
    assert_eq!(restored.presentation.last_started_at, None);
    assert!(restored.presentation.last_attempt.is_none());
}

impl Fixture {
    fn internal(
        &self,
        relative: &str,
        destination: &str,
        intent: Intent,
        policy: ConflictPolicy,
    ) -> WriteHandle {
        let item = self.item(
            Source::Remote {
                context: self.b.clone(),
                paths: vec![self.path(relative)],
            },
            policy,
        );
        item.task.lock().unwrap().target_path = destination.into();
        item.task.lock().unwrap().kind = if intent == Intent::Move {
            "serverMove"
        } else {
            "serverCopy"
        }
        .into();
        {
            let mut r = item.receipt.lock().unwrap();
            r.intent = intent;
            r.source_identity = self.b.identity().into();
            r.source_server_id = "b".into();
            r.source_paths = vec![self.path(relative)];
        }
        *item.auth.lock().unwrap() = None;
        item
    }
}
#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_copy_move_and_duplicate() {
    let f = Fixture::new();
    f.mkdir(&f.b, "source");
    f.mkdir(&f.b, "source/empty");
    f.mkdir(&f.b, "dest");
    f.put(&f.b, "source/.hidden", b"hello");
    f.put(&f.b, "source/quote'\n文件", b"content");
    f.exec(
        &f.b,
        &format!("ln -s -- . {}", crate::ssh::quote(&f.path("source/link"))),
    );
    let copy = f.internal(
        "source",
        &f.path("dest"),
        Intent::Copy,
        ConflictPolicy::Rename,
    );
    f.run(&copy);
    assert_eq!(f.read("dest/source/.hidden"), b"hello");
    assert!(f.exists("dest/source/empty"));
    assert!(!f.exists("dest/source/link"));
    assert!(f.exists("source/.hidden"));
    let selected_link = f.internal(
        "source/link",
        &f.path("dest"),
        Intent::Copy,
        ConflictPolicy::Rename,
    );
    f.run(&selected_link);
    assert!(selected_link
        .plan
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .entries
        .is_empty());
    assert!(!f.exists("dest/link"));
    let duplicate = f.internal("source", &f.root, Intent::Copy, ConflictPolicy::Rename);
    f.run(&duplicate);
    let p = duplicate.plan.lock().unwrap().clone().unwrap();
    assert_ne!(
        p.entries[0].target.as_deref(),
        Some(f.path("source").as_str())
    );
    let before = f.exec(
        &f.b,
        &format!(
            "stat -c %i -- {}",
            crate::ssh::quote(&f.path("source/.hidden"))
        ),
    );
    let moved = f.internal(
        "source/.hidden",
        &f.path("dest"),
        Intent::Move,
        ConflictPolicy::Rename,
    );
    f.run(&moved);
    assert!(!f.exists("source/.hidden"));
    let after = f.exec(
        &f.b,
        &format!(
            "stat -c %i -- {}",
            crate::ssh::quote(&f.path("dest/.hidden"))
        ),
    );
    assert_eq!(before, after, "same-filesystem move must preserve inode");
    let descendant = f.internal(
        "source",
        &f.path("source/empty"),
        Intent::Copy,
        ConflictPolicy::Rename,
    );
    assert_eq!(
        pipeline::run(&connection::tests::state(), None, &descendant)
            .unwrap_err()
            .kind,
        "path"
    );
}
#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_move_cross_mount_and_partial_merge() {
    let f = Fixture::new();
    f.mkdir(&f.b, "source");
    f.mkdir(&f.b, "dest");
    f.mkdir(&f.b, "dest/source");
    f.put(&f.b, "source/keep", b"original");
    f.put(&f.b, "source/move", b"move");
    f.put(&f.b, "dest/source/keep", b"existing");
    let item = f.internal(
        "source",
        &f.path("dest"),
        Intent::Move,
        ConflictPolicy::Skip,
    );
    let state = connection::tests::state();
    assert_eq!(
        pipeline::run(&state, None, &item).unwrap_err().kind,
        "sourceRetained"
    );
    assert!(f.exists("source/keep"));
    assert!(!f.exists("source/move"));
    assert_eq!(f.read("dest/source/keep"), b"existing");
    assert_eq!(f.read("dest/source/move"), b"move");
    let mounted = format!("/home/ops/mounted/{}", uuid::Uuid::new_v4());
    f.exec(&f.b, &format!("mkdir -- {}", crate::ssh::quote(&mounted)));
    let cross = f.internal(
        "source/keep",
        &mounted,
        Intent::Move,
        ConflictPolicy::Rename,
    );
    f.run(&cross);
    assert!(!f.exists("source/keep"));
    assert_eq!(
        f.exec(
            &f.b,
            &format!("cat -- {}", crate::ssh::quote(&format!("{mounted}/keep")))
        ),
        "original"
    );
    let plan = cross.plan.lock().unwrap();
    assert!(plan
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .all(|e| e.outcome == "completed" && e.staged.as_deref() != Some(e.source.as_str())));
}

#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_removal_is_durable_and_retry_does_not_recopy() {
    let f = Fixture::new();
    f.mkdir(&f.b, "protected");
    f.put(&f.b, "protected/file", b"original");
    let mounted = format!("/home/ops/mounted/{}", uuid::Uuid::new_v4());
    f.exec(
        &f.b,
        &format!(
            "mkdir -- {}; chmod 555 -- {}",
            crate::ssh::quote(&mounted),
            crate::ssh::quote(&f.path("protected"))
        ),
    );
    let item = f.internal(
        "protected/file",
        &mounted,
        Intent::Move,
        ConflictPolicy::Rename,
    );
    let state = connection::tests::state();
    let dir = tempfile::tempdir().unwrap();
    *state.transfers.store.lock().unwrap() =
        Some(task_store::TaskStore::open(&dir.path().join("tasks-v3.sqlite")).unwrap());
    assert_eq!(
        pipeline::run(&state, None, &item).unwrap_err().kind,
        "sourceRetained"
    );
    let loaded = task_store::TaskStore::open(&dir.path().join("tasks-v3.sqlite"))
        .unwrap()
        .load()
        .unwrap()
        .remove(0);
    assert_eq!(loaded.plan.entries[0].outcome, "copied");
    assert!(loaded.plan.entries[0].removal_pending);
    assert!(f.exists("protected/file"));
    let inode = f.exec(
        &f.b,
        &format!(
            "stat -c %i -- {}",
            crate::ssh::quote(&format!("{mounted}/file"))
        ),
    );
    *item.plan.lock().unwrap() = Some(loaded.plan.clone());
    *item.receipt.lock().unwrap() = loaded;
    cleanup(&item).unwrap(); // Temporary cleanup must never remove the source.
    assert!(f.exists("protected/file"));
    f.exec(
        &f.b,
        &format!("chmod 755 -- {}", crate::ssh::quote(&f.path("protected"))),
    );
    pipeline::run(&state, None, &item).unwrap();
    assert!(!f.exists("protected/file"));
    assert_eq!(
        inode,
        f.exec(
            &f.b,
            &format!(
                "stat -c %i -- {}",
                crate::ssh::quote(&format!("{mounted}/file"))
            )
        )
    );
    assert_eq!(
        item.plan.lock().unwrap().as_ref().unwrap().entries[0].outcome,
        "completed"
    );
}

#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_changed_source_is_retained_after_target_commit() {
    let f = Fixture::new();
    f.mkdir(&f.b, "protected");
    f.put(&f.b, "protected/file", b"original");
    let mounted = format!("/home/ops/mounted/{}", uuid::Uuid::new_v4());
    f.exec(
        &f.b,
        &format!(
            "mkdir -- {}; chmod 555 -- {}",
            crate::ssh::quote(&mounted),
            crate::ssh::quote(&f.path("protected"))
        ),
    );
    let item = f.internal(
        "protected/file",
        &mounted,
        Intent::Move,
        ConflictPolicy::Rename,
    );
    let state = connection::tests::state();
    assert!(pipeline::run(&state, None, &item).is_err());
    f.exec(
        &f.b,
        &format!("chmod 755 -- {}", crate::ssh::quote(&f.path("protected"))),
    );
    f.put(&f.b, "protected/file", b"changed!");
    assert_eq!(
        pipeline::run(&state, None, &item).unwrap_err().kind,
        "sourceRetained"
    );
    assert_eq!(f.read("protected/file"), b"changed!");
    assert_eq!(
        f.exec(
            &f.b,
            &format!("cat -- {}", crate::ssh::quote(&format!("{mounted}/file")))
        ),
        "original"
    );
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_directory_rename_recovers_lost_reply() {
    let f = Fixture::new();
    f.mkdir(&f.b, "tree");
    f.mkdir(&f.b, "tree/empty");
    f.mkdir(&f.b, "dest");
    f.put(&f.b, "tree/file", b"data");
    let before = f.exec(
        &f.b,
        &format!("stat -c %i -- {}", crate::ssh::quote(&f.path("tree"))),
    );
    let item = f.internal(
        "tree",
        &f.path("dest"),
        Intent::Move,
        ConflictPolicy::Rename,
    );
    let state = connection::tests::state();
    pipeline::run(&state, None, &item).unwrap();
    assert!(!f.exists("tree"));
    assert!(item.receipt.lock().unwrap().attempts.is_empty());
    assert_eq!(
        before,
        f.exec(
            &f.b,
            &format!("stat -c %i -- {}", crate::ssh::quote(&f.path("dest/tree")))
        )
    );
    // Simulate the durable pre-rename journal surviving but the response/save being lost.
    for entry in &mut item.plan.lock().unwrap().as_mut().unwrap().entries {
        entry.outcome = "pending".into();
    }
    item.receipt.lock().unwrap().pending_commit = Some(model::PendingCommit {
        rename_directory: true,
        destination: f.path("dest/tree"),
        stage_path: Some(f.path("tree")),
        expected_size: None,
        run_id: item.task.lock().unwrap().run_id.clone(),
        entry: 0,
        digest: None,
        directory: true,
    });
    recovery::reconcile(&state, &item).unwrap();
    assert!(item
        .plan
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .all(|e| e.outcome == "completed"));
    assert!(item.receipt.lock().unwrap().pending_commit.is_none());
}

#[test]
#[ignore = "isolated OpenSSH fixture"]
fn rsync_internal_cancel_keeps_source_and_reuses_only_stopped_staging() {
    let f = Fixture::new();
    f.mkdir(&f.b, "dest");
    f.exec(
        &f.b,
        &format!(
            "dd if=/dev/zero of={} bs=1M count=32 status=none",
            crate::ssh::quote(&f.path("large"))
        ),
    );
    let item = f.internal(
        "large",
        &f.path("dest"),
        Intent::Copy,
        ConflictPolicy::Rename,
    );
    item.receipt.lock().unwrap().verify_sha256 = false;
    let state = connection::tests::state();
    let worker_item = item.clone();
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || pipeline::run(&worker_state, None, &worker_item));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !item
        .receipt
        .lock()
        .unwrap()
        .attempts
        .iter()
        .any(|a| a.dispatched)
    {
        assert!(!worker.is_finished());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    item.request_cancel();
    assert!(worker.join().unwrap().is_err());
    assert!(f.exists("large"));
    assert!(!f.exists("dest/large"));
    item.stop.store(false, Ordering::SeqCst);
    item.cancel.reset().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut a = item
            .receipt
            .lock()
            .unwrap()
            .attempts
            .last()
            .unwrap()
            .clone();
        same_server::refresh(&item, &mut a).unwrap();
        let stopped = a.source_finished && a.receiver_finished;
        *item.receipt.lock().unwrap().attempts.last_mut().unwrap() = a;
        if stopped {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
    let root = item
        .receipt
        .lock()
        .unwrap()
        .attempts
        .last()
        .unwrap()
        .target_root
        .clone();
    pipeline::run(&state, None, &item).unwrap();
    assert_eq!(
        item.receipt
            .lock()
            .unwrap()
            .attempts
            .last()
            .unwrap()
            .target_root,
        root
    );
    assert!(f.exists("large"));
    assert_eq!(
        f.exec(
            &f.b,
            &format!("stat -c %s -- {}", crate::ssh::quote(&f.path("dest/large")))
        )
        .trim(),
        "33554432"
    );
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_password_and_special_paths() {
    let f = Fixture::new();
    f.mkdir(&f.a, "reports");
    f.mkdir(&f.a, "reports/empty");
    for name in [
        ".hidden",
        "中文 空格.txt",
        "a'b.txt",
        "a\\b.txt",
        "line\nbreak.txt",
        "trailing\n",
        "-leading",
        "$(not-a-command)",
    ] {
        f.put(
            &f.a,
            &format!("reports/{name}"),
            format!("bytes:{name}\0").as_bytes(),
        );
    }
    let item = f.remote("reports", ConflictPolicy::Rename);
    f.run(&item);
    assert!(f.exists("reports/empty"));
    for name in [
        ".hidden",
        "中文 空格.txt",
        "a'b.txt",
        "a\\b.txt",
        "line\nbreak.txt",
        "trailing\n",
        "-leading",
        "$(not-a-command)",
    ] {
        assert_eq!(
            f.read(&format!("reports/{name}")),
            format!("bytes:{name}\0").as_bytes()
        );
    }
    assert_eq!(item.task.lock().unwrap().files_done, 8);
    f.run(&f.remote("reports", ConflictPolicy::Rename));
    assert!(f.exists("reports (1)/empty"));
    f.put(&f.b, "reports/extra", b"keep");
    f.put(&f.a, "reports/.hidden", b"updated");
    f.run(&f.remote("reports", ConflictPolicy::Overwrite));
    assert_eq!(f.read("reports/.hidden"), b"updated");
    assert_eq!(f.read("reports/extra"), b"keep");
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_upload_nested_directories_and_skip() {
    let f = Fixture::new();
    let local = tempfile::tempdir().unwrap();
    let folder = local.path().join("nested");
    fs::create_dir_all(folder.join("child/empty")).unwrap();
    fs::write(folder.join("child/file"), b"binary\0\xff").unwrap();
    f.run(&f.item(Source::Local(vec![folder.clone()]), ConflictPolicy::Rename));
    assert_eq!(f.read("nested/child/file"), b"binary\0\xff");
    assert!(f.exists("nested/child/empty"));
    fs::write(folder.join("child/file"), b"other").unwrap();
    f.run(&f.item(Source::Local(vec![folder.clone()]), ConflictPolicy::Skip));
    assert_eq!(f.read("nested/child/file"), b"binary\0\xff");
    f.run(&f.item(Source::Local(vec![folder]), ConflictPolicy::Overwrite));
    assert_eq!(f.read("nested/child/file"), b"other");
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_bad_auth_preserves_original_and_does_not_block_upload() {
    let f = Fixture::new();
    let state = connection::tests::state();
    f.put(&f.a, "file", b"new");
    f.put(&f.b, "file", b"original");
    let direct = f.remote("file", ConflictPolicy::Overwrite);
    *direct.auth.lock().unwrap() = Some(model::DirectAuth::Password("wrong-secret".into()));
    let err = pipeline::run(&state, None, &direct).unwrap_err();
    assert!(!err.message.contains("wrong-secret"));
    assert_eq!(f.read("file"), b"original");
    state.transfers.writes.lock().unwrap().push(direct.clone());
    let local = tempfile::tempdir().unwrap();
    let path = local.path().join("upload");
    fs::write(&path, b"independent").unwrap();
    let upload = f.item(Source::Local(vec![path]), ConflictPolicy::Rename);
    pipeline::run(&state, None, &upload).unwrap();
    assert_eq!(f.read("upload"), b"independent");
    cleanup(&upload).unwrap();
    cleanup(&direct).unwrap();
}

#[test]
fn task_store_recovers_plan_without_secrets_or_old_quarantine() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("old.json"), b"{bad legacy receipt").unwrap();
    let store = task_store::TaskStore::open(&dir.path().join("tasks.sqlite")).unwrap();
    let receipt = Receipt {
        view: Some(view("/out", ConflictPolicy::Rename)),
        ..Receipt::default()
    };
    store.save(&receipt).unwrap();
    let rows = store.load().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].view.as_ref().unwrap().status, "interrupted");
    assert!(TransferState::open(dir.path().join("new"))
        .unwrap()
        .recovered
        .lock()
        .unwrap()
        .is_empty());
    assert!(dir.path().join("old.json").exists());
}
#[test]
fn locks_compare_path_components() {
    assert!(scheduler::overlaps("/a", "/a/b"));
    assert!(scheduler::overlaps("/a/b", "/a"));
    assert!(!scheduler::overlaps("/a", "/ab"));
    assert!(scheduler::overlaps("/", "/a"));
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_key_auth_and_special_destination() {
    let f = Fixture::new();
    let key = f.path("key");
    let public = f.exec(
        &f.a,
        &format!(
            "ssh-keygen -q -t ed25519 -N '' -f {} && cat {}",
            crate::ssh::quote(&key),
            crate::ssh::quote(&format!("{key}.pub"))
        ),
    );
    f.exec(
        &f.b,
        &format!(
            "printf '%s\\n' {} >> /home/ops/.ssh/authorized_keys",
            crate::ssh::quote(&public)
        ),
    );
    f.put(&f.a, "file", b"key transport");
    f.mkdir(&f.b, "中文 空间'\\\nline");
    let item = f.remote("file", ConflictPolicy::Rename);
    item.task.lock().unwrap().target_path = f.path("中文 空间'\\\nline");
    *item.auth.lock().unwrap() = Some(model::DirectAuth::Key(key));
    f.run(&item);
    assert_eq!(f.read("中文 空间'\\\nline/file"), b"key transport");
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_failure_drains_diagnostics_preserves_original_and_retries() {
    let f = Fixture::new();
    let state = connection::tests::state();
    f.put(&f.a, "file", b"replacement");
    f.put(&f.b, "file", b"original");
    f.mode(&f.b, "fail");
    let item = f.remote("file", ConflictPolicy::Overwrite);
    let result = pipeline::run(&state, None, &item);
    f.mode(&f.b, "");
    let err = result.unwrap_err();
    assert!(err.message.contains("fixture disk full"), "{}", err.message);
    assert!(err.message.len() < 34000);
    assert_eq!(f.read("file"), b"original");
    f.run(&item);
    assert_eq!(f.read("file"), b"replacement");
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_cancel_confirms_stop_and_resumes_without_committing_partial_data() {
    let f = Fixture::new();
    let state = connection::tests::state();
    let content = vec![17u8; 2 * 1024 * 1024];
    f.put(&f.a, "large", &content);
    f.put(&f.b, "large", b"original");
    f.mode(&f.a, "slow");
    let item = f.remote("large", ConflictPolicy::Overwrite);
    let worker_item = item.clone();
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || pipeline::run(&worker_state, None, &worker_item));
    let deadline = Instant::now() + Duration::from_secs(45);
    while item.task.lock().unwrap().bytes_done == 0 {
        assert!(!worker.is_finished(), "transfer ended before cancellation");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    item.request_cancel();
    let result = worker.join().unwrap();
    f.mode(&f.a, "");
    assert!(result.is_err());
    assert_eq!(f.read("large"), b"original");
    item.stop.store(false, Ordering::SeqCst);
    item.cancel.reset().unwrap();
    let deadline = Instant::now() + Duration::from_secs(70);
    loop {
        let mut a = item
            .receipt
            .lock()
            .unwrap()
            .attempts
            .last()
            .unwrap()
            .clone();
        direct::refresh_attempt(&item, &mut a).unwrap();
        let stopped = a.source_finished && a.receiver_finished;
        *item.receipt.lock().unwrap().attempts.last_mut().unwrap() = a;
        if stopped {
            break;
        }
        assert!(Instant::now() < deadline, "exit records never arrived");
        std::thread::sleep(Duration::from_millis(250));
    }
    let root = item
        .receipt
        .lock()
        .unwrap()
        .attempts
        .last()
        .unwrap()
        .target_root
        .clone();
    pipeline::run(&state, None, &item).unwrap();
    assert_eq!(f.read("large"), content);
    assert_eq!(
        item.receipt
            .lock()
            .unwrap()
            .attempts
            .last()
            .unwrap()
            .target_root,
        root
    );
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_pending_commit_blocks_only_its_path_and_digest_reconciles() {
    let f = Fixture::new();
    let state = connection::tests::state();
    f.put(&f.a, "file", b"new content");
    let item = f.remote("file", ConflictPolicy::Rename);
    pipeline::run(&state, None, &item).unwrap();
    let mut plan = item.plan.lock().unwrap().clone().unwrap();
    let e = plan.entries[0].clone();
    plan.entries[0].outcome = "pending".into();
    state.transfers.save_plan(&item, &plan).unwrap();
    item.receipt.lock().unwrap().pending_commit = Some(model::PendingCommit {
        rename_directory: false,
        destination: e.target.unwrap(),
        stage_path: e.staged,
        expected_size: Some(e.size),
        run_id: "lost-reply".into(),
        entry: 0,
        digest: e.digest,
        directory: false,
    });
    item.task.lock().unwrap().status = "needsAttention".into();
    state.transfers.writes.lock().unwrap().push(item.clone());
    let other = f.remote("file", ConflictPolicy::Overwrite);
    assert!(state
        .transfers
        .conflicting_commit(&other, &f.path("file"))
        .is_some());
    assert!(state
        .transfers
        .conflicting_commit(&other, &f.path("other"))
        .is_none());
    recovery::reconcile(&state, &item).unwrap();
    assert!(item.receipt.lock().unwrap().pending_commit.is_none());
    assert_eq!(item.task.lock().unwrap().status, "completed");
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_skip_type_conflict_skips_entire_subtree() {
    let f = Fixture::new();
    f.mkdir(&f.a, "dir");
    f.put(&f.a, "dir/file", b"child");
    f.put(&f.b, "dir", b"preserve");
    let item = f.remote("dir", ConflictPolicy::Skip);
    f.run(&item);
    assert_eq!(f.read("dir"), b"preserve");
    assert!(!f.exists("file"));
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_rejects_target_changes_during_transfer() {
    let f = Fixture::new();
    let state = connection::tests::state();
    f.put(&f.a, "file", &vec![99u8; 1024 * 1024]);
    f.put(&f.b, "file", b"original");
    f.mode(&f.a, "slow");
    let item = f.remote("file", ConflictPolicy::Overwrite);
    let copy = item.clone();
    let shared = state.clone();
    let worker = std::thread::spawn(move || pipeline::run(&shared, None, &copy));
    let deadline = Instant::now() + Duration::from_secs(30);
    while item.task.lock().unwrap().bytes_done == 0 {
        assert!(Instant::now() < deadline);
        assert!(!worker.is_finished());
        std::thread::sleep(Duration::from_millis(30));
    }
    f.put(&f.b, "file", b"external modification");
    let result = worker.join().unwrap();
    f.mode(&f.a, "");
    assert_eq!(result.unwrap_err().kind, "conflict");
    assert_eq!(f.read("file"), b"external modification");
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_merge_mount_stages_on_destination_filesystem() {
    let f = Fixture::new();
    f.mkdir(&f.a, "mounted");
    let name = uuid::Uuid::new_v4().to_string();
    f.put(&f.a, &format!("mounted/{name}"), b"mount data");
    let item = f.remote("mounted", ConflictPolicy::Overwrite);
    item.task.lock().unwrap().target_path = "/home/ops".into();
    let state = connection::tests::state();
    pipeline::run(&state, None, &item).unwrap();
    assert!(item.receipt.lock().unwrap().attempts[0]
        .target_root
        .starts_with("/home/ops/mounted/"));
    assert_eq!(
        f.exec(&f.b, &format!("cat /home/ops/mounted/{name}")),
        "mount data"
    );
    cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_store_restart_retains_completed_items_and_cleanup_independently() {
    let f = Fixture::new();
    let state = connection::tests::state();
    let dir = tempfile::tempdir().unwrap();
    *state.transfers.store.lock().unwrap() =
        Some(task_store::TaskStore::open(&dir.path().join("tasks-v3.sqlite")).unwrap());
    f.put(&f.a, "file", b"persisted");
    let item = f.remote("file", ConflictPolicy::Rename);
    pipeline::run(&state, None, &item).unwrap();
    item.task.lock().unwrap().status = "completed".into();
    state.transfers.persist(&item).unwrap();
    let recovered = TransferState::open(dir.path().into()).unwrap();
    let rows = recovered.recovered.lock().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].view.as_ref().unwrap().status, "completed");
    assert!(rows[0].view.as_ref().unwrap().cleanup_pending);
    assert_eq!(rows[0].plan.entries[0].outcome, "completed");
    assert!(!serde_json::to_string(&rows[0])
        .unwrap()
        .contains("fixture-password"));
    drop(rows);
    let id = item.task.lock().unwrap().id.clone();
    let prior = item.receipt.lock().unwrap().attempts.len();
    pipeline::run(&state, None, &item).unwrap();
    assert_eq!(item.receipt.lock().unwrap().attempts.len(), prior);
    assert!(!f.exists("file (1)"));
    cleanup(&item).unwrap();
    state.transfers.persist(&item).unwrap();
    let saved = state
        .transfers
        .store
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(saved[0].view.as_ref().unwrap().id, id);
    assert!(!saved[0].has_remote_resources());
}
