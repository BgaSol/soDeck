use super::{diagnostics, model::*, query};
use crate::{
    connection::WindowOwner,
    model::{AppError, AppResult},
    State,
};
use std::sync::Arc;
use tauri::{ipc::Channel, State as TauriState, WebviewWindow};
fn owner(state: &State, window: &WebviewWindow) -> AppResult<WindowOwner> {
    crate::moves::workspace_only(window)?;
    state.ensure_running()?;
    Ok(state.owner(window.label()))
}
#[tauri::command]
pub fn subscribe_netbird(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    interest: Interest,
    events: Channel<Frame>,
) -> AppResult<String> {
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    state
        .netbird
        .subscribe(state.inner(), session_id, owner, interest, events)
}
#[tauri::command]
pub fn update_netbird_interest(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    interest: Interest,
) -> AppResult<()> {
    state
        .netbird
        .interest(&subscription_id, &owner(&state, &window)?, interest)
}
#[tauri::command]
pub fn ack_netbird_frame(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    revision: u64,
) -> AppResult<()> {
    state
        .netbird
        .ack(&subscription_id, &owner(&state, &window)?, revision)
}
#[tauri::command]
pub fn unsubscribe_netbird(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
) -> AppResult<()> {
    state
        .netbird
        .unsubscribe(&subscription_id, &owner(&state, &window)?)
}
#[tauri::command]
pub fn refresh_netbird(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
) -> AppResult<()> {
    state
        .netbird
        .refresh(&subscription_id, &owner(&state, &window)?)
}
#[tauri::command]
pub async fn get_netbird_raw_status(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    binding_token: String,
) -> AppResult<Diagnostic> {
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state.netbird.read(
            &state,
            &session_id,
            &owner,
            &binding_token,
            |ssh, _, snapshot, check| diagnostics::raw(ssh, snapshot, check),
        )
    })
    .await
}
#[tauri::command]
pub async fn get_netbird_logs(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    binding_token: String,
) -> AppResult<Diagnostic> {
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state.netbird.read(
            &state,
            &session_id,
            &owner,
            &binding_token,
            |ssh, _, _, check| diagnostics::logs(ssh, check),
        )
    })
    .await
}
#[tauri::command]
pub async fn probe_netbird_peer(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    binding_token: String,
    request: ProbeRequest,
) -> AppResult<ProbeResult> {
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state.netbird.read(
            &state,
            &session_id,
            &owner,
            &binding_token,
            |ssh, spec, snapshot, check| {
                diagnostics::probe(ssh, snapshot, request, &spec.target().name, check)
            },
        )
    })
    .await
}
#[tauri::command]
pub async fn prepare_netbird_operation(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    binding_token: String,
    action: Action,
) -> AppResult<Preview> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state
            .netbird
            .prepare(&state, &session_id, &owner, &binding_token, action)
    })
    .await
}
#[tauri::command]
pub fn submit_netbird_operation(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    request: OperationRequest,
) -> AppResult<String> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    state
        .netbird
        .submit(state.inner(), &session_id, &owner, request)
}
#[tauri::command]
pub async fn reconcile_netbird_operation(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    operation_id: String,
) -> AppResult<()> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state
            .netbird
            .reconcile(&state, &session_id, &owner, &operation_id)
    })
    .await
}
#[tauri::command]
pub fn acknowledge_netbird_operation(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    operation_id: String,
) -> AppResult<()> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    state
        .netbird
        .acknowledge(&state, &session_id, &owner, &operation_id)
}
#[tauri::command]
pub fn set_netbird_cli_path(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    path: Option<String>,
) -> AppResult<()> {
    let _lease = state.workspace_operation(&context, window.label())?;
    let session_id = state.context_key(&context, window.label())?;
    let owner = owner(&state, &window)?;
    let _gate = state.configuration.lock().unwrap();
    let spec = state.workspace_context(&session_id, &owner.label)?;
    if state.netbird.has_server(&spec.target().id) {
        return Err(AppError::new("busy", "请等待操作完成后修改 CLI 路径"));
    }
    if let Some(p) = path.as_deref() {
        query::validate_path(p)?;
    }
    state
        .netbird
        .with_store(|s| s.set_path(&spec.target().id, path.as_deref()))?;
    let e = state.netbird.entry(spec.identity())?;
    if let Some(c) = e.poll.lock().unwrap().as_ref() {
        c.cancel();
    }
    {
        let mut c = e.core.lock().unwrap();
        c.generation += 1;
        c.snapshot.binding = None;
        c.snapshot.collection_status = "collecting".into();
        c.next = std::time::Instant::now();
    }
    e.publish();
    Ok(())
}

#[tauri::command]
pub fn release_netbird_preview(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    context: crate::workspaces::ToolContextRef,
    preview_id: String,
) -> AppResult<()> {
    let id = state.context_key(&context, window.label())?;
    state
        .netbird
        .release_preview(&id, &owner(&state, &window)?, &preview_id)
}
