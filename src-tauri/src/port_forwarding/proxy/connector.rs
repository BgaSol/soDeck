use super::{Client, Route};
use crate::{
    model::{AppError, AppResult},
    ssh_transport::{
        forwarding::{ForwardCore, Target, Update},
        pipe::{self, ProxyStream, WorkerPipe},
    },
};
use std::{
    collections::HashMap,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::watch,
};

#[derive(Clone)]
enum Phase {
    Opening,
    Open,
    Active,
    Failed(AppError),
    Finished(AppResult<()>),
}
pub struct Lease {
    pub id: u64,
    pub client: Arc<Client>,
    pub target: Target,
    pub protocol: &'static str,
    cancel: AtomicBool,
    activate: AtomicBool,
    phase: watch::Sender<Phase>,
    pending: Mutex<Option<tokio::sync::OwnedSemaphorePermit>>,
}
impl Lease {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.client.route.wake.notify();
    }
    pub async fn finished(&self) {
        let mut rx = self.phase.subscribe();
        loop {
            if matches!(*rx.borrow_and_update(), Phase::Finished(_)) {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}
struct Open {
    pipe: WorkerPipe,
    lease: Arc<Lease>,
}
pub struct Bridge {
    route: Arc<Route>,
    receiver: Receiver<Open>,
    flows: HashMap<u64, (Arc<Lease>, bool)>,
}
#[derive(Clone)]
pub struct Connector {
    sender: SyncSender<Open>,
    next: Arc<std::sync::atomic::AtomicU64>,
    registration: Option<(
        std::sync::Weak<crate::State>,
        Arc<std::sync::atomic::AtomicU64>,
    )>,
}
pub fn bridge(route: Arc<Route>) -> (Connector, Bridge) {
    let (sender, receiver) = mpsc::sync_channel(32);
    (
        Connector {
            sender,
            next: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            registration: None,
        },
        Bridge {
            route,
            receiver,
            flows: HashMap::new(),
        },
    )
}
pub struct PendingTunnel(Option<Tunnel>);
pub struct Tunnel {
    stream: ProxyStream,
    pub lease: Arc<Lease>,
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        self.lease.cancel();
    }
}
impl Connector {
    pub fn with_registration(
        mut self,
        state: &Arc<crate::State>,
        generation: Arc<std::sync::atomic::AtomicU64>,
    ) -> Self {
        self.registration = Some((Arc::downgrade(state), generation));
        self
    }
    pub async fn open(
        &self,
        client: Arc<Client>,
        target: Target,
        protocol: &'static str,
    ) -> AppResult<PendingTunnel> {
        if client.route.closed() {
            return Err(unavailable());
        }
        let previous = client
            .previous
            .lock()
            .unwrap()
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        if let Some(previous) = previous {
            previous.finished().await;
        }
        // Include the outer queue in the core's eight pending-open budget.
        let pending = client
            .route
            .open_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        let (pipe, stream) = pipe::pair(client.route.wake.clone());
        let (phase, mut rx) = watch::channel(Phase::Opening);
        let lease = Arc::new(Lease {
            id: self.next.fetch_add(1, Ordering::Relaxed),
            client,
            target,
            protocol,
            cancel: AtomicBool::new(false),
            activate: AtomicBool::new(false),
            phase,
            pending: Mutex::new(Some(pending)),
        });
        *lease.client.previous.lock().unwrap() = Some(Arc::downgrade(&lease));
        let tunnel = Tunnel {
            stream,
            lease: lease.clone(),
        };
        if let Some((state, generation)) = &self.registration {
            let state = state.upgrade().ok_or_else(unavailable)?;
            let _gate = state.configuration.lock().unwrap();
            state.ensure_running()?;
            self.sender
                .try_send(Open { pipe, lease })
                .map_err(|_| busy())?;
            generation.fetch_add(1, Ordering::SeqCst);
        } else {
            self.sender
                .try_send(Open { pipe, lease })
                .map_err(|_| busy())?;
        }
        let _ = tunnel.lease.client.route.wake.notify();
        loop {
            let phase = rx.borrow_and_update().clone();
            match phase {
                Phase::Open => {
                    return Ok(PendingTunnel(Some(tunnel)));
                }
                Phase::Finished(result) => return Err(result.err().unwrap_or_else(unavailable)),
                Phase::Failed(e) => return Err(e),
                _ => {}
            }
            rx.changed().await.map_err(|_| unavailable())?;
        }
    }
}
impl PendingTunnel {
    pub async fn activate(mut self) -> AppResult<Tunnel> {
        let tunnel = self.0.take().unwrap();
        let mut rx = tunnel.lease.phase.subscribe();
        tunnel.lease.activate.store(true, Ordering::SeqCst);
        let _ = tunnel.lease.client.route.wake.notify();
        loop {
            let phase = rx.borrow_and_update().clone();
            match phase {
                Phase::Active => return Ok(tunnel),
                Phase::Finished(result) => return Err(result.err().unwrap_or_else(unavailable)),
                Phase::Failed(e) => return Err(e),
                _ => {}
            }
            rx.changed().await.map_err(|_| unavailable())?;
        }
    }
}
pub fn unavailable() -> AppError {
    AppError::new("network", "SSH 路径已结束")
}
pub fn busy() -> AppError {
    AppError::new("busy", "代理连接或等待队列已满，请稍后重试")
}
impl Bridge {
    pub fn poll(&mut self, core: &mut ForwardCore) {
        for open in self.receiver.try_iter().take(16) {
            let lease = open.lease;
            if lease.cancel.load(Ordering::SeqCst) {
                lease
                    .phase
                    .send_replace(Phase::Finished(Err(unavailable())));
                continue;
            }
            let origin = lease.client.origin;
            match core.enqueue_suspended(
                lease.id,
                open.pipe,
                lease.target.clone(),
                (origin.ip().to_string(), origin.port()),
            ) {
                Ok(()) => {
                    self.flows.insert(lease.id, (lease, false));
                }
                Err(e) => {
                    lease.phase.send_replace(Phase::Finished(Err(e)));
                }
            }
        }
        for (lease, cancelled) in self.flows.values_mut() {
            if lease.cancel.load(Ordering::SeqCst) && !*cancelled {
                core.cancel(lease.id);
                *cancelled = true;
            } else if !*cancelled && lease.activate.swap(false, Ordering::SeqCst) {
                core.activate(lease.id);
            }
        }
    }
    pub fn update(&mut self, update: Update) {
        match update {
            Update::Opened(id) => {
                if let Some((l, _)) = self.flows.get(&id) {
                    l.pending.lock().unwrap().take();
                    l.phase.send_replace(Phase::Open);
                    self.route
                        .metrics
                        .lock()
                        .unwrap()
                        .target(l, true, "channelOpen");
                }
            }
            Update::Activated(id) => {
                if let Some((l, _)) = self.flows.get(&id) {
                    l.phase.send_replace(Phase::Active);
                }
            }
            Update::Failed(id, e) => {
                if let Some((l, _)) = self.flows.get(&id) {
                    l.phase.send_replace(Phase::Failed(e));
                    self.route
                        .metrics
                        .lock()
                        .unwrap()
                        .target(l, false, "channelOpen");
                }
            }
            Update::Finished(id) => {
                if let Some((l, _)) = self.flows.remove(&id) {
                    l.pending.lock().unwrap().take();
                    let result = match &*l.phase.borrow() {
                        Phase::Failed(e) => Err(e.clone()),
                        _ => Ok(()),
                    };
                    l.phase.send_replace(Phase::Finished(result));
                }
            }
            Update::Bytes { id, sent, .. } => {
                if let Some((l, _)) = self.flows.get(&id) {
                    if !l.client.probe {
                        self.route.sent.fetch_add(sent, Ordering::Relaxed);
                    }
                }
            }
        }
    }
    pub fn counts(&self) -> (usize, usize) {
        let opening = self
            .flows
            .values()
            .filter(|(l, _)| matches!(*l.phase.borrow(), Phase::Opening))
            .count();
        (opening, self.flows.len() - opening)
    }
    pub fn close(&mut self) {
        self.route.close();
        for (_, (lease, _)) in self.flows.drain() {
            lease
                .phase
                .send_replace(Phase::Finished(Err(unavailable())));
        }
        for open in self.receiver.try_iter() {
            open.lease
                .phase
                .send_replace(Phase::Finished(Err(unavailable())));
        }
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.close();
    }
}
impl AsyncRead for Tunnel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, b)
    }
}
impl AsyncWrite for Tunnel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
