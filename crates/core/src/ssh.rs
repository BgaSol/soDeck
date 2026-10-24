use crate::{
    model::{id, CoreError, Result, Server},
    secrets::Credential,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ssh2::{Channel, Session};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const OUTPUT_BUDGET: usize = 2 * 1024 * 1024;
const INPUT_BUDGET: usize = 256 * 1024;

#[derive(Clone)]
pub struct Frame {
    pub sequence: u64,
    pub bytes: Vec<u8>,
}
pub struct PtyState {
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
    Open {
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
    pub id: String,
    pub server: Server,
    pub workspace_id: String,
    pub generation: u64,
    pub credential_revision: u64,
    commands: SyncSender<Command>,
    canceled: Arc<AtomicBool>,
    socket: TcpStream,
    pub status: Arc<Mutex<String>>,
}
impl Connection {
    pub fn connect(
        server: Server,
        workspace_id: String,
        generation: u64,
        credential_revision: u64,
        known_key: Option<String>,
        credential: Option<Credential>,
        canceled: Arc<AtomicBool>,
        operation: &AtomicBool,
    ) -> Result<Arc<Self>> {
        let (resolved_tx, resolved_rx) = mpsc::sync_channel(1);
        let endpoint = (server.host.clone(), server.port);
        thread::spawn(move || {
            let _ = resolved_tx.send(
                endpoint
                    .to_socket_addrs()
                    .map(|addresses| addresses.collect::<Vec<_>>()),
            );
        });
        let resolve_deadline = Instant::now() + Duration::from_secs(15);
        let addresses = loop {
            if canceled.load(Ordering::SeqCst) || operation.load(Ordering::SeqCst) {
                return Err(CoreError::new("CANCELED", "连接已取消"));
            }
            match resolved_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(result) => {
                    break result.map_err(|_| CoreError::new("NETWORK_ERROR", "无法解析主机名"))?
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < resolve_deadline => {}
                _ => return Err(CoreError::new("CONNECT_TIMEOUT", "主机名解析超时")),
            }
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut socket = None;
        'addresses: for address in addresses {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let candidate_deadline = Instant::now() + remaining.min(Duration::from_secs(5));
            while Instant::now() < candidate_deadline {
                if canceled.load(Ordering::SeqCst) || operation.load(Ordering::SeqCst) {
                    return Err(CoreError::new("CANCELED", "连接已取消"));
                }
                match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
                    Ok(stream) => {
                        socket = Some(stream);
                        break 'addresses;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) => {}
                    Err(_) => break,
                }
            }
        }
        let socket = socket.ok_or_else(|| {
            CoreError::new("NETWORK_ERROR", "SSH 连接失败，请检查地址、端口及网络")
        })?;
        socket.set_nonblocking(true)?;
        socket.set_nodelay(true)?;
        let mut session =
            Session::new().map_err(|_| CoreError::new("SSH_ERROR", "无法创建 SSH 会话"))?;
        session.set_tcp_stream(socket.try_clone()?);
        session.set_blocking(false);
        retry(&canceled, operation, || session.handshake())?;
        let (key, _) = session
            .host_key()
            .ok_or_else(|| CoreError::new("SSH_ERROR", "服务器未提供主机密钥"))?;
        let fingerprint = format!(
            "SHA256:{}",
            STANDARD.encode(Sha256::digest(key)).trim_end_matches('=')
        );
        let algorithm_length = key
            .get(..4)
            .map(|v| u32::from_be_bytes(v.try_into().unwrap()) as usize)
            .unwrap_or(0);
        let algorithm = key
            .get(4..4 + algorithm_length)
            .and_then(|v| std::str::from_utf8(v).ok())
            .unwrap_or("unknown")
            .to_owned();
        if known_key.as_deref() != Some(fingerprint.as_str()) {
            let code = if known_key.is_some() {
                "HOST_KEY_CHANGED"
            } else {
                "HOST_KEY_REQUIRED"
            };
            return Err(CoreError::new(code, if known_key.is_some() {"服务器主机密钥已改变，认证已停止"} else {"请确认服务器主机身份"}).details(json!({"serverId":server.id,"host":server.host,"port":server.port,"algorithm":algorithm,"fingerprint":fingerprint,"previousFingerprint":known_key})));
        }
        let credential = credential.ok_or_else(|| {
            CoreError::new("AUTH_REQUIRED", "请输入 SSH 认证信息")
                .details(json!({"serverId":server.id,"authType":server.auth_type}))
        })?;
        let authentication = if server.auth_type == "password" {
            retry(&canceled, operation, || {
                session.userauth_password(&server.username, &credential.password)
            })
        } else {
            authenticate_key(
                &session,
                &server.username,
                &credential,
                &canceled,
                operation,
            )
        };
        authentication.map_err(|error| {
            if error.code == "CANCELED" || error.code == "TIMEOUT" {
                return error;
            }
            CoreError::new("AUTH_FAILED", "SSH 认证失败，请检查用户名与凭据")
                .details(json!({"cause":error.code,"diagnostic":error.message}))
        })?;
        if !session.authenticated() {
            return Err(CoreError::new("AUTH_FAILED", "SSH 认证未完成"));
        }
        session.set_keepalive(true, 30);
        let (sender, receiver) = mpsc::sync_channel(128);
        let status = Arc::new(Mutex::new("active".into()));
        let connection = Arc::new(Self {
            id: id(),
            server,
            workspace_id,
            generation,
            credential_revision,
            commands: sender,
            canceled: canceled.clone(),
            socket,
            status: status.clone(),
        });
        thread::Builder::new()
            .name("sodeck-ssh-io".into())
            .spawn(move || run(session, receiver, canceled, status))?;
        Ok(connection)
    }
    pub fn open(
        &self,
        pty_id: &str,
        cols: u32,
        rows: u32,
        operation: &AtomicBool,
    ) -> Result<Arc<Mutex<PtyState>>> {
        let state = Arc::new(Mutex::new(PtyState::new(cols, rows)));
        let (reply, response) = mpsc::sync_channel(1);
        self.send(Command::Open {
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

fn authenticate_key(
    session: &Session,
    username: &str,
    credential: &Credential,
    canceled: &AtomicBool,
    operation: &AtomicBool,
) -> Result<()> {
    retry(canceled, operation, || {
        session.userauth_pubkey_memory(
            username,
            None,
            &credential.private_key,
            (!credential.passphrase.is_empty()).then_some(credential.passphrase.as_str()),
        )
    })
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
fn retry<T>(
    canceled: &AtomicBool,
    operation: &AtomicBool,
    mut run: impl FnMut() -> std::result::Result<T, ssh2::Error>,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if canceled.load(Ordering::SeqCst) || operation.load(Ordering::SeqCst) {
            return Err(CoreError::new("CANCELED", "SSH 操作已取消"));
        }
        match run() {
            Ok(value) => return Ok(value),
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
            Err(error) => {
                return Err(CoreError::new(
                    "SSH_ERROR",
                    format!("SSH 协议操作失败 ({:?})", error.code()),
                ))
            }
        }
        if Instant::now() >= deadline {
            return Err(CoreError::new("TIMEOUT", "SSH 协议操作超时"));
        }
        thread::sleep(Duration::from_millis(4));
    }
}
struct ActivePty {
    channel: Channel,
    state: Arc<Mutex<PtyState>>,
    writes: VecDeque<Vec<u8>>,
    offset: usize,
    resize: Option<(u32, u32)>,
}
struct PendingOpen {
    id: String,
    cols: u32,
    rows: u32,
    state: Arc<Mutex<PtyState>>,
    reply: SyncSender<Result<()>>,
    channel: Option<Channel>,
    phase: u8,
    deadline: Instant,
}
impl PendingOpen {
    fn advance(&mut self, session: &Session) -> Result<bool> {
        if Instant::now() >= self.deadline {
            return Err(CoreError::new("TIMEOUT", "打开终端超时"));
        }
        let result = match self.phase {
            0 => session.channel_session().map(|channel| {
                self.channel = Some(channel);
            }),
            1 => self.channel.as_mut().unwrap().request_pty(
                "xterm-256color",
                None,
                Some((self.cols, self.rows, 0, 0)),
            ),
            _ => self.channel.as_mut().unwrap().shell(),
        };
        match result {
            Ok(()) => {
                self.phase += 1;
                Ok(self.phase == 3)
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
) {
    let mut channels: HashMap<String, ActivePty> = HashMap::new();
    let mut pending: VecDeque<PendingOpen> = VecDeque::new();
    let mut last_keepalive = Instant::now();
    while !canceled.load(Ordering::SeqCst) {
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
                Command::Open {
                    id,
                    cols,
                    rows,
                    state,
                    reply,
                } => {
                    pending.push_back(PendingOpen {
                        id,
                        cols,
                        rows,
                        state,
                        reply,
                        channel: None,
                        phase: 0,
                        deadline: Instant::now() + Duration::from_secs(30),
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
        pending.retain(|open| {
            if open.state.lock().unwrap().close_requested {
                let _ = open
                    .reply
                    .send(Err(CoreError::new("CANCELED", "终端创建已取消")));
                false
            } else {
                true
            }
        });
        if let Some(open) = pending.front_mut() {
            match open.advance(&session) {
                Ok(true) => {
                    let mut open = pending.pop_front().unwrap();
                    let pty = ActivePty {
                        channel: open.channel.take().unwrap(),
                        state: open.state,
                        writes: VecDeque::new(),
                        offset: 0,
                        resize: None,
                    };
                    channels.insert(open.id.clone(), pty);
                    if open.reply.send(Ok(())).is_err() {
                        channels.remove(&open.id);
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    let open = pending.pop_front().unwrap();
                    let _ = open.reply.send(Err(error));
                }
            }
        }
        let mut completed = Vec::new();
        for (id, pty) in &mut channels {
            if pty.state.lock().unwrap().close_requested {
                let _ = pty.channel.send_eof();
                let _ = pty.channel.close();
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
        if last_keepalive.elapsed() > Duration::from_secs(15) {
            match session.keepalive_send() {
                Ok(_) => {}
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(_) => break,
            }
            last_keepalive = Instant::now();
        }
        thread::sleep(Duration::from_millis(2));
    }
    for (_, pty) in channels {
        let mut state = pty.state.lock().unwrap();
        state.state = "disconnected".into();
        state.reason = Some("connection-closed".into());
        state.queued_input = 0;
    }
    *status.lock().unwrap() = "closed".into();
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
            credential_revision: 0,
            commands,
            canceled: Arc::new(AtomicBool::new(false)),
            socket,
            status: Arc::new(Mutex::new("active".into())),
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
