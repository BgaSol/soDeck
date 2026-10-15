//! Native file drops are granted in Rust. Internal drags carry only a short-lived
//! capability and use native pointer state, so enabling OS drops cannot disable
//! HTML5 tab drops on Windows: the internal path does not depend on HTML5 DnD.
pub(crate) mod targets;

use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    moves,
    transfers::{self, ConflictPolicy, DirectEndpoint, Source},
    State,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State as TauriState, WebviewWindow};

#[derive(Default)]
pub struct DragState {
    grants: Mutex<HashMap<String, Grant>>,
    drafts: Mutex<HashMap<String, Draft>>,
    active: Mutex<Option<(String, Arc<AtomicBool>)>>,
    targets: targets::TargetState,
}
#[derive(Clone)]
struct Grant {
    source: Source,
    owner: String,
    generation: u64,
    allowed_window: Option<String>,
    expires: Instant,
}
#[derive(Clone)]
struct Draft {
    source: Source,
    target: SharedContext,
    path: String,
    endpoint: Option<DirectEndpoint>,
    owner: String,
    generation: u64,
    expires: Instant,
    started: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPreview {
    id: String,
    source_name: String,
    source_paths: Vec<String>,
    count: usize,
    target_name: String,
    target_path: String,
    direct_endpoint: Option<EndpointPreview>,
    endpoint_required: bool,
    saved_password_available: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointPreview {
    host: String,
    port: u16,
    username: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DragEvent {
    phase: String,
    kind: String,
    token: String,
    x: f64,
    y: f64,
    count: usize,
    label: String,
}
fn emit(app: &tauri::AppHandle, label: &str, event: &DragEvent) {
    let _ = app.emit_to(label, "desktop-drag", event);
}
fn grant(state: &State, window: &str, source: Source, ready: bool) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let owner = state.owner(window);
    let mut grants = state.drags.grants.lock().unwrap();
    grants.retain(|_, g| g.expires > Instant::now());
    grants.insert(
        id.clone(),
        Grant {
            source,
            owner: window.into(),
            generation: owner.generation,
            allowed_window: ready.then(|| window.to_owned()),
            expires: Instant::now() + Duration::from_secs(120),
        },
    );
    id
}
pub fn native_drop(window: &tauri::Window, event: &tauri::DragDropEvent) {
    let label = window.label();
    if label != "main" && !label.starts_with("workspace-") {
        return;
    }
    let state = window.state::<Arc<State>>();
    let app = window.app_handle();
    let scale = window.scale_factor().unwrap_or(1.0);
    let mut payload = DragEvent {
        phase: "leave".into(),
        kind: "localFiles".into(),
        token: String::new(),
        x: 0.0,
        y: 0.0,
        count: 0,
        label: "上传本地项目".into(),
    };
    match event {
        tauri::DragDropEvent::Enter { paths, position }
        | tauri::DragDropEvent::Drop { paths, position } => {
            if paths.is_empty() {
                return;
            }
            let drop = matches!(event, tauri::DragDropEvent::Drop { .. });
            payload.phase = if drop { "drop" } else { "over" }.into();
            payload.count = paths.len();
            payload.x = position.x / scale;
            payload.y = position.y / scale;
            if drop {
                payload.token = grant(&state, label, Source::Local(paths.clone()), true);
            }
        }
        tauri::DragDropEvent::Over { position } => {
            payload.phase = "over".into();
            payload.x = position.x / scale;
            payload.y = position.y / scale;
        }
        _ => {}
    }
    emit(app, label, &payload);
}

#[cfg(target_os = "macos")]
fn pointer_buttons(_: &tauri::AppHandle) -> AppResult<(bool, bool)> {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
        fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    }
    // Only queried while an explicit user drag is active; no event tap, logging,
    // global keyboard hook or accessibility permission is installed.
    Ok(unsafe { (CGEventSourceButtonState(0, 0), CGEventSourceKeyState(0, 53)) })
}
#[cfg(target_os = "windows")]
fn pointer_buttons(_: &tauri::AppHandle) -> AppResult<(bool, bool)> {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    Ok(unsafe { (GetAsyncKeyState(1) < 0, GetAsyncKeyState(27) < 0) })
}
#[cfg(target_os = "linux")]
fn pointer_buttons(app: &tauri::AppHandle) -> AppResult<(bool, bool)> {
    #[link(name = "gdk-3")]
    extern "C" {
        fn gdk_display_get_default() -> *mut std::ffi::c_void;
        fn gdk_display_get_default_seat(display: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn gdk_seat_get_pointer(seat: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn gdk_get_default_root_window() -> *mut std::ffi::c_void;
        fn gdk_device_get_state(
            device: *mut std::ffi::c_void,
            window: *mut std::ffi::c_void,
            axes: *mut f64,
            mask: *mut u32,
        );
    }
    let (send, recv) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let result = unsafe {
            let display = gdk_display_get_default();
            let root = gdk_get_default_root_window();
            if display.is_null() || root.is_null() {
                None
            } else {
                let seat = gdk_display_get_default_seat(display);
                let pointer = if seat.is_null() {
                    std::ptr::null_mut()
                } else {
                    gdk_seat_get_pointer(seat)
                };
                if pointer.is_null() {
                    None
                } else {
                    let mut mask = 0;
                    gdk_device_get_state(pointer, root, std::ptr::null_mut(), &mut mask);
                    Some((mask & (1 << 8) != 0, false))
                }
            }
        };
        let _ = send.send(result);
    })
    .map_err(|e| AppError::new("drag", e))?;
    recv.recv_timeout(Duration::from_secs(2))
        .ok()
        .flatten()
        .ok_or_else(|| AppError::new("drag", "当前显示环境无法读取原生拖拽状态"))
}

fn begin_pointer_drag(
    app: tauri::AppHandle,
    state: Arc<State>,
    source_window: String,
    token: String,
    kind: &str,
    count: usize,
    label: String,
) -> AppResult<()> {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.drags.active.lock().unwrap();
        if active.is_some() {
            return Err(AppError::new("busy", "已有拖拽正在进行"));
        }
        *active = Some((source_window.clone(), stop.clone()));
    }
    let kind = kind.to_owned();
    std::thread::spawn(move || {
        let mut previous: Option<String> = None;
        let mut cancelled = false;
        let started = Instant::now();
        loop {
            let native = pointer_buttons(&app);
            let (pressed, escape) = match native {
                Ok(value) => value,
                Err(e) => {
                    let _ = app.emit_to(&source_window, "desktop-drag-error", &e);
                    cancelled = true;
                    break;
                }
            };
            if stop.load(Ordering::SeqCst)
                || escape
                || started.elapsed() > Duration::from_secs(120)
                || state.stopping.load(Ordering::SeqCst)
            {
                cancelled = true;
                break;
            }
            let cursor = match app.cursor_position() {
                Ok(p) => p,
                Err(_) => {
                    cancelled = true;
                    break;
                }
            };
            let mut windows: Vec<_> = app
                .webview_windows()
                .into_values()
                .filter(|w| {
                    (w.label() == "main" || w.label().starts_with("workspace-"))
                        && w.is_visible().unwrap_or(false)
                        && !w.is_minimized().unwrap_or(true)
                })
                .collect();
            windows.sort_by_key(|w| !w.is_focused().unwrap_or(false));
            let target = windows.iter().find_map(|window| {
                let position = window.inner_position().ok()?;
                let size = window.inner_size().ok()?;
                let x = cursor.x - f64::from(position.x);
                let y = cursor.y - f64::from(position.y);
                if x >= 0.0 && y >= 0.0 && x < f64::from(size.width) && y < f64::from(size.height) {
                    let scale = window.scale_factor().ok()?;
                    Some((window.label().to_owned(), x / scale, y / scale))
                } else {
                    None
                }
            });
            if previous.as_deref() != target.as_ref().map(|t| t.0.as_str()) {
                if let Some(previous) = &previous {
                    emit(
                        &app,
                        previous,
                        &DragEvent {
                            phase: "leave".into(),
                            kind: kind.clone(),
                            token: token.clone(),
                            x: 0.0,
                            y: 0.0,
                            count,
                            label: label.clone(),
                        },
                    );
                }
            }
            if let Some((window, x, y)) = &target {
                if !pressed && kind == "remoteFiles" {
                    if let Some(grant) = state.drags.grants.lock().unwrap().get_mut(&token) {
                        grant.allowed_window = Some(window.clone());
                    }
                }
                emit(
                    &app,
                    window,
                    &DragEvent {
                        phase: if pressed { "over" } else { "drop" }.into(),
                        kind: kind.clone(),
                        token: token.clone(),
                        x: *x,
                        y: *y,
                        count,
                        label: label.clone(),
                    },
                );
            }
            previous = target.map(|t| t.0);
            if !pressed {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if let Some(previous) = previous {
            emit(
                &app,
                &previous,
                &DragEvent {
                    phase: "leave".into(),
                    kind: kind.clone(),
                    token: token.clone(),
                    x: 0.0,
                    y: 0.0,
                    count,
                    label,
                },
            );
        }
        *state.drags.active.lock().unwrap() = None;
        let _ = app.emit_to(
            &source_window,
            "desktop-drag-ended",
            serde_json::json!({"token":token,"kind":kind,"cancelled":cancelled}),
        );
    });
    Ok(())
}
#[tauri::command]
pub fn begin_file_drag(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    session_id: String,
    paths: Vec<String>,
) -> AppResult<String> {
    moves::workspace_only(&window)?;
    let context = state.spec(&session_id, window.label())?;
    let paths = super::transfers::normalize_selection(&paths)?;
    let label = if paths.len() == 1 {
        crate::remote_path::name(&paths[0])?.to_owned()
    } else {
        format!("{} 个项目", paths.len())
    };
    let count = paths.len();
    let token = grant(
        &state,
        window.label(),
        Source::Remote { context, paths },
        false,
    );
    begin_pointer_drag(
        app,
        state.inner().clone(),
        window.label().into(),
        token.clone(),
        "remoteFiles",
        count,
        label,
    )?;
    Ok(token)
}
#[tauri::command]
pub fn begin_workspace_drag(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    token: String,
    tab_id: String,
) -> AppResult<()> {
    moves::begin_tab_drag(window.clone(), state.clone(), token.clone(), tab_id)?;
    begin_pointer_drag(
        app,
        state.inner().clone(),
        window.label().into(),
        token,
        "workspaceTab",
        1,
        "移动会话".into(),
    )
}
#[tauri::command]
pub fn cancel_native_drag(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<()> {
    moves::workspace_only(&window)?;
    if let Some((owner, stop)) = state.drags.active.lock().unwrap().as_ref() {
        if owner == window.label() {
            stop.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}
fn draft(
    state: &State,
    window: &WebviewWindow,
    source: Source,
    target: SharedContext,
    path: String,
) -> AppResult<DraftPreview> {
    create_draft(state, window.label(), source, target, path)
}

fn create_draft(
    state: &State,
    owner: &str,
    source: Source,
    target: SharedContext,
    path: String,
) -> AppResult<DraftPreview> {
    crate::remote_path::absolute(&path)?;
    let id = uuid::Uuid::new_v4().to_string();
    let remote = matches!(source, Source::Remote { .. });
    let endpoint = if remote {
        let config = state.database.lock().unwrap().get(&target.target().id)?;
        config.direct_host.map(|host| DirectEndpoint {
            host,
            port: config.direct_port,
            username: target.target().username.clone(),
            public_key: target.target_public_key().into(),
        })
    } else {
        None
    };
    let mut drafts = state.drags.drafts.lock().unwrap();
    drafts.retain(|_, d| d.expires > Instant::now());
    drafts.insert(
        id.clone(),
        Draft {
            source,
            target,
            path,
            endpoint,
            owner: owner.into(),
            generation: state.owner(owner).generation,
            expires: Instant::now() + Duration::from_secs(300),
            started: None,
        },
    );
    Ok(preview(&id, drafts.get(&id).unwrap()))
}

fn preview(id: &str, draft: &Draft) -> DraftPreview {
    let (source_name, source_paths, count) = match &draft.source {
        Source::Local(paths) => (
            "本机".to_owned(),
            paths
                .iter()
                .take(4)
                .map(|p| p.display().to_string())
                .collect(),
            paths.len(),
        ),
        Source::Remote { context, paths } => (
            context.target().name.clone(),
            paths.iter().take(4).cloned().collect(),
            paths.len(),
        ),
    };
    DraftPreview {
        id: id.into(),
        source_name,
        source_paths,
        count,
        target_name: draft.target.target().name.clone(),
        target_path: draft.path.clone(),
        direct_endpoint: draft.endpoint.as_ref().map(|e| EndpointPreview {
            host: e.host.clone(),
            port: e.port,
            username: e.username.clone(),
        }),
        endpoint_required: matches!(draft.source, Source::Remote { .. })
            && draft.endpoint.is_none(),
        saved_password_available: !draft.target.target().password.is_empty(),
    }
}
#[tauri::command]
pub fn claim_file_drop(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    token: String,
    session_id: String,
    parent_path: String,
) -> AppResult<DraftPreview> {
    moves::workspace_only(&window)?;
    let target = state.spec(&session_id, window.label())?;
    let grant = claim_grant(&state, &token, window.label())?;
    draft(&state, &window, grant.source, target, parent_path)
}
fn claim_grant(state: &State, token: &str, window: &str) -> AppResult<Grant> {
    let mut grants = state.drags.grants.lock().unwrap();
    let grant = grants
        .get(token)
        .ok_or_else(|| AppError::new("drag", "拖拽已领取或失效，请重试"))?;
    if grant.expires < Instant::now()
        || grant.allowed_window.as_deref() != Some(window)
        || state.owner(&grant.owner).generation != grant.generation
        || state.closed_windows.lock().unwrap().contains(&grant.owner)
    {
        return Err(AppError::new(
            "permission",
            "拖拽来源或目标窗口已变化，请重试",
        ));
    }
    Ok(grants.remove(token).unwrap())
}
#[tauri::command]
pub async fn pick_upload_files(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    parent_path: String,
) -> AppResult<Option<DraftPreview>> {
    moves::workspace_only(&window)?;
    let target = state.spec(&session_id, window.label())?;
    let generation = state.owner(window.label()).generation;
    let paths = crate::blocking(|| {
        Ok(rfd::FileDialog::new()
            .set_title("选择上传文件")
            .pick_files())
    })
    .await?;
    let Some(paths) = paths else { return Ok(None) };
    state.spec(&session_id, window.label())?;
    if state.owner(window.label()).generation != generation {
        return Err(AppError::new("permission", "窗口已变化"));
    }
    draft(&state, &window, Source::Local(paths), target, parent_path).map(Some)
}
#[tauri::command]
pub fn start_transfer(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<Arc<State>>,
    draft_id: String,
    conflict: ConflictPolicy,
    auth: Option<transfers::AuthInput>,
    verify_sha256: Option<bool>,
) -> AppResult<String> {
    moves::workspace_only(&window)?;
    let mut drafts = state.drags.drafts.lock().unwrap();
    let draft = drafts
        .get_mut(&draft_id)
        .ok_or_else(|| AppError::new("transfer", "传输准备已失效，请重新选择"))?;
    if draft.owner != window.label()
        || draft.generation != state.owner(window.label()).generation
        || draft.expires < Instant::now()
    {
        return Err(AppError::new("permission", "此窗口无权启动该传输"));
    }
    if let Some(id) = &draft.started {
        return Ok(id.clone());
    }
    let id = transfers::start_write(
        app,
        state.inner().clone(),
        draft.source.clone(),
        draft.target.clone(),
        draft.path.clone(),
        conflict,
        draft.endpoint.clone(),
        auth,
        verify_sha256.unwrap_or(false),
    )?;
    draft.started = Some(id.clone());
    Ok(id)
}

#[tauri::command]
pub fn set_transfer_endpoint(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    draft_id: String,
    host: String,
    port: u16,
) -> AppResult<DraftPreview> {
    moves::workspace_only(&window)?;
    let result = update_endpoint(&state, window.label(), &draft_id, host, port)?;
    let _ = window.emit_to("main", "server-configurations-changed", ());
    Ok(result)
}
fn update_endpoint(
    state: &State,
    owner: &str,
    draft_id: &str,
    host: String,
    port: u16,
) -> AppResult<DraftPreview> {
    let host = host.trim().to_owned();
    crate::remote_path::validate_host(&host)?;
    if port == 0 {
        return Err(AppError::new("validation", "端口必须为 1–65535"));
    }
    // Hold the draft transaction through saving and replacing the snapshot.
    // start_transfer uses the same lock: it cannot start the old draft halfway
    // through an address change or create a second task from the new draft.
    let mut drafts = state.drags.drafts.lock().unwrap();
    let value = drafts
        .get(draft_id)
        .ok_or_else(|| AppError::new("transfer", "复制准备已过期"))?;
    if value.owner != owner
        || value.generation != state.owner(owner).generation
        || value.expires < Instant::now()
        || value.started.is_some()
        || !matches!(value.source, Source::Remote { .. })
    {
        return Err(AppError::new("permission", "此复制准备不可修改"));
    }
    let mut existing = value.clone();
    {
        let _gate = state.configuration.lock().unwrap();
        state.check_context(&existing.target)?;
        state.database.lock().unwrap().update_direct_endpoint(
            &existing.target.target().id,
            host.clone(),
            port,
        )?;
    }
    existing.endpoint = Some(DirectEndpoint {
        host,
        port,
        username: existing.target.target().username.clone(),
        public_key: existing.target.target_public_key().into(),
    });
    existing.expires = Instant::now() + Duration::from_secs(300);
    let id = uuid::Uuid::new_v4().to_string();
    let result = preview(&id, &existing);
    drafts.remove(draft_id);
    drafts.insert(id, existing);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_snapshot_is_frozen_and_edits_replace_only_unstarted_owned_drafts() {
        use crate::model::{PasswordChange, SaveServerRequest, ServerConfig};
        let state = crate::connection::tests::state();
        let config = ServerConfig {
            id: String::new(),
            name: "B".into(),
            host: "entry.example".into(),
            port: 2222,
            username: "ops".into(),
            tags: vec!["keep".into()],
            auth_type: "password".into(),
            private_key: String::new(),
            password: "keep-secret".into(),
            jump_server_id: None,
            direct_host: Some("10.0.0.2".into()),
            direct_port: 22,
        };
        let saved = state
            .database
            .lock()
            .unwrap()
            .save(SaveServerRequest {
                server: config.view(),
                password_change: PasswordChange::Set {
                    value: config.password.clone(),
                },
            })
            .unwrap();
        let context = crate::connection::tests::mock_context(
            state.database.lock().unwrap().get(&saved.id).unwrap(),
        );
        let draft = create_draft(
            &state,
            "main",
            Source::Remote {
                context: context.clone(),
                paths: vec!["/source".into()],
            },
            context,
            "/target".into(),
        )
        .unwrap();
        state
            .database
            .lock()
            .unwrap()
            .update_direct_endpoint(&saved.id, "10.0.0.99".into(), 2200)
            .unwrap();
        assert_eq!(
            state.drags.drafts.lock().unwrap()[&draft.id]
                .endpoint
                .as_ref()
                .unwrap()
                .host,
            "10.0.0.2"
        );
        assert!(
            update_endpoint(&state, "workspace-other", &draft.id, "10.0.0.3".into(), 22).is_err()
        );
        let updated = update_endpoint(&state, "main", &draft.id, "10.0.0.3".into(), 2223).unwrap();
        assert!(!state.drags.drafts.lock().unwrap().contains_key(&draft.id));
        assert_eq!(updated.direct_endpoint.unwrap().port, 2223);
        let stored = state.database.lock().unwrap().get(&saved.id).unwrap();
        assert_eq!(stored.host, "entry.example");
        assert_eq!(stored.port, 2222);
        assert_eq!(stored.tags, ["keep"]);
        assert_eq!(stored.password, "keep-secret");
        state
            .drags
            .drafts
            .lock()
            .unwrap()
            .get_mut(&updated.id)
            .unwrap()
            .started = Some("task".into());
        assert!(update_endpoint(&state, "main", &updated.id, "10.0.0.4".into(), 22).is_err());
        assert_eq!(
            state
                .database
                .lock()
                .unwrap()
                .get(&saved.id)
                .unwrap()
                .direct_host
                .as_deref(),
            Some("10.0.0.3")
        );
    }
    #[test]
    fn native_grants_are_window_bound_one_use_and_expire() {
        let state = crate::connection::tests::state();
        let source = Source::Local(vec!["/selected/file".into()]);
        let id = grant(&state, "main", source.clone(), true);
        assert!(claim_grant(&state, &id, "workspace-other").is_err());
        assert!(claim_grant(&state, &id, "main").is_ok());
        assert!(claim_grant(&state, &id, "main").is_err());
        let id = grant(&state, "main", source.clone(), false);
        assert!(claim_grant(&state, &id, "main").is_err());
        let id = grant(&state, "main", source.clone(), true);
        state
            .drags
            .grants
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(claim_grant(&state, &id, "main").is_err());
        let id = grant(&state, "main", source.clone(), true);
        state
            .window_generations
            .lock()
            .unwrap()
            .insert("main".into(), 1);
        assert!(claim_grant(&state, &id, "main").is_err());
        let id = grant(&state, "main", source, true);
        state.closed_windows.lock().unwrap().insert("main".into());
        assert!(claim_grant(&state, &id, "main").is_err());
    }
}
