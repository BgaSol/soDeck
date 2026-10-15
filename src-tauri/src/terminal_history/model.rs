use crate::connection::ConnectionContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub server_id: String,
    pub server_name: String,
    pub address: String,
    pub username: String,
    pub container_name: Option<String>,
    pub container_id: Option<String>,
    pub endpoint: Option<String>,
}
impl Source {
    pub fn new(
        context: &ConnectionContext,
        container: Option<&crate::docker::ContainerTarget>,
    ) -> Self {
        let identity =
            serde_json::to_vec(&(context.identity(), container.map(|c| (&c.endpoint, &c.id))))
                .unwrap();
        Self {
            id: format!("{:x}", Sha256::digest(identity)),
            server_id: context.target().id.clone(),
            server_name: context.target().name.clone(),
            address: format!("{}:{}", context.target().host, context.target().port),
            username: context.target().username.clone(),
            container_name: container.map(|c| c.name.clone()),
            container_id: container.map(|c| c.id.clone()),
            endpoint: container.map(|c| c.endpoint.clone()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventKind {
    Start { cols: u16, rows: u16 },
    Output { bytes: Vec<u8> },
    Resize { cols: u16, rows: u16 },
    Cwd { path: String },
    Clear,
    Checkpoint,
    Gap { reason: String },
    End { reason: String },
}
#[derive(Clone, Debug)]
pub struct Event {
    pub seq: i64,
    pub time: i64,
    pub elapsed: i64,
    pub kind: EventKind,
}
impl Event {
    pub fn size(&self) -> usize {
        match &self.kind {
            EventKind::Output { bytes } => bytes.len() + 128,
            EventKind::Cwd { path } => path.len() + 128,
            EventKind::Gap { reason } | EventKind::End { reason } => reason.len() + 128,
            _ => 128,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub source: Source,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub state: String,
    pub saved_seq: String,
    pub indexed_seq: String,
    pub last_record: String,
    pub bytes: u64,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub session_id: String,
    pub seq: String,
    pub offset: usize,
    pub time: i64,
    pub segment: u64,
    pub kind: String,
    pub text: String,
    pub ansi: String,
}
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub session_id: Option<String>,
    pub source_id: Option<String>,
    pub workspace_id: Option<String>,
    pub keyword: String,
    pub case_sensitive: bool,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub snapshot: Option<String>,
    pub limit: usize,
    pub request_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<Row>,
    pub has_more: bool,
    pub snapshot: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub session: Session,
    pub row: Row,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub cursor: Option<String>,
    pub snapshot: String,
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
