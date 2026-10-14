use crate::{
    model::{AppError, AppResult},
    ssh, State,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    ipc::Channel, Emitter, Manager, State as TauriState, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

// All operations that touch both registries lock pending -> terminals -> stream.
// SSH workers only lock terminals -> stream and never consult this registry.
#[derive(Default)]
pub struct MoveState {
    pending: Mutex<HashMap<String, Transfer>>,
    drags: Mutex<HashMap<String, Drag>>,
}
#[derive(Clone)]
struct Transfer {
    from: String,
    to: String,
    payload: Value,
    before_id: Option<String>,
    session_id: Option<String>,
    claimed: bool,
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
        Err(AppError::new("permission", "此操作仅适用于会话窗口"))
    }
}

fn owned_terminal<'a>(
    terminals: &'a HashMap<String, ssh::TerminalHandle>,
    id: &str,
    owner: &str,
) -> AppResult<&'a ssh::TerminalHandle> {
    let terminal = terminals
        .get(id)
        .ok_or_else(|| AppError::new("disconnected", "SSH 会话已经结束"))?;
    if terminal.window != owner {
        return Err(AppError::new("permission", "此窗口无权访问该会话"));
    }
    Ok(terminal)
}

#[tauri::command]
pub fn terminal_freeze(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    token: String,
) -> AppResult<()> {
    workspace_only(&window)?;
    {
        let terminals = state.terminals.lock().unwrap();
        owned_terminal(&terminals, &session_id, window.label())?
            .stream
            .lock()
            .unwrap()
            .freeze(token.clone())?;
    }
    // A frontend that disappears between freeze and move registration must not
    // retain a paused PTY forever. The token distinguishes a later transfer.
    let state = state.inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(25));
        let pending = state.moves.pending.lock().unwrap();
        if pending
            .values()
            .any(|entry| entry.session_id.as_deref() == Some(&session_id))
        {
            return;
        }
        let mut terminals = state.terminals.lock().unwrap();
        if let Some(terminal) = terminals.get(&session_id) {
            let mut stream = terminal.stream.lock().unwrap();
            if stream.frozen_with(&token) {
                stream.rollback();
            }
            let remove = stream.closed && !stream.is_frozen();
            drop(stream);
            if remove {
                terminals.remove(&session_id);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn terminal_attach(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    output: Channel<ssh::TerminalEvent>,
) -> AppResult<()> {
    let pending = state.moves.pending.lock().unwrap();
    if !pending.values().any(|entry| {
        entry.claimed
            && entry.to == window.label()
            && entry.session_id.as_deref() == Some(&session_id)
    }) {
        return Err(AppError::new("permission", "此窗口没有等待接收的终端"));
    }
    let terminals = state.terminals.lock().unwrap();
    owned_terminal(&terminals, &session_id, window.label())?
        .stream
        .lock()
        .unwrap()
        .attach(output)?;
    Ok(())
}

#[tauri::command]
pub fn terminal_resume(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
) -> AppResult<()> {
    let pending = state.moves.pending.lock().unwrap();
    if pending
        .values()
        .any(|entry| entry.session_id.as_deref() == Some(&session_id))
    {
        return Err(AppError::new("busy", "会话正在移交，请先取消移动"));
    }
    let mut terminals = state.terminals.lock().unwrap();
    let terminal = owned_terminal(&terminals, &session_id, window.label())?;
    let mut stream = terminal.stream.lock().unwrap();
    stream.rollback();
    let closed = stream.closed;
    drop(stream);
    if closed {
        terminals.remove(&session_id);
    }
    Ok(())
}

#[tauri::command]
pub async fn move_workspace(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    payload: Value,
    target: Option<String>,
    before_id: Option<String>,
) -> AppResult<String> {
    workspace_only(&window)?;
    let tab_id = payload["tab"]["id"]
        .as_str()
        .ok_or_else(|| AppError::new("validation", "缺少会话标识"))?
        .to_owned();
    if payload.to_string().len() > 24 * 1024 * 1024 {
        return Err(AppError::new("limit", "终端快照过大，无法移动"));
    }
    let session_id = payload["terminal"]["sessionId"].as_str().map(str::to_owned);
    let id = uuid::Uuid::new_v4().to_string();
    let to = match target {
        Some(label) if label == "main" => label,
        Some(_) => return Err(AppError::new("validation", "只能移回主窗口")),
        None => format!("workspace-{id}"),
    };
    if to == window.label() {
        return Err(AppError::new("validation", "会话已经位于目标窗口"));
    }
    {
        let mut pending = state.moves.pending.lock().unwrap();
        if pending.values().any(|entry| {
            entry.payload["tab"]["id"].as_str() == Some(&tab_id)
                || (session_id.is_some() && entry.session_id == session_id)
        }) {
            return Err(AppError::new("busy", "会话正在移动"));
        }
        if let Some(session_id) = &session_id {
            let terminals = state.terminals.lock().unwrap();
            let terminal = owned_terminal(&terminals, session_id, window.label())?;
            if !terminal.stream.lock().unwrap().is_frozen() {
                return Err(AppError::new("transfer", "请先暂停终端输出再移动会话"));
            }
        }
        pending.insert(
            id.clone(),
            Transfer {
                from: window.label().into(),
                to: to.clone(),
                payload: payload.clone(),
                before_id,
                session_id,
                claimed: false,
            },
        );
    }
    // Native window operations may synchronously invoke lifecycle callbacks.
    // Never hold any registry lock while creating, focusing or destroying them.
    let result: tauri::Result<()> = if to == "main" {
        app.get_webview_window("main")
            .ok_or(tauri::Error::WindowNotFound)
            .and_then(|main| {
                app.emit_to("main", "workspace-arriving", &id)?;
                let _ = main.unminimize();
                let _ = main.set_focus();
                Ok(())
            })
    } else {
        let title = payload["tab"]["server"]["name"]
            .as_str()
            .unwrap_or("SSH 会话");
        WebviewWindowBuilder::new(
            &app,
            &to,
            WebviewUrl::App(format!("index.html?workspace={id}").into()),
        )
        .title(title)
        .inner_size(1080.0, 760.0)
        .min_inner_size(650.0, 480.0)
        .disable_drag_drop_handler()
        .build()
        .map(|_| ())
    };
    if let Err(error) = result {
        rollback(
            &app,
            &state,
            &id,
            "无法打开目标窗口，会话已留在原窗口",
            false,
        );
        return Err(AppError::new("window", error));
    }
    let deadline_id = id.clone();
    let state = state.inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        rollback(
            &app,
            &state,
            &deadline_id,
            "目标窗口未完成接收，会话已恢复",
            false,
        );
    });
    Ok(id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Incoming {
    payload: Value,
    before_id: Option<String>,
}

#[tauri::command]
pub fn take_workspace(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    transfer_id: String,
) -> AppResult<Incoming> {
    workspace_only(&window)?;
    let mut pending = state.moves.pending.lock().unwrap();
    let entry = pending
        .get_mut(&transfer_id)
        .ok_or_else(|| AppError::new("window", "会话移动已取消"))?;
    if entry.to != window.label() {
        return Err(AppError::new("permission", "此窗口无权接收会话"));
    }
    if entry.claimed {
        return Err(AppError::new("busy", "该会话已被接收"));
    }
    if let Some(id) = &entry.session_id {
        let mut terminals = state.terminals.lock().unwrap();
        let terminal = terminals
            .get_mut(id)
            .ok_or_else(|| AppError::new("disconnected", "会话已经结束，请返回原窗口"))?;
        if terminal.window != entry.from || !terminal.stream.lock().unwrap().is_frozen() {
            return Err(AppError::new("transfer", "会话归属已变化，请重试"));
        }
        terminal.window = entry.to.clone();
    }
    entry.claimed = true;
    Ok(Incoming {
        payload: entry.payload.clone(),
        before_id: entry.before_id.clone(),
    })
}

#[tauri::command]
pub fn workspace_ready(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    transfer_id: String,
) -> AppResult<()> {
    workspace_only(&window)?;
    // Commit under pending -> terminals -> stream, then notify the source.
    // The source must only unmount after the destination owns the live PTY.
    let (from, tab_id) = {
        let mut pending = state.moves.pending.lock().unwrap();
        let entry = pending
            .get(&transfer_id)
            .ok_or_else(|| AppError::new("window", "会话移动已取消"))?;
        if entry.to != window.label() || !entry.claimed {
            return Err(AppError::new("permission", "会话尚未由此窗口接收"));
        }
        let from = entry.from.clone();
        let tab_id = entry.payload["tab"]["id"].clone();
        let session_id = entry.session_id.clone();
        let mut terminals = state.terminals.lock().unwrap();
        let mut closed = false;
        if let Some(session_id) = &session_id {
            let terminal = owned_terminal(&terminals, session_id, window.label())?;
            let mut stream = terminal.stream.lock().unwrap();
            stream.commit()?;
            closed = stream.closed;
        }
        pending.remove(&transfer_id);
        if closed {
            if let Some(session_id) = &session_id {
                terminals.remove(session_id);
            }
        }
        (from, tab_id)
    };
    // A source destroyed during the transfer has nothing left to unmount.
    let _ = app.emit_to(
        &from,
        "workspace-moved",
        serde_json::json!({"tabId": tab_id, "transferId": transfer_id}),
    );
    Ok(())
}

#[tauri::command]
pub fn cancel_workspace_move(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    transfer_id: String,
) -> AppResult<()> {
    {
        let pending = state.moves.pending.lock().unwrap();
        if let Some(entry) = pending.get(&transfer_id) {
            if entry.from != window.label() && entry.to != window.label() {
                return Err(AppError::new("permission", "无权取消该操作"));
            }
        }
    }
    rollback(&app, &state, &transfer_id, "会话移动已取消", false);
    Ok(())
}

fn rollback(app: &tauri::AppHandle, state: &State, id: &str, message: &str, source_gone: bool) {
    let entry = {
        let mut pending = state.moves.pending.lock().unwrap();
        let Some(entry) = pending.remove(id) else {
            return;
        };
        if let Some(session_id) = &entry.session_id {
            let mut terminals = state.terminals.lock().unwrap();
            if let Some(terminal) = terminals.get_mut(session_id) {
                terminal.window = entry.from.clone();
                let mut stream = terminal.stream.lock().unwrap();
                if source_gone {
                    stream.abandon();
                    let _ = terminal.sender.send(ssh::Input::Close);
                } else {
                    stream.rollback();
                }
                let closed = stream.closed;
                drop(stream);
                if closed {
                    terminals.remove(session_id);
                }
            }
        }
        entry
    };
    let payload =
        serde_json::json!({"tabId":entry.payload["tab"]["id"], "transferId":id, "message":message});
    let _ = app.emit_to(&entry.from, "workspace-move-failed", &payload);
    let _ = app.emit_to(&entry.to, "workspace-move-failed", &payload);
    if entry.to.starts_with("workspace-") {
        if let Some(target) = app.get_webview_window(&entry.to) {
            let _ = target.destroy();
        }
    }
}

pub fn window_unavailable(app: &tauri::AppHandle, state: &State, label: &str, reloading: bool) {
    let transfers: Vec<_> = state
        .moves
        .pending
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, entry)| {
            entry.from == label || (entry.to == label && (!reloading || entry.claimed))
        })
        .map(|(id, entry)| (id.clone(), entry.from == label))
        .collect();
    for (id, source_gone) in transfers {
        rollback(
            app,
            state,
            &id,
            "窗口已关闭或刷新，会话移动已取消",
            source_gone,
        );
    }
    state
        .moves
        .drags
        .lock()
        .unwrap()
        .retain(|_, drag| drag.source != label);
    let mut terminals = state.terminals.lock().unwrap();
    terminals.retain(|_, terminal| {
        if terminal.window != label {
            return true;
        }
        let mut stream = terminal.stream.lock().unwrap();
        stream.abandon();
        let _ = terminal.sender.send(ssh::Input::Close);
        !stream.closed
    });
}

pub fn window_closed(app: &tauri::AppHandle, state: &State, label: &str) {
    window_unavailable(app, state, label, false);
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
        if cancelled || drag.claimed || drag.source != "main" {
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
        if !drag.claimed && outside {
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
