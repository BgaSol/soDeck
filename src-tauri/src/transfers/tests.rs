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
    let f=Fixture::new();let state=connection::tests::state();
    f.put(&f.a,"file",&vec![99u8;1024*1024]);f.put(&f.b,"file",b"original");
    f.mode(&f.a,"slow");let item=f.remote("file",ConflictPolicy::Overwrite);
    let copy=item.clone();let shared=state.clone();let worker=std::thread::spawn(move||pipeline::run(&shared,None,&copy));
    let deadline=Instant::now()+Duration::from_secs(30);
    while item.task.lock().unwrap().bytes_done==0 {assert!(Instant::now()<deadline);assert!(!worker.is_finished());std::thread::sleep(Duration::from_millis(30));}
    f.put(&f.b,"file",b"external modification");let result=worker.join().unwrap();f.mode(&f.a,"");
    assert_eq!(result.unwrap_err().kind,"conflict");assert_eq!(f.read("file"),b"external modification");cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_merge_mount_stages_on_destination_filesystem() {
    let f=Fixture::new();f.mkdir(&f.a,"mounted");
    let name=uuid::Uuid::new_v4().to_string();f.put(&f.a,&format!("mounted/{name}"),b"mount data");
    let item=f.remote("mounted",ConflictPolicy::Overwrite);item.task.lock().unwrap().target_path="/home/ops".into();
    let state=connection::tests::state();pipeline::run(&state,None,&item).unwrap();
    assert!(item.receipt.lock().unwrap().attempts[0].target_root.starts_with("/home/ops/mounted/"));
    assert_eq!(f.exec(&f.b,&format!("cat /home/ops/mounted/{name}")),"mount data");cleanup(&item).unwrap();
}

#[test]
#[ignore = "isolated OpenSSH/rsync containers"]
fn rsync_store_restart_retains_completed_items_and_cleanup_independently() {
    let f=Fixture::new();let state=connection::tests::state();let dir=tempfile::tempdir().unwrap();
    *state.transfers.store.lock().unwrap()=Some(task_store::TaskStore::open(&dir.path().join("tasks-v3.sqlite")).unwrap());
    f.put(&f.a,"file",b"persisted");let item=f.remote("file",ConflictPolicy::Rename);
    pipeline::run(&state,None,&item).unwrap();item.task.lock().unwrap().status="completed".into();state.transfers.persist(&item).unwrap();
    let recovered=TransferState::open(dir.path().into()).unwrap();let rows=recovered.recovered.lock().unwrap();
    assert_eq!(rows.len(),1);assert_eq!(rows[0].view.as_ref().unwrap().status,"completed");assert!(rows[0].view.as_ref().unwrap().cleanup_pending);
    assert_eq!(rows[0].plan.entries[0].outcome,"completed");assert!(!serde_json::to_string(&rows[0]).unwrap().contains("fixture-password"));drop(rows);
    let id=item.task.lock().unwrap().id.clone();let prior=item.receipt.lock().unwrap().attempts.len();
    pipeline::run(&state,None,&item).unwrap();assert_eq!(item.receipt.lock().unwrap().attempts.len(),prior);assert!(!f.exists("file (1)"));
    cleanup(&item).unwrap();state.transfers.persist(&item).unwrap();
    let saved=state.transfers.store.lock().unwrap().as_ref().unwrap().load().unwrap();assert_eq!(saved[0].view.as_ref().unwrap().id,id);assert!(!saved[0].has_remote_resources());
}
