use super::*;
use sodeck_plugin_protocol::replies::{CommitState, CopyPhase, CopyTransfer};

pub(super) struct CopyProgress {
    operation: Arc<Operation>,
    facts: CopyTransfer,
}
impl CopyProgress {
    pub fn new(operation: Arc<Operation>) -> Self {
        let value = Self {
            operation,
            facts: CopyTransfer::default(),
        };
        value.publish();
        value
    }
    fn publish(&self) {
        self.operation.state.lock().unwrap()["transfer"] = json!(self.facts);
    }
    pub fn phase(&mut self, phase: CopyPhase, stage: &str) {
        self.facts.phase = phase;
        self.facts.stage = stage.into();
        self.publish();
    }
    pub fn stage(&mut self, stage: &str) {
        self.phase(self.facts.phase, stage);
    }
    pub fn source(&mut self, attrs: &Value, reader: &Reader) {
        self.facts.source_version = Some(match reader {
            Reader::Local(v) => v.version().into(),
            Reader::Remote(_) => attrs["stamp"].as_str().unwrap().into(),
        });
        self.publish();
        self.operation.state.lock().unwrap()["total"] = attrs["size"].clone();
    }
    pub fn bytes(&self, offset: u64, total: Value) {
        let mut s = self.operation.state.lock().unwrap();
        s["received"] = json!(offset.to_string());
        s["total"] = total;
    }
    pub fn committing(&mut self) {
        self.facts.phase = CopyPhase::Committing;
        self.facts.stage = "commit".into();
        self.facts.commit_attempted = true;
        self.publish();
        self.operation.state.lock().unwrap()["state"] = json!("committing");
    }
    pub fn committed(&mut self) {
        self.facts.commit_state = CommitState::Committed;
        self.publish();
    }
    pub fn rejected(&mut self, e: &OperationError) {
        self.facts.commit_state = if e.code == "unknown" {
            CommitState::Unknown
        } else {
            CommitState::NotCommitted
        };
        self.publish();
    }
    pub fn is_unknown(&self) -> bool {
        self.facts.commit_state == CommitState::Unknown
    }
    pub fn temporary(&mut self, path: String) {
        self.facts.temporary_path = Some(path);
        self.publish();
    }
    pub fn error(&self, mut error: OperationError) -> OperationError {
        if error.stage.is_none() {
            error.stage = Some(self.facts.stage.clone());
        }
        error
    }
}
impl Drop for CopyProgress {
    fn drop(&mut self) {
        if self.facts.commit_state == CommitState::Pending {
            self.facts.commit_state = CommitState::NotCommitted;
            self.publish();
        }
    }
}
