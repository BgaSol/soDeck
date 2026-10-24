use serde_json::json;
use sodeck_plugin_host::{PluginProcess, RpcError};
use std::{
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[test]
fn independent_process_routes_calls_and_stops() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker.js");
    fs::write(&worker, "globalThis.sodeckPlugin={async invoke(command,params){return await sodeck.call(command,params)},stop(){}};").unwrap();
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    let process = PluginProcess::spawn(
        env!("CARGO_BIN_EXE_sodeck-plugin-host"),
        worker,
        Arc::new(move |method, params| {
            captured.lock().unwrap().push(method.to_owned());
            if method == "remote.futurePublicCapability" {
                Ok(json!({"accepted":params["payload"]}))
            } else {
                Err(RpcError::new(
                    "METHOD_NOT_FOUND",
                    "Unsupported core capability",
                ))
            }
        }),
    )
    .unwrap();
    process
        .initialize(json!({"pluginId":"fixture.identity"}))
        .unwrap();
    assert_ne!(process.process_id(), std::process::id());
    assert_eq!(
        process
            .invoke(
                "remote.futurePublicCapability",
                json!({"payload":"roundtrip"})
            )
            .unwrap()["accepted"],
        "roundtrip"
    );
    assert_eq!(
        process.invoke("unknown", json!({})).unwrap_err().code,
        "METHOD_NOT_FOUND"
    );
    assert_eq!(records.lock().unwrap().len(), 2);
    process.stop().unwrap();
    assert!(!process.is_alive());
    assert!(process.invoke("after-stop", json!({})).is_err());
}

#[test]
fn nonterminating_worker_is_contained_by_process_budget() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker.js");
    fs::write(&worker, "globalThis.sodeckPlugin={invoke(){while(true){}}}").unwrap();
    let process = PluginProcess::spawn(
        env!("CARGO_BIN_EXE_sodeck-plugin-host"),
        worker,
        Arc::new(|_, _| Ok(json!(null))),
    )
    .unwrap();
    process.initialize(json!({})).unwrap();
    let start = Instant::now();
    assert!(process
        .invoke_with_timeout("loop", json!({}), Duration::from_secs(9))
        .is_err());
    assert!(start.elapsed() < Duration::from_secs(9));
    assert!(!process.is_alive());
}

#[test]
fn oversize_core_reply_returns_explicit_error_and_host_survives() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker.js");
    fs::write(
        &worker,
        "globalThis.sodeckPlugin={async invoke(command){return await sodeck.call(command,{})}}",
    )
    .unwrap();
    let process = PluginProcess::spawn(
        env!("CARGO_BIN_EXE_sodeck-plugin-host"),
        worker,
        Arc::new(|method, _| {
            Ok(if method == "large" {
                json!({"data":"x".repeat(1024*1024)})
            } else {
                json!({"ok":true})
            })
        }),
    )
    .unwrap();
    process.initialize(json!({})).unwrap();
    let error = process
        .invoke_with_timeout("large", json!({}), Duration::from_secs(2))
        .unwrap_err();
    assert_eq!(error.code, "RESULT_UNKNOWN");
    assert_eq!(error.phase.as_deref(), Some("sent"));
    assert!(error.operation_id.is_some());
    assert_eq!(process.invoke("small", json!({})).unwrap()["ok"], true);
    process.stop().unwrap();
}

// Fill the actual child stdin while its JavaScript thread does not consume the
// bounded inbound queue. This exercises a blocked OS pipe, not a mock writer.
fn fill_pipe(process: &PluginProcess) -> Vec<usize> {
    let payload = "x".repeat(900_000);
    let mut cancelled = Vec::new();
    let mut sent = 0;
    let started = Instant::now();
    for tag in 0..48 {
        let error = process
            .invoke_with_timeout(
                "record",
                json!({"tag":tag,"payload":payload}),
                Duration::from_millis(40),
            )
            .unwrap_err();
        if error.phase.as_deref() == Some("sent") {
            sent += 1;
        }
        if error.code == "TIMEOUT" && error.phase.as_deref() == Some("notSent") {
            cancelled.push(tag);
        }
        if cancelled.len() >= 2 {
            break;
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "request deadline was blocked by child stdin: {:?}",
        started.elapsed()
    );
    assert!(
        !cancelled.is_empty(),
        "test did not reach the blocked pipe queue"
    );
    assert!(
        sent >= 32,
        "cancellation happened before the actual child inbound queue filled ({sent} frames)"
    );
    cancelled
}

#[test]
fn queued_expired_frames_are_not_executed_when_child_resumes() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker.js");
    fs::write(&worker,"const seen=[];globalThis.sodeckPlugin={invoke(command,params){if(command==='block'){const end=Date.now()+4200;while(Date.now()<end){};return null;}if(command==='inspect')return seen;seen.push(params.tag);return null}}").unwrap();
    let process = PluginProcess::spawn(
        env!("CARGO_BIN_EXE_sodeck-plugin-host"),
        worker,
        Arc::new(|_, _| Ok(json!(true))),
    )
    .unwrap();
    process.initialize(json!({})).unwrap();
    assert_eq!(
        process
            .invoke_with_timeout("block", json!({}), Duration::from_millis(10))
            .unwrap_err()
            .code,
        "RESULT_UNKNOWN"
    );
    let cancelled = fill_pipe(&process);
    let records = process
        .invoke_with_timeout("inspect", json!({}), Duration::from_secs(6))
        .unwrap();
    let records = records.as_array().unwrap();
    assert!(records.len() >= 32);
    for tag in cancelled {
        assert!(
            !records.contains(&json!(tag)),
            "expired unsent frame executed"
        );
    }
    process.stop().unwrap();
}

#[test]
fn stop_deadline_does_not_wait_for_blocked_stdin_writer() {
    let temp = tempfile::tempdir().unwrap();
    let worker = temp.path().join("worker.js");
    fs::write(
        &worker,
        "globalThis.sodeckPlugin={invoke(command){if(command==='block')while(true){};return null}}",
    )
    .unwrap();
    let process = PluginProcess::spawn(
        env!("CARGO_BIN_EXE_sodeck-plugin-host"),
        worker,
        Arc::new(|_, _| Ok(json!(null))),
    )
    .unwrap();
    process.initialize(json!({})).unwrap();
    assert_eq!(
        process
            .invoke_with_timeout("block", json!({}), Duration::from_millis(10))
            .unwrap_err()
            .code,
        "RESULT_UNKNOWN"
    );
    fill_pipe(&process);
    let started = Instant::now();
    let _ = process.stop();
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "stop exceeded its total deadline: {:?}",
        started.elapsed()
    );
    assert!(!process.is_alive());
}
