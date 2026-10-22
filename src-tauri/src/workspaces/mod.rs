//! Runtime server workspaces. Lock order: configuration -> registry -> moves ->
//! terminals -> streams (sorted by connection id). Never call a manager, emit an
//! event, or operate a native window while holding the registry lock.
mod commands;
mod contexts;
mod lifecycle;
mod model;
pub use commands::*;
pub use contexts::*;
pub use lifecycle::*;
pub use model::*;

use crate::{
    connection::{SharedContext, WindowOwner},
    model::{AppError, AppResult, ServerView},
};
use std::{collections::HashMap, sync::Mutex};
use tauri::Emitter;

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(crate) fn stale() -> AppError {
    AppError::new("workspaceChanged", "工作区状态已变化，请刷新后重试")
}

pub struct Workspace {
    pub view: WorkspaceView,
    pub context: Option<SharedContext>,
    pub requests: HashMap<String, String>,
    pub verification: Option<Verification>,
    pub operations: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub dependencies: Vec<String>,
    pub mounted: bool,
}
#[derive(Default)]
pub struct Registry {
    pub revision: u64,
    pub entries: HashMap<String, Workspace>,
    pub by_server: HashMap<String, String>,
}
impl Registry {
    pub fn terminal_action(&self, connection_id: &str) -> AppResult<()> {
        if self.entries.values().any(|w| {
            w.view.lifecycle != "active"
                && w.view
                    .terminals
                    .iter()
                    .any(|t| t.connection_id.as_deref() == Some(connection_id))
        }) {
            return Err(AppError::new(
                "busy",
                "服务器工作区正在移交，暂不能操作终端",
            ));
        }
        Ok(())
    }
    pub fn owned_mut(
        &mut self,
        id: &str,
        owner: &WindowOwner,
        owner_epoch: u64,
    ) -> AppResult<&mut Workspace> {
        let w = self.entries.get_mut(id).ok_or_else(stale)?;
        if w.view.owner.label != owner.label
            || w.view.owner.generation != owner.generation
            || w.view.owner.owner_epoch != owner_epoch
        {
            return Err(AppError::new("permission", "工作区已不属于此窗口页面"));
        }
        if w.view.lifecycle != "active" {
            return Err(AppError::new("busy", "工作区正在移动或关闭"));
        }
        Ok(w)
    }
    pub fn changed(&mut self, id: &str) {
        self.revision += 1;
        if let Some(w) = self.entries.get_mut(id) {
            w.view.revision = self.revision;
        }
    }
    pub fn open(
        &mut self,
        server: ServerView,
        owner: WindowOwner,
        create: bool,
        request: &str,
    ) -> AppResult<WorkspaceView> {
        if request.is_empty() || request.len() > 128 {
            return Err(AppError::new("validation", "无效的请求标识"));
        }
        let id = self
            .by_server
            .get(&server.id)
            .cloned()
            .unwrap_or_else(new_id);
        if !self.entries.contains_key(&id) {
            self.by_server.insert(server.id.clone(), id.clone());
            self.entries.insert(
                id.clone(),
                Workspace {
                    view: WorkspaceView {
                        id: id.clone(),
                        server,
                        owner: Owner {
                            label: owner.label,
                            generation: owner.generation,
                            owner_epoch: 0,
                        },
                        revision: 0,
                        lifecycle: "active".into(),
                        terminals: vec![],
                        active_terminal_id: None,
                        next_terminal_number: 1,
                        tools: ToolView::default(),
                    },
                    context: None,
                    requests: HashMap::new(),
                    verification: None,
                    operations: Default::default(),
                    dependencies: vec![],
                    mounted: false,
                },
            );
            self.entries
                .get_mut(&id)
                .unwrap()
                .create_terminal(request)?;
        } else if create {
            let w = self.entries.get_mut(&id).unwrap();
            if w.view.lifecycle != "active" {
                return Err(AppError::new("busy", "服务器工作区正在移动，请稍后重试"));
            }
            w.create_terminal(request)?;
        }
        self.changed(&id);
        Ok(self.entries[&id].view.clone())
    }
    pub fn remove(&mut self, id: &str) -> Option<Workspace> {
        let w = self.entries.remove(id)?;
        self.by_server.remove(&w.view.server.id);
        self.revision += 1;
        Some(w)
    }
}
impl Workspace {
    pub fn create_terminal(&mut self, request: &str) -> AppResult<String> {
        if request.is_empty() || request.len() > 128 {
            return Err(AppError::new("validation", "无效的请求标识"));
        }
        if let Some(id) = self.requests.get(request) {
            return Ok(id.clone());
        }
        if self.requests.len() >= 10000 {
            return Err(AppError::new(
                "limit",
                "本工作区的操作次数过多，请重新打开工作区",
            ));
        }
        let id = new_id();
        let number = self.view.next_terminal_number;
        self.view.next_terminal_number += 1;
        self.view.terminals.push(TerminalView {
            id: id.clone(),
            number,
            title: None,
            attempt: 0,
            connection_id: None,
            state: "queued".into(),
            request_id: None,
        });
        self.view.active_terminal_id = Some(id.clone());
        self.requests.insert(request.into(), id.clone());
        Ok(id)
    }
}
#[derive(Default)]
pub struct WorkspaceState {
    pub registry: Mutex<Registry>,
    app: Mutex<Option<tauri::AppHandle>>,
    pub verifications: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl WorkspaceState {
    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }
    pub fn snapshot(&self) -> WorkspaceList {
        let registry = self.registry.lock().unwrap();
        WorkspaceList {
            revision: registry.revision,
            workspaces: registry.entries.values().map(|w| w.view.clone()).collect(),
        }
    }
    pub fn publish(&self) {
        let snapshot = self.snapshot();
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit("server-workspaces-changed", snapshot);
        }
    }
    pub fn has_server(&self, id: &str) -> bool {
        self.registry.lock().unwrap().entries.values().any(|w| {
            w.view.server.id == id
                || w.dependencies.iter().any(|s| s == id)
                || w.context.as_ref().is_some_and(|c| c.uses_server(id))
                || w.verification
                    .as_ref()
                    .is_some_and(|v| v.plan.uses_server(id))
        })
    }
    pub fn is_empty(&self) -> bool {
        self.registry.lock().unwrap().entries.is_empty()
    }
    pub fn impact_ids(&self) -> Vec<String> {
        self.registry
            .lock()
            .unwrap()
            .entries
            .values()
            .map(|w| format!("workspace:{}:{}", w.view.id, w.view.revision))
            .collect()
    }
}

#[cfg(test)]
mod tests;
