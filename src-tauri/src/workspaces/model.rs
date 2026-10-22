use crate::{connection::ConnectionView, model::ServerView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolContextRef {
    pub id: String,
    pub epoch: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub label: String,
    pub generation: u64,
    pub owner_epoch: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalView {
    pub id: String,
    pub number: u64,
    pub title: Option<String>,
    pub attempt: u64,
    pub connection_id: Option<String>,
    pub state: String,
    #[serde(skip)]
    pub request_id: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub context: Option<ToolContextRef>,
    pub state: String,
    pub connection: Option<ConnectionView>,
    pub configuration_changed: bool,
}
impl Default for ToolView {
    fn default() -> Self {
        Self {
            context: None,
            state: "unavailable".into(),
            connection: None,
            configuration_changed: false,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub id: String,
    pub server: ServerView,
    pub owner: Owner,
    pub revision: u64,
    pub lifecycle: String,
    pub terminals: Vec<TerminalView>,
    pub active_terminal_id: Option<String>,
    pub next_terminal_number: u64,
    pub tools: ToolView,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceList {
    pub revision: u64,
    pub workspaces: Vec<WorkspaceView>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalBinding {
    pub workspace_id: String,
    pub terminal_id: String,
    pub owner_epoch: u64,
    pub attempt: u64,
    pub request_id: String,
}
