use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verifying,
    Ready,
    Preparing,
    Installing,
    Failed,
}
impl Phase {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Checking => "checking",
            Self::UpToDate => "upToDate",
            Self::Available => "available",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Ready => "ready",
            Self::Preparing => "preparing",
            Self::Installing => "installing",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Saved {
    pub schema_version: u32,
    pub preferences_revision: u64,
    pub auto_check: bool,
    pub auto_download: bool,
    pub skipped_version: Option<String>,
    pub last_successful_check_at: Option<u64>,
    pub next_check_at: u64,
    pub failures: u32,
    pub suppressed_version: Option<String>,
    pub suppression: Option<String>,
    pub pending_install: Option<PendingInstall>,
    pub unread_result: Option<InstallResult>,
    pub last_presented_version: Option<String>,
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            schema_version: 1,
            preferences_revision: 0,
            auto_check: true,
            auto_download: true,
            skipped_version: None,
            last_successful_check_at: None,
            next_check_at: 0,
            failures: 0,
            suppressed_version: None,
            suppression: None,
            pending_install: None,
            unread_result: None,
            last_presented_version: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PendingInstall {
    pub id: String,
    pub source_version: String,
    pub target_version: String,
    pub handed_off: bool,
    pub started_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallResult {
    pub id: String,
    pub target_version: String,
    pub outcome: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Release {
    pub id: String,
    pub version: String,
    pub published_at: Option<String>,
    pub notes: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Capability {
    pub mode: String,
    pub reason: Option<String>,
    pub target: Option<String>,
    pub manual_format: Option<String>,
}
#[derive(Clone, Serialize)]
pub(super) struct Operation {
    pub id: String,
    pub source: String,
}
#[derive(Clone, Serialize)]
pub(super) struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Preferences {
    pub revision: u64,
    pub auto_check: bool,
    pub auto_download: bool,
}
#[derive(Clone, Serialize)]
pub(super) struct UpdateError {
    pub stage: String,
    pub code: String,
    pub retry: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub(super) revision: u64,
    pub(super) phase: Phase,
    pub(super) current_version: String,
    pub(super) release: Option<Release>,
    pub(super) install: Capability,
    pub(super) operation: Option<Operation>,
    pub(super) progress: Option<Progress>,
    pub(super) preferences: Preferences,
    pub(super) last_successful_check_at: Option<u64>,
    pub(super) suppression: Option<String>,
    pub(super) error: Option<UpdateError>,
    pub(super) result: Option<InstallResult>,
    pub(super) message: Option<String>,
    pub(super) skipped_version: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferencePatch {
    pub(super) auto_check: Option<bool>,
    pub(super) auto_download: Option<bool>,
}
#[derive(Clone, Serialize)]
pub(super) struct Blocker {
    pub kind: String,
    pub count: usize,
    pub action: Option<String>,
}
#[derive(Clone, Serialize)]
pub(super) struct Interruption {
    pub kind: String,
    pub count: usize,
    pub connections: Option<usize>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReview {
    pub(super) release_id: String,
    pub(super) review_token: Option<String>,
    pub(super) expires_at: u64,
    pub(super) blockers: Vec<Blocker>,
    pub(super) interruptions: Vec<Interruption>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum InstallSubmission {
    Accepted,
    ReviewChanged { review: InstallReview },
}
#[derive(Clone)]
pub(super) struct Candidate {
    pub release: Release,
    pub update: Option<Update>,
    pub fingerprint: String,
    pub manual_url: Option<String>,
    pub capability: Capability,
}
pub(super) struct ReviewSecret {
    pub token: String,
    pub release_id: String,
    pub fingerprint: String,
    pub main_generation: u64,
    pub impact: String,
    pub expires: Instant,
}
