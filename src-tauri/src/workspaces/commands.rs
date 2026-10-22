use super::*;
use crate::{connection::WindowOwner, model::AppResult, State};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};

#[tauri::command]
pub fn list_server_workspaces(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<WorkspaceList> {
    crate::moves::workspace_only(&window)?;
    Ok(state.workspaces.snapshot())
}
#[tauri::command]
pub fn open_server_workspace(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    server_id: String,
    new_terminal: bool,
    request_id: String,
) -> AppResult<WorkspaceView> {
    crate::moves::workspace_only(&window)?;
    let _gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    let server = state.database.lock().unwrap().get(&server_id)?.view();
    let dependencies = state
        .database
        .lock()
        .unwrap()
        .resolve_connection(&server_id)?
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let owner = state.owner(window.label());
    let (view, reserved) = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let reserved = !r.by_server.contains_key(&server_id);
        let view = r.open(server, owner, new_terminal, &request_id)?;
        r.entries.get_mut(&view.id).unwrap().dependencies = dependencies;
        (view, reserved)
    };
    drop(_gate);
    state.workspaces.publish();
    let reserved_id = view.id.clone();
    let runtime = state.inner().clone();
    if reserved {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let removed = {
                let _gate = runtime.configuration.lock().unwrap();
                let mut r = runtime.workspaces.registry.lock().unwrap();
                if r.entries.get(&reserved_id).is_some_and(|w| {
                    !w.mounted
                        && w.context.is_none()
                        && w.verification.is_none()
                        && w.view.lifecycle == "active"
                        && w.view.terminals.iter().all(|t| t.connection_id.is_none())
                }) {
                    r.remove(&reserved_id);
                    true
                } else {
                    false
                }
            };
            if removed {
                runtime.workspaces.publish();
            }
        });
    }
    let _ = app.emit_to(&view.owner.label, "server-workspace-focus", &view.id);
    if view.owner.label != window.label() {
        if let Some(target) = app.get_webview_window(&view.owner.label) {
            let _ = target.show();
            let _ = target.unminimize();
            let _ = target.set_focus();
        }
    }
    Ok(view)
}
#[tauri::command]
pub fn create_workspace_terminal(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
    request_id: String,
) -> AppResult<WorkspaceView> {
    state.ensure_running()?;
    let owner = state.owner(window.label());
    let view = {
        let mut r = state.workspaces.registry.lock().unwrap();
        r.owned_mut(&workspace_id, &owner, owner_epoch)?
            .create_terminal(&request_id)?;
        r.changed(&workspace_id);
        r.entries[&workspace_id].view.clone()
    };
    state.workspaces.publish();
    Ok(view)
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum TerminalUpdate {
    Select {
        id: String,
    },
    Rename {
        id: String,
        title: Option<String>,
    },
    Reorder {
        ids: Vec<String>,
    },
    Report {
        id: String,
        attempt: u64,
        state: String,
    },
}
#[tauri::command]
pub fn update_workspace_terminal(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
    update: TerminalUpdate,
) -> AppResult<WorkspaceView> {
    let owner = state.owner(window.label());
    let view = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
        match update {
            TerminalUpdate::Select { id } => {
                if !w.view.terminals.iter().any(|t| t.id == id) {
                    return Err(stale());
                }
                w.view.active_terminal_id = Some(id);
            }
            TerminalUpdate::Rename { id, title } => {
                let title = title.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
                if title
                    .as_ref()
                    .is_some_and(|s| s.chars().count() > 80 || s.chars().any(char::is_control))
                {
                    return Err(AppError::new(
                        "validation",
                        "终端名称限 80 字且不能包含控制字符",
                    ));
                }
                w.view
                    .terminals
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(stale)?
                    .title = title;
            }
            TerminalUpdate::Reorder { ids } => {
                let mut expected: Vec<_> = w.view.terminals.iter().map(|t| t.id.clone()).collect();
                let mut actual = ids.clone();
                expected.sort();
                actual.sort();
                if expected != actual {
                    return Err(stale());
                }
                w.view
                    .terminals
                    .sort_by_key(|t| ids.iter().position(|id| id == &t.id).unwrap());
            }
            TerminalUpdate::Report { id, attempt, state } => {
                let terminal = w
                    .view
                    .terminals
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(stale)?;
                if terminal.attempt != attempt {
                    return Err(stale());
                }
                if ![
                    "queued",
                    "connecting",
                    "authenticating",
                    "connected",
                    "waiting",
                    "disconnected",
                    "failed",
                ]
                .contains(&state.as_str())
                {
                    return Err(AppError::new("validation", "未知的连接状态"));
                }
                terminal.state = state;
            }
        }
        r.changed(&workspace_id);
        r.entries[&workspace_id].view.clone()
    };
    state.workspaces.publish();
    Ok(view)
}

impl WorkspaceState {
    pub fn finish_connection(&self, connection_id: &str) {
        let mut r = self.registry.lock().unwrap();
        let mut changed = None;
        for (id, w) in &mut r.entries {
            if let Some(t) = w
                .view
                .terminals
                .iter_mut()
                .find(|t| t.connection_id.as_deref() == Some(connection_id))
            {
                // Frozen, closed streams still belong to the move until commit/rollback.
                if w.view.lifecycle != "moving" {
                    t.connection_id = None;
                }
                t.state = "disconnected".into();
                changed = Some(id.clone());
                break;
            }
        }
        if let Some(id) = changed {
            r.changed(&id);
        }
        drop(r);
        self.publish();
    }
    pub fn publish_context(
        &self,
        binding: &TerminalBinding,
        owner: &WindowOwner,
        connection_id: &str,
        context: SharedContext,
        configuration_changed: bool,
    ) -> AppResult<()> {
        let mut r = self.registry.lock().unwrap();
        let w = r.owned_mut(&binding.workspace_id, owner, binding.owner_epoch)?;
        let t = w
            .view
            .terminals
            .iter_mut()
            .find(|t| t.id == binding.terminal_id)
            .ok_or_else(stale)?;
        if t.attempt != binding.attempt || t.connection_id.as_deref() != Some(connection_id) {
            return Err(stale());
        }
        if w.context.is_none() {
            install_context(w, context);
            w.view.tools.configuration_changed = configuration_changed;
        }
        r.changed(&binding.workspace_id);
        drop(r);
        self.publish();
        Ok(())
    }
    pub fn connection_ready(&self, connection_id: &str) {
        let mut r = self.registry.lock().unwrap();
        let id = r.entries.iter_mut().find_map(|(id, w)| {
            w.view
                .terminals
                .iter_mut()
                .find(|t| t.connection_id.as_deref() == Some(connection_id))
                .map(|t| {
                    t.state = "connected".into();
                    id.clone()
                })
        });
        if let Some(id) = id {
            r.changed(&id);
        }
        drop(r);
        self.publish();
    }
}

pub fn refresh_configuration(state: &State) {
    let mut r = state.workspaces.registry.lock().unwrap();
    let ids: Vec<_> = r.entries.keys().cloned().collect();
    for id in ids {
        let w = r.entries.get_mut(&id).unwrap();
        let database = state.database.lock().unwrap();
        if let Ok(server) = database.get(&w.view.server.id) {
            w.view.server = server.view();
        }
        if let Ok(plan) = database.resolve_connection(&w.view.server.id) {
            w.dependencies = plan.iter().map(|s| s.id.clone()).collect();
        }
        if let Some(context) = &w.context {
            w.view.tools.configuration_changed = database
                .resolve_connection(&w.view.server.id)
                .and_then(crate::connection::ConnectionPlan::new)
                .map_or(true, |p| !context.matches_plan(&p));
        }
        drop(database);
        r.changed(&id);
    }
    drop(r);
    state.workspaces.publish();
}

#[tauri::command]
pub fn mount_server_workspace(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .owned_mut(&workspace_id, &owner, owner_epoch)?
        .mounted = true;
    Ok(())
}
