//! Target-channel actor. A canceled libssh2 open must be resumed with identical
//! arguments until completion, or its otherwise unused transport must be closed.
use super::*;
use crate::network::stream::Stream;

#[derive(Default, Clone, Debug)]
pub(crate) struct Metrics {
    pub active: usize,
    pub pending: usize,
    pub cleanup_pending: usize,
    pub draining: bool,
}
pub(crate) struct Open {
    pub host: String,
    pub port: u16,
    pub state: Arc<Mutex<Stream>>,
    pub canceled: Arc<AtomicBool>,
    pub result: Arc<Mutex<Option<Result<()>>>>,
    pub queued: Instant,
    started: Option<Instant>,
    in_flight: bool,
}
impl Open {
    pub fn new(host: String, port: u16, state: Arc<Mutex<Stream>>, canceled: Arc<AtomicBool>) -> Self {
        Self { host, port, state, canceled, result: Arc::new(Mutex::new(None)), queued: Instant::now(), started: None, in_flight: false }
    }
    fn complete(&self, result: Result<()>) {
        let mut stored = self.result.lock().unwrap();
        if stored.is_none() { *stored = Some(result); }
    }
}
struct Active {
    channel: Channel,
    state: Arc<Mutex<Stream>>,
}
#[derive(Default)]
pub(crate) struct Network {
    pending: VecDeque<Open>,
    channels: Vec<Active>,
    draining: bool,
    pub progress: bool,
}
impl Network {
    pub fn push(&mut self, open: Open) {
        if self.draining || self.pending.len() >= 16 {
            open.complete(Err(CoreError::new("CAPACITY", "网络传输暂不接收新目标")));
        } else { self.pending.push_back(open); }
    }
    pub fn opening(&self) -> bool { self.pending.front().is_some_and(|p| p.in_flight) }
    /// Returns true only when this draining transport carries no valid flow.
    pub fn tick(&mut self, session: &Session, can_open: bool) -> bool {
        self.progress=false;
        // Time out queued work too, without changing an in-flight libssh2 call.
        self.pending.retain(|open| {
            let cancel = open.canceled.load(Ordering::SeqCst) || open.state.lock().unwrap().closed;
            if !open.in_flight && (cancel || (open.started.is_none() && open.queued.elapsed() >= Duration::from_secs(5))) {
                open.complete(Err(CoreError::new(if cancel { "CANCELED" } else { "TARGET_CONNECT_TIMEOUT" }, "目标连接排队已结束")));
                false
            } else { true }
        });
        if let Some(open) = self.pending.front_mut() {
            let cancel = open.canceled.load(Ordering::SeqCst) || open.state.lock().unwrap().closed;
            let timeout = open.started.is_some_and(|t| t.elapsed() >= Duration::from_secs(15));
            if cancel || timeout {
                open.complete(Err(CoreError::new(if cancel { "CANCELED" } else { "TARGET_CONNECT_TIMEOUT" }, "目标通道未建立，正在排空连接请求")));
                open.state.lock().unwrap().close();
                if open.in_flight { self.draining = true; }
            }
            if can_open || open.in_flight {
                open.started.get_or_insert_with(Instant::now);
                match session.channel_direct_tcpip(&open.host, open.port, Some(("127.0.0.1", 0))) {
                    Ok(channel) => {
                        let open = self.pending.pop_front().unwrap();
                        if open.canceled.load(Ordering::SeqCst) || open.result.lock().unwrap().as_ref().is_some_and(|r| r.is_err()) {
                            open.state.lock().unwrap().close();
                            open.complete(Err(CoreError::new("CANCELED", "目标连接已取消")));
                        } else { open.complete(Ok(())); }
                        self.channels.push(Active { channel, state: open.state });
                    }
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => open.in_flight = true,
                    Err(e) => {
                        let open = self.pending.pop_front().unwrap();
                        open.complete(Err(CoreError::new("TARGET_CONNECT_FAILED", "最终 SSH 节点未能建立目标连接")
                            .details(json!({"stage":"target-open","sshCode":format!("{:?}",e.code())}))));
                    }
                }
            }
        }
        if self.draining {
            // Pending opens that have not entered libssh2 can be safely rejected.
            self.pending.retain(|open| {
                if open.in_flight { true } else { open.complete(Err(CoreError::new("CAPACITY", "网络传输正在排空，请使用其他池成员"))); false }
            });
        }
        self.channels.retain_mut(|active| {
            let mut state = active.state.lock().unwrap();
            if !state.closed {
                let before=(state.received,state.written);
                state.write_to(&mut active.channel);
                if state.wants_eof() {
                    match active.channel.send_eof() {
                        Ok(()) => state.write_eof = true,
                        Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {},
                        Err(_) => state.fail(CoreError::new("TRANSPORT_LOST", "目标通道结束写入失败")),
                    }
                }
                let was_eof = state.read_eof;
                state.read_from(&mut active.channel);
                if !was_eof && !active.channel.eof() { state.read_eof = false; }
                self.progress|=before!=(state.received,state.written);
            }
            if state.finished() {
                match active.channel.close() {
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => true,
                    _ => false,
                }
            } else { true }
        });
        self.draining && self.channels.iter().all(|c| c.state.lock().unwrap().closed)
    }
    pub fn metrics(&self) -> Metrics {
        Metrics { active: self.channels.iter().filter(|c| !c.state.lock().unwrap().closed).count(),
            pending: self.pending.len(),
            cleanup_pending: self.channels.iter().filter(|c| c.state.lock().unwrap().closed).count()
                + self.pending.iter().filter(|p| p.result.lock().unwrap().is_some()).count(),
            draining: self.draining }
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        for open in &self.pending { open.complete(Err(CoreError::new("TRANSPORT_LOST", "SSH 网络传输已结束"))); }
        for active in &self.channels { active.state.lock().unwrap().fail(CoreError::new("TRANSPORT_LOST", "SSH 网络传输已结束")); }
    }
}
