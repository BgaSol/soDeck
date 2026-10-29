//! T-019 E1/E2: opt-in isolated real OpenSSH; never selects a user server.
use serde_json::Value;
use ssh2::{ByteMeter, ErrorCode, Session};
use std::{io::{Read, Write}, net::TcpStream, path::Path, time::{Duration, Instant}};

fn retry<T>(mut action: impl FnMut() -> Result<T, ssh2::Error>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match action() {
            Ok(value) => return value,
            Err(e) if e.code() == ErrorCode::Session(-37) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("SSH operation failed: {e}"),
        }
    }
}

#[test]
fn counter_pool_is_bounded_and_generation_safe() {
    let meter = ByteMeter::new(2);
    let one = meter.reserve().unwrap();
    let two = meter.reserve().unwrap();
    assert!(meter.reserve().is_none());
    let old = one.token();
    let held = one.clone();
    drop(one);
    assert!(meter.reserve().is_none());
    drop(held);
    let next = meter.reserve().unwrap();
    assert_ne!(old, next.token());
    assert_eq!(two.counts().sent, 0);
    assert_eq!(next.counts().received, 0);
    assert!(!next.counts().invalid);
    assert_eq!(meter.unattributed().sent, 0);
}

#[test]
#[ignore = "requires dedicated loopback OpenSSH: SODECK_TEST_COMMUNICATION_CONFIG"]
fn real_socket_channel_sftp_and_response_probe() {
    let config: Value = serde_json::from_slice(&std::fs::read(
        std::env::var("SODECK_TEST_COMMUNICATION_CONFIG").expect("isolated configuration required")
    ).unwrap()).unwrap();
    for node in config["nodes"].as_array().unwrap() {
        assert_eq!(node["host"], "127.0.0.1", "only dedicated loopback fixtures allowed");
        let port = node["port"].as_u64().unwrap() as u16;
        assert!((22329..=22339).contains(&port));
        let meter = ByteMeter::new(64);
        let transport = meter.reserve().unwrap();
        let mut session = Session::new().unwrap();
        session.set_byte_meter(&meter, &transport).unwrap();
        session.set_tcp_stream(TcpStream::connect(("127.0.0.1", port)).unwrap());
        session.set_timeout(10_000);
        session.handshake().unwrap();
        let handshake = transport.counts();
        assert!(handshake.sent > 0 && handshake.received > 0, "banner/handshake covered");
        session.userauth_pubkey_file(node["username"].as_str().unwrap(), None,
            Path::new(node["key"].as_str().unwrap()), None).unwrap();
        assert!(session.authenticated());
        session.set_blocking(false);

        let probe_bytes = meter.reserve().unwrap();
        session.set_channel_origin(Some(&probe_bytes)).unwrap();
        let probe = retry(|| session.channel_session());
        assert_eq!(probe.byte_counter_token(), probe_bytes.token());
        for _ in 0..4 {
            retry(|| probe.probe_send());
            let sent = Instant::now();
            let accepted = retry(|| probe.probe_receive());
            assert!(!accepted, "unknown request must return CHANNEL_FAILURE on OpenSSH");
            assert!(sent.elapsed() < Duration::from_secs(3));
        }
        retry(|| probe.probe_send());
        probe.probe_cancel();
        assert!(probe.probe_receive().is_err());
        let mut probe = probe;
        retry(|| probe.close());
        drop(probe);
        assert!(probe_bytes.counts().closed);
        // Request messages are transport overhead, not CHANNEL_DATA payload.
        assert_eq!(probe_bytes.counts().sent, 0);

        session.set_blocking(true);
        let sftp_bytes = meter.reserve().unwrap();
        session.set_channel_origin(Some(&sftp_bytes)).unwrap();
        let sftp = session.sftp().unwrap();
        let init = sftp_bytes.counts();
        assert!(init.sent > 0 && init.received > 0, "SFTP initialization must be included");
        let _ = sftp.stat(Path::new(".")).unwrap();
        let _ = sftp.readdir(Path::new(".")).unwrap();
        let metadata = sftp_bytes.counts();
        assert!(metadata.sent > init.sent && metadata.received > init.received);
        drop(sftp);
        assert!(sftp_bytes.counts().closed);

        let data_bytes = meter.reserve().unwrap();
        session.set_channel_origin(Some(&data_bytes)).unwrap();
        let mut data = session.channel_session().unwrap();
        let command = if node["platform"] == "windows" {
            "powershell.exe -NoProfile -NonInteractive -Command \"$s=[Console]::OpenStandardInput();$h=$s.GetType().GetField('_handle',[Reflection.BindingFlags]'Instance,NonPublic').GetValue($s);$i=[IO.Pipes.AnonymousPipeClientStream]::new([IO.Pipes.PipeDirection]::In,[Microsoft.Win32.SafeHandles.SafePipeHandle]::new($h.DangerousGetHandle(),$false));$o=[Console]::OpenStandardOutput();$i.CopyTo($o);$o.Flush()\""
        } else { "cat" };
        data.exec(command).unwrap();
        let payload: Vec<u8> = (0..65_537).map(|n| (n % 251) as u8).collect();
        data.write_all(&payload).unwrap();
        data.send_eof().unwrap();
        let mut response = Vec::new();
        data.read_to_end(&mut response).unwrap();
        assert_eq!(response, payload);
        assert_eq!(data_bytes.counts().sent, payload.len() as u64);
        assert_eq!(data_bytes.counts().received, payload.len() as u64);
        data.wait_close().unwrap();
        drop(data);
        assert!(data_bytes.counts().closed);
        assert_eq!(meter.unattributed().sent, 0);
        assert_eq!(meter.unattributed().received, 0);
        assert!(transport.counts().sent > data_bytes.counts().sent + metadata.sent);
        assert!(transport.counts().received > data_bytes.counts().received + metadata.received);
        drop(session);
        assert!(transport.counts().closed);
        println!("E1/E2 {}: handshake, SFTP metadata, exact echo, matched FAILURE and retirement passed", node["platform"]);
    }
}
