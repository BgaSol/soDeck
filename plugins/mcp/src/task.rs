use crate::targets::{Descriptor, Target};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{watch, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Exec,
    Download,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecMode {
    Batch,
    Pipe,
    Pty,
}
impl ExecMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::Pipe => "pipe",
            Self::Pty => "pty",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecSpec {
    pub target_id: String,
    pub request_id: String,
    pub command: String,
    pub mode: ExecMode,
    pub timeout_seconds: Option<u64>,
    pub terminal_size: Option<(u32, u32)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadSpec {
    pub target_id: String,
    pub request_id: String,
    pub remote_path: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskSpec {
    Exec(ExecSpec),
    Download(DownloadSpec),
}
impl TaskSpec {
    pub fn terminal(&self) -> bool {
        matches!(self,TaskSpec::Exec(spec) if spec.mode==ExecMode::Pty)
    }
    pub fn kind(&self) -> Kind {
        match self {
            Self::Exec(_) => Kind::Exec,
            Self::Download(_) => Kind::Download,
        }
    }
    pub fn target_id(&self) -> &str {
        match self {
            Self::Exec(s) => &s.target_id,
            Self::Download(s) => &s.target_id,
        }
    }
    pub fn request_id(&self) -> &str {
        match self {
            Self::Exec(s) => &s.request_id,
            Self::Download(s) => &s.request_id,
        }
    }
    pub fn json(&self) -> Value {
        match self {
            Self::Exec(s) => {
                let mut v = json!({"targetId":s.target_id,"requestId":s.request_id,"command":s.command,"timeoutSeconds":s.timeout_seconds});
                if s.mode != ExecMode::Batch {
                    v["mode"] = json!(s.mode.name());
                }
                if let Some((cols, rows)) = s.terminal_size {
                    v["cols"] = json!(cols);
                    v["rows"] = json!(rows);
                }
                v
            }
            Self::Download(s) => {
                json!({"targetId":s.target_id,"requestId":s.request_id,"remotePath":s.remote_path})
            }
        }
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
pub fn fixture(root: PathBuf) -> Arc<Task> {
    Arc::new(Task::new(NewTask {
        run: "test-run".into(),
        order: 1,
        admission_sequence: 1,
        target: Arc::new(Target::new(
            "target".into(),
            1,
            "view".into(),
            crate::targets::Context {
                workspace_id: "workspace".into(),
                server_id: "server".into(),
                connection_id: "connection".into(),
                generation: 1,
            },
        )),
        spec: TaskSpec::Exec(ExecSpec {
            target_id: "target".into(),
            request_id: "request".into(),
            command: "中文\ncommand".into(),
            mode: ExecMode::Batch,
            timeout_seconds: Some(300),
            terminal_size: None,
        }),
        root,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_meaningful_facts_wake_and_terminal_publication_is_stable() {
        let task = fixture(PathBuf::new());
        task.observe(json!({"state":"running","elapsedMs":1}), Some(0));
        let before = task.snapshot();
        task.observe(json!({"state":"running","elapsedMs":999}), Some(0));
        assert_eq!(task.snapshot().revision, before.revision);
        task.observe(json!({"state":"running","received":"1"}), Some(12));
        assert!(task.snapshot().revision > before.revision);
        task.finalizing(true, Value::Null);
        let closing = task.request_cancel();
        assert_eq!(closing.cancel_requested, false);
        assert!(closing.phase != Phase::Done);
        task.publish(
            task.final_candidate(),
            Some(json!({"code":"disk","message":"disk full"})),
        );
        let done = task.snapshot();
        assert!(done.phase == Phase::Done);
        assert!(matches!(done.saved, Saved::Failed));
        task.request_cancel();
        task.observe(json!({"state":"cancelled"}), Some(999));
        task.cleanup(true, Value::Null);
        assert_eq!(task.snapshot().revision, done.revision);
        assert_eq!(task.snapshot().native, done.native);
        assert_eq!(task.snapshot().committed, 12);
    }
    #[test]
    fn input_control_keeps_only_latest_receipt_and_one_uncertain_attempt() {
        use crate::input::{Dispatch, NormalizedInput};
        let mut task = fixture(PathBuf::new());
        if let TaskSpec::Exec(spec) = &mut Arc::get_mut(&mut task).unwrap().spec {
            spec.mode = ExecMode::Pipe;
            spec.timeout_seconds = None;
        }
        let pool = Arc::new(Semaphore::new(4));
        let operation = NormalizedInput::Write(Arc::from(&b"raw\0\r"[..]));
        let native = |sequence: u64, phase: &str, state: &str| {
            json!({"phase":phase,"launchAccepted":true,"workerFinished":false,
            "input":{"state":"open","available":true,"nextInputSequence":(sequence+1).to_string(),"last":if sequence==0{Value::Null}else{json!({"sequence":sequence.to_string(),"action":"write","state":state})}}})
        };
        task.set_process("process".into(), native(0, "running", "sent"));
        for sequence in 1..=5000 {
            let guard = match task
                .input_dispatch(sequence, operation.clone(), &pool)
                .unwrap()
            {
                Dispatch::Send { guard, .. } => guard,
                _ => panic!("expected dispatch"),
            };
            assert!(matches!(
                task.input_dispatch(sequence, operation.clone(), &pool)
                    .unwrap(),
                Dispatch::Existing(_)
            ));
            assert!(task
                .input_dispatch(sequence + 1, operation.clone(), &pool)
                .is_err());
            task.observe(native(sequence, "running", "sent"), Some(sequence));
            drop(guard);
            assert!(task.snapshot().control.unwrap().attempt.is_none());
        }
        assert!(task.input_dispatch(4999, operation.clone(), &pool).is_err());
        assert!(matches!(
            task.input_dispatch(5000, operation.clone(), &pool).unwrap(),
            Dispatch::Existing(_)
        ));
        let guard = match task.input_dispatch(5001, operation.clone(), &pool).unwrap() {
            Dispatch::Send { guard, .. } => guard,
            _ => panic!(),
        };
        task.input_error(5001, json!({"code":"lost"}), true);
        drop(guard);
        let control = task.snapshot().control.unwrap();
        assert_eq!(control.last_accepted.unwrap().0, 5000);
        assert_eq!(control.attempt.unwrap().sequence, 5001);
        task.observe(native(5000, "finishing", "sent"), Some(5000));
        task.observe(native(4999, "running", "pending"), Some(0));
        let record = task.snapshot();
        assert_eq!(record.native["phase"], "finishing");
        assert_eq!(record.native["input"]["last"]["sequence"], "5000");
        assert_eq!(record.committed, 5000);
        task.seal_input();
        assert!(task.input_dispatch(5001, operation, &pool).is_err());
    }
    #[test]
    fn cancellation_receipt_is_the_same_record_as_its_revision_notification() {
        let task = fixture(PathBuf::new());
        let changes = task.subscribe();
        let receipt = task.request_cancel();
        assert!(receipt.cancel_requested);
        assert!(task.cancel.is_cancelled());
        assert_eq!(receipt.revision, *changes.borrow());
        assert_eq!(receipt.revision, 2);
        assert_eq!(task.request_cancel().revision, receipt.revision);
    }
    #[test]
    fn complete_target_is_owned_and_long_error_changes_wake() {
        let task = fixture(PathBuf::new());
        let mut descriptor:Descriptor=serde_json::from_value(json!({"name":"server","protocol":"ssh","host":"10.0.0.187","port":22,"endpoint":"10.0.0.187:22","username":"ops"})).unwrap();
        task.set_target(descriptor.clone());
        let captured = task.snapshot();
        descriptor.host = "10.0.1.187".into();
        assert_eq!(captured.target.as_ref().unwrap().host, "10.0.0.187");
        assert_eq!(captured.target.as_ref().unwrap().port, 22);
        let native = |suffix: &str| json!({"state":"unavailable","error":{"code":"remote","message":format!("{}{suffix}","x".repeat(600))}});
        task.observe(native("first"), None);
        let changes = task.subscribe();
        let before = task.snapshot().revision;
        task.observe(native("second"), None);
        assert_eq!(*changes.borrow(), before + 1);
        assert_eq!(task.snapshot().target, captured.target);
    }
    #[test]
    fn summary_is_a_bounded_preview_not_original_input() {
        let (text, cut) = preview("中中中", 8);
        assert_eq!(text, "中中");
        assert!(cut);
        let task = fixture(PathBuf::new());
        let summary = crate::response::ui_row(&CapturedTask::new(task));
        assert!(summary.get("input").is_none());
        assert!(summary.get("directory").is_none());
        assert_eq!(summary["inputPreview"], "中文\ncommand");
    }
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Download => "download",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Collecting,
    Finalizing,
    Done,
}
#[derive(Clone, Copy)]
pub(crate) enum Saved {
    Collecting,
    Finalizing,
    Complete,
    Incomplete,
    Failed,
}
#[derive(Clone)]
pub struct Record {
    pub native: Value,
    pub target: Option<Descriptor>,
    pub phase: Phase,
    pub saved: Saved,
    pub error: Value,
    pub path: Option<PathBuf>,
    pub capture_complete: bool,
    pub committed: u64,
    pub cancel_requested: bool,
    pub released: bool,
    pub cleanup_error: Value,
    pub observed: u64,
    pub finished: Option<u64>,
    pub revision: u64,
    pub(crate) control: Option<crate::input::CommandControl>,
}
pub struct CapturedTask {
    pub task: Arc<Task>,
    pub record: Record,
}
pub struct TaskFailure {
    pub error: Value,
    pub captured: Option<CapturedTask>,
}
impl From<Value> for TaskFailure {
    fn from(error: Value) -> Self {
        Self {
            error,
            captured: None,
        }
    }
}
impl CapturedTask {
    pub fn new(task: Arc<Task>) -> Self {
        let record = task.snapshot();
        Self { task, record }
    }
}
pub struct Task {
    pub id: String,
    pub run: String,
    pub order: u64,
    pub admission_sequence: u64,
    pub target: Arc<Target>,
    pub spec: TaskSpec,
    pub created: u64,
    pub directory: PathBuf,
    pub cancel: CancellationToken,
    pub waiters: Arc<Semaphore>,
    state: Mutex<Record>,
    changes: watch::Sender<u64>,
}
pub struct NewTask {
    pub run: String,
    pub order: u64,
    pub admission_sequence: u64,
    pub target: Arc<Target>,
    pub spec: TaskSpec,
    pub root: PathBuf,
}
pub fn preview(value: &str, bytes: usize) -> (String, bool) {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), end < value.len())
}
pub fn meaningful_native(native: &Value) -> Value {
    let mut value = json!({});
    for key in [
        "state",
        "admission",
        "execSent",
        "exitCode",
        "exitSignal",
        "remoteExitConfirmed",
        "workerFinished",
        "outputComplete",
        "received",
        "total",
        "streamId",
        "id",
        "error",
        "phase",
        "input",
        "launchAccepted",
        "launchQueued",
    ] {
        if let Some(field) = native.get(key) {
            value[key] = field.clone();
        }
    }
    value
}
impl Task {
    pub fn terminal(&self) -> bool {
        self.spec.terminal()
    }
    pub fn kind(&self) -> Kind {
        self.spec.kind()
    }
    pub fn start_input(&self) -> Value {
        self.spec.json()
    }
    pub fn new(meta: NewTask) -> Self {
        let id = format!("task-{}-{}", meta.run, meta.order);
        let directory = meta.root.join("evidence").join(&meta.run).join(&id);
        let (changes, _) = watch::channel(1);
        Self {
            id,
            directory,
            run: meta.run,
            order: meta.order,
            admission_sequence: meta.admission_sequence,
            target: meta.target,
            spec: meta.spec.clone(),
            created: now(),
            cancel: CancellationToken::new(),
            waiters: Arc::new(Semaphore::new(2)),
            changes,
            state: Mutex::new(Record {
                native: json!({"admission":"pending"}),
                target: None,
                phase: Phase::Collecting,
                saved: Saved::Collecting,
                error: Value::Null,
                path: None,
                capture_complete: false,
                committed: 0,
                cancel_requested: false,
                released: false,
                cleanup_error: Value::Null,
                observed: now(),
                finished: None,
                revision: 1,
                control: (meta.spec.kind() == Kind::Exec)
                    .then(crate::input::CommandControl::default),
            }),
        }
    }
    fn update(&self, change: impl FnOnce(&mut Record) -> bool) {
        let mut record = self.state.lock().unwrap();
        if record.phase == Phase::Done {
            return;
        }
        if change(&mut record) {
            record.revision += 1;
            self.changes.send_replace(record.revision);
        }
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
    pub fn observe(&self, native: Value, committed: Option<u64>) {
        self.update(|r| {
            let mut native = native;
            let old_sequence = r.native["input"]["last"]["sequence"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let new_sequence = native["input"]["last"]["sequence"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if new_sequence < old_sequence && native.get("input").is_some() {
                native["input"] = r.native["input"].clone();
            } else if new_sequence == old_sequence
                && r.native["input"]["last"]["state"] != "pending"
                && native["input"]["last"]["state"] == "pending"
            {
                native["input"]["last"] = r.native["input"]["last"].clone();
            }
            if r.native["remoteExitConfirmed"] == true
                && native.get("remoteExitConfirmed").is_some()
            {
                native["remoteExitConfirmed"] = json!(true);
                native["exitCode"] = r.native["exitCode"].clone();
                native["exitSignal"] = r.native["exitSignal"].clone();
            }
            let rank = |v: &Value| match v.as_str() {
                Some("opening") => 1,
                Some("running") => 2,
                Some("finishing") => 3,
                Some("finished") => 4,
                _ => 0,
            };
            if rank(&native["phase"]) > 0 && rank(&native["phase"]) < rank(&r.native["phase"]) {
                native["phase"] = r.native["phase"].clone();
                native["inputClosed"] = r.native["inputClosed"].clone();
                if native["input"].is_object() {
                    native["input"]["available"] = json!(false);
                    native["input"]["state"] = json!("closed");
                }
            }
            if r.native["workerFinished"] == true
                && native.get("workerFinished").is_some()
                && native["workerFinished"] != true
            {
                native = r.native.clone();
            }
            if let Some(control) = r.control.as_mut() {
                if let Some(attempt) = control.attempt.as_mut() {
                    if native["input"]["last"]["sequence"]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        == Some(attempt.sequence)
                    {
                        control.last_accepted = Some((attempt.sequence, attempt.operation.clone()));
                        attempt.admission_unknown = false;
                        if !attempt.rpc_in_flight {
                            control.attempt = None;
                        }
                    }
                }
            }
            let committed = committed.map(|n| n.max(r.committed));
            let changed = meaningful_native(&r.native) != meaningful_native(&native)
                || committed.is_some_and(|n| n != r.committed);
            r.native = native;
            if let Some(n) = committed {
                r.committed = n;
            }
            r.observed = now();
            changed
        });
    }
    pub fn set_process(&self, id: String, native: Value) {
        self.update(|r| {
            if let Some(control) = r.control.as_mut() {
                control.process_id = Some(id);
            }
            r.native = native;
            r.observed = now();
            true
        });
    }
    pub(crate) fn input_dispatch(
        self: &Arc<Self>,
        sequence: u64,
        operation: crate::input::NormalizedInput,
        pool: &Arc<Semaphore>,
    ) -> Result<crate::input::Dispatch, Value> {
        use crate::input::{Dispatch, InputAttempt, InputCallGuard, NormalizedInput};
        let mut r = self.state.lock().unwrap();
        let control = r
            .control
            .as_ref()
            .ok_or_else(|| crate::tasks::failure("unsupported", "下载任务不支持输入"))?;
        if let Some((last, original)) = &control.last_accepted {
            if sequence == *last {
                return if original == &operation {
                    Ok(Dispatch::Existing(CapturedTask {
                        task: self.clone(),
                        record: r.clone(),
                    }))
                } else {
                    Err(crate::tasks::failure("inputConflict", "同序号输入内容不同"))
                };
            }
            if sequence < *last {
                return Err(crate::tasks::failure("inputExpired", "旧输入不会重新发送"));
            }
        }
        if control.sealed || r.phase != Phase::Collecting {
            return Err(crate::tasks::failure("inputClosed", "任务输入已封闭"));
        }
        if let Some(attempt) = &control.attempt {
            if attempt.sequence != sequence || attempt.operation != operation {
                return Err(crate::tasks::failure("resourceBusy", "上一项输入尚未确认"));
            }
            if attempt.rpc_in_flight {
                return Ok(Dispatch::Existing(CapturedTask {
                    task: self.clone(),
                    record: r.clone(),
                }));
            }
        }
        let spec = match &self.spec {
            TaskSpec::Exec(s) => s,
            TaskSpec::Download(_) => unreachable!(),
        };
        if spec.mode == ExecMode::Batch
            || matches!(operation, NormalizedInput::Eof) && spec.mode != ExecMode::Pipe
            || matches!(operation, NormalizedInput::Resize(..)) && spec.mode != ExecMode::Pty
        {
            return Err(crate::tasks::failure(
                "unsupported",
                "执行模式不支持输入动作",
            ));
        }
        let process = control
            .process_id
            .clone()
            .ok_or_else(|| crate::tasks::failure("inputNotReady", "原进程尚未接纳"))?;
        if control.attempt.is_none() {
            if r.native["launchAccepted"] != true {
                return Err(crate::tasks::failure("inputNotReady", "远端启动尚未确认"));
            }
            let next = r.native["input"]["nextInputSequence"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| crate::tasks::failure("inputNotReady", "尚未取得输入序号"))?;
            if sequence < next {
                return Err(crate::tasks::failure("inputExpired", "旧输入不会重新发送"));
            }
            if sequence > next {
                return Err(crate::tasks::failure("invalidSequence", "输入序号不连续"));
            }
            if r.native["input"]["last"]["state"] == "pending" {
                return Err(crate::tasks::failure(
                    "resourceBusy",
                    "上一项输入尚未发送确认",
                ));
            }
            if r.native["input"]["available"] != true {
                return Err(crate::tasks::failure("inputClosed", "远端不再接受输入"));
            }
        }
        let permit = pool
            .clone()
            .try_acquire_owned()
            .map_err(|_| crate::tasks::failure("resourceBusy", "短 IO 调用已满"))?;
        let control = r.control.as_mut().unwrap();
        control.attempt = Some(InputAttempt {
            sequence,
            operation,
            rpc_in_flight: true,
            admission_unknown: false,
            error: Value::Null,
        });
        r.revision += 1;
        self.changes.send_replace(r.revision);
        Ok(Dispatch::Send {
            process,
            guard: InputCallGuard {
                task: self.clone(),
                sequence,
                _permit: permit,
            },
        })
    }
    pub(crate) fn input_error(&self, sequence: u64, error: Value, unknown: bool) {
        self.update(|r| {
            if let Some(attempt) = r
                .control
                .as_mut()
                .and_then(|c| c.attempt.as_mut())
                .filter(|a| a.sequence == sequence)
            {
                attempt.error = error;
                attempt.admission_unknown = unknown;
                return true;
            }
            false
        });
    }
    pub(crate) fn input_finished(&self, sequence: u64) {
        self.update(|r| {
            if let Some(control) = r.control.as_mut() {
                if let Some(attempt) = control.attempt.as_mut().filter(|a| a.sequence == sequence) {
                    attempt.rpc_in_flight = false;
                    if !attempt.admission_unknown {
                        control.attempt = None;
                    }
                    return true;
                }
            }
            false
        });
    }
    pub(crate) fn seal_input(&self) {
        self.update(|r| {
            if let Some(control) = r.control.as_mut() {
                if !control.sealed {
                    control.sealed = true;
                    return true;
                }
            }
            false
        });
    }
    pub(crate) fn input_running(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .control
            .as_ref()
            .and_then(|c| c.attempt.as_ref())
            .is_some_and(|a| a.rpc_in_flight)
    }
    pub fn interaction(&self, r: &Record) -> Option<Value> {
        let TaskSpec::Exec(spec) = &self.spec else {
            return None;
        };
        if spec.mode == ExecMode::Batch {
            return None;
        }
        let control = r.control.as_ref()?;
        let input = if r.native["input"].is_object() {
            &r.native["input"]
        } else {
            &r.native["lastResult"]["input"]
        };
        let attempt=control.attempt.as_ref().map(|a|json!({"sequence":a.sequence.to_string(),"state":if a.rpc_in_flight{"sending"}else{"unknown"},"error":a.error}));
        Some(
            json!({"mode":spec.mode.name(),"state":input["state"],"available":input["available"]==true&&!control.sealed&&control.attempt.is_none()&&r.phase==Phase::Collecting,"nextInputSequence":input["nextInputSequence"],"last":input["last"],"attempt":attempt}),
        )
    }
    pub fn set_target(&self, value: Descriptor) {
        self.update(|r| {
            let changed = r.target.as_ref() != Some(&value);
            r.target = Some(value);
            changed
        });
    }
    pub fn request_cancel(&self) -> Record {
        let mut r = self.state.lock().unwrap();
        if r.phase == Phase::Collecting && !r.cancel_requested {
            r.cancel_requested = true;
            if let Some(control) = r.control.as_mut() {
                control.sealed = true;
            }
            self.cancel.cancel();
            r.revision += 1;
            self.changes.send_replace(r.revision);
        }
        r.clone()
    }
    pub fn capture_error(&self, error: Value) {
        self.update(|r| {
            let changed = r.error != error;
            r.error = error;
            r.saved = Saved::Incomplete;
            changed
        });
    }
    pub fn finalizing(&self, complete: bool, error: Value) {
        let path = self.directory.join(if self.kind() == Kind::Exec {
            "output.jsonl"
        } else {
            "download.bin"
        });
        let path = path.exists().then_some(path);
        self.update(|r| {
            r.phase = Phase::Finalizing;
            r.saved = Saved::Finalizing;
            r.capture_complete = complete;
            r.error = error;
            r.path = path;
            true
        });
    }
    pub fn cleanup(&self, released: bool, error: Value) {
        self.update(|r| {
            let changed = r.released != released || r.cleanup_error != error;
            r.released = released;
            r.cleanup_error = error;
            changed
        });
    }
    pub fn final_candidate(&self) -> Record {
        let mut r = self.state.lock().unwrap().clone();
        assert!(r.phase == Phase::Finalizing);
        r.phase = Phase::Done;
        r.finished = Some(now());
        r.revision += 1;
        r.saved = if r.capture_complete && r.error.is_null() {
            Saved::Complete
        } else if r.error.is_null() {
            Saved::Incomplete
        } else {
            Saved::Failed
        };
        r
    }
    pub fn publish(&self, mut candidate: Record, save_error: Option<Value>) {
        if let Some(mut error) = save_error {
            if !candidate.error.is_null() {
                error["details"] = json!({"captureError":candidate.error});
            }
            candidate.saved = Saved::Failed;
            candidate.error = error;
        }
        let mut r = self.state.lock().unwrap();
        assert!(r.phase == Phase::Finalizing);
        *r = candidate;
        self.changes.send_replace(r.revision);
    }
    pub fn snapshot(&self) -> Record {
        self.state.lock().unwrap().clone()
    }
}
