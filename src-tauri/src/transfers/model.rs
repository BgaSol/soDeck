use super::{download::ConflictPolicy, planner::Plan};
use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    ssh_transport::Cancellation,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub source_name: String,
    pub source_path: String,
    pub target_name: String,
    pub target_path: String,
    pub conflict: ConflictPolicy,
    pub status: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub skipped: u64,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cleanup_pending: bool,
}
impl TaskView {
    pub fn active(&self) -> bool {
        active(&self.status)
    }
}
pub fn active(status: &str) -> bool {
    matches!(
        status,
        "queued" | "checking" | "scanning" | "running" | "committing" | "cancelling"
    )
}
#[derive(Clone)]
pub enum Source {
    Local(Vec<PathBuf>),
    Remote {
        context: SharedContext,
        paths: Vec<String>,
    },
}
#[derive(Clone)]
pub struct DirectEndpoint {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub public_key: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub source_server_id: String,
    pub target_server_id: String,
    pub source_identity: String,
    pub target_identity: String,
    pub remote_root: String,
    pub stage_dirs: Vec<String>,
    pub view: Option<TaskView>,
    pub pending_commit: Option<String>,
    #[serde(default)]
    pub remote_stopped: bool,
}
#[derive(Clone)]
pub struct WriteHandle {
    pub lifecycle: Arc<Mutex<()>>,
    pub source: Source,
    pub target: SharedContext,
    pub endpoint: Option<DirectEndpoint>,
    pub task: Arc<Mutex<TaskView>>,
    pub cancel: Cancellation,
    pub stop: Arc<AtomicBool>,
    pub plan: Arc<Mutex<Option<Plan>>>,
    pub receipt: Arc<Mutex<Receipt>>,
}
impl WriteHandle {
    pub fn check(&self) -> AppResult<()> {
        if self.stop.load(Ordering::SeqCst) {
            Err(AppError::new("cancelled", "传输已取消"))
        } else {
            self.cancel.check()
        }
    }
    pub fn uses_server(&self, id: &str) -> bool {
        self.target.uses_server(id)
            || matches!(&self.source, Source::Remote { context, .. } if context.uses_server(id))
    }
    pub fn request_cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
        // Direct execution needs the controller alive until the remote process
        // group acknowledges stopping. Upload sockets can be interrupted now.
        if matches!(self.source, Source::Local(_)) {
            self.cancel.cancel();
        }
    }
    pub fn warn(&self, message: String) {
        let mut task = self.task.lock().unwrap();
        if task.warnings.len() < 100 {
            task.warnings.push(message);
        }
    }
}
