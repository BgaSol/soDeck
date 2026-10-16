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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    #[default]
    Copy,
    Move,
}
pub fn operation_kind(
    source: &Source,
    target: &SharedContext,
    intent: Intent,
) -> AppResult<&'static str> {
    match source {
        Source::Local(_) if intent == Intent::Copy => Ok("upload"),
        Source::Remote { context, .. } if context.identity() == target.identity() => {
            Ok(if intent == Intent::Move {
                "serverMove"
            } else {
                "serverCopy"
            })
        }
        Source::Remote { context, .. } if context.target().id == target.target().id => Err(
            AppError::new("changed", "同一服务器的连接身份已变化，请重新连接"),
        ),
        Source::Remote { .. } if intent == Intent::Copy => Ok("direct"),
        _ => Err(AppError::new(
            "validation",
            "剪切和 Shift 拖拽仅支持同一服务器内移动；跨服务器请使用复制",
        )),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    Queued,
    Checking,
    Scanning,
    Running,
    Verifying,
    Committing,
    Cancelling,
    Completed,
    Failed,
    PartialFailed,
    SourceRetained,
    Cancelled,
    Unknown,
    Interrupted,
    NeedsAttention,
}
impl TransferStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Checking => "checking",
            Self::Scanning => "scanning",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Committing => "committing",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::PartialFailed => "partialFailed",
            Self::SourceRetained => "sourceRetained",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
            Self::Interrupted => "interrupted",
            Self::NeedsAttention => "needsAttention",
        }
    }
}
impl From<&str> for TransferStatus {
    fn from(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "checking" => Self::Checking,
            "scanning" => Self::Scanning,
            "running" => Self::Running,
            "verifying" => Self::Verifying,
            "committing" => Self::Committing,
            "cancelling" => Self::Cancelling,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "partialFailed" => Self::PartialFailed,
            "sourceRetained" => Self::SourceRetained,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            "needsAttention" => Self::NeedsAttention,
            _ => Self::Unknown,
        }
    }
}
impl PartialEq<&str> for TransferStatus {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub run_id: String,
    #[serde(flatten)]
    pub presentation: TaskPresentation,
    pub kind: String,
    pub source_name: String,
    pub source_path: String,
    pub target_name: String,
    pub target_path: String,
    pub conflict: ConflictPolicy,
    pub status: TransferStatus,
    pub bytes_done: u64,
    pub bytes_total: u64,
    #[serde(default)]
    pub current_file: Option<String>,
    #[serde(default)]
    pub current_bytes: u64,
    #[serde(default)]
    pub committed_bytes: u64,
    #[serde(default)]
    pub waiting_reason: Option<String>,
    pub files_done: u64,
    pub files_total: u64,
    pub skipped: u64,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub cleanup_pending: bool,
    #[serde(default)]
    pub items: Vec<ItemView>,
    #[serde(default)]
    pub direct_address: Option<String>,
    #[serde(default)]
    pub temporary_paths: Vec<String>,
    #[serde(default)]
    pub can_resume: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskPresentation {
    pub created_at: Option<u64>,
    pub last_started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub sort_order: u64,
    pub source_paths: Vec<String>,
    pub last_attempt: Option<AttemptSummary>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptSummary {
    pub run_id: String,
    pub started_at: Option<u64>,
    pub status: TransferStatus,
    pub error: Option<String>,
}

impl TaskPresentation {
    pub fn new(sort_order: u64, source_paths: Vec<String>) -> Self {
        let now = Self::now();
        Self {
            created_at: now,
            last_started_at: now,
            finished_at: None,
            sort_order,
            source_paths,
            last_attempt: None,
        }
    }

    pub fn restart(
        &mut self,
        sort_order: u64,
        run_id: String,
        status: TransferStatus,
        error: Option<String>,
    ) {
        self.last_attempt = Some(AttemptSummary {
            run_id,
            started_at: self.last_started_at,
            status,
            error,
        });
        self.sort_order = sort_order;
        self.last_started_at = Self::now();
        self.finished_at = None;
    }

    fn now() -> Option<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis() as u64)
    }
}

pub(super) fn sort_tasks(tasks: &mut [TaskView]) {
    tasks.sort_by(|a, b| {
        b.presentation
            .sort_order
            .cmp(&a.presentation.sort_order)
            .then_with(|| a.id.cmp(&b.id))
    });
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub source: String,
    pub target: Option<String>,
    pub status: String,
    pub error: Option<String>,
}
impl TaskView {
    pub fn active(&self) -> bool {
        active(self.status.as_str())
    }
}
pub fn active(status: &str) -> bool {
    matches!(
        status,
        "queued" | "checking" | "scanning" | "running" | "verifying" | "committing" | "cancelling"
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
#[derive(Clone, Serialize, Deserialize)]
pub struct DirectEndpoint {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub public_key: String,
}

// Secrets never implement Serialize and are never included in a task receipt.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    pub mode: String,
    pub password: Option<String>,
    pub key_path: Option<String>,
}
#[derive(Clone)]
pub enum DirectAuth {
    Password(String),
    Key(String),
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: String,
    pub target_root: String,
    pub source_root: Option<String>,
    pub dispatched: bool,
    pub source_finished: bool,
    pub receiver_finished: bool,
    pub successful: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    #[serde(default)]
    pub intent: Intent,
    #[serde(default)]
    pub clipboard_id: Option<String>,
    #[serde(default)]
    pub skipped_paths: Vec<String>,
    pub format_version: u32,
    pub source_server_id: String,
    pub target_server_id: String,
    pub source_identity: String,
    pub target_identity: String,
    #[serde(default)]
    pub target_write_key: String,
    #[serde(default)]
    pub source_write_key: String,
    pub attempts: Vec<Attempt>,
    pub source_paths: Vec<String>,
    pub endpoint: Option<DirectEndpoint>,
    pub auth_mode: String,
    pub key_path: Option<String>,
    pub verify_sha256: bool,
    #[serde(skip)]
    pub plan: Plan,
    pub view: Option<TaskView>,
    pub pending_commit: Option<PendingCommit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCommit {
    #[serde(default)]
    pub rename_directory: bool,
    pub destination: String,
    pub stage_path: Option<String>,
    pub expected_size: Option<u64>,
    pub run_id: String,
    pub entry: usize,
    pub digest: Option<String>,
    pub directory: bool,
}

impl Receipt {
    pub fn update_resources(&self, view: &mut TaskView) {
        view.cleanup_pending = self.has_remote_resources();
        view.temporary_paths = self
            .attempts
            .iter()
            .flat_map(|a| {
                let mut paths = vec![];
                if !a.target_root.is_empty() {
                    paths.push(format!("B · {}", a.target_root));
                }
                if let Some(p) = &a.source_root {
                    paths.push(format!("A · {p}"));
                }
                paths
            })
            .collect();
        view.can_resume = self.pending_commit.is_none()
            && self
                .attempts
                .iter()
                .any(|a| a.source_finished && a.receiver_finished && !a.target_root.is_empty());
    }
    pub fn has_remote_resources(&self) -> bool {
        self.attempts
            .iter()
            .any(|a| !a.target_root.is_empty() || a.source_root.is_some())
    }
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
    pub auth: Arc<Mutex<Option<DirectAuth>>>,
}
impl WriteHandle {
    pub fn same_server(&self) -> bool {
        matches!(&self.source, Source::Remote { context, .. } if context.identity() == self.target.identity())
    }
    pub fn moving(&self) -> bool {
        self.receipt.lock().unwrap().intent == Intent::Move
    }
    pub fn write_key(&self) -> String {
        // Deliberately conservative for aliases/cloned keys: serializing two
        // unrelated hosts is preferable to simultaneous writes to one host.
        self.target.target_public_key().to_owned()
    }
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
        // Keep the direct exec channel alive long enough to request termination.
        // Upload sockets can be interrupted immediately.
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
