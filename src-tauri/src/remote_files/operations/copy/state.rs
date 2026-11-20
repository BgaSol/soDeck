use super::*;
#[derive(Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum CommitState {
    Pending,
    Committed,
    NotCommitted,
    Unknown,
}

pub(super) struct CopyProgress {
    operation: Arc<Operation>,
    commit: CommitState,
}
impl CopyProgress {
    pub fn new(operation: Arc<Operation>) -> Self {
        operation.state.lock().unwrap()["transfer"] =
            json!({"commitState":"pending","commitAttempted":false});
        Self {
            operation,
            commit: CommitState::Pending,
        }
    }
    pub fn source(&self, attrs: &Value, reader: &Reader) {
        let mut s = self.operation.state.lock().unwrap();
        s["total"] = attrs["size"].clone();
        s["transfer"]["sourceVersion"] = match reader {
            Reader::Local(v) => json!(v.version()),
            Reader::Remote(_) => attrs["stamp"].clone(),
        };
    }
    pub fn bytes(&self, offset: u64, total: Value) {
        let mut s = self.operation.state.lock().unwrap();
        s["received"] = json!(offset.to_string());
        s["total"] = total;
    }
    pub fn committing(&self) {
        let mut s = self.operation.state.lock().unwrap();
        s["state"] = json!("committing");
        s["transfer"]["commitAttempted"] = json!(true);
    }
    pub fn committed(&mut self) {
        self.set(CommitState::Committed);
    }
    pub fn rejected(&mut self, e: &OperationError) {
        self.set(if e.code == "unknown" {
            CommitState::Unknown
        } else {
            CommitState::NotCommitted
        });
    }
    fn set(&mut self, v: CommitState) {
        self.commit = v;
        self.operation.state.lock().unwrap()["transfer"]["commitState"] = json!(v);
    }
    pub fn is_unknown(&self) -> bool {
        self.commit == CommitState::Unknown
    }
    pub fn temporary(&self, path: String) {
        self.operation.state.lock().unwrap()["transfer"]["temporaryPath"] = json!(path);
    }
}
impl Drop for CopyProgress {
    fn drop(&mut self) {
        if self.commit == CommitState::Pending {
            self.set(CommitState::NotCommitted);
        }
    }
}
