pub mod commands;
mod commands_allowlist;
pub mod event;
mod snapshot;
mod store;
pub use commands_allowlist::known_command;
#[cfg(test)]
mod tests;

use event::{now, Context, Event};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const QUEUE_BYTES: usize = 4 * 1024 * 1024;
static GLOBAL: OnceLock<Arc<Diagnostics>> = OnceLock::new();
static BOOT: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static EARLY_PANIC: Mutex<Option<(PathBuf, File)>> = Mutex::new(None);
pub fn global() -> Option<&'static Arc<Diagnostics>> {
    GLOBAL.get()
}
pub fn startup(name: &'static str) {
    Context::new().event("INFO", "app", name, json!({}));
}
pub fn failure(module: &'static str, name: &'static str, error: &crate::model::AppError) {
    Context::from_trace(error.trace_id.as_deref()).event(
        "ERROR",
        module,
        name,
        event::error_details(error),
    );
}
#[track_caller]
pub fn task_failure(module: &'static str, subject: &str, error: &crate::model::AppError) {
    let mut details = event::error_details(error);
    details["taskRef"] = json!(alias(subject));
    Context::from_trace(error.trace_id.as_deref()).event("ERROR", module, "task.failed", details);
}
pub fn record(event: Event) {
    if let Some(service) = global() {
        service.record(event);
    } else if let Ok(mut boot) = BOOT.lock() {
        if boot.len() < 64 {
            boot.push(event);
        }
    }
}
pub fn alias(value: &str) -> String {
    static SALT: OnceLock<String> = OnceLock::new();
    let salt = SALT.get_or_init(event::id);
    let mut hash = Sha256::new();
    hash.update(salt.as_bytes());
    hash.update([0]);
    hash.update(value.as_bytes());
    format!("ref-{}", &format!("{:x}", hash.finalize())[..24])
}
#[track_caller]
pub fn observe(module: &'static str, event: &'static str, subject: &str, details: Value) {
    let ctx = Context::new();
    let mut details = details;
    details["taskRef"] = json!(alias(subject));
    ctx.event("INFO", module, event, details);
}
/// Emit state changes immediately, but bound repeated progress notifications.
pub fn state_event(module: &'static str, subject: &str, phase: &str) {
    static STATES: OnceLock<Mutex<std::collections::HashMap<String, (String, Instant, u64)>>> =
        OnceLock::new();
    let reference = alias(subject);
    let key = format!("{module}:{reference}");
    let mut states = STATES.get_or_init(Default::default).lock().unwrap();
    if states.len() > 2048 {
        states.retain(|_, (_, at, _)| at.elapsed() < Duration::from_secs(60));
        if states.len() > 2048 {
            states.clear();
        }
    }
    let previous = states
        .entry(key)
        .or_insert((String::new(), Instant::now(), 0));
    previous.2 += 1;
    if previous.0 == phase && previous.1.elapsed() < Duration::from_secs(30) {
        return;
    }
    let count = previous.2;
    *previous = (phase.into(), Instant::now(), 0);
    drop(states);
    observe(
        module,
        "state.changed",
        subject,
        json!({"phase":phase,"count":count}),
    );
}
pub fn initialize(path: PathBuf) -> Arc<Diagnostics> {
    let temporary = path == std::env::temp_dir().join("SoDeck-diagnostics");
    let service = Diagnostics::start(path, temporary)
        .or_else(|_| Diagnostics::start(std::env::temp_dir().join("SoDeck-diagnostics"), true))
        .unwrap_or_else(|_| Diagnostics::unavailable());
    let service = Arc::new(service);
    let _ = GLOBAL.set(service.clone());
    if let Ok(mut early) = EARLY_PANIC.lock() {
        if let Some((path, file)) = early.take() {
            let empty = file.metadata().is_ok_and(|m| m.len() == 0);
            drop(file);
            if empty {
                let _ = fs::remove_file(path);
            }
        }
    }
    if let Ok(mut boot) = BOOT.lock() {
        for event in boot.drain(..) {
            service.record(event);
        }
    }
    Context::new().event("INFO", "app", "startup.diagnostics.ready", json!({}));
    if !service.status().unavailable {
        if let Ok(file) = store::open(&service.root.join(".export.lock")) {
            if file.try_lock().is_ok() {
                let _ = snapshot::cleanup(&service.root);
            }
        }
    }
    service
}
pub fn install_panic_hook() {
    // Open before Tauri setup, so even a failure before app paths are available
    // has an independent, bounded emergency file. No panic payload is retained.
    let early_path = std::env::temp_dir().join(format!("SoDeck-startup-{}.jsonl", event::id()));
    if let Ok(file) = store::open(&early_path) {
        if let Ok(mut early) = EARLY_PANIC.lock() {
            *early = Some((early_path, file));
        }
    }
    std::panic::set_hook(Box::new(move |info| {
        let source = info
            .location()
            .map(|p| event::source(p.file(), p.line()))
            .unwrap_or_default();
        let frames: Vec<String> = std::backtrace::Backtrace::force_capture()
            .to_string()
            .lines()
            .filter_map(|line| {
                let line = line.replace('\\', "/");
                let start = line.find("src-tauri/src/")?;
                let frame = line[start..].trim();
                (frame.len() < 180
                    && frame
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/_-.:".contains(&b)))
                .then(|| frame.to_owned())
            })
            .take(24)
            .collect();
        let event = json!({"schemaVersion":1,"tsUtc":event::timestamp(now()),"event":"rust.panic","source":source,"stackFrames":frames,"runId":global().map(|s|&s.run_id),"buildId":option_env!("SODECK_BUILD_ID"),"detailOmitted":true});
        if let Some(service) = global() {
            service.panic(&event);
        } else {
            if let Ok(mut file) = EARLY_PANIC.try_lock() {
                if let Some((_, file)) = file.as_mut() {
                    if file.metadata().is_ok_and(|m| m.len() < 240 * 1024) {
                        if let Ok(mut bytes) = serde_json::to_vec(&event) {
                            bytes.push(b'\n');
                            let _ = append_line(file, &bytes);
                            let _ = file.sync_data();
                        }
                    }
                }
            }
            let ctx = Context::new();
            ctx.event(
                "ERROR",
                "app",
                "startup.panic",
                json!({"errorSource":source,"detailOmitted":true}),
            );
        }
    }));
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub directory: String,
    pub run_id: String,
    pub started_at: i64,
    pub bytes: u64,
    pub runs: usize,
    pub dropped: u64,
    pub incomplete: bool,
    pub fallback: bool,
    pub unavailable: bool,
    pub error: Option<String>,
    pub export_phase: String,
    pub export_completed: u64,
    pub export_total: u64,
}
enum Message {
    Event(Event, usize),
    Flush(mpsc::Sender<io::Result<()>>),
    Snapshot(
        snapshot::Scope,
        mpsc::Sender<io::Result<snapshot::Snapshot>>,
    ),
    Clear(mpsc::Sender<io::Result<()>>),
    Finish(mpsc::Sender<io::Result<()>>),
}
pub struct Diagnostics {
    sender: mpsc::SyncSender<Message>,
    queued: Arc<AtomicUsize>,
    dropped: Arc<AtomicU64>,
    status: Arc<Mutex<Status>>,
    pub root: PathBuf,
    pub run_id: String,
    panic_file: Mutex<Option<File>>,
    pub exporting: AtomicBool,
    pub cancelled: AtomicBool,
    pub export_owner: Mutex<Option<String>>,
    pub exported: Mutex<Option<PathBuf>>,
}
fn health(error: &io::Error) -> String {
    format!(
        "诊断写入不可用（{:?}，系统码 {:?}），记录可能不完整。",
        error.kind(),
        error.raw_os_error()
    )
}
impl Diagnostics {
    pub fn start(root: PathBuf, fallback: bool) -> io::Result<Self> {
        store::checked_dir(&root)?;
        store::checked_dir(&root.join("runs"))?;
        let guard = store::lock(&root)?;
        let session = store::Session::new(fallback);
        let path = root.join("runs").join(&session.directory);
        store::checked_dir(&path)?;
        let active = store::open(&path.join(".active.lock"))?;
        active.try_lock().map_err(io::Error::other)?;
        let segment = path.join("events-0001.jsonl");
        store::maintain(&root, &path, &segment, 16384)?;
        store::atomic(&root.join("README.txt"), store::README.as_bytes())?;
        store::write_metadata(&root, &path, &session)?;
        let mut panic_file = store::open(&path.join("panic.jsonl"))?;
        use std::io::Seek;
        panic_file.seek(std::io::SeekFrom::End(0))?;
        let file = store::open(&segment)?;
        let status = Arc::new(Mutex::new(Status {
            directory: root.to_string_lossy().into(),
            run_id: session.run_id.clone(),
            started_at: session.started_at,
            bytes: store::usage(&root)?,
            runs: store::runs(&root)?.len(),
            dropped: 0,
            incomplete: false,
            fallback,
            unavailable: false,
            error: None,
            export_phase: "idle".into(),
            export_completed: 0,
            export_total: 0,
        }));
        drop(guard);
        let (sender, receive) = mpsc::sync_channel(4096);
        let queued = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicU64::new(0));
        let writer = Writer {
            root: root.clone(),
            path,
            segment,
            file,
            session: session.clone(),
            _active: active,
            index: 1,
            status: status.clone(),
            dropped: dropped.clone(),
            queued: queued.clone(),
            index_size: 0,
            finished: false,
        };
        thread::Builder::new()
            .name("diagnostics-writer".into())
            .spawn(move || writer.run(receive))?;
        Ok(Self {
            sender,
            queued,
            dropped,
            status,
            root,
            run_id: session.run_id,
            panic_file: Mutex::new(Some(panic_file)),
            exporting: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            export_owner: Mutex::new(None),
            exported: Mutex::new(None),
        })
    }
    fn unavailable() -> Self {
        let (sender, _) = mpsc::sync_channel(1);
        Self {
            sender,
            queued: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            status: Arc::new(Mutex::new(Status {
                directory: String::new(),
                run_id: event::id(),
                started_at: now(),
                bytes: 0,
                runs: 0,
                dropped: 0,
                incomplete: true,
                fallback: false,
                unavailable: true,
                error: Some("无法创建诊断目录，本次无法保存日志。".into()),
                export_phase: "idle".into(),
                export_completed: 0,
                export_total: 0,
            })),
            root: PathBuf::new(),
            run_id: event::id(),
            panic_file: Mutex::new(None),
            exporting: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            export_owner: Mutex::new(None),
            exported: Mutex::new(None),
        }
    }
    pub fn record(&self, event: Event) {
        let size = serde_json::to_vec(&event)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        let priority = matches!(event.level, "WARN" | "ERROR");
        let limit = if priority {
            QUEUE_BYTES
        } else {
            QUEUE_BYTES - 256 * 1024
        };
        if size > 16 * 1024
            || self
                .queued
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    v.checked_add(size).filter(|n| *n <= limit)
                })
                .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if self.sender.try_send(Message::Event(event, size)).is_err() {
            self.queued.fetch_sub(size, Ordering::Relaxed);
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn request<T>(
        &self,
        make: impl FnOnce(mpsc::Sender<io::Result<T>>) -> Message,
    ) -> io::Result<T> {
        let (tx, rx) = mpsc::channel();
        let mut message = make(tx);
        let start = Instant::now();
        loop {
            match self.sender.try_send(message) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(m)) if start.elapsed() < Duration::from_secs(3) => {
                    message = m;
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(io::Error::other("诊断队列暂不可用")),
            }
        }
        // These requests run on blocking workers. Keep the caller's export/clear
        // guard alive until the queued operation actually completes.
        rx.recv()
            .map_err(|_| io::Error::other("诊断写入线程已停止"))?
    }
    pub fn flush(&self) -> io::Result<()> {
        self.request(Message::Flush)
    }
    pub fn finish(&self) {
        let (reply, receive) = mpsc::channel();
        if self.sender.try_send(Message::Finish(reply)).is_ok() {
            let _ = receive.recv_timeout(Duration::from_secs(3));
        }
    }
    pub fn clear(&self) -> io::Result<()> {
        self.request(Message::Clear)
    }
    pub fn snapshot(&self, scope: snapshot::Scope) -> io::Result<snapshot::Snapshot> {
        self.request(|reply| Message::Snapshot(scope, reply))
    }
    pub fn status(&self) -> Status {
        let mut status = self.status.lock().unwrap().clone();
        status.dropped = self.dropped.load(Ordering::Relaxed);
        status.incomplete |= status.dropped > 0;
        status
    }
    pub fn progress(&self, phase: &str, completed: u64, total: u64) {
        if let Ok(mut s) = self.status.lock() {
            s.export_phase = phase.into();
            s.export_completed = completed;
            s.export_total = total;
        }
    }
    fn panic(&self, value: &Value) {
        if let Ok(mut file) = self.panic_file.try_lock() {
            if let Some(file) = file.as_mut() {
                if file.metadata().is_ok_and(|m| m.len() < 240 * 1024) {
                    if let Ok(mut bytes) = serde_json::to_vec(value) {
                        use std::io::Seek;
                        let _ = file.seek(std::io::SeekFrom::End(0));
                        bytes.push(b'\n');
                        let _ = file.write_all(&bytes);
                        let _ = file.sync_data();
                    }
                }
            }
        }
    }
}
struct Writer {
    root: PathBuf,
    path: PathBuf,
    segment: PathBuf,
    file: File,
    session: store::Session,
    _active: File,
    index: u32,
    status: Arc<Mutex<Status>>,
    dropped: Arc<AtomicU64>,
    queued: Arc<AtomicUsize>,
    index_size: u64,
    finished: bool,
}
/// Roll back a partial line before reporting a failed write. Subsequent events
/// must never turn a truncated JSON record into an invalid line in the middle.
fn append_line(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    use std::io::{Seek, SeekFrom};
    let offset = file.seek(SeekFrom::End(0))?;
    if let Err(error) = file.write_all(bytes) {
        file.set_len(offset)?;
        file.seek(SeekFrom::Start(offset))?;
        return Err(error);
    }
    Ok(())
}
impl Writer {
    fn run(mut self, receive: mpsc::Receiver<Message>) {
        let mut last = Instant::now();
        let mut pending = None;
        loop {
            match pending
                .take()
                .map(Ok)
                .unwrap_or_else(|| receive.recv_timeout(Duration::from_millis(500)))
            {
                Ok(Message::Event(event, size)) => {
                    let mut batch = vec![(event, size)];
                    while batch.len() < 64 {
                        match receive.try_recv() {
                            Ok(Message::Event(event, size)) => batch.push((event, size)),
                            Ok(other) => {
                                pending = Some(other);
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    let bytes: usize = batch.iter().map(|(_, size)| size).sum();
                    self.queued.fetch_sub(bytes, Ordering::Relaxed);
                    // One quota check and cross-process lock per bounded batch.
                    // Reserve envelope/index growth conservatively before writing.
                    let reservation = store::lock(&self.root).and_then(|guard| {
                        self.session.removed_segments += store::maintain(
                            &self.root,
                            &self.path,
                            &self.segment,
                            (bytes * 2 + batch.len() * 4096) as u64,
                        )?;
                        Ok(guard)
                    });
                    match reservation {
                        Ok(_guard) => {
                            for (event, _) in batch {
                                if let Err(e) = self.write(event) {
                                    self.dropped.fetch_add(1, Ordering::Relaxed);
                                    self.failed(&e);
                                }
                            }
                        }
                        Err(e) => {
                            self.dropped
                                .fetch_add(batch.len() as u64, Ordering::Relaxed);
                            self.failed(&e);
                        }
                    }
                }
                Ok(Message::Flush(reply)) => {
                    let result = self.flush(true);
                    let _ = reply.send(result);
                }
                Ok(Message::Snapshot(scope, reply)) => {
                    let result = self.snapshot(scope);
                    let _ = reply.send(result);
                }
                Ok(Message::Clear(reply)) => {
                    let result = self.clear();
                    let _ = reply.send(result);
                }
                Ok(Message::Finish(reply)) => {
                    self.session.ended_at = Some(now());
                    let result = self.flush(true);
                    self.finished = result.is_ok();
                    let _ = reply.send(result);
                    if self.finished {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = self.flush(true);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !self.finished && last.elapsed() >= Duration::from_secs(1) {
                if let Err(e) = self.flush(false) {
                    self.failed(&e);
                }
                last = Instant::now();
            }
        }
    }
    fn failed(&mut self, error: &io::Error) {
        self.session.incomplete = true;
        if let Ok(mut status) = self.status.lock() {
            status.incomplete = true;
            status.error = Some(health(error));
        }
    }
    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let index = self.index + 1;
        let segment = self.path.join(format!("events-{index:04}.jsonl"));
        let file = store::open(&segment)?;
        self.index = index;
        self.segment = segment;
        self.file = file;
        Ok(())
    }
    fn write(&mut self, event: Event) -> io::Result<()> {
        let ts = event.captured_at;
        let seq = self.session.last_seq + 1;
        let mut value = serde_json::to_value(&event)?;
        value["schemaVersion"] = json!(1);
        value["tsUtc"] = json!(event::timestamp(ts));
        value["seq"] = json!(seq);
        value["runId"] = json!(self.session.run_id);
        value["processId"] = json!(std::process::id());
        value["buildId"] = json!(self.session.build_id);
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        if self.file.metadata()?.len() + bytes.len() as u64 > store::SEGMENT {
            self.rotate()?;
        }
        append_line(&mut self.file, &bytes)?;
        if event.event == "frontend.dropped" {
            self.session.incomplete = true;
            self.dropped.fetch_add(
                event
                    .details
                    .get("dropped")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(1_000_000_000),
                Ordering::Relaxed,
            );
        }
        self.session.last_seq = seq;
        self.session.updated_at = now();
        if matches!(event.level, "WARN" | "ERROR") {
            if event.level == "WARN" {
                self.session.warnings += 1;
            } else {
                self.session.errors += 1;
            }
            let item = json!({"seq":seq,"tsUtc":event::timestamp(ts),"level":event.level,"event":event.event,"traceId":event.trace_id,"errorCode":event.details.get("errorCode")});
            self.session.recent_errors.push(item.clone());
            if self.session.recent_errors.len() > 20 {
                self.session.recent_errors.remove(0);
            }
            let mut index = serde_json::to_vec(&item)?;
            index.push(b'\n');
            if self.index_size + index.len() as u64 <= 1024 * 1024 {
                let mut file = store::open(&self.path.join("errors.jsonl"))?;
                use std::io::Seek;
                file.seek(std::io::SeekFrom::End(0))?;
                append_line(&mut file, &index)?;
                self.index_size += index.len() as u64;
            } else {
                self.session.incomplete = true;
            }
            self.file.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self, sync: bool) -> io::Result<()> {
        let _guard = store::lock(&self.root)?;
        self.file.flush()?;
        if sync {
            self.file.sync_data()?;
        }
        self.session.dropped = self.dropped.load(Ordering::Relaxed);
        self.session.removed_segments +=
            store::maintain(&self.root, &self.path, &self.segment, 65536)?;
        store::write_metadata(&self.root, &self.path, &self.session)?;
        let mut status = self.status.lock().unwrap();
        status.bytes = store::usage(&self.root)?;
        status.runs = store::runs(&self.root)?.len();
        status.incomplete = self.session.incomplete
            || self.session.removed_segments > 0
            || self.session.dropped > 0;
        status.error = None;
        Ok(())
    }
    fn snapshot(&mut self, scope: snapshot::Scope) -> io::Result<snapshot::Snapshot> {
        self.flush(true)?;
        let _guard = store::lock(&self.root)?;
        self.rotate()?;
        snapshot::capture(&self.root, &self.session, scope)
    }
    fn clear(&mut self) -> io::Result<()> {
        let _guard = store::lock(&self.root)?;
        let runs = store::runs(&self.root)?;
        if runs
            .iter()
            .any(|r| r.active && r.session.run_id != self.session.run_id)
        {
            return Err(io::Error::other(
                "请先关闭其他 SoDeck 实例，再清理全部诊断记录。",
            ));
        }
        self.file.set_len(0)?;
        use std::io::Seek;
        self.file.rewind()?;
        for run in runs {
            if run.session.run_id == self.session.run_id {
                for path in store::files(&run.path)? {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if path != self.segment && store::allowed(name) && name != "panic.jsonl" {
                        fs::remove_file(path)?;
                    }
                }
            } else {
                store::remove_run(&run)?;
            }
        }
        // The emergency handle remains open; truncate it without replacing its inode.
        if let Ok(file) = store::open(&self.path.join("panic.jsonl")) {
            file.set_len(0)?;
        }
        self.session.recent_errors.clear();
        self.session.removed_segments += 1;
        self.index_size = 0;
        self.session.incomplete = true;
        store::write_metadata(&self.root, &self.path, &self.session)?;
        Ok(())
    }
}
