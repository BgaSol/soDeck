use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "LegacyWindow", into = "LegacyWindow")]
pub struct WindowRecord {
    pub window_id: String,
    pub role: String,
    pub workspace_ids: Vec<String>,
    pub selection: Selection,
    pub home_open: bool,
    pub home_generation: String,
    pub last_workspace_id: Option<String>,
    pub navigation_revision: u64,
    pub focused_view_id: Option<String>,
    pub bounds: Option<WindowBounds>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection { ConnectionHome, Workspace(String), Empty }
// Historical snapshot input/output is a projection, never a second writable selection.
#[derive(Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
struct LegacyWindow {
    window_id:String, role:String,
    #[serde(default)] workspace_ids:Vec<String>,
    #[serde(default)] active_workspace_id:Option<String>,
    #[serde(default)] focused_view_id:Option<String>,
    #[serde(default)] bounds:Option<WindowBounds>,
}
impl From<LegacyWindow> for WindowRecord {
    fn from(v:LegacyWindow)->Self {let mut w=Self::new(&v.window_id);w.role=v.role;w.workspace_ids=v.workspace_ids;w.select_workspace(v.active_workspace_id);w.focused_view_id=v.focused_view_id;w.bounds=v.bounds;w}
}
impl From<WindowRecord> for LegacyWindow {
    fn from(w:WindowRecord)->Self {Self{active_workspace_id:w.selected_workspace().cloned(),window_id:w.window_id,role:w.role,workspace_ids:w.workspace_ids,focused_view_id:w.focused_view_id,bounds:w.bounds}}
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
            selection: if id=="main" {Selection::ConnectionHome}else{Selection::Empty},
            home_open: id=="main",
            home_generation: uuid::Uuid::new_v4().to_string(),
            last_workspace_id: None,
            navigation_revision: 1,
            focused_view_id: None,
            bounds: None,
        }
    }
    pub fn selected_workspace(&self)->Option<&String>{match &self.selection{Selection::Workspace(id)=>Some(id),_=>None}}
    pub fn select_workspace(&mut self,id:Option<String>){
        if let Some(id)=id{self.last_workspace_id=Some(id.clone());self.selection=Selection::Workspace(id);}else{self.selection=if self.window_id=="main"||self.home_open{self.home_open=true;Selection::ConnectionHome}else{Selection::Empty};}
        self.navigation_revision+=1;
    }
    pub fn select_home(&mut self){
        if !self.home_open{self.home_generation=uuid::Uuid::new_v4().to_string();}
        self.home_open=true;self.selection=Selection::ConnectionHome;self.navigation_revision+=1;
    }
    pub fn close_home(&mut self){
        self.home_open=false;
        if self.selection==Selection::ConnectionHome{
            let next=self.last_workspace_id.clone().filter(|id|self.workspace_ids.contains(id)).or_else(||self.workspace_ids.first().cloned());
            self.selection=next.map(Selection::Workspace).unwrap_or(Selection::Empty);
        }
        self.navigation_revision+=1;
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.workspace_ids.iter().position(|v| v == id) {
            self.workspace_ids.remove(index);
            if self.selected_workspace().map(String::as_str) == Some(id) {
                let next = self
                    .workspace_ids
                    .get(index)
                    .or_else(|| self.workspace_ids.last())
                    .cloned();
                self.select_workspace(next);
            }
        }
    }
}
