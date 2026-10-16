//! Socket ownership and bounded, cancellable SSH direct-tcpip forwarding.
pub mod forwarding;
mod meter;
mod resolver;
use crate::model::{AppError, AppResult};
use ssh2::Session;
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
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
    fn check_alive(&self) -> AppResult<()> {
        let socket = socket2::SockRef::from(self);
        if let Some(error) = socket.take_error()? {
            return Err(AppError::new("network", error));
        }
        let mut byte = [std::mem::MaybeUninit::uninit(); 1];
        match socket.peek(&mut byte) {
            Ok(0) => Err(AppError::new("network", "SSH 传输连接已关闭")),
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(AppError::new("network", e)),
        }
    }
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
impl std::os::fd::AsFd for Socket {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        match self {
            Self::Tcp(s) => s.as_fd(),
            Self::Local(s) => s.as_fd(),
        }
    }
}
#[cfg(windows)]
impl std::os::windows::io::AsSocket for Socket {
    fn as_socket(&self) -> std::os::windows::io::BorrowedSocket<'_> {
        match self {
            Self::Tcp(s) => s.as_socket(),
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
    let addresses = resolver::resolve(host, port, cancel)?;
    let deadline = Instant::now() + Duration::from_secs(12);
    for address in addresses {
        let start = Instant::now();
        loop {
            cancel.check()?;
            if Instant::now() >= deadline {
                break;
            }
            match TcpStream::connect_timeout(&address, Duration::from_millis(300)) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    let keepalive = socket2::TcpKeepalive::new()
                        .with_time(Duration::from_secs(30))
                        .with_interval(Duration::from_secs(10));
                    #[cfg(any(target_os = "linux", target_os = "macos"))]
                    let keepalive = keepalive.with_retries(3);
                    let _ = socket2::SockRef::from(&stream).set_tcp_keepalive(&keepalive);
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
    traffic: Arc<crate::traffic::ConnectionTraffic>,
}
impl SshConnection {
    pub fn new(socket: Arc<Socket>, tunnel: Option<TunnelGuard>) -> AppResult<Self> {
        let mut session = Session::new()?;
        let traffic = Arc::new(crate::traffic::ConnectionTraffic::default());
        let stream = meter::MeteredSocket::install(&session, socket.try_clone()?, traffic.clone());
        session.set_tcp_stream(stream);
        session.set_timeout(12000);
        session.set_blocking(false);
        Ok(Self {
            session,
            socket,
            tunnel,
            traffic,
        })
    }
    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn traffic(&self) -> Arc<crate::traffic::ConnectionTraffic> {
        self.traffic.clone()
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
    use forwarding::{ForwardCore, Target, Update};
    let (client, bridge) = socket_pair()?;
    let client = Arc::new(client);
    client.nonblocking()?;
    bridge.nonblocking()?;
    cancel.register(&client)?;
    let wake = bridge.try_clone()?;
    let mut core = ForwardCore::new(parent);
    core.enqueue(
        1,
        Some(bridge),
        Target {
            host: host.into(),
            port,
        },
        ("127.0.0.1".into(), 0),
    )?;
    let poller = polling::Poller::new()?;
    // Complete the first channel open here so failures retain the original
    // connection stage (jump forwarding, before the target SSH handshake).
    loop {
        cancel.check()?;
        let progress = core.drive()?;
        let mut opened = false;
        for update in core.updates.drain(..) {
            match update {
                Update::Opened(1) => opened = true,
                Update::Failed(1, e) => {
                    return Err(AppError::new(
                        "forwarding",
                        format!("跳板无法转发到 {host}:{port}：{}", e.message),
                    ))
                }
                _ => {}
            }
        }
        if opened {
            break;
        }
        if !progress {
            core.wait(&poller, None, Duration::from_millis(100))?;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let worker_stop = stop.clone();
    let worker_error = error.clone();
    let worker_cancel = cancel.clone();
    let worker = thread::Builder::new()
        .name("ssh-jump".into())
        .spawn(move || {
            let result: AppResult<()> = (|| {
                while !worker_stop.load(Ordering::SeqCst) && !worker_cancel.is_cancelled() {
                    let progress = core.drive()?;
                    core.updates.clear();
                    if core.len() == 0 {
                        break;
                    }
                    if !progress {
                        core.wait(&poller, None, Duration::from_millis(100))?;
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                if !worker_stop.load(Ordering::SeqCst) && !worker_cancel.is_cancelled() {
                    *worker_error.lock().unwrap() = Some(AppError::new(
                        "network",
                        format!("跳板转发中断：{}", e.message),
                    ));
                }
            }
            // core owns and releases bridge, channel and parent together.
        })?;
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
