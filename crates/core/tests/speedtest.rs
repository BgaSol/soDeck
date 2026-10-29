use serde_json::{json, Value};
use sodeck_core::speedtest::{adapter, driver, protocol::Wire};
use ssh2::Session;
use std::{io::{Cursor, Read, Write}, net::TcpStream, path::Path, sync::atomic::AtomicBool, time::Duration};

struct Traced<'a> { channel: &'a mut ssh2::Channel, left: usize }
impl Read for Traced<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let result = self.channel.read(bytes);
        if let Ok(n) = result { if self.left > 0 { self.left -= 1; println!("read {n}: {:?}", &bytes[..n.min(128)]); } }
        result
    }
}
impl Write for Traced<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let result = self.channel.write(bytes);
        if let Ok(n) = result { if self.left > 0 { self.left -= 1; println!("write {n}: {:?}", &bytes[..if n <= 256 {n} else {0}]); } }
        result
    }
    fn flush(&mut self) -> std::io::Result<()> { self.channel.flush() }
}

#[test]
fn fixed_agents_and_malformed_frames_are_bounded() {
    let windows = adapter::agent("windows", false).unwrap();
    assert!(windows.command.len() < 8191);
    assert!(windows.bootstrap.len() <= 16384);
    assert!(!windows.command.contains("ExecutionPolicy"));
    assert!(adapter::agent("unknown", false).is_err());
    for (flags, length, sequence) in [(1, 0u32, 0u32), (0, 65537, 0), (0, 0, 1)] {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(b"SDS1"); bytes[4] = 1; bytes[5] = 5; bytes[7] = flags;
        bytes[8..12].copy_from_slice(&sequence.to_be_bytes());
        bytes[12..].copy_from_slice(&length.to_be_bytes());
        let mut stream = Cursor::new(bytes.to_vec());
        let canceled = AtomicBool::new(false);
        let mut wire = Wire::new(&mut stream, &canceled);
        assert_eq!(wire.receive(&mut |_| {}).err().unwrap().code, "PROTOCOL_ERROR");
    }
    let mut stream = Cursor::new(vec![0; 16]);
    let canceled = AtomicBool::new(true);
    assert_eq!(Wire::new(&mut stream, &canceled).receive(&mut |_| {}).err().unwrap().code, "CANCELED");
}

#[test]
#[ignore = "requires dedicated loopback OpenSSH: SODECK_TEST_COMMUNICATION_CONFIG"]
fn real_memory_agents_upload_download_and_hash_receipts() {
    let config: Value = serde_json::from_slice(&std::fs::read(
        std::env::var("SODECK_TEST_COMMUNICATION_CONFIG").unwrap()).unwrap()).unwrap();
    let mut evidence = Vec::new();
    for node in config["nodes"].as_array().unwrap() {
        assert_eq!(node["host"], "127.0.0.1");
        let port = node["port"].as_u64().unwrap() as u16;
        assert!((22329..=22339).contains(&port));
        let variants = if node["platform"] == "windows" { vec![false, true] } else { vec![false] };
        for powershell7 in variants {
            let mut session = Session::new().unwrap();
            session.set_tcp_stream(TcpStream::connect(("127.0.0.1", port)).unwrap());
            session.set_timeout(10_000);
            session.handshake().unwrap();
            session.userauth_pubkey_file(node["username"].as_str().unwrap(), None,
                Path::new(node["key"].as_str().unwrap()), None).unwrap();
            let agent = adapter::agent(node["platform"].as_str().unwrap(), powershell7).unwrap();
            let mut channel = session.channel_session().unwrap();
            let command = if powershell7 {
                let executable = node["powershell7"].as_str().expect("fixture must name its isolated PowerShell 7 executable");
                assert!(!executable.contains(['\"', '\r', '\n']));
                agent.command.replacen("pwsh.exe", &format!("\"{executable}\""), 1)
            } else { agent.command.clone() };
            channel.exec(&command).unwrap();
            session.set_blocking(false);
            let started = std::time::Instant::now();
            let mut previous_stage = String::new();
            let mut trace = Traced { channel: &mut channel, left: if std::env::var_os("SODECK_TEST_TRACE_FRAMES").is_some() { 16 } else { 0 } };
            let result = driver::run(&mut trace, agent.bootstrap, agent.method, "save-data",
                &["upload".into(), "download".into()], &AtomicBool::new(false), |p| {
                    let stage = format!("{}:{}",p.phase,p.stage);
                    if stage != previous_stage { println!("{} {stage}", node["platform"]); previous_stage = stage; }
                });
            if let Err(ref error) = result {
                use std::io::Read;
                session.set_blocking(true); session.set_timeout(2000);
                let mut stderr = String::new();
                let _ = channel.stderr().take(8192).read_to_string(&mut stderr);
                panic!("{} / PowerShell7={powershell7}: {error}; fixture stderr={stderr}", node["platform"]);
            }
            let result = result.unwrap();
            assert_eq!(result.len(), 2);
            for direction in &result {
                assert!(direction.verified);
                let bytes: u64 = direction.bytes.parse().unwrap();
                assert!(bytes > 0);
                assert!(bytes + direction.warmup_bytes.parse::<u64>().unwrap() <= 32 * 1048576);
                assert!(direction.mean_bytes_per_second.is_finite());
            }
            assert!(started.elapsed() < Duration::from_secs(60));
            session.set_blocking(true); session.set_timeout(5000);
            channel.send_eof().unwrap();
            channel.wait_eof().unwrap();
            channel.wait_close().unwrap();
            assert_eq!(channel.exit_status().unwrap(), 0);
            evidence.push(json!({"platform":node["platform"],"powershell7":powershell7,"results":result,"elapsedMs":started.elapsed().as_millis()}));
            println!("E3 {} / PowerShell7={powershell7}: binary bidirectional hash verification and remote exit passed", node["platform"]);
        }
    }
    let evidence_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.qa/t019/memory-agent-results.json");
    std::fs::write(evidence_path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
}
