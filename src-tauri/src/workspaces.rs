//! Ephemeral workspace ownership. No transport, credentials or DOM lives here.
use crate::AppCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub workspace_id: String,
    pub server_id: String,
    pub connection_id: String,
    pub generation: u64,
}

/// Constructed by the future connection service, never by a frontend command.
pub struct ConnectedSession {
    pub server_id: String,
    pub connection_id: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub left_open: bool,
    pub right_open: bool,
    pub left_width: u16,
    pub right_width: u16,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            left_open: true,
            right_open: false,
            left_width: 220,
            right_width: 220,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub context: Context,
    pub name: String,
    pub endpoint: String,
    pub connected: bool,
    pub window_id: String,
    pub layout: Layout,
    // These are reserved component slots, not running plugins.
    pub view_ids: Vec<String>,
    pub active_view_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWindow {
    pub id: String,
    pub role: String,
    pub workspace_ids: Vec<String>,
    /// None selects the fixed catalog in main; secondary windows have no catalog.
    pub active_workspace: Option<String>,
    pub ready: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub revision: u64,
    pub window: WorkspaceWindow,
    pub workspaces: Vec<Workspace>,
    pub incoming: Option<Workspace>,
}

#[derive(Clone)]
pub struct Move {
    pub token: String,
    pub workspace_id: String,
    pub source: String,
    pub target: String,
    pub index: usize,
}

pub struct Runtime {
    revision: u64,
    pub(crate) windows: HashMap<String, WorkspaceWindow>,
    workspaces: HashMap<String, Workspace>,
    pending: HashMap<String, Move>,
}
impl Default for Runtime {
    fn default() -> Self {
        let main = WorkspaceWindow {
            id: "main".into(),
            role: "main".into(),
            workspace_ids: vec![],
            active_workspace: None,
            ready: false,
        };
        Self {
            revision: 0,
            windows: HashMap::from([("main".into(), main)]),
            workspaces: HashMap::new(),
            pending: HashMap::new(),
        }
    }
}

impl Runtime {
    pub fn projection(&self, window: &str) -> Result<Projection, String> {
        let win = self.windows.get(window).ok_or("窗口已关闭")?.clone();
        Ok(Projection {
            revision: self.revision,
            workspaces: win
                .workspace_ids
                .iter()
                .filter_map(|id| self.workspaces.get(id).cloned())
                .collect(),
            incoming: self
                .pending
                .values()
                .find(|m| m.target == window)
                .and_then(|m| self.workspaces.get(&m.workspace_id))
                .cloned(),
            window: win,
        })
    }
    pub fn register(&mut self, id: &str) -> Result<(), String> {
        if self.windows.contains_key(id) {
            return Err("窗口已存在".into());
        }
        self.windows.insert(
            id.into(),
            WorkspaceWindow {
                id: id.into(),
                role: "workspace".into(),
                workspace_ids: vec![],
                active_workspace: None,
                ready: false,
            },
        );
        self.revision += 1;
        Ok(())
    }
    pub fn ready(&mut self, window: &str) -> Result<(), String> {
        self.windows.get_mut(window).ok_or("窗口已关闭")?.ready = true;
        Ok(())
    }
    pub fn is_ready(&self, window: &str) -> bool {
        self.windows.get(window).is_some_and(|w| w.ready)
    }
    fn owned(&self, window: &str, id: &str) -> Result<&Workspace, String> {
        self.workspaces
            .get(id)
            .filter(|w| w.window_id == window)
            .ok_or_else(|| "工作区已移动或关闭，请重新读取".into())
    }
    fn movable(&self, window: &str, id: &str) -> Result<(), String> {
        self.owned(window, id)?;
        if self.pending.contains_key(id) {
            return Err("工作区正在移动".into());
        }
        Ok(())
    }
    pub fn activate(&mut self, window: &str, id: Option<&str>) -> Result<(), String> {
        if let Some(id) = id {
            self.owned(window, id)?;
        } else if window != "main" {
            return Err("此窗口没有服务器管理页".into());
        }
        self.windows
            .get_mut(window)
            .ok_or("窗口已关闭")?
            .active_workspace = id.map(str::to_owned);
        self.revision += 1;
        Ok(())
    }
    pub fn layout(&mut self, window: &str, id: &str, layout: Layout) -> Result<(), String> {
        self.movable(window, id)?;
        if !(160..=480).contains(&layout.left_width) || !(160..=480).contains(&layout.right_width) {
            return Err("面板宽度超出范围".into());
        }
        self.workspaces.get_mut(id).unwrap().layout = layout;
        self.revision += 1;
        Ok(())
    }
    pub fn prepare(
        &mut self,
        source: &str,
        id: &str,
        target: &str,
        index: usize,
        token: String,
    ) -> Result<Move, String> {
        self.movable(source, id)?;
        if !self.windows.contains_key(target) {
            return Err("目标窗口已关闭".into());
        }
        if self.pending.values().any(|m| m.target == target) {
            return Err("目标窗口正在接收其他工作区".into());
        }
        let m = Move {
            token,
            workspace_id: id.into(),
            source: source.into(),
            target: target.into(),
            index,
        };
        self.pending.insert(id.into(), m.clone());
        self.revision += 1;
        Ok(m)
    }
    pub fn cancel(&mut self, m: &Move) {
        if self
            .pending
            .get(&m.workspace_id)
            .is_some_and(|p| p.token == m.token)
        {
            self.pending.remove(&m.workspace_id);
            self.revision += 1;
        }
    }
    pub fn pending_move(&self, id: &str, source: &str) -> Option<Move> {
        self.pending.get(id).filter(|m| m.source == source).cloned()
    }
    pub fn owner(&self, id: &str) -> Option<String> {
        self.workspaces.get(id).map(|w| w.window_id.clone())
    }
    pub fn commit(&mut self, m: &Move) -> Result<(), String> {
        if !self
            .pending
            .get(&m.workspace_id)
            .is_some_and(|p| p.token == m.token)
        {
            return Err("移动已取消".into());
        }
        self.owned(&m.source, &m.workspace_id)?;
        if !self.is_ready(&m.target) {
            return Err("目标窗口尚未就绪".into());
        }
        self.remove_from_window(&m.source, &m.workspace_id);
        let target = self.windows.get_mut(&m.target).unwrap();
        target.workspace_ids.insert(
            m.index.min(target.workspace_ids.len()),
            m.workspace_id.clone(),
        );
        target.active_workspace = Some(m.workspace_id.clone());
        self.workspaces.get_mut(&m.workspace_id).unwrap().window_id = m.target.clone();
        self.pending.remove(&m.workspace_id);
        self.revision += 1;
        Ok(())
    }
    fn remove_from_window(&mut self, window: &str, id: &str) {
        if let Some(w) = self.windows.get_mut(window) {
            let index = w.workspace_ids.iter().position(|s| s == id).unwrap_or(0);
            w.workspace_ids.retain(|s| s != id);
            if w.active_workspace.as_deref() == Some(id) {
                w.active_workspace = w
                    .workspace_ids
                    .get(index.min(w.workspace_ids.len().saturating_sub(1)))
                    .cloned();
            }
        }
    }
    pub fn close(&mut self, window: &str, id: &str) -> Result<(), String> {
        self.movable(window, id)?;
        self.remove_from_window(window, id);
        self.workspaces.remove(id);
        self.revision += 1;
        Ok(())
    }
    pub fn close_window(&mut self, window: &str) -> Result<(), String> {
        if window == "main" {
            return Err("主窗口应通过应用退出流程关闭".into());
        }
        // Reject an outgoing move; a destroyed target cancels incoming moves.
        if self.pending.values().any(|m| m.source == window) {
            return Err("窗口中有工作区正在移动".into());
        }
        self.pending.retain(|_, m| m.target != window);
        if let Some(w) = self.windows.remove(window) {
            for id in w.workspace_ids {
                self.workspaces.remove(&id);
            }
        }
        self.revision += 1;
        Ok(())
    }
    pub fn empty_secondary(&self, window: &str) -> bool {
        window != "main"
            && self
                .windows
                .get(window)
                .is_some_and(|w| w.workspace_ids.is_empty())
            && !self.pending.values().any(|m| m.target == window)
    }
    pub fn server_open(&self, server: &str) -> bool {
        self.workspaces
            .values()
            .any(|w| w.context.server_id == server)
    }
    pub fn rename(&mut self, names: &HashMap<String, String>) {
        for w in self.workspaces.values_mut() {
            if let Some(name) = names.get(&w.context.server_id) {
                w.name = name.clone();
            }
        }
        self.revision += 1;
    }
    pub fn validate_context(&self, context: &Context) -> Result<(), String> {
        let w = self
            .workspaces
            .get(&context.workspace_id)
            .ok_or("工作区已关闭")?;
        if &w.context != context || !w.connected {
            return Err("连接已失效，请使用当前会话".into());
        }
        Ok(())
    }
    pub fn disconnected(&mut self, context: &Context) -> Result<(), String> {
        self.validate_context(context)?;
        self.workspaces
            .get_mut(&context.workspace_id)
            .unwrap()
            .connected = false;
        self.revision += 1;
        Ok(())
    }
    pub fn reconnect(
        &mut self,
        context: &Context,
        connection_id: String,
        endpoint: String,
    ) -> Result<Context, String> {
        let w = self
            .workspaces
            .get_mut(&context.workspace_id)
            .ok_or("工作区已关闭")?;
        if &w.context != context || w.connected || connection_id.is_empty() {
            return Err("重连上下文已失效".into());
        }
        w.context.generation = w
            .context
            .generation
            .checked_add(1)
            .ok_or("连接代次已超出范围")?;
        w.context.connection_id = connection_id;
        w.endpoint = endpoint;
        w.connected = true;
        self.revision += 1;
        Ok(w.context.clone())
    }
    pub fn lost_window(&mut self, window: &str) {
        if window == "main" {
            return;
        }
        self.pending
            .retain(|_, m| m.source != window && m.target != window);
        let _ = self.close_window(window);
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

impl AppCore {
    pub fn workspace_projection(&self, window: &str) -> Result<Projection, String> {
        self.workspaces.lock().unwrap().projection(window)
    }
    pub fn workspace_changed(&self) {
        self.event("workspaces-changed", serde_json::Value::Null);
    }

    /// Only the core connection service calls this after successful authentication.
    pub fn accept_connected(&self, session: ConnectedSession) -> Result<Workspace, String> {
        if self.is_closing() || session.connection_id.is_empty() {
            return Err("连接不可用".into());
        }
        // Same lock order as catalog edits, including deletion checks.
        let servers = self.servers.lock().unwrap();
        let names = servers.workspace_names();
        let name = names
            .get(&session.server_id)
            .ok_or("服务器档案已不存在")?
            .clone();
        let mut state = self.workspaces.lock().unwrap();
        if let Some(w) = state
            .workspaces
            .values()
            .find(|w| w.context.server_id == session.server_id)
            .cloned()
        {
            state.activate(&w.window_id, Some(&w.context.workspace_id))?;
            drop(state);
            drop(servers);
            self.workspace_changed();
            return Ok(w);
        }
        let id = self.next_id();
        let workspace = Workspace {
            context: Context {
                workspace_id: id.clone(),
                server_id: session.server_id,
                connection_id: session.connection_id,
                generation: 1,
            },
            name,
            endpoint: session.endpoint,
            connected: true,
            window_id: "main".into(),
            layout: Layout::default(),
            view_ids: vec![],
            active_view_id: None,
        };
        state.workspaces.insert(id.clone(), workspace.clone());
        let main = state.windows.get_mut("main").unwrap();
        main.workspace_ids.push(id.clone());
        main.active_workspace = Some(id);
        state.revision += 1;
        drop(state);
        drop(servers);
        self.workspace_changed();
        Ok(workspace)
    }
}
