//! Application-owned remote clipboard. Drag capabilities deliberately remain separate.
use super::*;
use crate::transfers::{Intent, WriteHandle};

#[derive(Default)]
pub struct ClipboardState(Mutex<Option<Clipboard>>);
struct Clipboard {
    id: String,
    context: SharedContext,
    paths: Vec<String>,
    intent: Intent,
    reserved: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardView {
    id: String,
    paths: Vec<String>,
    intent: Intent,
    reserved: bool,
    same_server: bool,
}
impl ClipboardState {
    fn changed(app: &tauri::AppHandle) {
        let _ = app.emit("remote-clipboard-changed", ());
    }
    pub fn reserve(&self, id: &str) -> AppResult<()> {
        let mut state = self.0.lock().unwrap();
        let c = state
            .as_mut()
            .filter(|c| c.id == id)
            .ok_or_else(|| AppError::new("clipboard", "剪贴板已变化，请重新粘贴"))?;
        if c.intent == Intent::Move {
            if c.reserved {
                return Err(AppError::new(
                    "busy",
                    "这些剪切项目正在移动，请等待任务结束",
                ));
            }
            c.reserved = true;
        }
        Ok(())
    }
    pub fn release(&self, id: &str) {
        if let Some(c) = self.0.lock().unwrap().as_mut().filter(|c| c.id == id) {
            c.reserved = false;
        }
    }
    pub fn reserve_retry(&self, id: &str) -> AppResult<()> {
        let mut state = self.0.lock().unwrap();
        if let Some(c) = state
            .as_mut()
            .filter(|c| c.id == id && c.intent == Intent::Move)
        {
            if c.reserved {
                return Err(AppError::new("busy", "剪切项目已有任务正在运行"));
            }
            c.reserved = true;
        }
        Ok(())
    }
    pub fn finish(&self, app: &tauri::AppHandle, item: &WriteHandle) {
        let id = item.receipt.lock().unwrap().clipboard_id.clone();
        let Some(id) = id else {
            return;
        };
        let plan = item.plan.lock().unwrap().clone();
        let next = self.finish_version(&id, plan.as_ref());
        if let Some(next) = next {
            item.receipt.lock().unwrap().clipboard_id = Some(next);
        }
        Self::changed(app);
    }
    fn finish_version(
        &self,
        id: &str,
        plan: Option<&crate::transfers::planner::Plan>,
    ) -> Option<String> {
        let mut state = self.0.lock().unwrap();
        if let Some(c) = state
            .as_mut()
            .filter(|c| c.id == id && c.intent == Intent::Move)
        {
            c.reserved = false;
            if let Some(plan) = plan {
                // Root directory completion is recorded only after rmdir succeeds.
                c.paths.retain(|p| {
                    !plan
                        .entries
                        .iter()
                        .any(|e| &e.source == p && e.outcome == "completed")
                });
            }
            // Invalidate already-prepared paste dialogs, including partial moves.
            c.id = uuid::Uuid::new_v4().to_string();
            let next = c.id.clone();
            if c.paths.is_empty() {
                *state = None;
            }
            return Some(next);
        }
        None
    }
}
#[tauri::command]
pub fn set_remote_clipboard(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    session_id: String,
    paths: Vec<String>,
    intent: Intent,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let context = state.spec(&session_id, window.label())?;
    let paths = transfers::normalize_selection(&paths)?;
    *state.drags.clipboard.0.lock().unwrap() = Some(Clipboard {
        id: uuid::Uuid::new_v4().to_string(),
        context,
        paths,
        intent,
        reserved: false,
    });
    ClipboardState::changed(&app);
    Ok(())
}
#[tauri::command]
pub fn get_remote_clipboard(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
) -> AppResult<Option<ClipboardView>> {
    moves::workspace_only(&window)?;
    let target = state.spec(&session_id, window.label())?;
    Ok(state
        .drags
        .clipboard
        .0
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| ClipboardView {
            id: c.id.clone(),
            paths: c.paths.clone(),
            intent: c.intent,
            reserved: c.reserved,
            same_server: c.context.identity() == target.identity(),
        }))
}
#[tauri::command]
pub fn clear_remote_clipboard(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let mut c = state.drags.clipboard.0.lock().unwrap();
    if c.as_ref().is_some_and(|c| c.id == id) {
        *c = None;
    }
    drop(c);
    ClipboardState::changed(&app);
    Ok(())
}
#[tauri::command]
pub fn prepare_remote_paste(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    parent_path: String,
) -> AppResult<DraftPreview> {
    moves::workspace_only(&window)?;
    let target = state.spec(&session_id, window.label())?;
    let (source, intent, id) = {
        let c = state.drags.clipboard.0.lock().unwrap();
        let c = c
            .as_ref()
            .ok_or_else(|| AppError::new("clipboard", "请先复制或剪切远端项目"))?;
        if c.reserved {
            return Err(AppError::new("busy", "这些剪切项目正在移动"));
        }
        state.check_context(&c.context)?;
        (
            Source::Remote {
                context: c.context.clone(),
                paths: c.paths.clone(),
            },
            c.intent,
            c.id.clone(),
        )
    };
    create_operation_draft(
        &state,
        window.label(),
        source,
        target,
        parent_path,
        intent,
        Some(id),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cut_is_reserved_and_stale_completion_does_not_clear_a_new_clipboard() {
        let config = crate::model::ServerConfig {
            id: "a".into(),
            name: "A".into(),
            host: "a".into(),
            port: 22,
            username: "ops".into(),
            tags: vec![],
            auth_type: "password".into(),
            private_key: String::new(),
            password: String::new(),
            jump_server_id: None,
            direct_host: None,
            direct_port: 22,
        };
        let c = ClipboardState(Mutex::new(Some(Clipboard {
            id: "first".into(),
            context: crate::connection::tests::mock_context(config),
            paths: vec!["/a".into(), "/b".into()],
            intent: Intent::Move,
            reserved: false,
        })));
        c.reserve("first").unwrap();
        assert!(c.reserve("first").is_err());
        c.release("wrong-version");
        assert!(c.0.lock().unwrap().as_ref().unwrap().reserved);
        let entry = |path: &str, outcome: &str| crate::transfers::planner::Entry {
            source: path.into(),
            parts: vec![path.into()],
            directory: false,
            size: 1,
            modified: None,
            mode: None,
            created_directory: false,
            target: None,
            outcome: outcome.into(),
            staged: None,
            digest: None,
            error: None,
            target_version: None,
            source_stamp: None,
            removal_pending: false,
        };
        let plan = crate::transfers::planner::Plan {
            entries: vec![entry("/a", "completed"), entry("/b", "copied")],
            skipped: vec![],
        };
        let next = c.finish_version("first", Some(&plan)).unwrap();
        assert_eq!(c.0.lock().unwrap().as_ref().unwrap().paths, vec!["/b"]);
        assert!(c.reserve("first").is_err());
        c.reserve(&next).unwrap();
        assert!(c.finish_version("first", Some(&plan)).is_none());
        assert!(c.0.lock().unwrap().as_ref().unwrap().reserved);
        c.0.lock().unwrap().as_mut().unwrap().intent = Intent::Copy;
        assert!(c.finish_version(&next, Some(&plan)).is_none());
        assert!(c.0.lock().unwrap().is_some());
    }
}
