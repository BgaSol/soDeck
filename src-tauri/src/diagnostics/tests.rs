use super::*;

#[test]
fn sensitive_error_text_never_enters_files_and_snapshot_is_portable() {
    let dir = tempfile::tempdir().unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    let secret = "password=SECRET123 host=192.0.2.22 /private/file";
    let error = crate::model::AppError::io("network", std::io::Error::from_raw_os_error(10061));
    let mut details = event::error_details(&error);
    details["message"] = json!(secret);
    details["password"] = json!(secret);
    service.record(Event {
        captured_at: now(),
        level: "ERROR",
        module: "ssh".into(),
        event: "connect.end".into(),
        trace_id: event::id(),
        span_id: event::id(),
        parent_span_id: None,
        source: "src/ssh.rs:12".into(),
        details: event::safe_details(details),
    });
    service.flush().unwrap();
    let snapshot = service.snapshot(snapshot::Scope::Current).unwrap();
    let zip = dir.path().join("report.zip");
    snapshot.zip(&zip, &service, vec![]).unwrap();
    let mut archive = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
    let mut found = false;
    for i in 0..archive.len() {
        use std::io::Read;
        let mut entry = archive.by_index(i).unwrap();
        let mut body = String::new();
        entry.read_to_string(&mut body).unwrap();
        assert!(!body.contains(secret));
        if entry.name().contains("events-") {
            assert!(body.contains("10061") || body.is_empty());
            found = true;
        }
    }
    assert!(found);
    service.finish();
}

#[test]
fn clear_does_not_touch_business_files_and_sequence_continues() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("servers.db"), "keep").unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    let event = Event {
        captured_at: now(),
        level: "INFO",
        module: "app".into(),
        event: "first".into(),
        trace_id: event::id(),
        span_id: event::id(),
        parent_span_id: None,
        source: String::new(),
        details: json!({}),
    };
    service.record(event.clone());
    service.flush().unwrap();
    service.clear().unwrap();
    service.record(Event {
        event: "second".into(),
        ..event
    });
    service.flush().unwrap();
    let _guard = store::lock(&service.root).unwrap();
    let run = store::runs(&service.root).unwrap().remove(0);
    assert_eq!(run.session.last_seq, 2);
    assert!(run.session.incomplete);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("servers.db")).unwrap(),
        "keep"
    );
    drop(_guard);
    service.finish();
}

#[test]
fn frontend_stack_preserves_bundle_positions_only() {
    let input="Error: password=SECRET\n at secret (http://tauri.localhost/assets/index-A1b2C3.js:12:8)\n at /Users/private/token.txt:1:2";
    assert_eq!(
        event::stack_frames(input),
        vec!["assets/index-A1b2C3.js:12:8"]
    );
    assert_eq!(
        event::safe_details(
            json!({"password":"secret","outcome":"ok","durationMs":12,"targetRef":"192.0.2.1"})
        ),
        json!({"outcome":"ok","durationMs":12})
    );
}

#[test]
fn simultaneous_instances_prevent_clear_and_have_separate_runs() {
    let dir = tempfile::tempdir().unwrap();
    let first = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    let second = Diagnostics::start(first.root.clone(), false).unwrap();
    assert_ne!(first.run_id, second.run_id);
    assert!(first.clear().is_err());
    first.finish();
    second.finish();
}

fn sample(name: &str) -> Event {
    Event {
        captured_at: now(),
        level: "INFO",
        module: "app".into(),
        event: name.into(),
        trace_id: event::id(),
        span_id: event::id(),
        parent_span_id: None,
        source: "src/diagnostics/tests.rs:1".into(),
        details: json!({}),
    }
}

#[test]
fn snapshot_is_frozen_and_every_manifest_hash_matches() {
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    service.record(sample("before_snapshot"));
    let snapshot = service.snapshot(snapshot::Scope::Current).unwrap();
    service.record(sample("after_snapshot"));
    service.flush().unwrap();
    let destination = dir.path().join("snapshot.zip");
    snapshot.zip(&destination, &service, vec![]).unwrap();
    let mut archive = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let mut found = false;
    for entry in manifest["files"].as_array().unwrap() {
        let mut bytes = vec![];
        archive
            .by_name(entry["name"].as_str().unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(entry["bytes"], bytes.len() as u64);
        assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("after_snapshot"));
        found |= text.contains("before_snapshot");
        if entry["name"].as_str().unwrap().ends_with("session.json") {
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["lastSeq"], 1);
        }
    }
    assert!(found);
    service.finish();
}

#[test]
fn cancelled_export_preserves_existing_destination_and_cleans_staging() {
    let dir = tempfile::tempdir().unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    let snapshot = service.snapshot(snapshot::Scope::Current).unwrap();
    let destination = dir.path().join("existing.zip");
    fs::write(&destination, b"keep-existing").unwrap();
    service.cancelled.store(true, Ordering::Relaxed);
    assert!(snapshot.zip(&destination, &service, vec![]).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"keep-existing");
    assert_eq!(
        fs::read_dir(service.root.join(".snapshots"))
            .unwrap()
            .count(),
        0
    );
    service.finish();
}

#[test]
fn retention_and_capacity_remove_old_segments_without_touching_unknown_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let current = root.join("runs/current");
    fs::create_dir_all(&current).unwrap();
    let active = current.join("events-0012.jsonl");
    fs::write(&active, b"active\n").unwrap();
    fs::write(current.join("business.txt"), b"keep").unwrap();
    for i in 1..=11 {
        store::open(&current.join(format!("events-{i:04}.jsonl")))
            .unwrap()
            .set_len(store::SEGMENT)
            .unwrap();
    }
    let removed = store::maintain(root, &current, &active, 65536).unwrap();
    assert!(removed >= 2);
    assert!(store::usage(root).unwrap() + store::RESERVE + 65536 <= store::LIMIT);
    assert_eq!(fs::read(current.join("business.txt")).unwrap(), b"keep");
    assert_eq!(fs::read(active).unwrap(), b"active\n");
    let old = current.join("events-0010.jsonl");
    store::open(&old)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
        .unwrap();
    assert!(store::maintain(root, &current, &current.join("events-0012.jsonl"), 0).unwrap() >= 1);
    assert!(!old.exists());
}

#[test]
fn burst_is_bounded_and_losses_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    for _ in 0..6000 {
        service.record(sample("burst"));
    }
    service.flush().unwrap();
    let run = store::runs(&service.root).unwrap().remove(0);
    assert_eq!(run.session.last_seq + run.session.dropped, 6000);
    assert!(store::usage(&service.root).unwrap() <= store::LIMIT);
    for path in store::files(&run.path)
        .unwrap()
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
    {
        for line in fs::read_to_string(path).unwrap().lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }
    service.finish();
}

#[test]
fn frontend_loss_marks_export_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let service = Diagnostics::start(dir.path().join("diagnostics"), false).unwrap();
    let mut event = sample("frontend.dropped");
    event.details = json!({"dropped":40});
    service.record(event);
    let snapshot = service.snapshot(snapshot::Scope::Current).unwrap();
    assert!(!snapshot.warnings.is_empty());
    assert!(service.status().incomplete);
    service.finish();
}

#[test]
fn real_tcp_failure_retains_available_native_details_and_operation_trace() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let cancel = crate::ssh_transport::Cancellation::default();
    let context = event::Context::new();
    cancel.set_diagnostic(context.clone());
    let error = crate::ssh_transport::tcp("127.0.0.1", port, &cancel)
        .err()
        .expect("closed local port");
    assert_eq!(error.trace_id.as_deref(), Some(context.trace_id.as_str()));
    assert_eq!(error.diagnostic.domain, Some("os"));
    assert!(error.diagnostic.native_kind.is_some());
    let safe = event::error_details(&error).to_string();
    assert!(!safe.contains("127.0.0.1"));
    // Windows may not deliver connection-refused inside the transport's short
    // cancellable attempt. A synthesized timeout has no OS code: keep its
    // actual kind rather than inventing 10061.
    if error.native_code.is_none() {
        assert!(matches!(
            error.diagnostic.native_kind.as_deref(),
            Some("TimedOut" | "WouldBlock")
        ));
    }
}
