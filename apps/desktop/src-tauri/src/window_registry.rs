use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRecord {
    pub window_id: String,
    pub role: String,
    #[serde(default)]
    pub workspace_ids: Vec<String>,
    #[serde(default)]
    pub active_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_view_id: Option<String>,
    #[serde(default)]
    pub bounds: Option<WindowBounds>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowRecord {
    pub fn new(id: &str) -> Self {
        Self {
            window_id: id.into(),
            role: if id == "main" { "main" } else { "workspace" }.into(),
            workspace_ids: Vec::new(),
            active_workspace_id: None,
            focused_view_id: None,
            bounds: None,
        }
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.workspace_ids.iter().position(|v| v == id) {
            self.workspace_ids.remove(index);
            if self.active_workspace_id.as_deref() == Some(id) {
                self.active_workspace_id = self
                    .workspace_ids
                    .get(index)
                    .or_else(|| self.workspace_ids.last())
                    .cloned();
            }
        }
    }
}
