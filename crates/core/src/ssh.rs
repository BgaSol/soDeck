use crate::{
    model::{id, CoreError, Result, Server},
    secrets::Credential,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ssh2::{Channel, Session};
pub(crate) mod remote;
pub(crate) mod observer;
pub(crate) mod network;
mod transport;
mod route_executor;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const OUTPUT_BUDGET: usize = 2 * 1024 * 1024;
const INPUT_BUDGET: usize = 256 * 1024;

#[derive(Default)]
pub(crate) struct TransportBudget { pub sessions: usize, pub bridges: usize, pub proxy_contexts: usize, pub snapshot_bytes: usize }
pub(crate) struct TransportReservation { budget: Arc<Mutex<TransportBudget>>, cost: crate::route::budget::Cost }
impl TransportReservation {
    fn reserve(budget: Arc<Mutex<TransportBudget>>, cost: &crate::route::budget::Cost) -> Result<Self> {
        {
            let mut used = budget.lock().unwrap();
            if cost.sessions > 256 - used.sessions || cost.bridges > 256 - used.bridges
                || cost.proxy_contexts > 512 - used.proxy_contexts || cost.snapshot_bytes > 16*1024*1024 - used.snapshot_bytes {
                return Err(CoreError::new("CONNECTION_RESOURCES_EXHAUSTED", "连接资源不足，请释放其他连接后重试").details(json!({"required":cost,"available":{"sessions":256-used.sessions,"bridges":256-used.bridges,"proxyContexts":512-used.proxy_contexts,"snapshotBytes":16*1024*1024-used.snapshot_bytes}})));
            }
            used.sessions += cost.sessions; used.bridges += cost.bridges;
            used.proxy_contexts += cost.proxy_contexts; used.snapshot_bytes += cost.snapshot_bytes;
        }
        Ok(Self { budget, cost: cost.clone() })
    }
}
impl Drop for TransportReservation {
    fn drop(&mut self) {
        let mut used = self.budget.lock().unwrap();
        used.sessions -= self.cost.sessions; used.bridges -= self.cost.bridges;
        used.proxy_contexts -= self.cost.proxy_contexts; used.snapshot_bytes -= self.cost.snapshot_bytes;
    }
}

#[derive(Clone)]
pub struct Frame {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}
pub struct PtyState {
    pub observer: Option<observer::Observer>,
    pub frames: VecDeque<Frame>,
    pub buffered: usize,
    pub last_sequence: u64,
    pub acknowledged: u64,
    pub last_input: u64,
    pub queued_input: usize,
    pub state: String,
    pub reason: Option<String>,
    pub view_id: Option<String>,
    pub mount_generation: Option<u64>,
    pub cols: u32,
    pub rows: u32,
    close_requested: bool,
}
impl PtyState {
    fn new(cols: u32, rows: u32) -> Self {
        Self {
            observer:None,
            frames: VecDeque::new(),
            buffered: 0,
            last_sequence: 0,
            acknowledged: 0,
            last_input: 0,
            queued_input: 0,
            state: "active".into(),
            reason: None,
            view_id: None,
            mount_generation: None,
            cols,
            rows,
            close_requested: false,
        }
    }
    pub fn read(&self, after: u64) -> Result<Value> {
        if after < self.acknowledged {
            return Err(CoreError::new(
                "STREAM_CURSOR_EXPIRED",
                "输出游标已过期，需恢复显示快照",
            ));
        }
        if after > self.last_sequence {
            return Err(CoreError::new("INVALID_CURSOR", "输出游标超出已产生的数据"));
        }
        let mut bytes = 0;
        let frames: Vec<Value> = self
            .frames
            .iter()
            .filter(|frame| frame.sequence > after)
            .take_while(|frame| {
                bytes += frame.bytes.len();
                bytes <= 256 * 1024
            })
            .map(|frame| json!({"sequence":frame.sequence,"bytes":STANDARD.encode(&frame.bytes)}))
            .collect();
        Ok(
            json!({"frames":frames,"state":self.state,"reason":self.reason,"lastSequence":self.last_sequence,"acknowledged":self.acknowledged}),
        )
    }
    pub fn ack(&mut self, sequence: u64) -> Result<()> {
        if sequence > self.last_sequence {
            return Err(CoreError::new("INVALID_CURSOR", "不能确认未产生的数据"));
        }
        self.acknowledged = self.acknowledged.max(sequence);
        while self
            .frames
            .front()
            .is_some_and(|frame| frame.sequence <= self.acknowledged)
        {
            self.buffered -= self.frames.pop_front().unwrap().bytes.len();
        }
        Ok(())
    }
}

enum Command {
    Tcp(network::Open),
    Remote(remote::Job),
    Open {
        traffic: Option<crate::traffic::Registration>,
        command:Option<String>,
        id: String,
        cols: u32,
        rows: u32,
        state: Arc<Mutex<PtyState>>,
        reply: SyncSender<Result<()>>,
    },
    Write {
        id: String,
        bytes: Vec<u8>,
    },
    Resize {
        id: String,
        cols: u32,
        rows: u32,
    },
}
pub struct Connection {
    pub purpose: String,
    pub id: String,
    pub server: Server,
    pub workspace_id: String,
    pub generation: u64,
    pub route: crate::route::RoutePlan,
    pub bridge_metrics: Option<Arc<Mutex<Value>>>,
    pub bridge_segments: Vec<Arc<Mutex<Value>>>,
    pub failure_hop: Arc<AtomicUsize>,
    pub transport_live: Arc<AtomicBool>,
    commands: SyncSender<Command>,
    canceled: Arc<AtomicBool>,
    socket: TcpStream,
    pub status: Arc<Mutex<String>>,
    pub lifecycle: Arc<Mutex<Option<Value>>>,
    pub remote_metrics: Arc<Mutex<Value>>,
    pub network_metrics: Arc<Mutex<network::Metrics>>,
    traffic: Option<Arc<crate::traffic::Traffic>>,
    probe: crate::quality::probe::Slot,
}
impl Connection {
    pub(crate) fn check_response(&self,budget:&Arc<crate::quality::probe::Budget>)->Result<Arc<crate::quality::probe::Request>> {
        if self.canceled.load(Ordering::SeqCst)||*self.status.lock().unwrap()!="active" {return Err(CoreError::new("DISCONNECTED","没有可用的已认证连接"));}
        let mut slot=self.probe.lock().unwrap();
        if slot.is_some(){return Err(CoreError::new("CAPACITY","该连接正在探测或退役诊断通道"));}
        let caller=crate::Caller{instance_id:"connection-diagnostics".into(),..crate::Caller::shell()};
        let traffic=self.traffic_resource(&caller,&id(),&self.generation.to_string(),"diagnostic",None);
        let probe=crate::quality::probe::Probe::new(budget,traffic)?;let request=probe.request.clone();*slot=Some(probe);Ok(request)
    }
    pub(crate) fn traffic_resource(&self, caller:&crate::Caller, resource:&str, generation:&str, kind:&str,
        workspace_generation:Option<&str>) -> Option<crate::traffic::Registration> {
        self.traffic.as_ref().map(|traffic|traffic.register(crate::traffic::Owner {
            server_id:self.server.id.clone(),config_epoch:traffic.config_epoch.clone(),
            workspace_id:Some(self.workspace_id.clone()),workspace_generation:workspace_generation.map(str::to_owned),
            plugin_id:caller.plugin_id.clone(),instance_id:caller.instance_id.clone(),resource_id:resource.into(),
            generation:generation.into(),kind:kind.into(),route_run_id:self.id.clone(),layer:"channel".into(),access_mode:self.route.access_path.mode().into(),
        }))
    }
    pub(crate) fn open_tcp(&self, open: network::Open) -> Result<()> { self.send(Command::Tcp(open)) }
    pub(crate) fn remote(&self, job: remote::Job) -> Result<()> { self.send(Command::Remote(job)) }
    pub fn connect(
        route: crate::route::RoutePlan,
        workspace_id: String,
        generation: u64,
        canceled: Arc<AtomicBool>,
        operation: &AtomicBool,
        budget: Arc<Mutex<TransportBudget>>,
        verify: impl FnMut(usize, &str, &str, &[String]) -> Result<Credential>,
        mut progress: impl FnMut(usize, &str),
    ) -> Result<Arc<Self>> {
        Self::connect_for("session".into(), route, workspace_id, generation, canceled, operation, budget, None, None, verify, |_,_|Ok(Credential::default()), |index,event|{if event.state=="running"{progress(index,event.stage)}})
    }
    pub fn connect_for(
        purpose: String,
        route: crate::route::RoutePlan,
        workspace_id: String,
        generation: u64,
        canceled: Arc<AtomicBool>,
        operation: &AtomicBool,
        budget: Arc<Mutex<TransportBudget>>,
        traffic: Option<Arc<crate::traffic::Traffic>>,
        observation: Option<Arc<crate::observability::Observability>>,
        mut verify: impl FnMut(usize, &str, &str, &[String]) -> Result<Credential>,
        mut proxy_auth: impl FnMut(usize, &[String])->Result<Credential>,
        mut progress: impl FnMut(usize, crate::connection_diagnostics::PhaseEvent<'_>),
    ) -> Result<Arc<Self>> {
        let reservation = TransportReservation::reserve(budget, &route.cost)?;
        let connection_id=id();
        let deadline=Instant::now()+Duration::from_millis(route.cost.attempt_timeout_ms());
        let transport_failed=Arc::new(AtomicBool::new(false));
        let failure_hop=Arc::new(AtomicUsize::new(usize::MAX));
        let mut hooks=route_executor::ConnectionHooks {
            verify:&mut verify, proxy_auth:&mut proxy_auth, progress:&mut progress,
            traffic:traffic.as_ref(), route:&route, workspace_id:&workspace_id,
            connection_id:&connection_id, generation, registrations:Vec::new(),proxy_registrations:Default::default(),
        };
        let route_executor::Established {session,socket,bridges} = route_executor::Execution {
            steps:&route.steps,canceled:&canceled,operation,deadline,failed:transport_failed.clone(),failure_step:failure_hop.clone(),
        }.connect(&mut hooks).map_err(|mut error| {
            let details=error.details.get_or_insert_with(||json!({}));
            details["targetServerId"]=json!(route.target_server_id);
            details["routeRevision"]=json!(route.revision);
            error
        })?;
        let registrations=std::mem::take(&mut hooks.registrations);
        let proxy_registrations=std::mem::take(&mut hooks.proxy_registrations);
        drop(hooks);
        let (sender,receiver)=mpsc::sync_channel(128);
        let status=Arc::new(Mutex::new("active".into()));
        let lifecycle=Arc::new(Mutex::new(None));
        let remote_metrics=Arc::new(Mutex::new(json!({"sessions":0,"handles":0,"execs":0,"queued":0,"cleanupPending":0})));
        let network_metrics=Arc::new(Mutex::new(network::Metrics::default()));
        let transport_live=Arc::new(AtomicBool::new(true));
        let probe=Arc::new(Mutex::new(None));
        let connection=Arc::new(Self {
            purpose, network_metrics: network_metrics.clone(),
            id:connection_id,server:route.target().clone(),workspace_id,generation,traffic,
            route,bridge_metrics:bridges.0.first().map(|b|b.metrics.clone()),
            bridge_segments:bridges.0.iter().map(|b|b.metrics.clone()).collect(),commands:sender,
            failure_hop:failure_hop.clone(),transport_live:transport_live.clone(),lifecycle:lifecycle.clone(),
            canceled:canceled.clone(),socket,status:status.clone(),remote_metrics:remote_metrics.clone(),probe:probe.clone(),
        });
        let resource=connection.id.clone();let server=connection.server.id.clone();
        if let Some(store)=&observation {store.connection_lifecycle(&resource,generation,&server,true);}
        thread::Builder::new().name("sodeck-ssh-io".into()).spawn(move||{
            run(session,receiver,canceled.clone(),status,remote_metrics,transport_failed.clone(),network_metrics,probe);
            let index=failure_hop.load(Ordering::SeqCst);
            *lifecycle.lock().unwrap()=Some(json!({"state":if transport_failed.load(Ordering::SeqCst)||!canceled.load(Ordering::SeqCst){"disconnected"}else{"closed"},"stepIndex":(index!=usize::MAX).then_some(index),"observedAt":crate::model::now_ms()}));
            drop(bridges);
            drop(registrations);drop(proxy_registrations);
            drop(reservation);
            transport_live.store(false,Ordering::SeqCst);
            if let Some(store)=observation{store.connection_lifecycle(&resource,generation,&server,false);}
        })?;
        Ok(connection)
    }
    pub fn open(
        &self,
        pty_id: &str,
        cols: u32,
        rows: u32,
        operation: &AtomicBool,
        command:Option<String>,
        observe:bool,
        traffic:Option<crate::traffic::Registration>,
    ) -> Result<Arc<Mutex<PtyState>>> {
        let state = Arc::new(Mutex::new(PtyState::new(cols, rows)));
        if observe {state.lock().unwrap().observer=Some(observer::Observer::new(0));}
        let (reply, response) = mpsc::sync_channel(1);
        self.send(Command::Open {
            traffic,
            command,
            id: pty_id.into(),
            cols,
            rows,
            state: state.clone(),
            reply,
        })?;
        let deadline = Instant::now() + Duration::from_secs(31);
        loop {
            if operation.load(Ordering::SeqCst) || self.canceled.load(Ordering::SeqCst) {
                self.close_pty(&state);
                return Err(CoreError::new("CANCELED", "终端创建已取消"));
            }
            match response.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => {
                    result?;
                    return Ok(state);
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Err(_) => {
                    self.close_pty(&state);
                    return Err(CoreError::new(
                        "TIMEOUT",
                        "打开终端超时，正在清理未交付的通道",
                    ));
                }
            }
        }
    }
    fn send(&self, command: Command) -> Result<()> {
        self.commands.try_send(command).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => {
                CoreError::new("BACKPRESSURE", "SSH 控制队列已满，请稍后重试")
            }
            _ => CoreError::new("CONNECTION_CLOSED", "SSH 连接已结束"),
        })
    }
    pub fn write(
        &self,
        pty_id: &str,
        state: &Arc<Mutex<PtyState>>,
        sequence: u64,
        bytes: Vec<u8>,
    ) -> Result<Value> {
        if bytes.len() > 64 * 1024 {
            return Err(CoreError::new("LIMIT_EXCEEDED", "单次终端输入最多 64 KiB"));
        }
        let mut current = state.lock().unwrap();
        if current.state != "active" || current.close_requested {
            return Err(CoreError::new("PTY_CLOSED", "终端已结束"));
        }
        if sequence <= current.last_input {
            return Err(
                CoreError::new("INPUT_ALREADY_ACCEPTED", "输入序号已经受理，不重复执行")
                    .details(json!({"inputSeq":current.last_input})),
            );
        }
        if sequence != current.last_input + 1 {
            return Err(CoreError::new("INPUT_SEQUENCE_GAP", "输入序号不连续"));
        }
        if current.queued_input + bytes.len() > INPUT_BUDGET {
            return Err(CoreError::new("BACKPRESSURE", "终端输入队列已满"));
        }
        let length = bytes.len();
        self.send(Command::Write {
            id: pty_id.into(),
            bytes,
        })?;
        current.last_input = sequence;
        current.queued_input += length;
        Ok(json!({"acceptedBytes":length,"inputSeq":sequence}))
    }
    pub fn resize(&self, id: &str, cols: u32, rows: u32) -> Result<()> {
        self.send(Command::Resize {
            id: id.into(),
            cols,
            rows,
        })
    }
    pub fn close_pty(&self, state: &Arc<Mutex<PtyState>>) {
        // Closing must remain available when the control queue is saturated.
        // The worker owns this same state from admission through channel teardown.
        state.lock().unwrap().close_requested = true;
    }
    pub fn stop(&self) {
        self.canceled.store(true, Ordering::SeqCst);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn dimensions(cols: u64, rows: u64) -> Result<(u32, u32)> {
    if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
        Err(CoreError::new(
            "INVALID_PARAMS",
            "终端尺寸必须为 2–1000 列、1–1000 行",
        ))
    } else {
        Ok((cols as u32, rows as u32))
    }
}
struct ActivePty {
    _traffic:Option<crate::traffic::Registration>,
    channel: Channel,
    state: Arc<Mutex<PtyState>>,
    writes: VecDeque<Vec<u8>>,
    offset: usize,
    resize: Option<(u32, u32)>,
}
struct PendingOpen {
    traffic:Option<crate::traffic::Registration>,
    command:Option<String>,
    id: String,
    cols: u32,
    rows: u32,
    state: Arc<Mutex<PtyState>>,
    reply: SyncSender<Result<()>>,
    channel: Option<Channel>,
    phase: u8,
    deadline: Instant,
    in_flight: bool,
    failure: Option<CoreError>,
}
impl PendingOpen {
    fn advance(&mut self, session: &Session) -> Result<bool> {
        if self.failure.is_none() {
            if self.state.lock().unwrap().close_requested {
                self.failure = Some(CoreError::new("CANCELED", "终端创建已取消"));
            } else if Instant::now() >= self.deadline {
                self.failure = Some(CoreError::new("TIMEOUT", "打开终端超时"));
            }
            if let Some(error) = &self.failure {
                let _ = self.reply.try_send(Err(error.clone()));
            }
        }
        // libssh2 keeps channel-open and request state across EAGAIN. Drain the
        // exact started operation before freeing its channel or starting another.
        if self.failure.is_some() && !self.in_flight {
            if let Some(channel) = &mut self.channel {
                match channel.close() {
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => return Ok(false),
                    _ => {}
                }
            }
            return Err(self.failure.clone().unwrap());
        }
        self.in_flight = true;
        let result = match self.phase {
            0 => crate::traffic::origin(session,&self.traffic).and_then(|_|session.channel_session()).map(|channel| {
                self.channel = Some(channel);
            }),
            1 => self.channel.as_mut().unwrap().request_pty(
                "xterm-256color",
                None,
                Some((self.cols, self.rows, 0, 0)),
            ),
            _ => if let Some(command)=&self.command {self.channel.as_mut().unwrap().exec(command)} else {self.channel.as_mut().unwrap().shell()},
        };
        match result {
            Ok(()) => {
                self.in_flight = false;
                self.phase += 1;
                Ok(self.phase == 3 && self.failure.is_none())
            }
            Err(error) if error.code() == ssh2::ErrorCode::Session(-37) => Ok(false),
            Err(error) => Err(CoreError::new(
                "SSH_ERROR",
                format!("终端通道创建失败 ({:?})", error.code()),
            )),
        }
    }
}
fn run(
    session: Session,
    commands: Receiver<Command>,
    canceled: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
    remote_metrics: Arc<Mutex<Value>>,
    transport_failed: Arc<AtomicBool>,
    network_metrics: Arc<Mutex<network::Metrics>>,
    probe: crate::quality::probe::Slot,
) {
    let mut channels: HashMap<String, ActivePty> = HashMap::new();
    let mut pending: VecDeque<PendingOpen> = VecDeque::new();
    let mut last_keepalive = Instant::now();
    let mut remote = remote::Remote::default();
    let mut network = network::Network::default();
    while !canceled.load(Ordering::SeqCst) && !transport_failed.load(Ordering::SeqCst) {
        for _ in 0..32 {
            let command = match commands.try_recv() {
                Ok(c) => c,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    canceled.store(true, Ordering::SeqCst);
                    break;
                }
            };
            match command {
                Command::Tcp(open) => network.push(open),
                Command::Remote(job) => remote.push(job),
                Command::Open {
                    traffic,
                    command,
                    id,
                    cols,
                    rows,
                    state,
                    reply,
                } => {
                    pending.push_back(PendingOpen {
                        traffic,
                        command,
                        id,
                        cols,
                        rows,
                        state,
                        reply,
                        channel: None,
                        phase: 0,
                        deadline: Instant::now() + Duration::from_secs(30),
                        in_flight: false,
                        failure: None,
                    });
                }
                Command::Write { id, bytes } => {
                    if let Some(pty) = channels.get_mut(&id) {
                        pty.writes.push_back(bytes);
                    }
                }
                Command::Resize { id, cols, rows } => {
                    if let Some(pty) = channels.get_mut(&id) {
                        pty.resize = Some((cols, rows));
                    }
                }
            }
        }
        let probe_opening=probe.lock().unwrap().as_ref().is_some_and(|p|p.opening());
        if let Some(open) = pending.front_mut().filter(|_| !remote.opening() && !network.opening() && !probe_opening) {
            match open.advance(&session) {
                Ok(true) => {
                    let mut open = pending.pop_front().unwrap();
                    let pty = ActivePty {
                        _traffic:open.traffic,
                        channel: open.channel.take().unwrap(),
                        state: open.state,
                        writes: VecDeque::new(),
                        offset: 0,
                        resize: None,
                    };
                    channels.insert(open.id.clone(), pty);
                    if open.reply.try_send(Ok(())).is_err() {
                        if let Some(pty) = channels.get(&open.id) {
                            pty.state.lock().unwrap().close_requested = true;
                        }
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    let open = pending.pop_front().unwrap();
                    let _ = open.reply.try_send(Err(error));
                }
            }
        }
        let mut completed = Vec::new();
        for (id, pty) in &mut channels {
            if pty.state.lock().unwrap().close_requested {
                match pty.channel.close() {
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => continue,
                    _ => {}
                }
                let mut state = pty.state.lock().unwrap();
                state.state = "closed".into();
                state.reason = Some("closed-by-user".into());
                state.queued_input = 0;
                completed.push(id.clone());
                continue;
            }
            let mut failed = None;
            if let Some((cols, rows)) = pty.resize {
                match pty.channel.request_pty_size(cols, rows, None, None) {
                    Ok(()) => {
                        pty.resize = None;
                        let mut state = pty.state.lock().unwrap();
                        state.cols = cols;
                        state.rows = rows;
                    }
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                    Err(_) => {
                        failed = Some("resize-failed");
                    }
                }
            }
            if let Some(bytes) = pty.writes.front() {
                match pty.channel.write(&bytes[pty.offset..]) {
                    Ok(0) => {}
                    Ok(count) => {
                        pty.offset += count;
                        pty.state.lock().unwrap().queued_input -= count;
                        if pty.offset == bytes.len() {
                            pty.writes.pop_front();
                            pty.offset = 0;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => failed = Some("input-result-unknown"),
                }
            }
            let available = OUTPUT_BUDGET.saturating_sub(pty.state.lock().unwrap().buffered);
            if available > 0 {
                let mut data = vec![0; available.min(32 * 1024)];
                match pty.channel.read(&mut data) {
                    Ok(0) => {}
                    Ok(count) => {
                        data.truncate(count);
                        let mut state = pty.state.lock().unwrap();
                        state.last_sequence += 1;
                        let sequence = state.last_sequence;
                        if let Some(observer)=&mut state.observer {observer.push(sequence,&data);}
                        state.buffered += count;
                        state.frames.push_back(Frame {
                            sequence,
                            bytes: data,
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => failed = Some("connection-lost"),
                }
            }
            if failed.is_some() || pty.channel.eof() {
                let mut state = pty.state.lock().unwrap();
                state.state = if failed.is_some() {
                    "disconnected"
                } else {
                    "closed"
                }
                .into();
                state.reason = Some(failed.unwrap_or("shell-exited").into());
                state.queued_input = 0;
                completed.push(id.clone());
            }
        }
        for id in completed {
            channels.remove(&id);
        }
        remote.tick(&session, pending.is_empty() && !network.opening() && !probe_opening);
        let drain_complete = network.tick(&session, pending.is_empty() && !remote.opening() && !probe_opening);
        *network_metrics.lock().unwrap() = network.metrics();
        if drain_complete { break; }
        *remote_metrics.lock().unwrap() = remote.metrics();
        {
            let mut slot=probe.lock().unwrap();
            if slot.as_mut().is_some_and(|p|p.tick(&session,pending.is_empty()&&!remote.opening()&&!network.opening())){slot.take();}
        }
        if last_keepalive.elapsed() > Duration::from_secs(15) {
            match session.keepalive_send() {
                Ok(_) => {}
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(_) => break,
            }
            last_keepalive = Instant::now();
        }
        if network.progress {thread::yield_now();}else{thread::park_timeout(Duration::from_millis(if network.metrics().active>0 {1}else{2}));}
    }
    for (_, pty) in channels {
        let mut state = pty.state.lock().unwrap();
        state.state = "disconnected".into();
        state.reason = Some("connection-closed".into());
        state.queued_input = 0;
    }
    probe.lock().unwrap().take();
    *status.lock().unwrap() = "closed".into();
    drop(network);
    *network_metrics.lock().unwrap() = network::Metrics::default();
    *remote_metrics.lock().unwrap() = json!({"sessions":0,"handles":0,"execs":0,"queued":0,"cleanupPending":0});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saturated_control_queue_does_not_block_pty_close() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        let (commands, _receiver) = mpsc::sync_channel(1);
        let connection = Connection {
            probe:Arc::new(Mutex::new(None)),
            traffic: None,
            purpose: "session".into(),
            network_metrics: Arc::new(Mutex::new(network::Metrics::default())),
            id: "queue-test".into(),
            server: Server {
                id: "server".into(),
                name: "isolated".into(),
                host: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
                username: "test".into(),
                auth_type: "password".into(),
                revision: 1,
                has_secret: false,
            },
            workspace_id: "test".into(),
            generation: 1,
            route: crate::route::RoutePlan {target_server_id:"test".into(),revision:"test".into(),identity_revision:"test".into(),steps:vec![],access_path:crate::route::AccessPath::default(),cost:crate::route::budget::Cost::for_steps(&[])},
            bridge_metrics:None,
            bridge_segments:Vec::new(),
            failure_hop:Arc::new(AtomicUsize::new(usize::MAX)),
            transport_live:Arc::new(AtomicBool::new(false)),
            commands,
            canceled: Arc::new(AtomicBool::new(false)),
            socket,
            status: Arc::new(Mutex::new("active".into())),
            lifecycle:Arc::new(Mutex::new(None)),
            remote_metrics: Arc::new(Mutex::new(Value::Null)),
        };
        let state = Arc::new(Mutex::new(PtyState::new(80, 24)));
        connection.resize("pty", 80, 24).unwrap();
        assert_eq!(
            connection.resize("pty", 80, 25).unwrap_err().code,
            "BACKPRESSURE"
        );
        connection.close_pty(&state);
        connection.close_pty(&state);
        assert!(state.lock().unwrap().close_requested);
        assert_eq!(
            connection
                .write("pty", &state, 1, b"late".to_vec())
                .unwrap_err()
                .code,
            "PTY_CLOSED"
        );
    }

    #[test]
    fn output_ack_is_monotonic_and_rejects_missing_data() {
        let mut state = PtyState::new(80, 24);
        state.last_sequence = 2;
        state.frames.push_back(Frame {
            sequence: 1,
            bytes: vec![1, 2],
        });
        state.frames.push_back(Frame {
            sequence: 2,
            bytes: vec![3],
        });
        state.buffered = 3;
        assert!(state.ack(3).is_err());
        state.ack(1).unwrap();
        assert_eq!(state.buffered, 1);
        state.ack(0).unwrap();
        assert_eq!(state.acknowledged, 1);
        assert!(state.read(0).is_err());
        assert_eq!(
            state.read(1).unwrap()["frames"].as_array().unwrap().len(),
            1
        );
    }
    #[test]
    fn reject_zero_hidden_and_excessive_dimensions() {
        assert!(dimensions(0, 0).is_err());
        assert!(dimensions(1001, 2).is_err());
        assert_eq!(dimensions(80, 24).unwrap(), (80, 24));
    }
}
