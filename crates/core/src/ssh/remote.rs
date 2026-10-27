//! Nonblocking remote I/O, advanced only by the connection's owner thread.
use super::*;
use ssh2::{File, OpenFlags, OpenType, Sftp};
use std::io::{Seek, SeekFrom};

#[derive(Default)]
pub(crate) struct IoBudget {
    pub running: usize,
    pub queued: usize,
}
pub(crate) struct IoTicket {
    budget: Arc<Mutex<IoBudget>>,
    started: bool,
}
impl IoTicket {
    pub fn reserve(budget: Arc<Mutex<IoBudget>>) -> Result<Self> {
        {
            let mut b = budget.lock().unwrap();
            if b.queued >= 64 {
                return Err(CoreError::new("LIMIT_EXCEEDED", "远端读取等待队列已满"));
            }
            b.queued += 1;
        }
        Ok(Self {
            budget,
            started: false,
        })
    }
    fn start(&mut self) -> bool {
        if self.started {
            return true;
        }
        let mut b = self.budget.lock().unwrap();
        if b.running >= 16 {
            return false;
        }
        b.queued -= 1;
        b.running += 1;
        self.started = true;
        true
    }
}
impl Drop for IoTicket {
    fn drop(&mut self) {
        let mut b = self.budget.lock().unwrap();
        if self.started {
            b.running -= 1;
        } else {
            b.queued -= 1;
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn execution_and_waiting_limits_return_to_zero_after_cancel() {
        let budget = Arc::new(Mutex::new(IoBudget::default()));
        let mut running: Vec<_> = (0..16)
            .map(|_| IoTicket::reserve(budget.clone()).unwrap())
            .collect();
        for ticket in &mut running {
            assert!(ticket.start());
        }
        let mut queued: Vec<_> = (0..64)
            .map(|_| IoTicket::reserve(budget.clone()).unwrap())
            .collect();
        assert!(!queued[0].start());
        assert!(IoTicket::reserve(budget.clone()).is_err());
        running.pop();
        assert!(queued[0].start());
        drop(running);
        drop(queued);
        let b = budget.lock().unwrap();
        assert_eq!((b.running, b.queued), (0, 0));
    }
}

pub struct Job {
    ticket: IoTicket,
    pub method: String,
    pub params: Value,
    pub lease: String,
    pub lease_alive: Arc<AtomicBool>,
    pub canceled: Arc<AtomicBool>,
    pub deadline: Instant,
    pub result: Arc<Mutex<Option<Result<Value>>>>,
    pub exec: Option<Arc<Mutex<ExecState>>>,
    pub resource_alive: Option<Arc<AtomicBool>>,
    entries: Vec<Value>,
    bytes: usize,
    buffer: Vec<u8>,
    positioned: bool,
}
impl Job {
    pub fn new(
        method: &str,
        params: Value,
        lease: String,
        lease_alive: Arc<AtomicBool>,
        canceled: Arc<AtomicBool>,
        ticket: IoTicket,
    ) -> Self {
        let timeout = params["timeoutMs"]
            .as_u64()
            .unwrap_or(if method == "exec.start" {
                10_000
            } else {
                15_000
            })
            .min(30_000);
        Self {
            ticket,
            method: method.into(),
            params,
            lease,
            lease_alive,
            canceled,
            deadline: Instant::now() + Duration::from_millis(timeout),
            result: Arc::new(Mutex::new(None)),
            exec: None,
            resource_alive: None,
            entries: vec![],
            bytes: 0,
            buffer: vec![],
            positioned: false,
        }
    }
    fn finish(&self, result: Result<Value>) {
        let mut slot = self.result.lock().unwrap();
        if slot.is_none() {
            *slot = Some(result);
        }
    }
    fn valid(&self) -> Result<()> {
        if !self.lease_alive.load(Ordering::SeqCst)
            || self.canceled.load(Ordering::SeqCst)
            || self
                .resource_alive
                .as_ref()
                .is_some_and(|a| !a.load(Ordering::SeqCst))
        {
            return Err(CoreError::new("CANCELED", "读取已取消"));
        }
        if Instant::now() >= self.deadline {
            return Err(CoreError::new("TIMEOUT", "远端读取超时"));
        }
        Ok(())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        // Socket teardown must resolve queued callers and streaming consumers;
        // a vanished native owner cannot leave an eternally running receipt.
        self.finish(Err(CoreError::new("CONNECTION_LOST","远端通道已经结束")));
        if let Some(exec)=&self.exec {
            let mut state=exec.lock().unwrap();
            if matches!(state.state.as_str(),"starting"|"running") {
                state.state="failed".into();state.reason=Some("CONNECTION_LOST".into());
            }
        }
    }
}
pub struct ExecState {
    pub frames: VecDeque<Value>,
    pub buffered: usize,
    pub total: usize,
    pub sequence: u64,
    pub ack: u64,
    pub state: String,
    pub exit: Option<i32>,
    pub reason: Option<String>,
    pub truncated: bool,
    pub canceled: bool,
}
impl Default for ExecState {
    fn default() -> Self {
        Self {
            frames: VecDeque::new(),
            buffered: 0,
            total: 0,
            sequence: 0,
            ack: 0,
            state: "starting".into(),
            exit: None,
            reason: None,
            truncated: false,
            canceled: false,
        }
    }
}
impl ExecState {
    pub fn read(&self, after: u64, max: usize) -> Result<Value> {
        if after < self.ack {
            return Err(CoreError::new("CURSOR_EXPIRED", "输出游标已过期"));
        }
        if after > self.sequence {
            return Err(CoreError::new("INVALID_PARAMS", "输出游标超出范围"));
        }
        if let Some(frame)=self.frames.iter().find(|f|f["sequence"].as_u64().unwrap()>after) {
            let required=frame["length"].as_u64().unwrap() as usize;
            if required>max{return Err(CoreError::new("LIMIT_EXCEEDED","读取预算小于下一输出块，请增大 maxBytes").details(json!({"minimumBytes":required})));}
        }
        let mut used = 0;
        let frames = self
            .frames
            .iter()
            .filter(|f| f["sequence"].as_u64().unwrap() > after)
            .take_while(|f| {
                used += f["length"].as_u64().unwrap() as usize;
                used <= max
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(
            json!({"frames":frames,"state":self.state,"exitCode":self.exit,"reason":self.reason,"truncated":self.truncated,"lastSequence":self.sequence,"acknowledged":self.ack}),
        )
    }
    pub fn acknowledge(&mut self, sequence: u64) -> Result<Value> {
        if sequence > self.sequence {
            return Err(CoreError::new("INVALID_PARAMS", "不能确认未产生的输出"));
        }
        self.ack = self.ack.max(sequence);
        while self
            .frames
            .front()
            .is_some_and(|f| f["sequence"].as_u64().unwrap() <= self.ack)
        {
            let f = self.frames.pop_front().unwrap();
            self.buffered -= f["length"].as_u64().unwrap() as usize;
        }
        Ok(json!({"acknowledged":self.ack}))
    }
}
struct Handle {
    file: File,
    alive: Arc<AtomicBool>,
    touched: Instant,
    cursor: u64,
    eof: bool,
    pending_entry: Option<Value>,
}
impl Drop for Handle {fn drop(&mut self){self.alive.store(false,Ordering::SeqCst);}}
struct SftpSession {
    sftp: Sftp,
    alive: Arc<AtomicBool>,
    handles: HashMap<String, Handle>,
    queue: VecDeque<Job>,
    active: Option<Job>,
}
struct Exec {
    job: Job,
    channel: Option<Channel>,
    phase: u8,
    input: Vec<u8>,
    input_offset: usize,
    closing: bool,
    in_flight: bool,
}
#[derive(Default)]
pub struct Remote {
    sessions: HashMap<String, SftpSession>,
    queue: VecDeque<Job>,
    execs: Vec<Exec>,
    opening: bool,
}
fn pending(error: &ssh2::Error) -> bool {
    error.code() == ssh2::ErrorCode::Session(-37)
}
fn failure(e: ssh2::Error) -> CoreError {
    let code = match e.code() {
        ssh2::ErrorCode::SFTP(2) => "NOT_FOUND",
        ssh2::ErrorCode::SFTP(3) => "PERMISSION_DENIED",
        ssh2::ErrorCode::SFTP(8) => "SFTP_UNAVAILABLE",
        _ => "REMOTE_IO_ERROR",
    };
    CoreError::new(code, format!("远端协议操作失败 ({:?})", e.code()))
}
fn path(p: &Value) -> Result<Vec<u8>> {
    STANDARD
        .decode(p["path"]["bytesBase64"].as_str().unwrap_or(""))
        .map_err(|_| CoreError::new("INVALID_PARAMS", "路径编码无效"))
}
pub fn attributes(s: ssh2::FileStat) -> Value {
    let kind = match s.perm.map(|m| m & 0o170000) {
        Some(0o040000) => "directory",
        Some(0o100000) => "file",
        Some(0o120000) => "symlink",
        Some(0o010000) => "fifo",
        Some(0o020000) => "characterDevice",
        Some(0o060000) => "blockDevice",
        Some(0o140000) => "socket",
        _ => "unknown",
    };
    json!({"type":kind,"size":s.size.map(|n|n.to_string()),"mtime":s.mtime.map(|n|n.to_string()),"atime":s.atime.map(|n|n.to_string()),"permissions":s.perm,"uid":s.uid,"gid":s.gid})
}
impl Remote {
    pub fn metrics(&self) -> Value {
        json!({"sessions":self.sessions.len(),"handles":self.sessions.values().map(|s|s.handles.len()).sum::<usize>(),"execs":self.execs.len(),"queued":self.queue.len()+self.sessions.values().map(|s|s.queue.len()).sum::<usize>(),"cleanupPending":self.sessions.values().filter(|s|!s.alive.load(Ordering::SeqCst)||s.active.as_ref().is_some_and(|j|j.valid().is_err())||s.handles.values().any(|h|!h.alive.load(Ordering::SeqCst))).count()+self.execs.iter().filter(|e|e.closing||e.job.valid().is_err()).count()})
    }
    pub fn push(&mut self, job: Job) {
        self.queue.push_back(job);
    }
    pub fn opening(&self) -> bool {
        self.opening || self.execs.iter().any(|e| e.phase == 0)
    }
    pub fn tick(&mut self, session: &Session, allow_open: bool) {
        // Only one channel-opening state machine can use libssh2's session at a time.
        if let Some(job) = self.queue.front_mut() {
            if job.valid().is_err() && !self.opening {
                job.finish(job.valid().map(|_| Value::Null));
                self.queue.pop_front();
            } else if !job.ticket.start() { /* Other connections own all execution slots. */
            } else if job.method == "exec.start" {
                if allow_open && !self.opening() {
                    let job = self.queue.pop_front().unwrap();
                    let input = STANDARD
                        .decode(job.params["stdinBase64"].as_str().unwrap_or(""))
                        .unwrap_or_default();
                    self.execs.push(Exec {
                        job,
                        channel: None,
                        phase: 0,
                        input,
                        input_offset: 0,
                        closing: false,
                        in_flight: false,
                    });
                }
            } else if self.sessions.contains_key(&job.lease) {
                let job = self.queue.pop_front().unwrap();
                self.sessions
                    .get_mut(&job.lease)
                    .unwrap()
                    .queue
                    .push_back(job);
            } else if (allow_open && !self.opening()) || self.opening {
                match session.sftp() {
                    Ok(sftp) => {
                        let job = self.queue.pop_front().unwrap();
                        self.sessions.insert(
                            job.lease.clone(),
                            SftpSession {
                                sftp,
                                alive: job.lease_alive.clone(),
                                handles: HashMap::new(),
                                queue: VecDeque::from([job]),
                                active: None,
                            },
                        );
                        self.opening = false;
                    }
                    Err(e) if pending(&e) => self.opening = true,
                    Err(_) => {
                        self.queue.pop_front().unwrap().finish(Err(CoreError::new(
                            "SFTP_UNAVAILABLE",
                            "服务器无法打开 SFTP 子系统",
                        )));
                        self.opening = false;
                    }
                }
            }
        }
        for sf in self.sessions.values_mut() {
            if sf.active.is_none() {
                while let Some(job) = sf.queue.pop_front() {
                    if let Err(error) = job.valid() {
                        job.finish(Err(error));
                    } else {
                        sf.active = Some(job);
                        break;
                    }
                }
            }
            if let Some(mut job) = sf.active.take() {
                // Cancellation revokes delivery immediately. Keep advancing the
                // exact native operation through EAGAIN before closing handles;
                // abandoning its state would corrupt a later SFTP request.
                let canceled = job.valid().err();
                if let Some(error) = &canceled {
                    job.finish(Err(error.clone()));
                }
                let result = sf.advance(&mut job);
                if canceled.is_some() {
                    if let Some(alive) = &job.resource_alive {
                        alive.store(false, Ordering::SeqCst);
                    }
                }
                match result {
                    Ok(None) => sf.active = Some(job),
                    Ok(Some(value)) => job.finish(Ok(value)),
                    Err(e) => job.finish(Err(e)),
                }
            }
            if sf.active.is_some() {
                continue;
            }
            let ids = sf
                .handles
                .iter()
                .filter(|(_, h)| {
                    !sf.alive.load(Ordering::SeqCst)
                        || !h.alive.load(Ordering::SeqCst)
                        || h.touched.elapsed() > Duration::from_secs(60)
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in ids {
                let h = sf.handles.get_mut(&id).unwrap();
                h.alive.store(false, Ordering::SeqCst);
                match h.file.close() {
                    Err(e) if pending(&e) => {}
                    _ => {
                        sf.handles.remove(&id);
                    }
                }
            }
        }
        // Explicit nonblocking shutdown, never drop a live SFTP session on this loop.
        let ids = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                !s.alive.load(Ordering::SeqCst)
                    && s.handles.is_empty()
                    && s.active.is_none()
                    && s.queue.is_empty()
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            match self.sessions.get_mut(&id).unwrap().sftp.shutdown() {
                Err(e) if pending(&e) => {}
                _ => {
                    self.sessions.remove(&id);
                }
            }
        }
        let mut opening_exec = false;
        for ex in &mut self.execs {
            let can_open = (allow_open || ex.phase == 0) && !self.opening && !opening_exec;
            ex.advance(session, can_open);
            if ex.phase == 0 && ex.channel.is_none() {
                opening_exec = true;
            }
        }
        self.execs.retain(|e| e.phase != 255);
    }
}
impl SftpSession {
    fn advance(&mut self, job: &mut Job) -> Result<Option<Value>> {
        let p = &job.params;
        let result: std::result::Result<Option<Value>, ssh2::Error> = match job.method.as_str() {
            "files.openDirectory" | "files.openRead" => {
                let directory = job.method == "files.openDirectory";
                self.sftp
                    .open_mode_bytes(
                        &path(p)?,
                        OpenFlags::READ,
                        0,
                        if directory {
                            OpenType::Dir
                        } else {
                            OpenType::File
                        },
                    )
                    .map(|file| {
                        let id = p["resourceId"].as_str().unwrap().to_owned();
                        self.handles.insert(
                            id.clone(),
                            Handle {
                                file,
                                alive: job.resource_alive.clone().unwrap(),
                                touched: Instant::now(),
                                cursor: 0,
                                eof: false,
                                pending_entry: None,
                            },
                        );
                        Some(if directory {
                            json!({"directoryId":id,"cursor":0})
                        } else {
                            json!({"fileId":id})
                        })
                    })
            }
            "files.stat" => {
                let path = path(p)?;
                if p["followLinks"] == true {
                    self.sftp.stat_bytes(&path)
                } else {
                    self.sftp.lstat_bytes(&path)
                }
                .map(|s| Some(attributes(s)))
            }
            "files.readLink" | "files.realPath" => {
                let path = path(p)?;
                if job.method == "files.readLink" {
                    self.sftp.readlink_bytes(&path)
                } else {
                    self.sftp.realpath_bytes(&path)
                }
                .map(|b| Some(json!({"path":{"bytesBase64":STANDARD.encode(b)}})))
            }
            "files.readDirectory" => {
                let id = p["directoryId"].as_str().unwrap();
                let h = self
                    .handles
                    .get_mut(id)
                    .ok_or_else(|| CoreError::new("CURSOR_EXPIRED", "目录句柄已过期，请刷新"))?;
                if p["cursor"].as_u64() != Some(h.cursor) {
                    return Err(CoreError::new("CURSOR_EXPIRED", "目录游标不再有效"));
                }
                h.touched = Instant::now();
                let max = p["maxEntries"].as_u64().unwrap_or(256) as usize;
                for _ in 0..8 {
                    if h.eof {
                        break;
                    }
                    let entry = if let Some(e) = h.pending_entry.take() {
                        Some(e)
                    } else {
                        match h.file.readdir_bytes() {
                            Ok((name, stat)) => {
                                if name == b"." || name == b".." {
                                    continue;
                                }
                                Some(
                                    json!({"name":{"bytesBase64":STANDARD.encode(name)},"attributes":attributes(stat)}),
                                )
                            }
                            Err(e) if e.code() == ssh2::ErrorCode::Session(-16) => {
                                h.eof = true;
                                None
                            }
                            Err(e) if pending(&e) => break,
                            Err(e) => return Err(failure(e)),
                        }
                    };
                    if let Some(e) = entry {
                        let size = e.to_string().len();
                        if job.bytes + size > 250 * 1024 {
                            h.pending_entry = Some(e);
                            break;
                        }
                        job.bytes += size;
                        job.entries.push(e);
                    }
                    if h.eof || job.entries.len() >= max {
                        break;
                    }
                }
                if h.eof || job.entries.len() >= max || h.pending_entry.is_some() {
                    if h.eof {
                        match h.file.close() {
                            Err(e) if pending(&e) => return Ok(None),
                            Err(e) => return Err(failure(e)),
                            Ok(()) => {}
                        }
                    }
                    h.cursor += 1;
                    let cursor = h.cursor;
                    let eof = h.eof;
                    let entries = std::mem::take(&mut job.entries);
                    if eof {
                        self.handles.remove(id);
                    }
                    Ok(Some(
                        json!({"entries":entries,"nextCursor":cursor,"eof":eof}),
                    ))
                } else {
                    Ok(None)
                }
            }
            "files.read" => {
                let h = self
                    .handles
                    .get_mut(p["fileId"].as_str().unwrap())
                    .ok_or_else(|| CoreError::new("STALE_HANDLE", "文件句柄已结束"))?;
                h.touched = Instant::now();
                let offset = p["offset"]
                    .as_str()
                    .and_then(|n| n.parse::<u64>().ok())
                    .ok_or_else(|| CoreError::new("INVALID_PARAMS", "读取偏移无效"))?;
                if !job.positioned {
                    h.file
                        .seek(SeekFrom::Start(offset))
                        .map_err(|_| CoreError::new("REMOTE_IO_ERROR", "无法设置读取位置"))?;
                    job.positioned = true;
                    job.buffer = vec![0; p["maxBytes"].as_u64().unwrap_or(65536) as usize];
                }
                match h.file.read(&mut job.buffer) {
                    Ok(n) => Ok(Some(
                        json!({"bytesBase64":STANDARD.encode(&job.buffer[..n]),"eof":n==0,"nextOffset":(offset+n as u64).to_string()}),
                    )),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                    Err(e) => {
                        return Err(CoreError::new(
                            "REMOTE_IO_ERROR",
                            format!("远端文件读取失败 ({:?})", e.kind()),
                        ))
                    }
                }
            }
            "files.closeDirectory" | "files.close" => {
                let id = p["resourceId"].as_str().unwrap();
                if let Some(h) = self.handles.get_mut(id) {
                    h.file.close().map(|_| {
                        self.handles.remove(id);
                        Some(json!({"closed":true}))
                    })
                } else {
                    Ok(Some(json!({"closed":true})))
                }
            }
            _ => return Err(CoreError::new("METHOD_NOT_FOUND", "不支持的文件方法")),
        };
        match result {
            Ok(v) => Ok(v),
            Err(e) if pending(&e) => Ok(None),
            Err(e) => Err(failure(e)),
        }
    }
}
impl Exec {
    fn start_step(&mut self, session: &Session, allow_open: bool) -> Result<bool> {
        if self.phase == 0 && !allow_open {
            return Ok(false);
        }
        self.in_flight = true;
        let outcome = match self.phase {
            0 => session.channel_session().map(|c| self.channel = Some(c)),
            1 => self
                .channel
                .as_mut()
                .unwrap()
                .exec(self.job.params["command"].as_str().unwrap()),
            _ => {
                if self.input_offset < self.input.len() {
                    match self
                        .channel
                        .as_mut()
                        .unwrap()
                        .write(&self.input[self.input_offset..])
                    {
                        Ok(n) => {
                            self.in_flight = false;
                            self.input_offset += n;
                            return Ok(false);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                        Err(_) => {
                            self.in_flight = false;
                            return Err(CoreError::new("EXEC_FAILED", "无法写入执行输入"));
                        }
                    }
                }
                self.channel.as_mut().unwrap().send_eof()
            }
        };
        match outcome {
            Ok(()) => {
                self.in_flight = false;
                self.phase += 1;
                Ok(self.phase == 3)
            }
            Err(e) if pending(&e) => Ok(false),
            Err(e) => {
                self.in_flight = false;
                Err(failure(e))
            }
        }
    }
    fn advance(&mut self, session: &Session, allow_open: bool) {
        let shared = self.job.exec.as_ref().unwrap().clone();
        if let Err(e) = self.job.valid() {
            let mut s = shared.lock().unwrap();
            if !["completed", "failed", "canceled"].contains(&s.state.as_str()) {
                s.state = if e.code == "CANCELED" {
                    "canceled"
                } else {
                    "failed"
                }
                .into();
                s.reason = Some(e.code);
            }
            self.closing = true;
        }
        if shared.lock().unwrap().canceled {
            self.closing = true;
            let mut s = shared.lock().unwrap();
            s.state = "canceled".into();
            s.reason = Some("CANCELED".into());
        }
        if self.closing {
            if self.in_flight {
                let _ = self.start_step(session, true);
                if self.in_flight {
                    return;
                }
            }
            if let Some(c) = &mut self.channel {
                match c.close() {
                    Err(e) if pending(&e) => return,
                    _ => {}
                }
            }
            self.job
                .finish(Err(CoreError::new("CANCELED", "执行已结束")));
            self.phase = 255;
            return;
        }
        if self.phase < 3 {
            match self.start_step(session, allow_open) {
                Ok(true) => {
                    shared.lock().unwrap().state = "running".into();
                    self.job
                        .finish(Ok(json!({"execId":self.job.params["resourceId"]})));
                }
                Ok(false) => {}
                Err(e) => {
                    let mut s = shared.lock().unwrap();
                    s.state = "failed".into();
                    s.reason = Some(e.code.clone());
                    self.job.finish(Err(e));
                    self.closing = true;
                }
            }
            return;
        }
        let c = self.channel.as_mut().unwrap();
        for stream in 0..2 {
            let capacity = 512 * 1024 - shared.lock().unwrap().buffered;
            if capacity == 0 {
                break;
            }
            let mut bytes = vec![0; capacity.min(32768)];
            match c.stream(stream).read(&mut bytes) {
                Ok(n) if n > 0 => {
                    bytes.truncate(n);
                    let mut s = shared.lock().unwrap();
                    let limit = self.job.params["maxOutputBytes"]
                        .as_u64()
                        .unwrap_or(4 * 1024 * 1024) as usize;
                    if s.total + n > limit {
                        s.truncated = true;
                        s.state = "failed".into();
                        s.reason = Some("LIMIT_EXCEEDED".into());
                        self.closing = true;
                        break;
                    }
                    s.total += n;
                    s.buffered += n;
                    s.sequence += 1;
                    let sequence = s.sequence;
                    s.frames.push_back(json!({"sequence":sequence,"stream":if stream==0{"stdout"}else{"stderr"},"bytesBase64":STANDARD.encode(bytes),"length":n}));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    let mut s = shared.lock().unwrap();
                    s.state = "failed".into();
                    s.reason = Some("CONNECTION_LOST".into());
                    self.closing = true;
                }
            }
        }
        if !self.closing && c.eof() {
            match c.wait_close() {
                Ok(()) => {
                    let mut s = shared.lock().unwrap();
                    s.exit = c.exit_status().ok();
                    s.state = "completed".into();
                    self.closing = true;
                }
                Err(e) if pending(&e) => {}
                Err(_) => {
                    let mut s = shared.lock().unwrap();
                    s.state = "failed".into();
                    s.reason = Some("CONNECTION_LOST".into());
                    self.closing = true;
                }
            }
        }
    }
}
