use crate::{
    connection,
    model::{AppError, AppResult},
    terminal, State,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    ipc::Channel, Emitter, Manager, State as TauriState, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
mod transfer;
pub use transfer::*;

// Lock order: workspace registry -> pending -> terminals -> sorted streams.
#[derive(Default)]
pub struct MoveState {
    pending: Mutex<HashMap<String, Transfer>>,
    completed: Mutex<HashMap<String, (connection::WindowOwner, String)>>,
    drags: Mutex<HashMap<String, Drag>>,
}
impl MoveState {
    pub fn has_server(&self, id: &str) -> bool {
        self.pending
            .lock()
            .unwrap()
            .values()
            .any(|t| t.server_id == id)
    }
}
struct Transfer {
    workspace_id: String,
    server_id: String,
    from: String,
    from_generation: u64,
    owner_epoch: u64,
    to: String,
    to_generation: Option<u64>,
    payload: Value,
    before_id: Option<String>,
    members: Vec<(String, Option<String>)>,
    sessions: Vec<String>,
    acknowledgements: HashSet<String>,
    claimed: bool,
    target_context: Option<crate::workspaces::ToolContextRef>,
}
#[derive(Clone)]
struct Drag {
    source: String,
    tab_id: String,
    claimed: bool,
    finishing: bool,
    started: Instant,
}
pub fn workspace_only(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == "main" || window.label().starts_with("workspace-") {
        Ok(())
    } else {
        Err(AppError::new(
            "permission",
            "此操作仅适用于服务器工作区窗口",
        ))
    }
}
#[tauri::command]
pub fn request_workspace_connection(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    server_id: String,
    request_id: String,
) -> AppResult<()> {
    crate::workspaces::open_server_workspace(window, app, state, server_id, true, request_id)
        .map(|_| ())
}
fn owned_terminal<'a>(
    terminals: &'a HashMap<String, connection::ConnectionHandle>,
    id: &str,
    owner: &str,
) -> AppResult<&'a connection::ConnectionHandle> {
    let terminal = terminals
        .get(id)
        .ok_or_else(|| AppError::new("disconnected", "SSH 会话已经结束"))?;
    if terminal.window != owner {
        return Err(AppError::new("permission", "此窗口无权访问该会话"));
    }
    if terminal.spec.is_none() {
        return Err(AppError::new("connecting", "连接中无法移动会话"));
    }
    Ok(terminal)
}

#[tauri::command]
pub fn begin_tab_drag(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    token: String,
    tab_id: String,
) -> AppResult<()> {
    workspace_only(&window)?;
    let mut drags = state.moves.drags.lock().unwrap();
    drags.retain(|_, drag| drag.started.elapsed() < Duration::from_secs(120));
    if drags.contains_key(&token) {
        return Err(AppError::new("busy", "拖拽标识已在使用"));
    }
    drags.insert(
        token,
        Drag {
            source: window.label().into(),
            tab_id,
            claimed: false,
            finishing: false,
            started: Instant::now(),
        },
    );
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveRequest {
    tab_id: String,
    target: Option<String>,
    before_id: Option<String>,
}

#[tauri::command]
pub fn drop_tab(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    token: String,
    before_id: Option<String>,
) -> AppResult<()> {
    workspace_only(&window)?;
    let mut drags = state.moves.drags.lock().unwrap();
    let drag = drags
        .get_mut(&token)
        .ok_or_else(|| AppError::new("validation", "拖拽已失效，请重试"))?;
    if drag.claimed {
        return Ok(());
    }
    if window.label() != drag.source && window.label() != "main" {
        return Err(AppError::new("validation", "请将会话拖回主窗口标签栏"));
    }
    app.emit_to(
        &drag.source,
        "tab-drop",
        MoveRequest {
            tab_id: drag.tab_id.clone(),
            target: Some(window.label().into()),
            before_id,
        },
    )
    .map_err(|error| AppError::new("window", error))?;
    drag.claimed = true;
    Ok(())
}

#[tauri::command]
pub fn finish_tab_drag(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    token: String,
    cancelled: bool,
) -> AppResult<()> {
    workspace_only(&window)?;
    {
        let mut drags = state.moves.drags.lock().unwrap();
        let Some(drag) = drags.get_mut(&token) else {
            return Ok(());
        };
        if drag.source != window.label() {
            return Err(AppError::new("permission", "无权结束拖拽"));
        }
        if cancelled || drag.claimed {
            drags.remove(&token);
            return Ok(());
        }
        if drag.finishing {
            return Ok(());
        }
        drag.finishing = true;
    }
    // dragend and drop IPC arrive from different WebViews. Leave a claim window
    // so a slightly later native drop always wins over tear-off detection.
    let state = state.inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let outside = (|| -> tauri::Result<bool> {
            let cursor = app.cursor_position()?;
            let position = window.outer_position()?;
            let size = window.outer_size()?;
            Ok(cursor.x < f64::from(position.x)
                || cursor.x > f64::from(position.x) + f64::from(size.width)
                || cursor.y < f64::from(position.y)
                || cursor.y > f64::from(position.y) + f64::from(size.height))
        })()
        .unwrap_or(false);
        let mut drags = state.moves.drags.lock().unwrap();
        let Some(drag) = drags.remove(&token) else {
            return;
        };
        if !drag.claimed && outside && drag.source == "main" {
            let _ = app.emit_to(
                &drag.source,
                "tab-drop",
                MoveRequest {
                    tab_id: drag.tab_id,
                    target: None,
                    before_id: None,
                },
            );
        }
    });
    Ok(())
}
