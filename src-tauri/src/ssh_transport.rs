//! Socket ownership and bounded, cancellable SSH direct-tcpip forwarding.
use crate::model::{AppError, AppResult};
use ssh2::{Channel, Session};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub enum Socket {
    Tcp(TcpStream),
    #[cfg(unix)]
    Local(std::os::unix::net::UnixStream),
}
impl Socket {
    pub fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            Self::Tcp(s) => s.try_clone().map(Self::Tcp),
            #[cfg(unix)]
            Self::Local(s) => s.try_clone().map(Self::Local),
        }
    }
    pub fn shutdown(&self, how: Shutdown) {
        match self {
            Self::Tcp(s) => {
                let _ = s.shutdown(how);
            }
            #[cfg(unix)]
            Self::Local(s) => {
                let _ = s.shutdown(how);
            }
        }
    }
    fn nonblocking(&self) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_nonblocking(true),
            #[cfg(unix)]
            Self::Local(s) => s.set_nonblocking(true),
        }
    }
}
#[cfg(unix)]
impl std::os::fd::AsRawFd for Socket {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        match self {
            Self::Tcp(s) => s.as_raw_fd(),
            Self::Local(s) => s.as_raw_fd(),
        }
    }
}
#[cfg(windows)]
impl std::os::windows::io::AsRawSocket for Socket {
    fn as_raw_socket(&self) -> std::os::windows::io::RawSocket {
        match self {
            Self::Tcp(s) => s.as_raw_socket(),
        }
    }
}
impl Read for Socket {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(b),
            #[cfg(unix)]
            Self::Local(s) => s.read(b),
        }
    }
}
impl Write for Socket {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(b),
            #[cfg(unix)]
            Self::Local(s) => s.write(b),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<CancelState>);
#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    sockets: Mutex<Vec<Weak<Socket>>>,
}
impl Cancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        for socket in self
            .0
            .sockets
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
        {
            socket.shutdown(Shutdown::Both);
        }
    }
    pub fn reset(&self) -> AppResult<()> {
        let mut sockets = self.0.sockets.lock().unwrap();
        if sockets.iter().any(|s| s.strong_count() > 0) {
            return Err(AppError::new("busy", "上一次连接尚未释放"));
        }
        sockets.clear();
        self.0.cancelled.store(false, Ordering::SeqCst);
        Ok(())
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> AppResult<()> {
        if self.is_cancelled() {
            Err(AppError::new("cancelled", "连接已取消"))
        } else {
            Ok(())
        }
    }
    fn register(&self, socket: &Arc<Socket>) -> AppResult<()> {
        let mut sockets = self.0.sockets.lock().unwrap();
        self.check()?;
        sockets.retain(|s| s.strong_count() > 0);
        sockets.push(Arc::downgrade(socket));
        Ok(())
    }
}

pub fn retry<T>(
    cancel: &Cancellation,
    timeout: Duration,
    mut operation: impl FnMut() -> Result<T, ssh2::Error>,
) -> AppResult<T> {
    let started = Instant::now();
    loop {
        cancel.check()?;
        match operation() {
            Ok(value) => {
                cancel.check()?;
                return Ok(value);
            }
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                if started.elapsed() >= timeout {
                    return Err(AppError::new("timeout", "SSH 操作超时"));
                }
                thread::sleep(Duration::from_millis(8));
            }
            Err(e) => {
                cancel.check()?;
                return Err(e.into());
            }
        }
    }
}

pub fn tcp(host: &str, port: u16, cancel: &Cancellation) -> AppResult<Arc<Socket>> {
    // The system resolver is blocking. Keep it off the connection worker so
    // cancellation does not wait for DNS; late results have no socket side effects.
    let (send, recv) = mpsc::channel();
    let host = host.to_owned();
    thread::spawn(move || {
        let result = (host.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.take(8).collect::<Vec<_>>());
        let _ = send.send(result);
    });
    let start = Instant::now();
    let addresses = loop {
        cancel.check()?;
        match recv.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => break result.map_err(|e| AppError::new("network", e))?,
            Err(mpsc::RecvTimeoutError::Timeout) if start.elapsed() < Duration::from_secs(12) => {}
            _ => return Err(AppError::new("network", "服务器地址解析失败或超时")),
        }
    };
    for address in addresses {
        let start = Instant::now();
        loop {
            cancel.check()?;
            match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    let socket = Arc::new(Socket::Tcp(stream));
                    socket.nonblocking()?;
                    cancel.register(&socket)?;
                    return Ok(socket);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) && start.elapsed() < Duration::from_secs(8) => {}
                Err(_) => break,
            }
        }
    }
    Err(AppError::new(
        "network",
        "连接失败或超时，请检查地址、端口及网络",
    ))
}

#[cfg(unix)]
fn socket_pair() -> std::io::Result<(Socket, Socket)> {
    let (a, b) = std::os::unix::net::UnixStream::pair()?;
    Ok((Socket::Local(a), Socket::Local(b)))
}
#[cfg(windows)]
fn socket_pair() -> std::io::Result<(Socket, Socket)> {
    use std::net::TcpListener;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let client = TcpStream::connect_timeout(&listener.local_addr()?, Duration::from_secs(2))?;
    let expected = client.local_addr()?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((peer, address)) if address == expected => {
                client.set_nodelay(true)?;
                peer.set_nodelay(true)?;
                return Ok((Socket::Tcp(client), Socket::Tcp(peer)));
            }
            Ok((peer, _)) => {
                let _ = peer.shutdown(Shutdown::Both);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        if started.elapsed() > Duration::from_secs(2) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "本地转发配对超时",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

// This wrapper is deliberately not Clone/Deref: callers must retain the owner
// for the whole lifetime of their SFTP/exec channel or terminal worker.
pub struct SshConnection {
    session: Session,
    socket: Arc<Socket>,
    tunnel: Option<TunnelGuard>,
}
impl SshConnection {
    pub fn new(socket: Arc<Socket>, tunnel: Option<TunnelGuard>) -> AppResult<Self> {
        let mut session = Session::new()?;
        session.set_tcp_stream(socket.try_clone()?);
        session.set_timeout(12000);
        session.set_blocking(false);
        Ok(Self {
            session,
            socket,
            tunnel,
        })
    }
    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn handshake(&mut self, cancel: &Cancellation) -> AppResult<()> {
        retry(cancel, Duration::from_secs(12), || self.session.handshake())
    }
    pub fn transport_error(&self) -> Option<AppError> {
        self.tunnel
            .as_ref()
            .and_then(|t| t.error.lock().unwrap().clone())
    }
}
impl Drop for SshConnection {
    fn drop(&mut self) {
        self.session.set_blocking(false);
        self.socket.shutdown(Shutdown::Both);
        // Stop forwarding before the parent SSH owner is dropped in its worker.
        self.tunnel.take();
    }
}

pub struct TunnelGuard {
    stop: Arc<AtomicBool>,
    wake: Socket,
    worker: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<AppError>>>,
}
impl Drop for TunnelGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn forward(
    parent: SshConnection,
    host: &str,
    port: u16,
    cancel: &Cancellation,
) -> AppResult<SshConnection> {
    parent.session().set_blocking(false);
    let channel = retry(cancel, Duration::from_secs(12), || {
        parent.session().channel_direct_tcpip(host, port, None)
    })
    .map_err(|e| {
        if e.kind == "ssh" {
            AppError::new(
                "forwarding",
                format!("跳板无法转发到 {host}:{port}：{}", e.message),
            )
        } else {
            e
        }
    })?;
    let (client, bridge) = socket_pair()?;
    let client = Arc::new(client);
    client.nonblocking()?;
    bridge.nonblocking()?;
    cancel.register(&client)?;
    let wake = bridge.try_clone()?;
    let stop = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let worker_stop = stop.clone();
    let worker_error = error.clone();
    let worker_cancel = cancel.clone();
    let worker = thread::spawn(move || {
        let mut bridge = bridge;
        if let Err(e) = pump(&parent, channel, &mut bridge, &worker_stop, &worker_cancel) {
            if !worker_stop.load(Ordering::SeqCst) && !worker_cancel.is_cancelled() {
                *worker_error.lock().unwrap() =
                    Some(AppError::new("network", format!("跳板转发中断：{e}")));
            }
        }
        bridge.shutdown(Shutdown::Both);
    });
    SshConnection::new(
        client,
        Some(TunnelGuard {
            stop,
            wake,
            worker: Some(worker),
            error,
        }),
    )
}

fn pump(
    parent: &SshConnection,
    mut channel: Channel,
    socket: &mut Socket,
    stop: &AtomicBool,
    cancel: &Cancellation,
) -> std::io::Result<()> {
    // A pending SSH write retains exactly the same allocation, offset and length
    // on EAGAIN. Never append new bytes to it until that write succeeds.
    let mut outgoing = [0u8; 32768];
    let mut incoming = [0u8; 32768];
    let (mut out_pos, mut out_len, mut in_pos, mut in_len) = (0, 0, 0, 0);
    let (mut local_eof, mut remote_eof, mut sent_eof, mut sent_shutdown) =
        (false, false, false, false);
    let mut keepalive = Instant::now();
    while !stop.load(Ordering::SeqCst) && !cancel.is_cancelled() {
        let mut progress = false;
        if out_pos == out_len && !local_eof {
            match socket.read(&mut outgoing) {
                Ok(0) => local_eof = true,
                Ok(n) => {
                    out_pos = 0;
                    out_len = n;
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        if out_pos < out_len {
            match channel.write(&outgoing[out_pos..out_len]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "跳板通道已关闭",
                    ))
                }
                Ok(n) => {
                    out_pos += n;
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        if local_eof && out_pos == out_len && !sent_eof {
            match channel.send_eof() {
                Ok(()) => {
                    sent_eof = true;
                    progress = true;
                }
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if in_pos == in_len && !remote_eof {
            match channel.read(&mut incoming) {
                Ok(0) => remote_eof = channel.eof(),
                Ok(n) => {
                    in_pos = 0;
                    in_len = n;
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        if in_pos < in_len {
            match socket.write(&incoming[in_pos..in_len]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "本地转发已关闭",
                    ))
                }
                Ok(n) => {
                    in_pos += n;
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        if remote_eof && in_pos == in_len && !sent_shutdown {
            socket.shutdown(Shutdown::Write);
            sent_shutdown = true;
        }
        if sent_eof && sent_shutdown {
            break;
        }
        if keepalive.elapsed() >= Duration::from_secs(20) {
            match parent.session().keepalive_send() {
                Ok(_) => keepalive = Instant::now(),
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if !progress {
            thread::sleep(Duration::from_millis(4));
        }
    }
    let _ = channel.close();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_shuts_registered_sockets_and_releases_old_registrations() {
        let cancel = Cancellation::default();
        let (socket, mut peer) = socket_pair().unwrap();
        let socket = Arc::new(socket);
        cancel.register(&socket).unwrap();
        cancel.cancel();
        assert!(cancel.reset().is_err());
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
        assert!(cancel.register(&socket).is_err());
        drop(socket);
        cancel.reset().unwrap();
        assert!(!cancel.is_cancelled());
        for _ in 0..20 {
            let (socket, _) = socket_pair().unwrap();
            cancel.register(&Arc::new(socket)).unwrap();
        }
        assert_eq!(cancel.0.sockets.lock().unwrap().len(), 1);
    }
}
