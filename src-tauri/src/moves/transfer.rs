use super::*;
use crate::workspaces::{new_id, stale, ToolContextRef};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedMove {
    transfer_id: String,
    workspace: crate::workspaces::WorkspaceView,
}

#[tauri::command]
pub fn prepare_workspace_move(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    workspace_id: String,
    owner_epoch: u64,
) -> AppResult<PreparedMove> {
    workspace_only(&window)?;
    let gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    let owner = state.owner(window.label());
    let server = {
        let mut r = state.workspaces.registry.lock().unwrap();
        r.owned_mut(&workspace_id, &owner, owner_epoch)?
            .view
            .server
            .id
            .clone()
    };
    if let Some(message) = crate::workspaces::blockers(&state, &server).first() {
        return Err(AppError::new("busy", message));
    }
    let id = new_id();
    let view = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let w = r.owned_mut(&workspace_id, &owner, owner_epoch)?;
        w.ensure_idle()?;
        if w.verification.is_some()
            || w.view
                .terminals
                .iter()
                .any(|t| ["queued", "connecting", "authenticating"].contains(&t.state.as_str()))
        {
            return Err(AppError::new("busy", "请先完成或取消连接和工具验证"));
        }
        let members: Vec<_> = w
            .view
            .terminals
            .iter()
            .map(|t| (t.id.clone(), t.connection_id.clone()))
            .collect();
        let mut sessions: Vec<_> = members.iter().filter_map(|(_, id)| id.clone()).collect();
        sessions.sort();
        let target_context = w.view.tools.context.as_ref().map(|c| ToolContextRef {
            id: format!("{}.{}", new_id(), c.epoch),
            epoch: c.epoch,
        });
        state.moves.pending.lock().unwrap().insert(
            id.clone(),
            Transfer {
                workspace_id: workspace_id.clone(),
                server_id: server,
                from: owner.label,
                from_generation: owner.generation,
                owner_epoch,
                to: String::new(),
                to_generation: None,
                payload: Value::Null,
                before_id: None,
                members,
                sessions,
                acknowledgements: HashSet::new(),
                claimed: false,
                target_context,
            },
        );
        w.view.lifecycle = "moving".into();
        r.changed(&workspace_id);
        r.entries[&workspace_id].view.clone()
    };
    drop(gate);
    state.workspaces.publish();
    let state = state.inner().clone();
    let deadline = id.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(25));
        rollback(
            &app,
            &state,
            &deadline,
            "目标窗口未完成接收，工作区已恢复",
            false,
        );
    });
    Ok(PreparedMove {
        transfer_id: id,
        workspace: view,
    })
}

#[tauri::command]
pub fn terminal_freeze(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    token: String,
) -> AppResult<()> {
    workspace_only(&window)?;
    let pending = state.moves.pending.lock().unwrap();
    if !pending
        .values()
        .any(|t| t.from == window.label() && t.sessions.contains(&session_id) && !t.claimed)
    {
        return Err(stale());
    }
    let terminals = state.terminals.lock().unwrap();
    let result = owned_terminal(&terminals, &session_id, window.label())?
        .stream
        .lock()
        .unwrap()
        .freeze(token);
    result
}
#[tauri::command]
pub fn terminal_attach(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    output: Channel<connection::ConnectionEvent>,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let pending = state.moves.pending.lock().unwrap();
    let (id, entry) = pending
        .iter()
        .find(|(_, t)| t.claimed && t.to == window.label() && t.sessions.contains(&session_id))
        .ok_or_else(stale)?;
    if entry.to_generation != Some(owner.generation) {
        return Err(stale());
    }
    let terminals = state.terminals.lock().unwrap();
    let mut stream = owned_terminal(&terminals, &session_id, &entry.from)?
        .stream
        .lock()
        .unwrap();
    stream.attach(output)?;
    stream.reception_barrier(id.clone(), session_id)
}
#[tauri::command]
pub fn acknowledge_workspace_terminal(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    transfer_id: String,
    terminal_id: String,
) -> AppResult<()> {
    let owner = state.owner(window.label());
    let mut pending = state.moves.pending.lock().unwrap();
    let t = pending.get_mut(&transfer_id).ok_or_else(stale)?;
    if !t.claimed
        || t.to != window.label()
        || t.to_generation != Some(owner.generation)
        || !t.members.iter().any(|(id, _)| id == &terminal_id)
    {
        return Err(stale());
    }
    t.acknowledgements.insert(terminal_id);
    Ok(())
}
#[tauri::command]
pub fn terminal_resume(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
) -> AppResult<()> {
    let pending = state.moves.pending.lock().unwrap();
    if pending.values().any(|t| t.sessions.contains(&session_id)) {
        return Err(AppError::new("busy", "请先取消工作区移动"));
    }
    let mut terminals = state.terminals.lock().unwrap();
    let Some(t) = terminals.get(&session_id) else {
        return Ok(());
    };
    if t.window != window.label() {
        return Err(stale());
    }
    let mut stream = t.stream.lock().unwrap();
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
    if payload["version"].as_u64() != Some(2) {
        return Err(AppError::new("version", "不支持的工作区快照版本"));
    }
    if payload.to_string().len() > 24 * 1024 * 1024 {
        return Err(AppError::new("limit", "工作区总快照超过 24 MiB，无法移动"));
    }
    let id = payload["transferId"].as_str().ok_or_else(stale)?.to_owned();
    let to = match target {
        Some(label) if label == "main" => label,
        None => format!("workspace-{id}"),
        _ => return Err(AppError::new("validation", "只能移回主窗口")),
    };
    if to == window.label() {
        return Err(AppError::new("validation", "工作区已位于目标窗口"));
    }
    let owner = state.owner(window.label());
    {
        let r = state.workspaces.registry.lock().unwrap();
        let mut pending = state.moves.pending.lock().unwrap();
        let t = pending.get_mut(&id).ok_or_else(stale)?;
        let w = r.entries.get(&t.workspace_id).ok_or_else(stale)?;
        if t.from != window.label()
            || t.from_generation != owner.generation
            || t.owner_epoch != w.view.owner.owner_epoch
            || !t.to.is_empty()
            || w.view.lifecycle != "moving"
            || payload["tab"]["id"].as_str() != Some(&t.workspace_id)
        {
            return Err(stale());
        }
        let entries = payload["terminals"].as_array().ok_or_else(stale)?;
        let mut members = Vec::new();
        for entry in entries {
            members.push((
                entry["id"].as_str().ok_or_else(stale)?.to_owned(),
                entry["terminal"]["sessionId"].as_str().map(str::to_owned),
            ));
        }
        members.sort();
        let mut expected = t.members.clone();
        expected.sort();
        if members != expected {
            return Err(AppError::new("transfer", "终端成员集合已变化，无法移动"));
        }
        let terminals = state.terminals.lock().unwrap();
        for session in &t.sessions {
            if !owned_terminal(&terminals, session, &t.from)?
                .stream
                .lock()
                .unwrap()
                .is_frozen()
            {
                return Err(AppError::new("transfer", "终端尚未全部冻结"));
            }
        }
        t.to = to.clone();
        t.payload = payload;
        t.before_id = before_id;
    }
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
        crate::window_chrome::configure(
            &app,
            WebviewWindowBuilder::new(
                &app,
                &to,
                WebviewUrl::App(format!("index.html?workspace={id}").into()),
            ),
        )
        .title(crate::brand::window_title(&app, "服务器工作区"))
        .inner_size(1080.0, 760.0)
        .min_inner_size(650.0, 480.0)
        .build()
        .map(|_| ())
    };
    if let Err(error) = result {
        rollback(&app, &state, &id, "无法打开目标窗口，工作区已恢复", false);
        return Err(AppError::new("window", error));
    }
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
    let owner = state.owner(window.label());
    let r = state.workspaces.registry.lock().unwrap();
    let mut pending = state.moves.pending.lock().unwrap();
    let t = pending.get_mut(&transfer_id).ok_or_else(stale)?;
    if t.to != window.label() || t.claimed {
        return Err(stale());
    }
    let w = r.entries.get(&t.workspace_id).ok_or_else(stale)?;
    let mut view = w.view.clone();
    view.owner.label = owner.label;
    view.owner.generation = owner.generation;
    view.owner.owner_epoch += 1;
    view.tools.context = t.target_context.clone();
    t.payload["tab"] = serde_json::to_value(view).map_err(|e| AppError::new("serialization", e))?;
    // Snapshot caches are permitted only for the same verified route. This is
    // an ownership rebind, not a host change; install the new capability keys.
    if let Some(context) = &t.target_context {
        if let Some(files) = t.payload.get_mut("files").filter(|v| v.is_object()) {
            files["contextId"] = serde_json::json!(context.id);
        }
    }
    t.to_generation = Some(owner.generation);
    t.claimed = true;
    Ok(Incoming {
        payload: t.payload.clone(),
        before_id: t.before_id.clone(),
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
    let target_owner = state.owner(window.label());
    if let Some((owner, _)) = state.moves.completed.lock().unwrap().get(&transfer_id) {
        return if owner.label == target_owner.label && owner.generation == target_owner.generation {
            Ok(())
        } else {
            Err(stale())
        };
    }
    let (from, workspace_id, old_context, new_context) = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let mut pending = state.moves.pending.lock().unwrap();
        let t = pending.get(&transfer_id).ok_or_else(stale)?;
        if t.to != target_owner.label
            || t.to_generation != Some(target_owner.generation)
            || !t.claimed
            || t.acknowledgements.len() != t.members.len()
        {
            return Err(AppError::new("transfer", "目标窗口尚未消费全部终端输出"));
        }
        let w = r.entries.get_mut(&t.workspace_id).ok_or_else(stale)?;
        if w.view.owner.owner_epoch != t.owner_epoch || w.view.lifecycle != "moving" {
            return Err(stale());
        }
        let mut terminals = state.terminals.lock().unwrap();
        // Every stream lock remains held from preflight through the last commit.
        let streams: Vec<_> = t
            .sessions
            .iter()
            .map(|id| owned_terminal(&terminals, id, &t.from).map(|h| h.stream.clone()))
            .collect::<AppResult<_>>()?;
        let mut guards: Vec<_> = streams.iter().map(|s| s.lock().unwrap()).collect();
        commit_streams(&mut guards)?;
        let closed: Vec<_> = t
            .sessions
            .iter()
            .zip(&guards)
            .filter(|(_, s)| s.closed)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &t.sessions {
            terminals.get_mut(id).unwrap().window = t.to.clone();
        }
        for id in &closed {
            terminals.remove(id);
        }
        for terminal in &mut w.view.terminals {
            if terminal
                .connection_id
                .as_ref()
                .is_some_and(|id| closed.contains(id))
            {
                terminal.connection_id = None;
                terminal.state = "disconnected".into();
            }
        }
        let old_context = w.view.tools.context.clone();
        let new_context = t.target_context.clone();
        w.view.tools.context = t.target_context.clone();
        w.view.owner.label = target_owner.label.clone();
        w.view.owner.generation = target_owner.generation;
        w.view.owner.owner_epoch += 1;
        w.view.lifecycle = "active".into();
        let from = t.from.clone();
        let workspace_id = t.workspace_id.clone();
        drop(guards);
        drop(terminals);
        r.changed(&workspace_id);
        pending.remove(&transfer_id);
        let mut completed = state.moves.completed.lock().unwrap();
        if completed.len() > 4096 {
            completed.clear();
        }
        completed.insert(transfer_id.clone(), (target_owner, workspace_id.clone()));
        (from, workspace_id, old_context, new_context)
    };
    if let Some(c) = old_context {
        if let Some(new) = new_context {
            state.monitor.rebind_context(&c.id, &new.id);
        }
        state.netbird.close_session(&c.id);
    }
    state.workspaces.publish();
    let _ = app.emit_to(
        &from,
        "workspace-moved",
        serde_json::json!({"tabId":workspace_id,"transferId":transfer_id}),
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
        if let Some(t) = pending.get(&transfer_id) {
            if t.from != window.label() && t.to != window.label() {
                return Err(stale());
            }
        }
    }
    rollback(&app, &state, &transfer_id, "工作区移动已取消", false);
    Ok(())
}
fn rollback(app: &tauri::AppHandle, state: &State, id: &str, message: &str, source_gone: bool) {
    let (entry, contexts) = {
        let mut r = state.workspaces.registry.lock().unwrap();
        let mut pending = state.moves.pending.lock().unwrap();
        let Some(entry) = pending.remove(id) else {
            return;
        };
        let mut terminals = state.terminals.lock().unwrap();
        let mut closed = vec![];
        for session in &entry.sessions {
            if let Some(t) = terminals.get_mut(session) {
                t.window = entry.from.clone();
                let mut stream = t.stream.lock().unwrap();
                if source_gone {
                    stream.abandon();
                    let _ = t.sender.send(terminal::Input::Close);
                } else {
                    stream.rollback();
                }
                if stream.closed {
                    closed.push(session.clone());
                }
            } else {
                closed.push(session.clone());
            }
        }
        for session in &closed {
            terminals.remove(session);
        }
        let mut contexts = None;
        if let Some(w) = r.entries.get_mut(&entry.workspace_id) {
            w.view.lifecycle = "active".into();
            w.view.owner.owner_epoch += 1;
            for t in &mut w.view.terminals {
                if t.connection_id
                    .as_ref()
                    .is_some_and(|id| closed.contains(id))
                {
                    t.connection_id = None;
                    t.state = "disconnected".into();
                }
            }
            // Revoke stale capabilities even when the same source label returns.
            if let Some(c) = &mut w.view.tools.context {
                let old = c.id.clone();
                c.id = format!("{}.{}", new_id(), c.epoch);
                contexts = Some((old, c.id.clone()));
            }
        }
        r.changed(&entry.workspace_id);
        (entry, contexts)
    };
    if let Some((old, new)) = contexts {
        state.monitor.rebind_context(&old, &new);
        state.netbird.close_session(&old);
    }
    if source_gone {
        crate::workspaces::close_window(state, &entry.from);
    }
    state.workspaces.publish();
    let payload = serde_json::json!({"tabId":entry.workspace_id,"transferId":id,"message":message});
    let _ = app.emit_to(&entry.from, "workspace-move-failed", &payload);
    if !entry.to.is_empty() {
        let _ = app.emit_to(&entry.to, "workspace-move-failed", &payload);
    }
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
        .filter(|(_, t)| t.from == label || (t.to == label && (!reloading || t.claimed)))
        .map(|(id, t)| (id.clone(), t.from == label))
        .collect();
    for (id, source_gone) in transfers {
        rollback(
            app,
            state,
            &id,
            "窗口已关闭或刷新，工作区移动已取消",
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
    terminals.retain(|_, t| {
        if t.window != label {
            return true;
        }
        let mut s = t.stream.lock().unwrap();
        s.abandon();
        let _ = t.sender.send(terminal::Input::Close);
        !s.closed
    });
}
pub fn window_closed(app: &tauri::AppHandle, state: &State, label: &str) {
    window_unavailable(app, state, label, false);
}

/// Keep every stream locked throughout preflight and commit. A missing receiver
/// must fail the entire set before any stream releases its source fallback.
fn commit_streams(
    streams: &mut [std::sync::MutexGuard<'_, terminal::TerminalStream>],
) -> AppResult<()> {
    for stream in streams.iter() {
        stream.ready_to_commit()?;
    }
    for stream in streams.iter_mut() {
        stream.commit()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sink() -> (Channel<connection::ConnectionEvent>, Arc<Mutex<Vec<Value>>>) {
        let events = Arc::new(Mutex::new(vec![]));
        let received = events.clone();
        (
            Channel::new(move |body| {
                if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                    received
                        .lock()
                        .unwrap()
                        .push(serde_json::from_str(&json).unwrap());
                }
                Ok(())
            }),
            events,
        )
    }
    #[test]
    fn failed_nth_receiver_never_partially_commits_and_all_sources_replay() {
        for failed in 0..3 {
            let mut sources = vec![];
            let mut streams = vec![];
            for i in 0..3 {
                let (source, received) = sink();
                sources.push(received);
                let mut stream = terminal::TerminalStream::new(source);
                stream.freeze(format!("freeze-{i}")).unwrap();
                stream.send(connection::ConnectionEvent::Data {
                    data: format!("output-{i}"),
                });
                if i != failed {
                    stream.attach(sink().0).unwrap();
                }
                streams.push(Mutex::new(stream));
            }
            let mut guards: Vec<_> = streams.iter().map(|s| s.lock().unwrap()).collect();
            assert!(commit_streams(&mut guards).is_err());
            assert!(guards.iter().all(|s| s.is_frozen()));
            for s in &mut guards {
                s.rollback();
            }
            for (i, source) in sources.iter().enumerate() {
                let output: Vec<_> = source
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|e| e["type"] == "data")
                    .map(|e| e["data"].as_str().unwrap().to_string())
                    .collect();
                assert_eq!(output, vec![format!("output-{i}")]);
            }
        }
    }
    #[test]
    fn collective_commit_preserves_closed_stream_and_reception_order() {
        let mut streams = vec![];
        for i in 0..3 {
            let mut stream = terminal::TerminalStream::new(sink().0);
            stream.freeze(format!("freeze-{i}")).unwrap();
            stream.send(connection::ConnectionEvent::Data {
                data: "pending-output".into(),
            });
            if i == 1 {
                stream.send(connection::ConnectionEvent::Closed {
                    message: "ended".into(),
                    reason: "ended".into(),
                    recovery: connection::RecoveryAction::None,
                    error: None,
                });
            }
            let (target, events) = sink();
            stream.attach(target).unwrap();
            stream
                .reception_barrier("transfer".into(), format!("session-{i}"))
                .unwrap();
            assert_eq!(
                events.lock().unwrap().last().unwrap()["type"],
                "receptionBarrier"
            );
            streams.push(Mutex::new(stream));
        }
        let mut guards: Vec<_> = streams.iter().map(|s| s.lock().unwrap()).collect();
        commit_streams(&mut guards).unwrap();
        assert!(guards.iter().all(|s| !s.is_frozen()));
        assert!(guards[1].closed);
        commit_streams(&mut []).unwrap();
    }
}

#[tauri::command]
pub fn workspace_move_status(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    transfer_id: String,
) -> AppResult<String> {
    workspace_only(&window)?;
    let caller = state.owner(window.label());
    if let Some((owner, _)) = state.moves.completed.lock().unwrap().get(&transfer_id) {
        return if owner.label == caller.label && owner.generation == caller.generation {
            Ok("committed".into())
        } else {
            Err(stale())
        };
    }
    if let Some(t) = state.moves.pending.lock().unwrap().get(&transfer_id) {
        if !(t.from == caller.label && t.from_generation == caller.generation)
            && !(t.to == caller.label && t.to_generation == Some(caller.generation))
        {
            return Err(stale());
        }
        return Ok("pending".into());
    }
    Ok("gone".into())
}
