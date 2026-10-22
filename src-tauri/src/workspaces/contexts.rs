use super::*;
use crate::{
    connection::{
        self, ConnectEventSink, ConnectionAnswer, ConnectionEvent, ConnectionPlan, Interaction,
    },
    ssh_transport::Cancellation,
    State,
};
use std::sync::Arc;
use tauri::{ipc::Channel, State as TauriState, WebviewWindow};

/// Holds the authorization to finish an in-flight operation. Lifecycle changes
/// check this counter under the same registry lock used to acquire it.
pub struct OperationLease(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for OperationLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
impl Workspace {
    pub fn ensure_idle(&self) -> AppResult<()> {
        if self.operations.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            return Err(AppError::new(
                "busy",
                "服务器操作仍在执行，请完成后再关闭、移动或更新工具连接",
            ));
        }
        Ok(())
    }
}
pub struct Verification {
    pub id: String,
    pub owner: WindowOwner,
    pub cancel: Cancellation,
    pub interaction: Arc<Interaction>,
    pub plan: Arc<ConnectionPlan>,
}
pub(crate) fn install_context(w: &mut Workspace, context: SharedContext) {
    let epoch = w.view.tools.context.as_ref().map_or(1, |r| r.epoch + 1);
    // A fresh capability id on each generation prevents old internal leases,
    // drafts and subscriptions from authorizing against a new target.
    w.view.tools.context = Some(ToolContextRef {
        id: format!("{}.{}", new_id(), epoch),
        epoch,
    });
    w.view.tools.connection = Some(context.view());
    w.view.tools.state = "available".into();
    w.view.tools.configuration_changed = false;
    w.context = Some(context);
}
impl WorkspaceState {
    pub fn lease(
        &self,
        reference: &ToolContextRef,
        owner: &WindowOwner,
    ) -> AppResult<OperationLease> {
        let r = self.registry.lock().unwrap();
        let w = r
            .entries
            .values()
            .find(|w| w.view.tools.context.as_ref() == Some(reference))
            .ok_or_else(stale)?;
        if w.view.owner.label != owner.label
            || w.view.owner.generation != owner.generation
            || w.view.lifecycle != "active"
        {
            return Err(stale());
        }
        w.operations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(OperationLease(w.operations.clone()))
    }
    pub fn context(&self, id: &str, owner: &WindowOwner) -> AppResult<SharedContext> {
        let r = self.registry.lock().unwrap();
        let w = r
            .entries
            .values()
            .find(|w| w.view.tools.context.as_ref().is_some_and(|c| c.id == id))
            .ok_or_else(|| AppError::new("disconnected", "服务器工具上下文已失效"))?;
        if w.view.owner.label != owner.label
            || w.view.owner.generation != owner.generation
            || w.view.lifecycle != "active"
        {
            return Err(AppError::new("permission", "此窗口当前无权访问服务器工具"));
        }
        w.context.clone().ok_or_else(stale)
    }
    pub fn context_key(
        &self,
        reference: &ToolContextRef,
        owner: &WindowOwner,
    ) -> AppResult<String> {
        self.context(&reference.id, owner)?;
        let r = self.registry.lock().unwrap();
        if !r
            .entries
            .values()
            .any(|w| w.view.tools.context.as_ref() == Some(reference))
        {
            return Err(stale());
        }
        Ok(reference.id.clone())
    }
    pub fn context_alive(&self, id: &str) -> bool {
        self.registry.lock().unwrap().entries.values().any(|w| {
            w.context.is_some() && w.view.tools.context.as_ref().is_some_and(|r| r.id == id)
        })
    }
    pub fn capabilities_for(
        &self,
        contexts: &[SharedContext],
    ) -> AppResult<Vec<(ToolContextRef, WindowOwner)>> {
        let r = self.registry.lock().unwrap();
        contexts
            .iter()
            .map(|input| {
                let w = r
                    .entries
                    .values()
                    .find(|w| {
                        w.view.lifecycle == "active"
                            && w.context.as_ref().is_some_and(|c| Arc::ptr_eq(c, input))
                    })
                    .ok_or_else(stale)?;
                Ok((
                    w.view.tools.context.clone().ok_or_else(stale)?,
                    WindowOwner {
                        label: w.view.owner.label.clone(),
                        generation: w.view.owner.generation,
                    },
                ))
            })
            .collect()
    }
    pub fn contexts(&self) -> Vec<(String, String, SharedContext)> {
        self.registry
            .lock()
            .unwrap()
            .entries
            .values()
            .filter(|w| w.view.lifecycle == "active")
            .filter_map(|w| {
                Some((
                    w.view.tools.context.as_ref()?.id.clone(),
                    w.view.owner.label.clone(),
                    w.context.clone()?,
                ))
            })
            .collect()
    }
}
struct VerificationJob(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for VerificationJob {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
struct VerificationSink(Channel<ConnectionEvent>);
impl ConnectEventSink for VerificationSink {
    fn send(&self, event: ConnectionEvent) -> AppResult<()> {
        self.0
            .send(event)
            .map_err(|e| AppError::new("cancelled", e))
    }
}

#[tauri::command]
pub fn refresh_workspace_context(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
    events: Channel<ConnectionEvent>,
) -> AppResult<String> {
    let _gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    let owner = state.owner(window.label());
    let server = state
        .workspaces
        .registry
        .lock()
        .unwrap()
        .owned_mut(&workspace_id, &owner, owner_epoch)?
        .view
        .server
        .id
        .clone();
    if let Some(message) = super::blockers(&state, &server).first() {
        return Err(AppError::new("busy", message));
    }
    let mut r = state.workspaces.registry.lock().unwrap();
    let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
    w.ensure_idle()?;
    if w.verification.is_some() {
        return Err(AppError::new("busy", "工具连接正在验证"));
    }
    let plan = Arc::new(ConnectionPlan::new(
        state
            .database
            .lock()
            .unwrap()
            .resolve_connection(&w.view.server.id)?,
    )?);
    let id = new_id();
    let cancel = Cancellation::default();
    let interaction = Arc::new(Interaction::default());
    w.verification = Some(Verification {
        id: id.clone(),
        owner,
        cancel: cancel.clone(),
        interaction: interaction.clone(),
        plan: plan.clone(),
    });
    w.view.tools.state = "verifying".into();
    r.changed(&workspace_id);
    drop(r);
    drop(_gate);
    state.workspaces.publish();
    let state = state.inner().clone();
    let worker_id = id.clone();
    state
        .workspaces
        .verifications
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let job = VerificationJob(state.workspaces.verifications.clone());
    std::thread::spawn(move || {
        let _job = job;
        let result = connection::connect_interactive(
            &state,
            &plan,
            Arc::new(VerificationSink(events.clone())),
            interaction,
            None,
            &cancel,
            crate::traffic::TrafficPurpose::Shared,
        );
        let _gate = state.configuration.lock().unwrap();
        let server = state
            .workspaces
            .registry
            .lock()
            .unwrap()
            .entries
            .get(&workspace_id)
            .map(|w| w.view.server.id.clone());
        let result = result.and_then(|ready| {
            if let Some(server) = server {
                if let Some(message) = super::blockers(&state, &server).first() {
                    return Err(AppError::new("busy", message));
                }
            }
            Ok(ready)
        });
        let mut r = state.workspaces.registry.lock().unwrap();
        let Some(w) = r.entries.get_mut(&workspace_id) else {
            return;
        };
        if !w.verification.as_ref().is_some_and(|v| v.id == worker_id) {
            return;
        }
        let old = w.view.tools.context.clone();
        let result = result.and_then(|(_socket, context)| {
            cancel.check()?;
            w.ensure_idle()?;
            let changed = state
                .database
                .lock()
                .unwrap()
                .resolve_connection(&w.view.server.id)
                .and_then(ConnectionPlan::new)
                .map_or(true, |latest| !context.matches_plan(&latest));
            install_context(w, context);
            w.view.tools.configuration_changed = changed;
            Ok(())
        });
        w.verification = None;
        if result.is_err() {
            w.view.tools.state = if w.context.is_some() {
                "available"
            } else {
                "error"
            }
            .into();
        }
        r.changed(&workspace_id);
        drop(r);
        if result.is_ok() {
            if let Some(old) = old {
                state.monitor.close_session(&old.id);
                state.netbird.close_session(&old.id);
            }
        }
        let _ = events.send(ConnectionEvent::Closed {
            message: result
                .as_ref()
                .err()
                .map_or_else(|| "工具验证完成".into(), |e| e.message.clone()),
            reason: if result.is_ok() { "ended" } else { "failed" }.into(),
            recovery: connection::RecoveryAction::None,
            error: result.err(),
        });
        state.workspaces.publish();
    });
    Ok(id)
}
#[tauri::command]
pub fn answer_workspace_context(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    verification_id: String,
    challenge_id: String,
    response: ConnectionAnswer,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let r = state.workspaces.registry.lock().unwrap();
    let v = r
        .entries
        .values()
        .filter_map(|w| w.verification.as_ref())
        .find(|v| v.id == verification_id)
        .ok_or_else(stale)?;
    if v.owner.label != owner.label || v.owner.generation != owner.generation {
        return Err(stale());
    }
    v.cancel.check()?;
    v.interaction.answer(&challenge_id, response)
}
#[tauri::command]
pub fn cancel_workspace_context(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    verification_id: String,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let r = state.workspaces.registry.lock().unwrap();
    if let Some(v) = r
        .entries
        .values()
        .filter_map(|w| w.verification.as_ref())
        .find(|v| v.id == verification_id)
    {
        if v.owner.label != owner.label || v.owner.generation != owner.generation {
            return Err(stale());
        }
        v.cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn present_workspace_context_challenge(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    verification_id: String,
    challenge_id: String,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let r = state.workspaces.registry.lock().unwrap();
    let v = r
        .entries
        .values()
        .filter_map(|w| w.verification.as_ref())
        .find(|v| v.id == verification_id)
        .ok_or_else(stale)?;
    if v.owner.label != owner.label || v.owner.generation != owner.generation {
        return Err(stale());
    }
    v.interaction.present(&challenge_id)
}
