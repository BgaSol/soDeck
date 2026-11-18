use crate::tasks::{now, Target};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{watch, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Exec,
    Download,
}

#[cfg(test)]
pub fn fixture(root: PathBuf) -> Arc<Task> {
    Arc::new(Task::new(NewTask {
        run: "test-run".into(),
        order: 1,
        admission_sequence: 1,
        target: Arc::new(Target::new(
            "target".into(),
            "view".into(),
            json!({"serverId":"server"}),
        )),
        input: json!({"requestId":"request","command":"中文\ncommand"}),
        kind: Kind::Exec,
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
        assert_eq!(
            crate::response::metadata(&task, &done)["evidence"]["state"],
            "failed"
        );
        task.request_cancel();
        task.observe(json!({"state":"cancelled"}), Some(999));
        task.cleanup(true, Value::Null);
        assert_eq!(task.metadata(), crate::response::metadata(&task, &done));
        assert_eq!(task.snapshot().committed, 12);
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
    pub target: Value,
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
}
pub struct CapturedTask {
    pub task: Arc<Task>,
    pub record: Record,
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
    pub input: Value,
    pub kind: Kind,
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
    pub input: Value,
    pub kind: Kind,
    pub root: PathBuf,
}
pub fn preview(value: &str, bytes: usize) -> (String, bool) {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), end < value.len())
}
fn revision_facts(native: &Value) -> Value {
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
    ] {
        if let Some(field) = native.get(key) {
            value[key] = field.clone();
        }
    }
    value
}
impl Task {
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
            input: meta.input,
            kind: meta.kind,
            created: now(),
            cancel: CancellationToken::new(),
            waiters: Arc::new(Semaphore::new(2)),
            changes,
            state: Mutex::new(Record {
                native: json!({"admission":"pending"}),
                target: Value::Null,
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
            let changed = revision_facts(&r.native) != revision_facts(&native)
                || committed.is_some_and(|n| n != r.committed);
            r.native = native;
            if let Some(n) = committed {
                r.committed = n;
            }
            r.observed = now();
            changed
        });
    }
    pub fn set_target(&self, value: Value) {
        self.update(|r| {
            let changed = r.target != value;
            r.target = value;
            changed
        });
    }
    pub fn request_cancel(&self) -> Record {
        let mut r = self.state.lock().unwrap();
        if r.phase == Phase::Collecting && !r.cancel_requested {
            r.cancel_requested = true;
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
        let path = self.directory.join(if self.kind == Kind::Exec {
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
    pub fn metadata(&self) -> Value {
        self.metadata_for(&self.state.lock().unwrap())
    }
    pub fn metadata_for(&self, r: &Record) -> Value {
        crate::response::metadata(self, r)
    }
}
