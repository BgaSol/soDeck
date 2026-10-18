pub(crate) mod store;

use crate::{
    model::{AppError, AppResult},
    remote_delete::{self, DeleteTask},
    transfers::{self, TaskView},
    State,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, State as TauriState, WebviewWindow};

pub struct FileTaskState {
    pub(crate) store: Mutex<store::Store>,
    pub(crate) actions: Mutex<()>,
    app: Mutex<Option<tauri::AppHandle>>,
}
impl Default for FileTaskState {
    fn default() -> Self {
        Self::open(Path::new(":memory:")).expect("in-memory task store")
    }
}
impl FileTaskState {
    pub fn open(path: &Path) -> AppResult<Self> {
        Ok(Self {
            store: Mutex::new(store::Store::open(path)?),
            actions: Mutex::new(()),
            app: Mutex::new(None),
        })
    }
    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }
    pub fn emit(&self, id: &str) {
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit("file-task-updated", serde_json::json!({"taskId":id}));
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn active(status: &str) -> bool {
    matches!(
        status,
        "queued" | "checking" | "scanning" | "running" | "verifying" | "committing" | "cancelling"
    )
}
fn attention(status: &str) -> bool {
    matches!(
        status,
        "failed"
            | "partialFailed"
            | "sourceRetained"
            | "unknown"
            | "interrupted"
            | "needsAttention"
    )
}

#[derive(Serialize)]
#[serde(tag = "category", content = "task", rename_all = "camelCase")]
pub enum TaskData {
    Transfer(TaskView),
    Delete(DeleteTask),
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Actions {
    stop: bool,
    retry: bool,
    reconcile: bool,
    cleanup: bool,
    dismiss: bool,
}
#[derive(Serialize)]
pub struct FileTask {
    #[serde(flatten)]
    data: TaskData,
    actions: Actions,
}
impl FileTask {
    fn id(&self) -> &str {
        match &self.data {
            TaskData::Transfer(t) => &t.id,
            TaskData::Delete(t) => &t.id,
        }
    }
    fn status(&self) -> &str {
        match &self.data {
            TaskData::Transfer(t) => t.status.as_str(),
            TaskData::Delete(t) => &t.status,
        }
    }
    fn order(&self) -> u64 {
        match &self.data {
            TaskData::Transfer(t) => t.presentation.sort_order,
            TaskData::Delete(t) => t.sort_order,
        }
    }
    fn compact(&mut self) {
        if let TaskData::Delete(t) = &mut self.data {
            t.entries.truncate(3);
            for entry in &mut t.entries {
                entry.failures.clear();
            }
        }
    }
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    total: usize,
    active: usize,
    attention: usize,
    completed: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPage {
    tasks: Vec<FileTask>,
    summary: Summary,
    next_cursor: Option<String>,
}

pub(crate) fn snapshot(state: &State) -> AppResult<Vec<FileTask>> {
    let transfers = transfers::snapshot(state);
    let deletes = state.deletes.snapshot();
    let mut tasks = Vec::with_capacity(transfers.len() + deletes.len());
    for t in transfers {
        let running = t.active();
        let busy = state.transfers.task_busy(&t.id);
        let unresolved = t.kind != "download"
            && matches!(
                t.status.as_str(),
                "unknown" | "needsAttention" | "interrupted"
            );
        let cleanup = !running && t.cleanup_pending;
        let pending = state.transfers.has_pending_task(&t.id);
        let actions = Actions {
            stop: running && t.status != "cancelling",
            retry: t.kind != "docker"
                && !running
                && !busy
                && !pending
                && matches!(
                    t.status.as_str(),
                    "failed" | "partialFailed" | "sourceRetained" | "cancelled" | "interrupted"
                ),
            reconcile: !running && !busy && unresolved && t.kind != "download",
            cleanup: cleanup && !busy,
            dismiss: !running && !busy && !unresolved && !cleanup && !pending,
        };
        tasks.push(FileTask {
            data: TaskData::Transfer(t),
            actions,
        });
    }
    for t in deletes {
        let running = active(&t.status);
        let busy = state.deletes.busy(&t.id);
        let uncertain = t.entries.iter().any(|e| {
            e.failures
                .iter()
                .any(|f| f.uncertain && f.current_state.is_none())
        });
        let actions = Actions {
            stop: running && t.status != "cancelling",
            retry: !running
                && !busy
                && !uncertain
                && t.entries
                    .iter()
                    .any(|e| !matches!(e.status.as_str(), "deleted" | "missing")),
            reconcile: !running
                && !busy
                && t.entries
                    .iter()
                    .any(|e| e.failures.iter().any(|f| f.uncertain)),
            cleanup: false,
            dismiss: !running && !busy && !uncertain,
        };
        tasks.push(FileTask {
            data: TaskData::Delete(t),
            actions,
        });
    }
    let store = state.file_tasks.store.lock().unwrap();
    // Transfer receipts own recovery; this idempotent projection adds shared
    // history metadata without replacing those receipts or restoring hidden rows.
    for item in &tasks {
        if let TaskData::Transfer(t) = &item.data {
            // Downloads already have durable execution metadata in this table.
            // A presentation projection must never replace that receipt.
            if t.kind != "download" && t.kind != "docker" {
                store.save("transfer", &t.id, t)?;
            }
        }
    }
    let hidden = store.hidden()?;
    tasks.retain(|t| active(t.status()) || !hidden.contains(t.id()));
    tasks.sort_by(|a, b| b.order().cmp(&a.order()).then_with(|| a.id().cmp(b.id())));
    Ok(tasks)
}

#[tauri::command]
pub fn list_file_tasks(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    filter: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> AppResult<TaskPage> {
    crate::moves::workspace_only(&window)?;
    let mut tasks = snapshot(&state)?;
    let summary = Summary {
        total: tasks.len(),
        active: tasks.iter().filter(|t| active(t.status())).count(),
        attention: tasks.iter().filter(|t| attention(t.status())).count(),
        completed: tasks
            .iter()
            .filter(|t| t.status() == "completed" && t.actions.dismiss)
            .count(),
    };
    match filter.as_deref().unwrap_or("all") {
        "all" => {}
        "active" => tasks.retain(|t| active(t.status())),
        "attention" => tasks.retain(|t| attention(t.status())),
        _ => return Err(AppError::new("validation", "无效的任务筛选")),
    }
    let start = cursor
        .as_ref()
        .and_then(|id| tasks.iter().position(|t| t.id() == id))
        .map_or(0, |i| i + 1);
    let end = (start + limit.unwrap_or(50).clamp(1, 1000)).min(tasks.len());
    let next_cursor = if end < tasks.len() {
        Some(tasks[end - 1].id().to_owned())
    } else {
        None
    };
    let mut tasks: Vec<_> = tasks.drain(start..end).collect();
    for task in &mut tasks {
        task.compact();
    }
    Ok(TaskPage {
        tasks,
        summary,
        next_cursor,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailItem {
    path: String,
    status: String,
    root: bool,
    deleted_count: u64,
    missing_count: u64,
    error: Option<String>,
    uncertain: bool,
    current_state: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailPage {
    task: FileTask,
    items: Vec<DetailItem>,
    total: usize,
    next_offset: Option<usize>,
}
#[tauri::command]
pub fn get_file_task(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<DetailPage> {
    crate::moves::workspace_only(&window)?;
    let mut task = snapshot(&state)?
        .into_iter()
        .find(|t| t.id() == id)
        .ok_or_else(|| AppError::new("notFound", "文件任务不存在"))?;
    let mut items = vec![];
    if let TaskData::Delete(t) = &task.data {
        for entry in &t.entries {
            items.push(DetailItem {
                path: entry.path.clone(),
                status: entry.status.clone(),
                root: true,
                deleted_count: entry.deleted_count,
                missing_count: entry.missing_count,
                error: None,
                uncertain: false,
                current_state: None,
            });
            for f in &entry.failures {
                items.push(DetailItem {
                    path: f.path.clone(),
                    status: "failed".into(),
                    root: false,
                    deleted_count: 0,
                    missing_count: 0,
                    error: Some(f.message.clone()),
                    uncertain: f.uncertain,
                    current_state: f.current_state.clone(),
                });
            }
        }
    }
    let total = items.len();
    let start = offset.unwrap_or(0).min(total);
    let end = (start + limit.unwrap_or(100).clamp(1, 1000)).min(total);
    let items = items.drain(start..end).collect();
    task.compact();
    Ok(DetailPage {
        task,
        items,
        total,
        next_offset: (end < total).then_some(end),
    })
}

#[tauri::command]
pub fn cancel_file_task(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    if state.deletes.contains(&id) {
        remote_delete::cancel_remote_delete(window, app, state, id)
    } else {
        let result = transfers::cancel_transfer(window, state.clone(), id.clone());
        state.file_tasks.emit(&id);
        result
    }
}
#[tauri::command]
pub async fn reconcile_file_task(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    id: String,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    if state
        .docker
        .reconcile_transfer(state.inner(), &state.owner(window.label()), &id)?
    {
        state.file_tasks.emit(&id);
        return Ok(());
    }
    if state.deletes.contains(&id) {
        remote_delete::reconcile_remote_delete(window, app, state, id)
            .await
            .map(|_| ())
    } else {
        let result = transfers::reconcile_transfer(window, app, state.clone(), id.clone()).await;
        state.file_tasks.emit(&id);
        result
    }
}
#[tauri::command]
pub fn dismiss_file_task(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    let _action = state.file_tasks.actions.lock().unwrap();
    let task = snapshot(&state)?
        .into_iter()
        .find(|t| t.id() == id)
        .ok_or_else(|| AppError::new("notFound", "文件任务不存在"))?;
    if !task.actions.dismiss {
        return Err(AppError::new(
            "busy",
            "任务仍在执行、核对或有待处理资源，不能移除记录",
        ));
    }
    state
        .file_tasks
        .store
        .lock()
        .unwrap()
        .dismiss(std::slice::from_ref(&id))?;
    state.file_tasks.emit(&id);
    Ok(())
}
#[tauri::command]
pub fn clear_completed_file_tasks(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    let _action = state.file_tasks.actions.lock().unwrap();
    let ids: Vec<_> = snapshot(&state)?
        .iter()
        .filter(|t| t.status() == "completed" && t.actions.dismiss)
        .map(|t| t.id().to_owned())
        .collect();
    state.file_tasks.store.lock().unwrap().dismiss(&ids)?;
    state.file_tasks.emit("");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_delete::{DeleteEntry, DeleteFailure};
    #[test]
    fn recovered_task_actions_preserve_uncertainty_and_completed_results() {
        let mut state = crate::connection::tests::state();
        {
            let store = state.file_tasks.store.lock().unwrap();
            for (id, status, uncertain, order) in [
                ("done", "completed", false, 1),
                ("unknown", "unknown", true, 2),
                ("stopped", "cancelled", false, 3),
            ] {
                let task = DeleteTask {
                    id: id.into(),
                    status: status.into(),
                    sort_order: order,
                    entry_count: 1,
                    entries: vec![DeleteEntry {
                        path: "/data/file".into(),
                        status: if status == "completed" {
                            "deleted"
                        } else {
                            "notExecuted"
                        }
                        .into(),
                        failures: if uncertain {
                            vec![DeleteFailure {
                                path: "/data/file".into(),
                                message: "lost reply".into(),
                                uncertain: true,
                                current_state: None,
                            }]
                        } else {
                            vec![]
                        },
                        ..DeleteEntry::default()
                    }],
                    ..DeleteTask::default()
                };
                store.save("delete", id, &task).unwrap();
            }
        }
        let deletes =
            remote_delete::DeleteState::restore(&mut state.file_tasks.store.lock().unwrap())
                .unwrap();
        Arc::get_mut(&mut state).unwrap().deletes = deletes;
        let tasks = snapshot(&state).unwrap();
        assert_eq!(
            tasks.iter().map(|t| t.id()).collect::<Vec<_>>(),
            ["stopped", "unknown", "done"]
        );
        assert!(tasks[0].actions.retry && tasks[0].actions.dismiss);
        assert!(tasks[1].actions.reconcile);
        assert!(!tasks[1].actions.retry && !tasks[1].actions.dismiss);
        assert!(tasks[2].actions.dismiss && !tasks[2].actions.retry);
        state
            .file_tasks
            .store
            .lock()
            .unwrap()
            .dismiss(&["done".into()])
            .unwrap();
        assert_eq!(snapshot(&state).unwrap().len(), 2);
        assert_eq!(
            state.deletes.snapshot().len(),
            3,
            "hiding history cannot remove execution evidence"
        );
    }
}
