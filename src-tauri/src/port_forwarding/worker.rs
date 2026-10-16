use super::{model::*, Control, Run};
use crate::{
    connection::{self, ConnectEventSink, ConnectionEvent, ConnectionPlan, SharedContext},
    model::{AppError, AppResult},
    ssh_transport::{
        forwarding::{ForwardCore, Target, Update},
        Cancellation, Socket, SshConnection,
    },
    traffic::{now, TrafficPurpose},
    State,
};
use polling::{Event, Events};
use std::{
    collections::HashSet,
    net::{Shutdown, SocketAddr, TcpListener},
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

struct Sink {
    state: Arc<State>,
    run: Arc<Run>,
    attempt: String,
}
impl ConnectEventSink for Sink {
    fn send(&self, event: ConnectionEvent) -> AppResult<()> {
        let owner = self.run.owner.lock().unwrap().clone();
        let current = self.state.owner(&owner.label);
        if self.run.stopped.load(Ordering::SeqCst)
            || current.generation != owner.generation
            || self
                .state
                .closed_windows
                .lock()
                .unwrap()
                .contains(&owner.label)
        {
            return Err(AppError::new("cancelled", "认证窗口已失效"));
        }
        let mut view = self.run.view.lock().unwrap();
        match &event {
            ConnectionEvent::AuthRequired { .. } | ConnectionEvent::HostKeyRequired { .. } => {
                view.phase = "authenticating".into();
                view.prompt = Some(Prompt {
                    attempt_id: self.attempt.clone(),
                    event,
                });
            }
            ConnectionEvent::Progress { hop, stage } => {
                view.prompt = None;
                view.note(format!("{} · {}", hop.server.name, stage));
            }
            _ => {}
        }
        Ok(())
    }
}

fn listen(settings: &Settings) -> AppResult<TcpListener> {
    let address: SocketAddr = SocketAddr::new(
        settings
            .listen_address
            .parse()
            .map_err(|_| AppError::new("validation", "无效的本地地址"))?,
        settings.listen_port.unwrap_or(0),
    );
    let socket = socket2::Socket::new(
        if address.is_ipv6() {
            socket2::Domain::IPV6
        } else {
            socket2::Domain::IPV4
        },
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    if address.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            setsockopt, WSAGetLastError, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
        };
        let enabled: i32 = 1;
        // Winsock has no socket2 wrapper for this option. The owned socket and
        // fixed-size option remain valid for the whole synchronous call.
        if unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&enabled as *const i32).cast(),
                std::mem::size_of_val(&enabled) as i32,
            )
        } != 0
        {
            return Err(std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() }).into());
        }
    }
    socket
        .bind(&address.into())
        .map_err(|e| AppError::new("listen", format!("无法监听 {address}：{e}")))?;
    socket.listen(64)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

type RouteResult = AppResult<(SshConnection, SharedContext)>;
struct RouteJob {
    result: Receiver<RouteResult>,
    join: thread::JoinHandle<()>,
}
fn connect(
    state: &Arc<State>,
    run: &Arc<Run>,
    plan: &Arc<ConnectionPlan>,
    interactive: bool,
) -> AppResult<RouteJob> {
    let state = state.clone();
    let run = run.clone();
    let context = run.context.lock().unwrap().clone();
    // An existing run always reconnects its original route, including accepted credentials.
    let plan = context
        .as_ref()
        .map(|c| Arc::new(c.plan()))
        .unwrap_or_else(|| plan.clone());
    let cancel = Cancellation::default();
    *run.cancel.lock().unwrap() = Some(cancel.clone());
    if run.stopped.load(Ordering::SeqCst) {
        cancel.cancel();
    }
    let attempt = uuid::Uuid::new_v4().to_string();
    let interaction = Arc::new(connection::Interaction::default());
    if interactive {
        *run.interaction.lock().unwrap() = Some((attempt.clone(), interaction.clone()));
    }
    let (send, receive) = mpsc::sync_channel(1);
    let join = thread::Builder::new()
        .name("forward-route".into())
        .spawn(move || {
            let result = if interactive {
                connection::connect_interactive(
                    &state,
                    &plan,
                    Arc::new(Sink {
                        state: state.clone(),
                        run: run.clone(),
                        attempt,
                    }),
                    interaction,
                    context,
                    &cancel,
                    TrafficPurpose::PortForward,
                )
            } else {
                let context = context.expect("automatic reconnect requires verified context");
                connection::connect_verified(&context, &cancel, TrafficPurpose::PortForward)
                    .map(|c| (c, context))
            };
            // Dropping a late result releases its entire SSH path.
            let _ = send.send(result);
            let _ = run.wake.notify();
        })?;
    Ok(RouteJob {
        result: receive,
        join,
    })
}

fn wait(run: &Run, listener: &TcpListener) -> AppResult<()> {
    unsafe {
        run.wake.add(listener, Event::readable(0))?;
    }
    let result = run
        .wake
        .wait(&mut Events::new(), Some(Duration::from_millis(200)));
    let _ = run.wake.delete(listener);
    result.map(|_| ()).map_err(Into::into)
}

// Permits include pending opens. RAII returns them even when the route is lost.
struct Clients {
    state: Arc<State>,
    ids: HashSet<u64>,
}
impl Clients {
    fn reserve(&mut self, id: u64) -> bool {
        if self
            .state
            .port_forwarding
            .total_connections
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < 256).then_some(n + 1)
            })
            .is_err()
        {
            return false;
        }
        self.ids.insert(id);
        true
    }
    fn remove(&mut self, id: u64) {
        if self.ids.remove(&id) {
            self.state
                .port_forwarding
                .total_connections
                .fetch_sub(1, Ordering::SeqCst);
        }
    }
    fn clear(&mut self) {
        self.state
            .port_forwarding
            .total_connections
            .fetch_sub(self.ids.len(), Ordering::SeqCst);
        self.ids.clear();
    }
}
impl Drop for Clients {
    fn drop(&mut self) {
        self.clear();
    }
}

pub(super) fn run(
    state: Arc<State>,
    run: Arc<Run>,
    rule: Rule,
    plan: Arc<ConnectionPlan>,
    commands: Receiver<Control>,
) {
    let result = serve(&state, &run, &rule, &plan, commands);
    // serve has dropped listeners, route jobs and channels before publishing completion.
    run.context.lock().unwrap().take();
    run.cancel.lock().unwrap().take();
    run.interaction.lock().unwrap().take();
    drop(plan);
    let mut view = run.view.lock().unwrap();
    view.active_connections = 0;
    view.sent_per_second = 0.0;
    view.received_per_second = 0.0;
    view.retry_at = None;
    view.probe_id = None;
    view.actual_address = None;
    if run.stopped.load(Ordering::SeqCst) {
        view.phase("stopped");
        view.note("已停止，监听端口与连接已释放");
    } else if let Err(error) = result {
        view.phase(if run.ever_ready.load(Ordering::SeqCst) {
            "stopped"
        } else {
            "failed"
        });
        view.note(error.message.clone());
        view.error = Some(error);
    }
    run.done.store(true, Ordering::SeqCst);
}

fn serve(
    state: &Arc<State>,
    run: &Arc<Run>,
    rule: &Rule,
    plan: &Arc<ConnectionPlan>,
    commands: Receiver<Control>,
) -> AppResult<()> {
    let listener = listen(&rule.settings)?;
    run.view.lock().unwrap().actual_address = Some(listener.local_addr()?.to_string());
    let target = Target {
        host: rule.settings.target_host.clone(),
        port: rule.settings.target_port,
    };
    let mut core: Option<ForwardCore> = None;
    let mut job = Some(connect(state, run, plan, true)?);
    let mut clients = Clients {
        state: state.clone(),
        ids: HashSet::new(),
    };
    let mut next_id = 1u64;
    let mut probe: Option<(String, u64)> = None;
    let mut retry_at: Option<Instant> = None;
    let mut failures = 0u32;
    let (mut sent, mut received, mut last_sent, mut last_received) = (0u64, 0u64, 0u64, 0u64);
    let mut sampled = Instant::now();
    let result = (|| loop {
        if run.stopped.load(Ordering::SeqCst) {
            break Ok(());
        }
        for command in commands.try_iter().take(16) {
            match command {
                Control::Resume(owner) if job.is_none() && core.is_none() => {
                    *run.owner.lock().unwrap() = owner;
                    run.view.lock().unwrap().phase("reconnecting");
                    retry_at = None;
                    job = Some(connect(state, run, plan, true)?);
                }
                Control::CancelAuth(attempt) => {
                    if run
                        .interaction
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|(id, _)| id == &attempt)
                    {
                        if let Some(cancel) = run.cancel.lock().unwrap().as_ref() {
                            cancel.cancel();
                        }
                    }
                }
                Control::Probe(uuid) => {
                    if let Some(core) = &mut core {
                        if probe.is_none() {
                            let id = next_id;
                            next_id += 1;
                            match core.enqueue(
                                id,
                                None,
                                target.clone(),
                                (rule.settings.listen_address.clone(), 0),
                            ) {
                                Ok(()) => {
                                    probe = Some((uuid.clone(), id));
                                    run.view.lock().unwrap().probe_id = Some(uuid);
                                }
                                Err(e) => {
                                    let mut view = run.view.lock().unwrap();
                                    view.probe_id = None;
                                    view.note(e.message);
                                }
                            }
                        }
                    }
                }
                Control::CancelProbe(uuid) => {
                    if probe.as_ref().is_some_and(|(p, _)| p == &uuid) {
                        let (_, id) = probe.take().unwrap();
                        if let Some(core) = &mut core {
                            core.cancel(id);
                        }
                        run.view.lock().unwrap().probe_id = None;
                    }
                }
                _ => {}
            }
        }
        let route_result = job.as_ref().and_then(|j| match j.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(AppError::new("internal", "连接任务异常退出")))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        let mut failure = None;
        if let Some(result) = route_result {
            let old = job.take().unwrap();
            let _ = old.join.join();
            run.interaction.lock().unwrap().take();
            if run.stopped.load(Ordering::SeqCst) {
                drop(result);
                break Ok(());
            }
            match result {
                Ok((connection, context)) => {
                    let mut view = run.view.lock().unwrap();
                    view.connection = Some(context.view());
                    view.phase("ready");
                    view.error = None;
                    view.retry_at = None;
                    view.note("SSH 路径已就绪，可以连接本地端口");
                    *run.context.lock().unwrap() = Some(context);
                    core = Some(ForwardCore::new(connection));
                    run.ever_ready.store(true, Ordering::SeqCst);
                    failures = 0;
                }
                Err(e) => failure = Some(e),
            }
        }
        // Drain a bounded accept batch even while authentication/reconnect is pending.
        for _ in 0..32 {
            match listener.accept() {
                Ok((socket, address)) => {
                    if let Some(core) = &mut core {
                        let id = next_id;
                        next_id += 1;
                        if clients.reserve(id) {
                            let _ = socket.set_nodelay(true);
                            if let Err(e) = core.enqueue(
                                id,
                                Some(Socket::Tcp(socket)),
                                target.clone(),
                                (address.ip().to_string(), address.port()),
                            ) {
                                clients.remove(id);
                                run.view.lock().unwrap().note(e.message);
                            }
                        } else {
                            let _ = socket.shutdown(Shutdown::Both);
                            run.view
                                .lock()
                                .unwrap()
                                .note("全局连接数已达 256，已拒绝新连接");
                        }
                    } else {
                        let _ = socket.shutdown(Shutdown::Both);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        let mut progress = false;
        if let Some(core) = &mut core {
            match core.drive() {
                Ok(p) => progress = p,
                Err(e) => failure = Some(e),
            }
            let mut view = run.view.lock().unwrap();
            for update in core.updates.drain(..) {
                match update {
                    Update::Bytes {
                        sent: s,
                        received: r,
                    } => {
                        sent = sent.saturating_add(s);
                        received = received.saturating_add(r);
                    }
                    Update::Opened(id) => {
                        view.target_result = Some(Note {
                            at: now(),
                            message: format!("目标 {}:{} TCP 连接成功", target.host, target.port),
                            count: 1,
                        });
                        if probe.as_ref().is_some_and(|(_, p)| *p == id) {
                            probe = None;
                            view.probe_id = None;
                        }
                    }
                    Update::Failed(id, e) => {
                        clients.remove(id);
                        view.note(format!(
                            "目标 {}:{}：{}",
                            target.host, target.port, e.message
                        ));
                        view.target_result = Some(Note {
                            at: now(),
                            message: e.message,
                            count: 1,
                        });
                        if probe.as_ref().is_some_and(|(_, p)| *p == id) {
                            probe = None;
                            view.probe_id = None;
                        }
                    }
                    Update::Finished(id) => clients.remove(id),
                }
            }
            view.active_connections = clients.ids.len();
        }
        if let Some(error) = failure {
            core = None;
            clients.clear();
            probe = None;
            run.cancel.lock().unwrap().take();
            if !run.ever_ready.load(Ordering::SeqCst) || !rule.settings.auto_reconnect {
                break Err(error);
            }
            let mut view = run.view.lock().unwrap();
            view.active_connections = 0;
            view.probe_id = None;
            view.note(format!("路径中断，旧业务连接已关闭：{}", error.message));
            let retryable = matches!(error.kind.as_str(), "network" | "timeout" | "io");
            view.error = Some(error);
            if retryable {
                let seconds = (1u64 << failures.min(5)).min(30);
                failures = failures.saturating_add(1);
                let delay = Duration::from_millis(seconds * 1000 + (now() % 251));
                retry_at = Some(Instant::now() + delay);
                view.phase("reconnecting");
                view.retry_at = Some(now() + delay.as_millis() as u64);
            } else {
                retry_at = None;
                view.retry_at = None;
                view.phase("attention");
            }
        }
        if job.is_none() && retry_at.is_some_and(|at| Instant::now() >= at) {
            retry_at = None;
            let mut view = run.view.lock().unwrap();
            view.attempts = view.attempts.saturating_add(1);
            view.retry_at = None;
            drop(view);
            job = Some(connect(state, run, plan, false)?);
        }
        if sampled.elapsed() >= Duration::from_secs(1) {
            let elapsed = sampled.elapsed().as_secs_f64();
            let mut view = run.view.lock().unwrap();
            view.sent_bytes = sent.to_string();
            view.received_bytes = received.to_string();
            view.sent_per_second = (sent - last_sent) as f64 / elapsed;
            view.received_per_second = (received - last_received) as f64 / elapsed;
            last_sent = sent;
            last_received = received;
            sampled = Instant::now();
        }
        if !progress {
            if let Some(core) = &core {
                core.wait(&run.wake, Some(&listener), Duration::from_millis(200))?;
            } else {
                wait(run, &listener)?;
            }
        }
    })();
    // Route worker owns socket cleanup; never advertise stopped while it is alive.
    if let Some(job) = job {
        if let Some(cancel) = run.cancel.lock().unwrap().as_ref() {
            cancel.cancel();
        }
        let _ = job.join.join();
        drop(job.result);
    }
    run.view.lock().unwrap().sent_bytes = sent.to_string();
    run.view.lock().unwrap().received_bytes = received.to_string();
    result
}
