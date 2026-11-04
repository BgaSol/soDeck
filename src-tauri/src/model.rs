use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub target: String,
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecord {
    pub directory: PathBuf,
    pub manifest: Manifest,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginStatus {
    Unloaded,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub directory: PathBuf,
    pub manifest: Manifest,
    pub status: PluginStatus,
    pub instance_id: Option<String>,
    pub pid: Option<u32>,
    pub started_at: Option<u64>,
    pub error: Option<String>,
    pub cleanup_complete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub id: String,
    pub received_at: u64,
    pub source: String,
    pub plugin_id: Option<String>,
    pub instance_id: Option<String>,
    pub level: String,
    pub message: String,
    pub fields: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilter {
    pub plugin_id: String,
    pub level: String,
    pub query: String,
    pub since: Option<u64>,
    pub until: Option<u64>,
}

impl LogFilter {
    pub fn matches(&self, record: &LogRecord) -> bool {
        (self.plugin_id.is_empty()
            || (self.plugin_id == "core" && record.source == "core")
            || record.plugin_id.as_deref() == Some(self.plugin_id.as_str()))
            && (self.level.is_empty() || record.level == self.level)
            && self.since.is_none_or(|since| record.received_at >= since)
            && self.until.is_none_or(|until| record.received_at <= until)
            && (self.query.is_empty()
                || record
                    .message
                    .to_lowercase()
                    .contains(&self.query.to_lowercase()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub plugin_id: Option<String>,
    pub instance_id: Option<String>,
    pub source: String,
    pub received_at: u64,
    pub title: String,
    pub body: String,
    pub level: String,
    pub read: bool,
    pub count: u32,
    pub dedupe_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishMessage {
    pub title: String,
    pub body: String,
    pub level: String,
    #[serde(default)]
    pub notify: bool,
    pub dedupe_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Preferences {
    pub theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "system".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: String,
    pub target: String,
    pub os: String,
    pub arch: String,
    pub started_at: u64,
    pub uptime_ms: u64,
    pub data_directory: PathBuf,
    pub log_directory: PathBuf,
    pub preferences: Preferences,
    pub plugins: Vec<PluginView>,
    pub unread_count: usize,
    pub storage_errors: Vec<String>,
}
