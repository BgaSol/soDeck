use super::*;
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
};

struct Fixture {
    root: tempfile::TempDir,
    child: Child,
    a: SharedContext,
    b: SharedContext,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let ports: Vec<_> = (0..2)
            .map(|_| {
                TcpListener::bind(("127.0.0.1", 0))
                    .unwrap()
                    .local_addr()
                    .unwrap()
                    .port()
            })
            .collect();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let python = std::env::var_os("SSH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or(repo.join(".qa/transfer-venv/bin/python"));
        let mut child = Command::new(python)
            .arg(repo.join("tests/transfer_fixture.py"))
            .arg("--root")
            .arg(&path)
            .arg("--port-a")
            .arg(ports[0].to_string())
            .arg("--port-b")
            .arg(ports[1].to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !path.join("a-ready").exists() || !path.join("b-ready").exists() {
            assert!(child.try_wait().unwrap().is_none(), "fixture exited");
            assert!(started.elapsed() < Duration::from_secs(15));
            thread::sleep(Duration::from_millis(30));
        }
        Self {
            root,
            child,
            a: crate::connection::tests::transfer_context("a", ports[0]),
            b: crate::connection::tests::transfer_context("b", ports[1]),
        }
    }
    fn path(&self, path: &str) -> PathBuf {
        fs::canonicalize(self.root.path()).unwrap().join(path)
    }
    fn item(&self, source: Source, policy: ConflictPolicy) -> WriteHandle {
        let mut receipt = Receipt {
            target_server_id: "b".into(),
            target_identity: self.b.identity().into(),
            ..Receipt::default()
        };
        let endpoint = if let Source::Remote { context, .. } = &source {
            receipt.source_identity = context.identity().into();
            receipt.source_server_id = "a".into();
            Some(DirectEndpoint {
                host: "127.0.0.1".into(),
                port: self.b.target().port,
                username: "ops".into(),
                public_key: self.b.target_public_key().into(),
            })
        } else {
            None
        };
        WriteHandle {
            lifecycle: Arc::new(Mutex::new(())),
            source,
            target: self.b.clone(),
            endpoint,
            task: Arc::new(Mutex::new(TaskView {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: uuid::Uuid::new_v4().to_string(),
                kind: "direct".into(),
                source_name: "A".into(),
                source_path: String::new(),
                target_name: "B".into(),
                target_path: self.path("b").display().to_string(),
                conflict: policy,
                status: "queued".into(),
                bytes_done: 0,
                bytes_total: 0,
                files_done: 0,
                files_total: 0,
                skipped: 0,
                error: None,
                warnings: vec![],
                cleanup_pending: false,
            })),
            cancel: Cancellation::default(),
            stop: Arc::new(AtomicBool::new(false)),
            plan: Arc::new(Mutex::new(None)),
            receipt: Arc::new(Mutex::new(receipt)),
        }
    }
    fn remote(&self, name: &str, policy: ConflictPolicy) -> WriteHandle {
        self.item(
            Source::Remote {
                context: self.a.clone(),
                paths: vec![self.path(&format!("a/{name}")).display().to_string()],
            },
            policy,
        )
    }
    fn no_temporary_files(&self) {
        for side in ["a", "b"] {
            for path in fs::read_dir(self.path(side)).unwrap() {
                assert!(
                    !path
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ssh-workspace-"),
                    "orphan task directory"
                );
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn direct_copy_content_conflicts_and_no_client_payload() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let dir = fixture.path("a/reports");
    fs::create_dir_all(dir.join("nested/empty")).unwrap();
    for name in [
        ".hidden",
        "中文 空格.txt",
        "a'b.txt",
        "a\\b.txt",
        "line\nbreak.txt",
        "$(not-a-command)",
    ] {
        fs::write(dir.join(name), format!("contents:{name}")).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&dir, dir.join("link")).unwrap();
    let item = fixture.remote("reports", ConflictPolicy::Rename);
    upload::run(&state, None, &item).unwrap();
    assert!(fixture.path("b/reports/nested/empty").is_dir());
    for name in [
        ".hidden",
        "中文 空格.txt",
        "a'b.txt",
        "a\\b.txt",
        "line\nbreak.txt",
        "$(not-a-command)",
    ] {
        assert_eq!(
            fs::read(dir.join(name)).unwrap(),
            fs::read(fixture.path("b/reports").join(name)).unwrap()
        );
    }
    assert!(!fixture.path("b/reports/link").exists());
    assert_eq!(item.task.lock().unwrap().skipped, 1);
    upload::run(
        &state,
        None,
        &fixture.remote("reports", ConflictPolicy::Rename),
    )
    .unwrap();
    assert!(fixture.path("b/reports (1)/nested/empty").is_dir());
    fs::write(fixture.path("b/reports/extra"), b"preserve").unwrap();
    fs::write(dir.join(".hidden"), b"updated").unwrap();
    upload::run(
        &state,
        None,
        &fixture.remote("reports", ConflictPolicy::Overwrite),
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.path("b/reports/.hidden")).unwrap(),
        b"updated"
    );
    assert_eq!(
        fs::read(fixture.path("b/reports/extra")).unwrap(),
        b"preserve"
    );
    let logs = fs::read_to_string(fixture.path("events.jsonl")).unwrap();
    for line in logs.lines() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(
            !(event["event"] == "read"
                && event["side"] == "a"
                && event["path"].as_str().unwrap_or("").contains("/reports/")),
            "client relayed file contents: {line}"
        );
    }
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn direct_failures_do_not_relay_or_replace_existing_files() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    fs::write(fixture.path("a/file"), b"new").unwrap();
    fs::write(fixture.path("b/file"), b"original").unwrap();
    fs::write(fixture.path("deny-direct"), b"deny").unwrap();
    let item = fixture.remote("file", ConflictPolicy::Overwrite);
    assert!(upload::run(&state, None, &item)
        .unwrap_err()
        .message
        .contains("A cannot reach B"));
    assert_eq!(fs::read(fixture.path("b/file")).unwrap(), b"original");
    fs::remove_file(fixture.path("deny-direct")).unwrap();
    let mut item = fixture.remote("file", ConflictPolicy::Overwrite);
    item.endpoint.as_mut().unwrap().public_key = fixture.a.target_public_key().into();
    assert!(upload::run(&state, None, &item)
        .unwrap_err()
        .message
        .to_lowercase()
        .contains("host key"));
    let mut item = fixture.remote("file", ConflictPolicy::Overwrite);
    item.endpoint.as_mut().unwrap().username = "not-authorized".into();
    assert!(upload::run(&state, None, &item).is_err());
    assert_eq!(fs::read(fixture.path("b/file")).unwrap(), b"original");
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn direct_cancel_stops_remote_writer_before_cleanup_and_retry() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    fs::write(fixture.path("a/large.bin"), vec![77; 8 * 1024 * 1024]).unwrap();
    fs::write(fixture.path("delay-write"), "0.03").unwrap();
    let item = fixture.remote("large.bin", ConflictPolicy::Rename);
    let worker_item = item.clone();
    let worker_state = state.clone();
    let worker = thread::spawn(move || upload::run(&worker_state, None, &worker_item));
    let started = Instant::now();
    while !fs::read_to_string(fixture.path("events.jsonl"))
        .unwrap_or_default()
        .contains("\"bytes\": 32768")
    {
        assert!(started.elapsed() < Duration::from_secs(20));
        thread::sleep(Duration::from_millis(30));
    }
    item.request_cancel();
    let error = worker.join().unwrap().unwrap_err();
    assert_eq!(error.kind, "cancelled", "{}", error.message);
    fixture.no_temporary_files();
    assert!(!fixture.path("b/large.bin").exists());
    fs::remove_file(fixture.path("delay-write")).unwrap();
    item.stop.store(false, Ordering::SeqCst);
    item.cancel.reset().unwrap();
    item.task.lock().unwrap().run_id = uuid::Uuid::new_v4().to_string();
    upload::run(&state, None, &item).unwrap();
    assert_eq!(
        fs::read(fixture.path("b/large.bin")).unwrap(),
        fs::read(fixture.path("a/large.bin")).unwrap()
    );
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SFTP endpoint; requires Python paramiko"]
fn upload_preserves_empty_directories_and_conflicts() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let local = fixture.path("local");
    fs::create_dir_all(local.join("empty")).unwrap();
    fs::write(local.join("file"), b"binary\0\xff").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(local.join("file"), local.join("link")).unwrap();
    let item = fixture.item(Source::Local(vec![local.clone()]), ConflictPolicy::Rename);
    upload::run(&state, None, &item).unwrap();
    assert_eq!(
        fs::read(fixture.path("b/local/file")).unwrap(),
        b"binary\0\xff"
    );
    assert!(fixture.path("b/local/empty").is_dir());
    assert!(!fixture.path("b/local/link").exists());
    fs::write(local.join("file"), b"other").unwrap();
    upload::run(
        &state,
        None,
        &fixture.item(Source::Local(vec![local]), ConflictPolicy::Skip),
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.path("b/local/file")).unwrap(),
        b"binary\0\xff"
    );
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn lost_control_stops_remote_writer_and_requires_reconciliation() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    fs::write(fixture.path("a/large.bin"), vec![55; 8 * 1024 * 1024]).unwrap();
    fs::write(fixture.path("delay-write"), "0.03").unwrap();
    let item = fixture.remote("large.bin", ConflictPolicy::Rename);
    let worker_item = item.clone();
    let worker_state = state.clone();
    let worker = thread::spawn(move || upload::run(&worker_state, None, &worker_item));
    let started = Instant::now();
    while !fs::read_to_string(fixture.path("events.jsonl"))
        .unwrap_or_default()
        .contains("\"bytes\": 32768")
    {
        assert!(started.elapsed() < Duration::from_secs(15));
        thread::sleep(Duration::from_millis(30));
    }
    item.cancel.cancel();
    assert_eq!(worker.join().unwrap().unwrap_err().kind, "unknown");
    assert!(!item.receipt.lock().unwrap().remote_root.is_empty());
    let started = Instant::now();
    while verify_stopped(&item).is_err() {
        assert!(started.elapsed() < Duration::from_secs(35));
        thread::sleep(Duration::from_millis(100));
    }
    cleanup(&item).unwrap();
    assert!(!fixture.path("b/large.bin").exists());
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn lost_publish_ack_records_exact_destination_without_retrying_commit() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let receipts = fixture.path("receipts");
    fs::create_dir(&receipts).unwrap();
    *state.transfers.receipts_path.lock().unwrap() = Some(receipts.clone());
    fs::write(fixture.path("a/file"), b"new content").unwrap();
    fs::write(fixture.path("b/file"), b"old content").unwrap();
    fs::write(fixture.path("drop-commit-ack"), b"").unwrap();
    let item = fixture.remote("file", ConflictPolicy::Overwrite);
    let error = upload::run(&state, None, &item).unwrap_err();
    assert_eq!(error.kind, "unknown", "{}", error.message);
    assert_eq!(
        item.receipt.lock().unwrap().pending_commit.as_deref(),
        Some(fixture.path("b/file").to_str().unwrap())
    );
    assert_eq!(fs::read(fixture.path("b/file")).unwrap(), b"new content");
    let recovered = TransferState::open(receipts).unwrap();
    let records = recovered.recovered.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].view.as_ref().unwrap().status, "unknown");
    assert_eq!(
        records[0].pending_commit,
        item.receipt.lock().unwrap().pending_commit
    );
    assert!(!serde_json::to_string(&records[0])
        .unwrap()
        .contains("fixture-password"));
    drop(records);
    verify_stopped(&item).unwrap();
    cleanup(&item).unwrap();
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn retry_reuses_completed_files_and_directory_names() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    fs::create_dir(fixture.path("a/reports")).unwrap();
    fs::create_dir(fixture.path("b/reports")).unwrap();
    fs::write(fixture.path("a/reports/first"), b"first").unwrap();
    fs::write(fixture.path("a/reports/second"), b"second").unwrap();
    fs::write(fixture.path("deny-after-first"), b"").unwrap();
    let item = fixture.remote("reports", ConflictPolicy::Rename);
    assert!(upload::run(&state, None, &item).is_err());
    assert_eq!(item.task.lock().unwrap().files_done, 1);
    fs::remove_file(fixture.path("deny-after-first")).unwrap();
    fs::remove_file(fixture.path("deny-direct")).unwrap();
    item.task.lock().unwrap().run_id = uuid::Uuid::new_v4().to_string();
    upload::run(&state, None, &item).unwrap();
    assert_eq!(
        fs::read(fixture.path("b/reports (1)/first")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(fixture.path("b/reports (1)/second")).unwrap(),
        b"second"
    );
    assert!(!fixture.path("b/reports (2)").exists());
    assert!(!fixture.path("b/reports (1)/first (1)").exists());
    assert_eq!(item.task.lock().unwrap().files_done, 2);
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SFTP endpoint; requires Python paramiko"]
fn create_remote_entries_never_truncates_an_existing_file() {
    use crate::remote_fs::{create_entry, EntryKind};
    let fixture = Fixture::new();
    let connection = connection::connect_verified(&fixture.b, &Cancellation::default()).unwrap();
    let sftp = connection.session().sftp().unwrap();
    let root = fixture.path("b").display().to_string();
    create_entry(&sftp, &root, "新 文件.txt", EntryKind::File).unwrap();
    assert_eq!(
        fs::metadata(fixture.path("b/新 文件.txt")).unwrap().len(),
        0
    );
    fs::write(fixture.path("b/新 文件.txt"), b"keep existing data").unwrap();
    assert_eq!(
        create_entry(&sftp, &root, "新 文件.txt", EntryKind::File)
            .unwrap_err()
            .kind,
        "conflict"
    );
    assert_eq!(
        fs::read(fixture.path("b/新 文件.txt")).unwrap(),
        b"keep existing data"
    );
    create_entry(&sftp, &root, "新目录", EntryKind::Directory).unwrap();
    assert!(fixture.path("b/新目录").is_dir());
    assert!(create_entry(&sftp, &root, "../escape", EntryKind::File).is_err());
}

#[test]
#[ignore = "real SSH endpoints and native OpenSSH scp; requires Python paramiko"]
fn direct_target_path_preserves_spaces_quotes_backslashes_and_newlines() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let target = fixture.path("b/中文 空间'\\\nline");
    fs::create_dir(&target).unwrap();
    fs::write(fixture.path("a/file"), b"exact bytes").unwrap();
    let item = fixture.remote("file", ConflictPolicy::Rename);
    item.task.lock().unwrap().target_path = target.display().to_string();
    upload::run(&state, None, &item).unwrap();
    assert_eq!(fs::read(target.join("file")).unwrap(), b"exact bytes");
    fixture.no_temporary_files();
}

#[test]
#[ignore = "waits through a real 30 second supervisor lease; requires Python paramiko"]
fn controller_keeps_lease_alive_between_file_commands() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let item = fixture.remote("file", ConflictPolicy::Rename);
    let mut controller = direct::Controller::start(&state, &item).unwrap();
    thread::sleep(Duration::from_secs(32));
    controller.finish(&item).unwrap();
    assert!(controller.confirmed_stopped());
    drop(controller);
    verify_stopped(&item).unwrap();
    cleanup(&item).unwrap();
    fixture.no_temporary_files();
}

#[test]
#[ignore = "real SFTP endpoint; requires Python paramiko"]
fn cleanup_failure_stops_new_writes_and_retry_preserves_committed_files() {
    let fixture = Fixture::new();
    let state = crate::connection::tests::state();
    let local = fixture.path("local");
    fs::create_dir(&local).unwrap();
    fs::write(local.join("first"), b"first").unwrap();
    fs::write(local.join("second"), b"second").unwrap();
    fs::write(fixture.path("deny-cleanup"), b"").unwrap();
    let item = fixture.item(Source::Local(vec![local]), ConflictPolicy::Rename);
    assert_eq!(
        upload::run(&state, None, &item).unwrap_err().kind,
        "cleanup"
    );
    assert_eq!(item.task.lock().unwrap().files_done, 1);
    assert_eq!(item.receipt.lock().unwrap().stage_dirs.len(), 1);
    assert!(!fixture.path("b/local/second").exists());
    fs::remove_file(fixture.path("deny-cleanup")).unwrap();
    cleanup(&item).unwrap();
    upload::run(&state, None, &item).unwrap();
    assert_eq!(fs::read(fixture.path("b/local/first")).unwrap(), b"first");
    assert_eq!(fs::read(fixture.path("b/local/second")).unwrap(), b"second");
    assert!(!fixture.path("b/local/first (1)").exists());
    fixture.no_temporary_files();
}
