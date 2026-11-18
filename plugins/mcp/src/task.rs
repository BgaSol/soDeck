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
        assert_eq!(closing.value["cancelRequested"], false);
        assert!(!closing.done);
        task.publish(
            task.final_candidate(),
            Some(json!({"code":"disk","message":"disk full"})),
        );
        let done = task.snapshot();
        assert!(done.done);
        assert_eq!(done.value["evidence"]["state"], "failed");
        task.request_cancel();
        task.observe(json!({"state":"cancelled"}), Some(999));
        task.cleanup(true, Value::Null);
        assert_eq!(task.snapshot().value, done.value);
        assert_eq!(task.snapshot().committed, 12);
    }
    #[test]
    fn summary_is_a_bounded_preview_not_original_input() {
        let (text, cut) = preview("中中中", 8);
        assert_eq!(text, "中中");
        assert!(cut);
        let task = fixture(PathBuf::new());
        let summary = task.summary();
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
enum Phase {
    Collecting,
    Finalizing,
    Done,
}
#[derive(Clone, Copy)]
enum Saved {
    Collecting,
    Finalizing,
    Complete,
    Incomplete,
    Failed,
}
#[derive(Clone)]
pub struct Record {
    native: Value,
    target: Value,
    phase: Phase,
    saved: Saved,
    error: Value,
    path: Option<PathBuf>,
    capture_complete: bool,
    committed: u64,
    cancel_requested: bool,
    released: bool,
    cleanup_error: Value,
    observed: u64,
    finished: Option<u64>,
    revision: u64,
}
pub struct Snapshot {
    pub value: Value,
    pub committed: u64,
    pub revision: u64,
    pub done: bool,
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
fn brief_error(value: &Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    let mut result = json!({"code":value["code"],"message":value["message"]});
    if let Some(message) = value["message"].as_str() {
        let (text, cut) = preview(message, 512);
        result["message"] = json!(text);
        if cut {
            result["messageTruncated"] = json!(true);
        }
    }
    result
}
fn facts(native: &Value) -> Value {
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
            let changed =
                facts(&r.native) != facts(&native) || committed.is_some_and(|n| n != r.committed);
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
    pub fn request_cancel(&self) -> Snapshot {
        self.update(|r| {
            if r.phase != Phase::Collecting || r.cancel_requested {
                return false;
            }
            r.cancel_requested = true;
            self.cancel.cancel();
            true
        });
        self.snapshot()
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
    fn value(&self, r: &Record, summary: bool) -> Value {
        let state = match r.saved {
            Saved::Collecting => "collecting",
            Saved::Finalizing => "finalizing",
            Saved::Complete => "saved",
            Saved::Incomplete => "incomplete",
            Saved::Failed => "failed",
        };
        let mut native = if summary {
            facts(&r.native)
        } else {
            r.native.clone()
        };
        if summary && native.get("error").is_some() {
            native["error"] = brief_error(&native["error"]);
        }
        let mut evidence = json!({"state":state,"complete":matches!(r.saved,Saved::Complete),"error":if summary {brief_error(&r.error)} else {r.error.clone()}});
        if !summary {
            evidence["path"] = json!(r.path);
        }
        let mut value = json!({
            "taskId":self.id,"runId":self.run,"kind":self.kind.name(),"targetId":self.target.id,
            "requestId":self.input["requestId"],"target":r.target,"createdAt":self.created,"finishedAt":r.finished,
            "native":native,"observedAt":r.observed,"resourceReleased":r.released,
            "cleanupError":if summary {brief_error(&r.cleanup_error)} else {r.cleanup_error.clone()},
            "done":r.phase==Phase::Done,"cancelRequested":r.cancel_requested,
            "phase":match r.phase {Phase::Collecting=>"collecting",Phase::Finalizing=>"finalizing",Phase::Done=>"done"},
            "revision":r.revision.to_string(),"evidence":evidence,
        });
        if summary {
            let text = self.input[if self.kind == Kind::Exec {
                "command"
            } else {
                "remotePath"
            }]
            .as_str()
            .unwrap();
            let (text, cut) = preview(text, 256);
            value["context"] = self.target.context.clone();
            value["inputPreview"] = json!(text);
            value["inputTruncated"] = json!(cut);
        } else {
            value["input"] = self.input.clone();
            value["directory"] = json!(self.directory);
            value["outputInfo"] = json!({"savedBytes":if self.kind==Kind::Exec {json!(r.committed.to_string())} else {Value::Null}});
        }
        value
    }
    pub fn snapshot(&self) -> Snapshot {
        let r = self.state.lock().unwrap();
        Snapshot {
            value: self.value(&r, false),
            committed: r.committed,
            revision: r.revision,
            done: r.phase == Phase::Done,
        }
    }
    pub fn summary(&self) -> Value {
        self.value(&self.state.lock().unwrap(), true)
    }
    pub fn metadata(&self) -> Value {
        self.metadata_for(&self.state.lock().unwrap())
    }
    pub fn metadata_for(&self, r: &Record) -> Value {
        let mut value = self.value(r, false);
        value["schemaVersion"] = json!(1);
        value["context"] = self.target.context.clone();
        value
    }
}
