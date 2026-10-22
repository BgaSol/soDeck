use super::*;
use crate::{terminal::Input, State};
use serde::Serialize;
use std::sync::Arc;
use tauri::{State as TauriState, WebviewWindow};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseImpact {
    pub revision: u64,
    pub terminals: usize,
    pub connected: usize,
    pub verifying: bool,
    pub blockers: Vec<String>,
    pub independent_tasks: usize,
}
pub fn blockers(state: &State, server: &str) -> Vec<String> {
    let mut blockers = vec![];

    let contexts: Vec<_> = state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .entries
        .values()
        .filter(|w| w.view.server.id == server)
        .filter_map(|w| w.view.tools.context.as_ref().map(|c| c.id.clone()))
        .collect();
    if contexts
        .iter()
        .any(|id| state.docker.has_context_drafts(id) || state.netbird.has_context_drafts(id))
    {
        blockers.push("请先确认或取消工具操作预检".into());
    }
    blockers
}
#[tauri::command]
pub fn prepare_workspace_close(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
) -> AppResult<CloseImpact> {
    let _gate = state.configuration.lock().unwrap();
    let owner = state.owner(window.label());
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
    let server = w.view.server.id.clone();
    let operation_busy = w.ensure_idle().err().map(|e| e.message);
    let mut impact = CloseImpact {
        revision: w.view.revision,
        terminals: w.view.terminals.len(),
        connected: w
            .view
            .terminals
            .iter()
            .filter(|t| t.connection_id.is_some())
            .count(),
        verifying: w.verification.is_some(),
        blockers: vec![],
        independent_tasks: 0,
    };
    drop(r);
    impact.blockers = blockers(&state, &server);
    if let Some(message) = operation_busy {
        impact.blockers.push(message);
    }
    impact.independent_tasks = usize::from(state.transfers.has_server_activity(&server));
    Ok(impact)
}
#[tauri::command]
pub fn close_server_workspace(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
    revision: u64,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    let owner = state.owner(window.label());
    let server = {
        let mut r = state.workspaces.registry.lock().unwrap();
        if !r.entries.contains_key(&workspace_id) {
            return Ok(());
        }
        let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
        w.ensure_idle()?;
        if w.view.revision != revision {
            return Err(stale());
        }
        w.view.server.id.clone()
    };
    if let Some(message) = blockers(&state, &server).first() {
        return Err(AppError::new("busy", message));
    }
    let w = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
        w.ensure_idle()?;
        if w.view.revision != revision {
            return Err(stale());
        }
        r.remove(&workspace_id).ok_or_else(stale)?
    };
    dispose(&state, w);
    drop(_gate);
    state.workspaces.publish();
    Ok(())
}
#[tauri::command]
pub fn close_workspace_terminal(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
    terminal_id: String,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
    let Some(index) = w.view.terminals.iter().position(|t| t.id == terminal_id) else {
        return Ok(());
    };
    let terminal = w.view.terminals.remove(index);
    if w.view.active_terminal_id.as_deref() == Some(&terminal_id) {
        w.view.active_terminal_id = w
            .view
            .terminals
            .get(index.saturating_sub(1))
            .map(|t| t.id.clone());
    }
    if let Some(id) = terminal.connection_id {
        if let Some(h) = state.terminals.lock().unwrap().get(&id) {
            let _ = h.sender.send(Input::Close);
        }
    }
    r.changed(&workspace_id);
    drop(r);
    state.workspaces.publish();
    Ok(())
}
fn dispose(state: &State, w: Workspace) {
    if let Some(v) = w.verification {
        v.cancel.cancel();
    }
    {
        let terminals = state.terminals.lock().unwrap();
        for t in &w.view.terminals {
            if let Some(h) = t.connection_id.as_ref().and_then(|id| terminals.get(id)) {
                let _ = h.sender.send(Input::Close);
            }
        }
    }
    if let Some(context) = w.view.tools.context {
        state.monitor.close_session(&context.id);
        state.netbird.close_session(&context.id);
    }
}
pub fn close_window(state: &State, label: &str) {
    let removed = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let ids: Vec<_> = r
            .entries
            .values()
            .filter(|w| w.view.owner.label == label)
            .map(|w| w.view.id.clone())
            .collect();
        ids.iter().filter_map(|id| r.remove(id)).collect::<Vec<_>>()
    };
    for w in removed {
        dispose(state, w);
    }
    state.workspaces.publish();
}
pub fn shutdown(state: &State) {
    let removed = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let ids: Vec<_> = r.entries.keys().cloned().collect();
        ids.iter().filter_map(|id| r.remove(id)).collect::<Vec<_>>()
    };
    for w in removed {
        dispose(state, w);
    }
    state.workspaces.publish();
}
