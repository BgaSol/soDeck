pub mod connector;
pub mod executor;
mod headers;
mod http;
mod probe;
mod socks5;
#[cfg(test)]
mod tests;

use crate::model::{AppError, AppResult};
use connector::{Connector, Lease};
use polling::Poller;
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader, ReadBuf},
    runtime::Handle,
    sync::{watch, OwnedSemaphorePermit, Semaphore},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetOutcome {
    pub at: u64,
    pub source: &'static str,
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub stage: String,
    pub success: bool,
    pub message: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub protocol: ProbeProtocol,
    pub target: ProbeTarget,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeProtocol {
    HttpConnect,
    Socks5,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTarget {
    pub host: String,
    pub port: u16,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub probe_id: String,
    pub run_id: String,
    pub protocol: ProbeProtocol,
    pub target: ProbeTarget,
    pub started_at: u64,
    pub finished_at: u64,
    pub success: bool,
    pub stage: String,
    pub message: String,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub negotiating_connections: usize,
    pub waiting_channels: usize,
    pub idle_clients: usize,
    pub active_target_channels: usize,
    pub last_probe: Option<ProbeResult>,
    pub last_connection: Option<TargetOutcome>,
}
#[derive(Default)]
pub struct Metrics {
    pub last_connection: Option<TargetOutcome>,
    pub last_probe: Option<ProbeResult>,
}
impl Metrics {
    fn target(&mut self, lease: &Lease, success: bool, stage: &str) {
        if lease.client.probe {
            return;
        }
        self.last_connection = Some(TargetOutcome {
            at: crate::traffic::now(),
            source: "connection",
            host: lease.target.host.clone(),
            port: lease.target.port,
            protocol: lease.protocol.into(),
            stage: stage.into(),
            success,
            message: if success {
                "目标 TCP 通路已连接"
            } else {
                "目标通路未能完成，请检查目标与 SSH 转发权限"
            }
            .into(),
        });
    }
}
#[derive(Default)]
struct Tasks {
    count: Mutex<usize>,
    done: Condvar,
}
struct TaskGuard(Arc<Tasks>);
impl Drop for TaskGuard {
    fn drop(&mut self) {
        let mut n = self.0.count.lock().unwrap();
        *n -= 1;
        self.0.done.notify_all();
    }
}
pub struct Route {
    pub wake: Arc<Poller>,
    handle: Handle,
    closed: watch::Sender<bool>,
    tasks: Arc<Tasks>,
    clients: Arc<Semaphore>,
    negotiating: Arc<Semaphore>,
    pub open_slots: Arc<Semaphore>,
    global: Arc<AtomicUsize>,
    pub sent: AtomicU64,
    pub received: AtomicU64,
    pub metrics: Mutex<Metrics>,
    pub probes: Mutex<probe::Probes>,
}
impl Route {
    pub fn new(handle: Handle, wake: Arc<Poller>, global: Arc<AtomicUsize>) -> Arc<Self> {
        let (closed, _) = watch::channel(false);
        Arc::new(Self {
            wake,
            handle,
            closed,
            tasks: Arc::new(Tasks::default()),
            clients: Arc::new(Semaphore::new(64)),
            negotiating: Arc::new(Semaphore::new(16)),
            open_slots: Arc::new(Semaphore::new(8)),
            global,
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            metrics: Mutex::new(Metrics::default()),
            probes: Mutex::new(probe::Probes::default()),
        })
    }
    pub fn closed(&self) -> bool {
        *self.closed.borrow()
    }
    pub fn close(&self) {
        self.closed.send_replace(true);
    }
    pub fn spawn(
        self: &Arc<Self>,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        *self.tasks.count.lock().unwrap() += 1;
        let guard = TaskGuard(self.tasks.clone());
        let mut closed = self.closed.subscribe();
        self.handle.spawn(async move {
            let _guard = guard;
            if *closed.borrow() {
                return;
            }
            tokio::select! { _ = closed.changed() => {}, _ = future => {} }
        })
    }
    pub fn wait(&self) {
        let n = self.tasks.count.lock().unwrap();
        let mut n = self.tasks.done.wait_while(n, |n| *n > 0).unwrap();
        *n = 0;
    }
    pub fn runtime_info(&self, waiting: usize, active: usize) -> RuntimeInfo {
        let clients = 64 - self.clients.available_permits();
        let negotiating = 16 - self.negotiating.available_permits();
        let metrics = self.metrics.lock().unwrap();
        RuntimeInfo {
            negotiating_connections: negotiating,
            waiting_channels: waiting,
            active_target_channels: active,
            idle_clients: clients.saturating_sub(negotiating + waiting + active),
            last_probe: metrics.last_probe.clone(),
            last_connection: metrics.last_connection.clone(),
        }
    }
    pub fn accept(
        self: &Arc<Self>,
        connector: Connector,
        socket: std::net::TcpStream,
        origin: SocketAddr,
        local: SocketAddr,
    ) -> AppResult<()> {
        let slot = self
            .clients
            .clone()
            .try_acquire_owned()
            .map_err(|_| connector::busy())?;
        let negotiating = self
            .negotiating
            .clone()
            .try_acquire_owned()
            .map_err(|_| connector::busy())?;
        self.global
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < 256).then_some(n + 1)
            })
            .map_err(|_| connector::busy())?;
        let probe = self.probes.lock().unwrap().consume(origin, local);
        let client = Arc::new(Client {
            route: self.clone(),
            origin,
            probe,
            _slot: slot,
            negotiating: Mutex::new(Some(negotiating)),
            count_received: AtomicBool::new(false),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            previous: Mutex::new(None),
        });
        socket.set_nonblocking(true)?;
        let _ = socket.set_nodelay(true);
        self.spawn(async move {
            let result = async {
                let socket = tokio::net::TcpStream::from_std(socket)?;
                let mut io = BufReader::with_capacity(
                    4096,
                    ClientIo {
                        socket,
                        client: client.clone(),
                    },
                );
                let bytes = tokio::time::timeout(Duration::from_secs(10), io.fill_buf())
                    .await
                    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
                let first = bytes
                    .first()
                    .copied()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
                if first == 5 {
                    socks5::serve(io, client, connector).await
                } else if first.is_ascii_uppercase() {
                    http::serve(io, client, connector).await
                } else {
                    Err(io::Error::from(io::ErrorKind::InvalidData))
                }
            }
            .await;
            // Protocol errors use fixed categories; never log HTTP parser text.
            let _ = result;
        });
        Ok(())
    }
}
pub struct Client {
    pub route: Arc<Route>,
    pub origin: SocketAddr,
    pub probe: bool,
    _slot: OwnedSemaphorePermit,
    negotiating: Mutex<Option<OwnedSemaphorePermit>>,
    pub count_received: AtomicBool,
    pub gate: Arc<tokio::sync::Mutex<()>>,
    pub previous: Mutex<Option<std::sync::Weak<Lease>>>,
}
impl Client {
    pub fn negotiated(&self) {
        self.negotiating.lock().unwrap().take();
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.route.global.fetch_sub(1, Ordering::SeqCst);
    }
}
pub struct ClientIo {
    socket: tokio::net::TcpStream,
    client: Arc<Client>,
}
impl AsyncRead for ClientIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, b)
    }
}
impl AsyncWrite for ClientIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write(cx, b);
        if let Poll::Ready(Ok(n)) = result {
            if !self.client.probe && self.client.count_received.load(Ordering::Relaxed) {
                self.client
                    .route
                    .received
                    .fetch_add(n as u64, Ordering::Relaxed);
            }
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}
pub fn validate_target(
    host: &str,
    port: u16,
) -> AppResult<crate::ssh_transport::forwarding::Target> {
    let host = host.trim();
    let host = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };
    if host.is_empty()
        || host.len() > 253
        || !host.is_ascii()
        || port == 0
        || host
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "/\\@?#[]%".contains(c))
        || (host.contains(':') && host.parse::<std::net::Ipv6Addr>().is_err())
    {
        return Err(AppError::new(
            "validation",
            "请输入有效的目标主机和端口（1–65535）",
        ));
    }
    Ok(crate::ssh_transport::forwarding::Target {
        host: host.into(),
        port,
    })
}
