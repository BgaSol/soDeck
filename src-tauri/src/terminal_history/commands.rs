use super::{model::*, store};
use crate::{
    model::{AppError, AppResult},
    State,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn root(window: &WebviewWindow, state: &State) -> AppResult<PathBuf> {
    if !(window.label() == "main"
        || window.label() == "terminal-history"
        || window.label().starts_with("workspace-")
        || window.label().starts_with("container-"))
    {
        return Err(AppError::new("permission", "此窗口不能访问终端历史"));
    }
    if let Some(error) = &state.history.startup_error {
        return Err(AppError::new("history", error));
    }
    state
        .history
        .root
        .clone()
        .ok_or_else(|| AppError::new("history", "历史服务未初始化"))
}
fn query_key(window: &str, id: &str) -> String {
    format!("{window}:{id}")
}
fn start_query(state: &State, window: &str, id: Option<&str>) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(id) = id {
        if let Some(old) = state
            .history
            .queries
            .lock()
            .unwrap()
            .insert(query_key(window, id), cancel.clone())
        {
            old.store(true, Ordering::Relaxed);
        }
    }
    cancel
}
fn end_query(state: &State, window: &str, id: Option<&str>, cancel: &Arc<AtomicBool>) {
    if let Some(id) = id {
        let mut queries = state.history.queries.lock().unwrap();
        let key = query_key(window, id);
        if queries.get(&key).is_some_and(|c| Arc::ptr_eq(c, cancel)) {
            queries.remove(&key);
        }
    }
}
#[tauri::command]
pub async fn terminal_history_sessions(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    query: Query,
) -> AppResult<Vec<Session>> {
    let root = root(&window, &state)?;
    crate::blocking(move || store::sessions(&root, &query)).await
}
#[tauri::command]
pub async fn terminal_history_status(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
) -> AppResult<Session> {
    root(&window, &state)?;
    let state = state.inner().clone();
    crate::blocking(move || state.history.status(&session_id)).await
}
#[tauri::command]
pub async fn terminal_history_page(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    query: Query,
) -> AppResult<Page> {
    let root = root(&window, &state)?;
    let state = state.inner().clone();
    let label = window.label().to_owned();
    let cancel = start_query(&state, &label, query.request_id.as_deref());
    crate::blocking(move || {
        let result = query
            .session_id
            .as_deref()
            .ok_or_else(|| AppError::new("history", "请选择历史会话"))
            .and_then(|id| store::page(&root, id, &query, &cancel));
        end_query(&state, &label, query.request_id.as_deref(), &cancel);
        result
    })
    .await
}
#[derive(Serialize, Deserialize)]
struct SearchSnapshot {
    sessions: Vec<(String, String)>,
}
#[tauri::command]
pub async fn terminal_history_search(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    query: Query,
    cursor: Option<String>,
) -> AppResult<SearchPage> {
    let root = root(&window, &state)?;
    let state = state.inner().clone();
    let label = window.label().to_owned();
    let cancel = start_query(&state, &label, query.request_id.as_deref());
    crate::blocking(move || {
        let result = (|| {
            if query.keyword.chars().count() > 4096 {
                return Err(AppError::new("history", "搜索内容过长"));
            }
            let snapshot: SearchSnapshot = if let Some(s) = &query.snapshot {
                serde_json::from_str(s).map_err(|_| AppError::new("history", "搜索快照已失效"))?
            } else {
                SearchSnapshot {
                    sessions: store::sessions(&root, &query)?
                        .into_iter()
                        .map(|s| (s.id, s.last_record))
                        .collect(),
                }
            };
            let (mut index, mut after): (usize, String) = cursor
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|_| AppError::new("history", "搜索位置已失效"))?
                .unwrap_or((0, "0".into()));
            let limit = query.limit.clamp(1, 200);
            let mut hits = Vec::new();
            let mut bytes = 0;
            while index < snapshot.sessions.len() && hits.len() < limit {
                if cancel.load(Ordering::Relaxed) {
                    return Err(AppError::new("cancelled", "查询已取消"));
                }
                let (id, max) = &snapshot.sessions[index];
                let page = store::page(
                    &root,
                    id,
                    &Query {
                        after: Some(after.clone()),
                        before: None,
                        snapshot: Some(max.clone()),
                        limit: limit - hits.len(),
                        ..query.clone()
                    },
                    &cancel,
                )?;
                let session = store::get_session(&root, id)?;
                let count = page.rows.len();
                let mut consumed = 0;
                for row in page.rows {
                    let size = row.text.len() + row.ansi.len() + 1024;
                    if !hits.is_empty() && bytes + size > 4 * 1024 * 1024 {
                        break;
                    }
                    bytes += size;
                    consumed += 1;
                    after = row.id.clone();
                    hits.push(SearchHit {
                        session: session.clone(),
                        row,
                    });
                }
                if consumed < count {
                    break;
                }
                if !page.has_more {
                    index += 1;
                    after = "0".into();
                }
            }
            Ok(SearchPage {
                hits,
                cursor: (index < snapshot.sessions.len())
                    .then(|| serde_json::to_string(&(index, after)).unwrap()),
                snapshot: serde_json::to_string(&snapshot).unwrap(),
            })
        })();
        end_query(&state, &label, query.request_id.as_deref(), &cancel);
        result
    })
    .await
}
#[tauri::command]
pub fn terminal_history_cancel(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    request_id: String,
) {
    if let Some(cancel) = state
        .history
        .queries
        .lock()
        .unwrap()
        .remove(&query_key(window.label(), &request_id))
    {
        cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayEvent {
    seq: String,
    time: i64,
    kind: String,
    data: String,
    cols: u16,
    rows: u16,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPage {
    events: Vec<ReplayEvent>,
    after: String,
    has_more: bool,
}
#[tauri::command]
pub async fn terminal_history_replay(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    after: String,
    through: Option<String>,
    offset: Option<usize>,
) -> AppResult<ReplayPage> {
    let root = root(&window, &state)?;
    crate::blocking(move || {
        let session = store::get_session(&root, &session_id)?;
        let after = store::parse_seq(Some(&after), 0)?;
        let through = store::parse_seq(through.as_deref(), session.saved_seq.parse().unwrap_or(0))?;
        let input = store::read_events(&root, &session_id, after, 128)?;
        let mut cursor = after;
        let mut events = Vec::new();
        for event in input.into_iter().take_while(|e| e.seq <= through) {
            cursor = event.seq;
            let mut view = ReplayEvent {
                seq: event.seq.to_string(),
                time: event.time,
                kind: String::new(),
                data: String::new(),
                cols: 0,
                rows: 0,
            };
            match event.kind {
                EventKind::Output { mut bytes } => {
                    if cursor == through {
                        if let Some(offset) = offset {
                            bytes.truncate(offset.saturating_add(1));
                        }
                    }
                    view.kind = "output".into();
                    view.data = STANDARD.encode(bytes);
                }
                EventKind::Start { cols, rows } | EventKind::Resize { cols, rows } => {
                    view.kind = "resize".into();
                    view.cols = cols;
                    view.rows = rows;
                }
                EventKind::Clear => view.kind = "clear".into(),
                EventKind::Gap { reason } => {
                    view.kind = "gap".into();
                    view.data = reason;
                }
                _ => {}
            }
            if !view.kind.is_empty() {
                events.push(view);
            }
        }
        Ok(ReplayPage {
            events,
            after: cursor.to_string(),
            has_more: cursor < through && cursor > after,
        })
    })
    .await
}
#[tauri::command]
pub async fn terminal_history_copy(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    from: String,
    to: String,
) -> AppResult<String> {
    let root = root(&window, &state)?;
    crate::blocking(move || {
        let from = store::parse_seq(Some(&from), 1)?;
        let to = store::parse_seq(Some(&to), i64::MAX)?;
        let mut out = Vec::new();
        export_text(
            &root,
            &session_id,
            from,
            to,
            &mut out,
            Some(8 * 1024 * 1024),
        )?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    })
    .await
}
fn export_text(
    root: &std::path::Path,
    id: &str,
    from: i64,
    to: i64,
    out: &mut dyn Write,
    max: Option<usize>,
) -> AppResult<()> {
    let snapshot = store::get_session(root, id)?.last_record;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut after = from.saturating_sub(1);
    let mut size = 0;
    loop {
        let page = store::page(
            root,
            id,
            &Query {
                after: Some(after.to_string()),
                before: Some(to.saturating_add(1).to_string()),
                snapshot: Some(snapshot.clone()),
                limit: 500,
                ..Default::default()
            },
            &cancel,
        )?;
        for row in page.rows {
            size += row.text.len() + 1;
            if max.is_some_and(|max| size > max) {
                return Err(AppError::new("limit", "复制内容超过 8 MiB，请使用导出"));
            }
            out.write_all(row.text.as_bytes())?;
            out.write_all(b"\n")?;
            after = row.id.parse().unwrap();
        }
        if !page.has_more {
            break;
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn terminal_history_export(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    raw: bool,
    from: Option<String>,
    to: Option<String>,
) -> AppResult<Option<String>> {
    let root = root(&window, &state)?;
    let path = rfd::AsyncFileDialog::new()
        .set_file_name(if raw {
            "terminal-recording.jsonl"
        } else {
            "terminal-history.txt"
        })
        .save_file()
        .await;
    let Some(path) = path else { return Ok(None) };
    let path = path.path().to_owned();
    crate::blocking(move||{
        store::get_session(&root,&session_id)?;
        // Write beside the destination, then replace only when the complete export is ready.
        let mut file=tempfile::NamedTempFile::new_in(path.parent().ok_or_else(||AppError::new("history","无效的导出位置"))?)?;
        if raw {
            let session=store::get_session(&root,&session_id)?;let max=session.saved_seq.parse::<i64>().unwrap_or(0);let mut cursor=0;
            writeln!(file,"{}",serde_json::to_string(&serde_json::json!({"format":"ssh-workspace-terminal-v1","session":session})).unwrap())?;
            loop{let events=store::read_events(&root,&session_id,cursor,128)?;if events.is_empty(){break;}for e in events.into_iter().take_while(|e|e.seq<=max){cursor=e.seq;let event=match e.kind{EventKind::Output{bytes}=>serde_json::json!({"type":"output","base64":STANDARD.encode(bytes)}),kind=>serde_json::to_value(kind).unwrap()};writeln!(file,"{}",serde_json::json!({"seq":e.seq.to_string(),"time":e.time,"elapsed":e.elapsed,"event":event}))?;}if cursor>=max{break;}}
        }else{export_text(&root,&session_id,store::parse_seq(from.as_deref(),1)?,store::parse_seq(to.as_deref(),i64::MAX-1)?,file.as_file_mut(),None)?;}
        file.as_file_mut().sync_all()?;file.persist(&path).map_err(|e|AppError::new("history",e))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    }).await
}
#[tauri::command]
pub async fn open_terminal_history_window(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<()> {
    root(&window, &state)?;
    if let Some(window) = app.get_webview_window("terminal-history") {
        window.show().map_err(|e| AppError::new("window", e))?;
        window.set_focus().map_err(|e| AppError::new("window", e))?;
        return Ok(());
    }
    crate::window_chrome::configure(WebviewWindowBuilder::new(
        &app,
        "terminal-history",
        WebviewUrl::App("index.html?terminal-history=1".into()),
    ))
    .title(crate::brand::window_title(&app, "终端历史"))
    .inner_size(1100.0, 760.0)
    .min_inner_size(640.0, 420.0)
    .build()
    .map_err(|e| AppError::new("window", e))?;
    Ok(())
}
