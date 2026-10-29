//! Actual SSH protocol load; this fixture produces bytes rather than a Linux shell.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sodeck_core::{Caller, Core};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    child: Child,
    directory: tempfile::TempDir,
    port: u16,
}
impl Fixture {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut child =
            Command::new(std::env::var("SODECK_TEST_PYTHON").expect("SODECK_TEST_PYTHON required"))
                .args([
                    "-u",
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../tests/core/ssh_fixture.py"
                    ),
                ])
                .arg(directory.path())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
        Self {
            child,
            directory,
            port,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn write(core: &Core, caller: &Caller, p: &Value, seq: u64, text: &str) {
    core.call(caller,"pty.write",json!({"ptyId":p["ptyId"],"generation":p["generation"],"inputSeq":seq,"bytes":STANDARD.encode(text)})).unwrap();
}
fn percentile(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    samples[((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)]
}
#[cfg(windows)]
fn working_set() -> usize {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    assert_ne!(
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) },
        0
    );
    counters.WorkingSetSize
}
#[cfg(not(windows))]
fn working_set() -> usize {
    0
}
#[test]
#[ignore = "60-second actual loopback SSH load; requires explicit Python/Paramiko fixture"]
fn four_ptys_backpressure_latency_and_twenty_lifecycles() {
    let memory_baseline = working_set();
    let mut memory_peak = memory_baseline;
    let fixture = Fixture::start();
    let directory = tempfile::tempdir().unwrap();
    let core = Core::open(directory.path()).unwrap();
    let caller = Caller::shell();
    let server = core
        .call(
            &caller,
            "servers.create",
            json!({"name":"load","host":"127.0.0.1","port":fixture.port,"username":"test-user"}),
        )
        .unwrap();
    core.call(
        &caller,
        "auth.ssh.set",
        json!({"serverId":server["id"],"password":"isolated-test-password"}),
    )
    .unwrap();
    let acquire = json!({"serverId":server["id"],"workspaceId":"load-workspace"});
    let challenge = core
        .call(&caller, "connections.acquire", acquire.clone())
        .unwrap_err();
    assert_eq!(challenge.code, "HOST_KEY_REQUIRED");
    core.call(&caller, "hosts.trust", challenge.details.unwrap())
        .unwrap();
    let mut ptys = Vec::new();
    let mut leases = Vec::new();
    for _ in 0..4 {
        let lease = core
            .call(&caller, "connections.acquire", acquire.clone())
            .unwrap();
        ptys.push(
            core.call(&caller, "pty.open", json!({"leaseId":lease["leaseId"]}))
                .unwrap(),
        );
        leases.push(lease);
    }
    for p in &ptys {
        write(&core, &caller, p, 1, "__SODECK_TEST_STREAM__\r");
    }
    let started = Instant::now();
    let mut cursors = [0u64; 4];
    let mut input_sequences = [1u64; 4];
    let mut partials: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::new());
    let mut expected = [0u64; 4];
    let mut total_bytes = [0usize; 4];
    let mut peak_buffered = [0u64; 4];
    let mut marker_sent = None;
    let mut marker_times = Vec::new();
    let mut query_times = Vec::new();
    let mut last_marker = Instant::now();
    let mut last_query = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        let elapsed = started.elapsed();
        if last_marker.elapsed() >= Duration::from_secs(1) && marker_sent.is_none() {
            input_sequences[1] += 1;
            write(
                &core,
                &caller,
                &ptys[1],
                input_sequences[1],
                "INPUT-MARKER\r",
            );
            marker_sent = Some(Instant::now());
            last_marker = Instant::now();
        }
        for index in 0..4 {
            if index == 0 && elapsed >= Duration::from_secs(20) && elapsed < Duration::from_secs(30)
            {
                continue;
            }
            let p = &ptys[index];
            let batch = core
                .call(
                    &caller,
                    "pty.read",
                    json!({"ptyId":p["ptyId"],"generation":p["generation"],"after":cursors[index]}),
                )
                .unwrap();
            assert_eq!(batch["state"], "active");
            for frame in batch["frames"].as_array().unwrap() {
                let bytes = STANDARD.decode(frame["bytes"].as_str().unwrap()).unwrap();
                total_bytes[index] += bytes.len();
                partials[index].extend(bytes);
                cursors[index] = frame["sequence"].as_u64().unwrap();
            }
            while let Some(end) = partials[index].iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = partials[index].drain(..=end).collect();
                if line.starts_with(b"FLOOD:") {
                    assert_eq!(line.len(), 8192);
                    let sequence = std::str::from_utf8(&line[6..14])
                        .unwrap()
                        .parse::<u64>()
                        .unwrap();
                    assert_eq!(
                        sequence, expected[index],
                        "output gap/duplicate on PTY {index}"
                    );
                    expected[index] += 1;
                }
                if line.starts_with(b"INPUT-MARKER") {
                    if let Some(sent) = marker_sent.take() {
                        marker_times.push(sent.elapsed().as_millis());
                    }
                }
            }
            core.call(
                &caller,
                "pty.ack",
                json!({"ptyId":p["ptyId"],"generation":p["generation"],"sequence":cursors[index]}),
            )
            .unwrap();
        }
        if last_query.elapsed() >= Duration::from_millis(100) {
            let query = Instant::now();
            let resources = core.call(&caller, "resources.list", json!({})).unwrap();
            query_times.push(query.elapsed().as_millis());
            let mut global = 0;
            for resource in resources["ptys"].as_array().unwrap() {
                let index = ptys
                    .iter()
                    .position(|p| p["ptyId"] == resource["id"])
                    .unwrap();
                let buffered = resource["bufferedBytes"].as_u64().unwrap();
                assert!(buffered <= 2 * 1024 * 1024);
                global += buffered;
                peak_buffered[index] = peak_buffered[index].max(buffered);
            }
            assert!(global <= 16 * 1024 * 1024);
            last_query = Instant::now();
        }
        memory_peak = memory_peak.max(working_set());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(marker_times.len() >= 40);
    let marker_p95 = percentile(&mut marker_times);
    let query_p95 = percentile(&mut query_times);
    assert!(marker_p95 <= 500, "input P95 {marker_p95}ms");
    assert!(query_p95 <= 500, "query P95 {query_p95}ms");
    for bytes in total_bytes {
        assert!(
            bytes >= 14 * 1024 * 1024,
            "producer throughput was below 14 MiB/60 seconds"
        );
    }
    assert!(
        peak_buffered[0] >= 1900 * 1024,
        "pause did not exercise backpressure budget"
    );
    for (pty, lease) in ptys.iter().zip(&leases) {
        core.call(
            &caller,
            "pty.close",
            json!({"ptyId":pty["ptyId"],"generation":pty["generation"]}),
        )
        .unwrap();
        core.call(
            &caller,
            "connections.release",
            json!({"leaseId":lease["leaseId"]}),
        )
        .unwrap();
    }
    for _ in 0..20 {
        let lease = core
            .call(&caller, "connections.acquire", acquire.clone())
            .unwrap();
        let pty = core
            .call(&caller, "pty.open", json!({"leaseId":lease["leaseId"]}))
            .unwrap();
        core.call(
            &caller,
            "pty.close",
            json!({"ptyId":pty["ptyId"],"generation":pty["generation"]}),
        )
        .unwrap();
        core.call(
            &caller,
            "connections.release",
            json!({"leaseId":lease["leaseId"]}),
        )
        .unwrap();
        assert!(core.resources()["connections"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    core.shutdown();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let events =
            std::fs::read_to_string(fixture.directory.path().join("events.jsonl")).unwrap();
        let opened = events
            .lines()
            .filter(|line| line.contains("connection-open"))
            .count();
        let closed = events
            .lines()
            .filter(|line| line.contains("connection-close"))
            .count();
        if opened == closed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fixture still has active SSH sockets"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let report = json!({"test":"real-loopback-ssh-protocol","durationSeconds":60,"ptys":4,"producerBytesPerSecondPerPty":262144,"receivedBytes":total_bytes,"validatedBlocks":expected,"pausedConsumerSeconds":10,"peakBufferedBytes":peak_buffered,"inputMarkerP95Ms":marker_p95,"resourceQueryP95Ms":query_p95,"createCloseCycles":20,"remoteSocketBaselineRestored":true,"linuxOpenSsh":false,"testProcessWorkingSet":{"baseline":memory_baseline,"peak":memory_peak,"afterCleanup":working_set()},"webviewMemoryMeasured":false});
    if let Ok(path) = std::env::var("SODECK_CORE_LOAD_RESULT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
}
