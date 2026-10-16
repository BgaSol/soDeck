use super::*;
use crate::ssh_transport::{self, Cancellation};
use std::{
    net::TcpStream,
    process::{Child, Command, Stdio},
    thread,
};

pub(crate) struct Fixture {
    pub child: Child,
    pub dir: tempfile::TempDir,
    pub port: u16,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let python = std::env::var("SSH_TEST_PYTHON").unwrap_or_else(|_| "python".into());
        let mut child = Command::new(python)
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tests/forwarding_fixture.py"),
            )
            .arg(dir.path())
            .arg(port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "fixture exited; install tests/requirements.txt"
            );
            assert!(start.elapsed() < Duration::from_secs(15));
            thread::sleep(Duration::from_millis(25));
        }
        Self { child, dir, port }
    }
    pub fn restart(&mut self, replace_key: bool) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if replace_key {
            std::fs::remove_file(self.dir.path().join("host-key")).unwrap();
        }
        self.child =
            Command::new(std::env::var("SSH_TEST_PYTHON").unwrap_or_else(|_| "python".into()))
                .arg(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../tests/forwarding_fixture.py"),
                )
                .arg(self.dir.path())
                .arg(self.port.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
        let start = Instant::now();
        while TcpStream::connect(("127.0.0.1", self.port)).is_err() {
            assert!(self.child.try_wait().unwrap().is_none());
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(30));
        }
    }
    fn core(&self) -> ForwardCore {
        let cancel = Cancellation::default();
        let socket = ssh_transport::tcp("127.0.0.1", self.port, &cancel).unwrap();
        let mut connection = SshConnection::new(socket, None).unwrap();
        connection.handshake(&cancel).unwrap();
        ssh_transport::retry(&cancel, Duration::from_secs(10), || {
            connection
                .session()
                .userauth_password("forward", "test-only")
        })
        .unwrap();
        ForwardCore::new(connection)
    }
}

#[test]
#[ignore = "isolated real SSH forwarding; requires Python paramiko"]
fn forward_core_concurrency_half_close_and_denied_target() {
    let fixture = Fixture::new();
    let mut core = fixture.core();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut clients = Vec::new();
    for id in 1..=4 {
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(40)))
            .unwrap();
        let (peer, address) = listener.accept().unwrap();
        core.enqueue(
            id,
            Some(Socket::Tcp(peer)),
            Target {
                host: "echo.internal.invalid".into(),
                port: 7,
            },
            (address.ip().to_string(), address.port()),
        )
        .unwrap();
        clients.push(thread::spawn(move || {
            let bytes: Vec<_> = (0..(3 * 1024 * 1024))
                .map(|n| ((n + id as usize) % 251) as u8)
                .collect();
            client.write_all(&bytes).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            let mut received = Vec::new();
            client.read_to_end(&mut received).unwrap();
            assert_eq!(received.len(), bytes.len());
            assert_eq!(received, bytes);
        }));
    }
    core.enqueue(
        100,
        None,
        Target {
            host: "denied".into(),
            port: 1,
        },
        ("127.0.0.1".into(), 0),
    )
    .unwrap();
    let poller = Poller::new().unwrap();
    let started = Instant::now();
    let (mut sent, mut received, mut denied) = (0, 0, false);
    while core.len() > 0 {
        assert!(
            started.elapsed() < Duration::from_secs(40),
            "forwarding stalled"
        );
        let progress = core.drive().unwrap();
        for update in core.updates.drain(..) {
            match update {
                Update::Bytes {
                    sent: s,
                    received: r,
                } => {
                    sent += s;
                    received += r;
                }
                Update::Failed(100, _) => denied = true,
                _ => {}
            }
        }
        if !progress {
            core.wait(&poller, None, Duration::from_millis(100))
                .unwrap();
        }
    }
    for client in clients {
        client.join().unwrap();
    }
    assert!(denied);
    assert_eq!(sent, 12 * 1024 * 1024);
    assert_eq!(received, sent);
}

#[test]
#[ignore = "isolated real SSH; requires Python paramiko"]
fn forward_core_slow_reader_does_not_block_other_channels_and_cancel_releases_probe() {
    let fixture = Fixture::new();
    let mut core = fixture.core();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut slow = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let (peer, address) = listener.accept().unwrap();
    socket2::SockRef::from(&peer)
        .set_send_buffer_size(4096)
        .unwrap();
    socket2::SockRef::from(&slow)
        .set_recv_buffer_size(32768)
        .unwrap();
    let target = Target {
        host: "echo.internal.invalid".into(),
        port: 7,
    };
    core.enqueue(
        1,
        Some(Socket::Tcp(peer)),
        target.clone(),
        (address.ip().to_string(), address.port()),
    )
    .unwrap();
    let (release, resume) = std::sync::mpsc::sync_channel(1);
    let slow = thread::spawn(move || {
        let bytes = vec![0x5b; 3 * 1024 * 1024];
        slow.write_all(&bytes).unwrap();
        slow.shutdown(Shutdown::Write).unwrap();
        resume.recv_timeout(Duration::from_secs(15)).unwrap();
        let mut result = Vec::new();
        slow.read_to_end(&mut result).unwrap();
        assert_eq!(result, bytes);
    });
    let poller = Poller::new().unwrap();
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(15));
        let progress = core.drive().unwrap();
        core.updates.clear();
        if !progress && core.flows.iter().any(|f| f.id == 1 && !f.incoming.empty()) {
            break;
        }
        if !progress {
            core.wait(&poller, None, Duration::from_millis(20)).unwrap();
        }
    }
    let mut fast = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    fast.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
    let (peer, address) = listener.accept().unwrap();
    core.enqueue(
        2,
        Some(Socket::Tcp(peer)),
        target.clone(),
        (address.ip().to_string(), address.port()),
    )
    .unwrap();
    core.enqueue(3, None, target, ("127.0.0.1".into(), 0))
        .unwrap();
    core.cancel(3);
    assert!(!core.queue.iter().any(|f| f.id == 3));
    // Exercise the serialized public keepalive operation without a real 20s sleep.
    core.keepalive = Instant::now() - Duration::from_secs(21);
    let fast = thread::spawn(move || {
        fast.write_all(b"unblocked").unwrap();
        fast.shutdown(Shutdown::Write).unwrap();
        let mut result = Vec::new();
        fast.read_to_end(&mut result).unwrap();
        assert_eq!(result, b"unblocked");
        release.send(()).unwrap();
    });
    while core.len() > 0 {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "slow reader stalled the route"
        );
        let progress = core.drive().unwrap();
        core.updates.clear();
        if !progress {
            core.wait(&poller, None, Duration::from_millis(20)).unwrap();
        }
    }
    fast.join().unwrap();
    slow.join().unwrap();
}
