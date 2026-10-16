mod traffic;
mod brand;
mod connection;
mod db;
mod desktop_drag;
mod docker;
mod files;
mod file_tasks;
mod log_ingest;
mod log_store;
mod logs;
mod model;
mod monitor;
mod moves;
mod remote_delete;
mod remote_fs;
mod remote_mounts;
mod remote_path;
mod remote_read;
mod remote_write;
mod shell_integration;
mod ssh;
mod ssh_transport;
mod terminal;
mod terminal_cwd;
mod terminal_history;
mod transfers;
mod window_chrome;

use model::{AppError, AppResult, SaveServerRequest, ServerView};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{
    ipc::Channel, Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub struct State {
    traffic: Arc<traffic::TrafficManager>,
    moves: moves::MoveState,
    files: files::FileState,
    file_tasks: file_tasks::FileTaskState,
    deletes: remote_delete::DeleteState,
    transfers: transfers::TransferState,
    drags: desktop_drag::DragState,
    logs: logs::LogState,
    history: terminal_history::HistoryState,
    monitor: monitor::MonitorManager,
    database: Mutex<db::Database>,
    configuration: Mutex<()>,
    stopping: AtomicBool,
    terminals: Mutex<HashMap<String, connection::ConnectionHandle>>,
    pending_windows: Mutex<HashMap<String, PendingContainer>>,
    container_windows: Mutex<HashMap<String, String>>,
    closed_windows: Mutex<HashSet<String>>,
    window_generations: Mutex<HashMap<String, u64>>,
}
#[derive(Clone)]
struct PendingContainer {
    spec: connection::SharedContext,
    target: docker::ContainerTarget,
}
impl State {
    fn connected_context(&self, identity: &str) -> AppResult<connection::SharedContext> {
        self.ensure_running()?;
        self.terminals.lock().unwrap().values()
            .filter(|t| !t.sender.cancel.is_cancelled())
            .filter_map(|t| t.spec.clone())
            .find(|c| c.identity() == identity)
            .ok_or_else(|| AppError::new("disconnected", "请先重新连接任务涉及的服务器；连接身份必须与原任务一致"))
    }
    fn check_context(&self, context: &connection::ConnectionContext) -> AppResult<()> {
        self.ensure_running()?;
        let database = self.database.lock().unwrap();
        for id in context.server_ids() {
            database.get(id)?;
        }
        Ok(())
    }
    fn ensure_running(&self) -> AppResult<()> {
        if self.stopping.load(Ordering::SeqCst) {
            Err(AppError::new("cancelled", "应用正在退出"))
        } else {
            Ok(())
        }
    }
    fn delete_server(&self, id: &str) -> AppResult<()> {
        let _gate = self.configuration.lock().unwrap();
        if self.moves.has_server(id)
            || self
                .terminals
                .lock()
                .unwrap()
                .values()
                .any(|terminal| terminal.uses_server(id))
            || self
                .pending_windows
                .lock()
                .unwrap()
                .values()
                .any(|window| window.spec.uses_server(id))
            || self.logs.has_server(id)
        {
            return Err(AppError::new(
                "busy",
                "请先关闭此服务器的会话标签页、独立会话、容器终端和日志窗口，再删除配置。",
            ));
        }
        if self.files.has_server_activity(id) || self.transfers.has_server_activity(id) {
            return Err(AppError::new(
                "busy",
                "请先保存并关闭此服务器的文件编辑窗口，等待传输完成或取消传输，再删除配置。",
            ));
        }
        self.database.lock().unwrap().delete(id)
    }
    fn owner(&self, label: &str) -> connection::WindowOwner {
        connection::WindowOwner {
            label: label.into(),
            generation: *self
                .window_generations
                .lock()
                .unwrap()
                .get(label)
                .unwrap_or(&0),
        }
    }
    fn spec(&self, id: &str, window: &str) -> AppResult<connection::SharedContext> {
        self.ensure_running()?;
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals
            .get(id)
            .ok_or_else(|| AppError::new("disconnected", "SSH 会话已经结束"))?;
        if terminal.window != window {
            return Err(AppError::new("permission", "此窗口无权访问该会话"));
        }
        if terminal.sender.cancel.is_cancelled() || terminal.stream.lock().unwrap().closed {
            return Err(AppError::new("disconnected", "SSH 会话已经结束"));
        }
        terminal
            .spec
            .clone()
            .ok_or_else(|| AppError::new("connecting", "SSH 连接尚未完成"))
    }
}
async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|e| AppError::new("internal", e))?
}
fn main_only(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(AppError::new("permission", "仅主窗口可执行此操作"))
    }
}

#[tauri::command]
fn list_servers(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Vec<ServerView>> {
    main_only(&window)?;
    Ok(state
        .database
        .lock()
        .unwrap()
        .list()?
        .into_iter()
        .map(|s| s.view())
        .collect())
}
#[tauri::command]
fn reset_incompatible_database(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<String> {
    main_only(&window)?;
    let _gate = state.configuration.lock().unwrap();
    state.ensure_running()?;
    if !state.terminals.lock().unwrap().is_empty()
        || state.logs.active_count() > 0
        || state.files.has_activity()
        || state.transfers.has_activity()
        || !state.pending_windows.lock().unwrap().is_empty()
    {
        return Err(AppError::new(
            "busy",
            "请先关闭活动会话并保存远程文件，再重建数据库",
        ));
    }
    Ok(state
        .database
        .lock()
        .unwrap()
        .reset_incompatible()?
        .to_string_lossy()
        .into_owned())
}
#[tauri::command]
fn save_server(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    server: SaveServerRequest,
) -> AppResult<ServerView> {
    main_only(&window)?;
    let _gate = state.configuration.lock().unwrap();
    state.database.lock().unwrap().save(server)
}
#[tauri::command]
fn delete_server(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    id: String,
) -> AppResult<()> {
    main_only(&window)?;
    state.delete_server(&id)
}
#[tauri::command]
fn session_count(window: WebviewWindow, state: TauriState<Arc<State>>) -> AppResult<usize> {
    main_only(&window)?;
    Ok(state
        .terminals
        .lock()
        .unwrap()
        .values()
        .filter(|t| !t.stream.lock().unwrap().closed)
        .count())
}
#[tauri::command]
async fn quit_app(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    discard_unsaved: Option<bool>,
) -> AppResult<()> {
    main_only(&window)?;
    if !discard_unsaved.unwrap_or(false) && state.files.unsaved_count() > 0 {
        return Err(AppError::new(
            "unsaved",
            "仍有未保存的远程文件，请保存或确认放弃修改后退出",
        ));
    }
    {
        let _gate = state.configuration.lock().unwrap();
        state.stopping.store(true, Ordering::SeqCst);
    }
    state.deletes.stop_all();
    state.transfers.stop_transfers();
    let shutdown_state = state.inner().clone();
    let result = blocking(move || {
        shutdown_state.transfers.interrupt_operations_after_grace();
        shutdown_state.monitor.shutdown()?;
        shutdown_state.transfers.wait_transfers()?;
        shutdown_state.logs.shutdown()?;
        for terminal in shutdown_state.terminals.lock().unwrap().values() {
            let _ = terminal.sender.send(terminal::Input::Close);
        }
        connection::wait_closed(&shutdown_state)?;
        shutdown_state.traffic.flush();
        shutdown_state.history.shutdown()
    })
    .await;
    if let Err(error) = result {
        state.stopping.store(false, Ordering::SeqCst);
        return Err(error);
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn terminal_input(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    data: String,
) -> AppResult<()> {
    state.spec(&session_id, window.label())?;
    if data.len() > 1024 * 1024 {
        return Err(AppError::new("limit", "单次输入超过 1 MB"));
    }
    state
        .terminals
        .lock()
        .unwrap()
        .get(&session_id)
        .ok_or_else(|| AppError::new("disconnected", "会话已结束"))?
        .sender
        .send(terminal::Input::Data(data.into_bytes()))
        .map_err(|e| AppError::new("disconnected", e))
}
#[tauri::command]
fn terminal_resize(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> AppResult<()> {
    state.spec(&session_id, window.label())?;
    let terminals = state.terminals.lock().unwrap();
    if let Some(terminal) = terminals.get(&session_id) {
        let _ = terminal.sender.send(terminal::Input::Resize(
            cols.clamp(2, 1000),
            rows.clamp(2, 1000),
        ));
    }
    Ok(())
}

#[tauri::command]
fn terminal_clear_display(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
) -> AppResult<()> {
    state.spec(&session_id, window.label())?;
    let terminals = state.terminals.lock().unwrap();
    let terminal = terminals
        .get(&session_id)
        .ok_or_else(|| AppError::new("disconnected", "会话已结束"))?;
    terminal
        .sender
        .send(terminal::Input::ClearDisplay)
        .map_err(|e| AppError::new("disconnected", e))
}
#[tauri::command]
async fn detect_docker(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
) -> AppResult<docker::DockerStatus> {
    moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    blocking(move || docker::detect(&spec)).await
}
#[tauri::command]
async fn docker_images(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    endpoint: String,
) -> AppResult<Vec<docker::Image>> {
    moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    blocking(move || docker::images(&spec, &endpoint)).await
}
#[tauri::command]
async fn docker_containers(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    endpoint: String,
) -> AppResult<Vec<docker::Container>> {
    moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    blocking(move || docker::containers(&spec, &endpoint)).await
}

#[tauri::command]
async fn open_container(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    target: docker::ContainerTarget,
) -> AppResult<()> {
    moves::workspace_only(&window)?;
    let gate = state.configuration.lock().unwrap();
    let spec = state.spec(&session_id, window.label())?;
    let key = format!("{}|{}|{}", spec.identity(), target.endpoint, target.id);
    let (label, create) = {
        let mut windows = state.container_windows.lock().unwrap();
        if let Some(label) = windows.get(&key) {
            (label.clone(), false)
        } else {
            let label = format!("container-{}", uuid::Uuid::new_v4());
            windows.insert(key.clone(), label.clone());
            (label, true)
        }
    };
    // Native window operations may wait for the UI thread; never hold a state lock here.
    if !create {
        drop(gate);
        if let Some(existing) = app.get_webview_window(&label) {
            existing
                .set_focus()
                .map_err(|e| AppError::new("window", e))?;
        }
        return Ok(());
    }
    let title = brand::window_title(
        &app,
        &format!("{} 终端 · {}", target.name, spec.target().name),
    );
    state
        .pending_windows
        .lock()
        .unwrap()
        .insert(label.clone(), PendingContainer { spec, target });
    drop(gate);
    let result = crate::window_chrome::configure(WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App("index.html?container=1".into()),
    ))
    .title(title)
    .inner_size(880.0, 580.0)
    .min_inner_size(480.0, 320.0)
    .build();
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            state.pending_windows.lock().unwrap().remove(&label);
            state.container_windows.lock().unwrap().remove(&key);
            Err(AppError::new("window", e))
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContainerInfo {
    server_name: String,
    container_name: String,
}
#[tauri::command]
fn container_info(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<ContainerInfo> {
    let pending = state.pending_windows.lock().unwrap();
    let target = pending
        .get(window.label())
        .ok_or_else(|| AppError::new("window", "容器窗口配置已失效"))?;
    Ok(ContainerInfo {
        server_name: target.spec.target().name.clone(),
        container_name: target.target.name.clone(),
    })
}
#[tauri::command]
async fn connect_container(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    output: Channel<connection::ConnectionEvent>,
) -> AppResult<String> {
    let state = state.inner().clone();
    let label = window.label().to_string();
    let owner = state.owner(&label);
    let pending = state
        .pending_windows
        .lock()
        .unwrap()
        .get(&label)
        .cloned()
        .ok_or_else(|| AppError::new("window", "容器窗口配置已失效"))?;
    connection::open_container(state, owner, pending.spec, pending.target, output)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let database = db::Database::open(&dir.join("servers.db"))
                .map_err(|e| std::io::Error::other(e.message))?;
            let file_tasks = file_tasks::FileTaskState::open(&dir.join("file-tasks.sqlite"))
                .map_err(|e| std::io::Error::other(e.message))?;
            file_tasks.attach(app.handle().clone());
            let deletes = remote_delete::DeleteState::restore(&mut file_tasks.store.lock().unwrap())
                .map_err(|e| std::io::Error::other(e.message))?;
            let transfers = transfers::TransferState::open(dir.join("transfer-receipts"))
                .map_err(|e| std::io::Error::other(e.message))?;
            transfers.restore_downloads(&file_tasks).map_err(|e| std::io::Error::other(e.message))?;
            transfers.observe_order(deletes.snapshot().iter().map(|t| t.sort_order).max().unwrap_or(0));
            app.manage(Arc::new(State {
                traffic: traffic::TrafficManager::new(),
                moves: moves::MoveState::default(),
                monitor: monitor::MonitorManager::default(),
                files: files::FileState::default(),
                deletes,
                transfers,
                file_tasks,
                drags: desktop_drag::DragState::default(),
                logs: logs::LogState::default(),
                history: terminal_history::HistoryState::open(dir.join("terminal-history")),
                database: Mutex::new(database),
                configuration: Mutex::new(()),
                stopping: AtomicBool::new(false),
                terminals: Mutex::new(HashMap::new()),
                pending_windows: Mutex::new(HashMap::new()),
                container_windows: Mutex::new(HashMap::new()),
                closed_windows: Mutex::new(HashSet::new()),
                window_generations: Mutex::new(HashMap::new()),
            }));
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(state) = webview.try_state::<Arc<State>>() {
                    state.deletes.discard_window_drafts(webview.label());
                    state.monitor.stop_window(webview.label());
                    state.logs.stop(webview.label(), false);
                    state.history.cancel_window(webview.label());
                    let mut generations = state.window_generations.lock().unwrap();
                    *generations.entry(webview.label().into()).or_default() += 1;
                    for terminal in state.terminals.lock().unwrap().values() {
                        if terminal.window == webview.label() {
                            let _ = terminal.sender.send(terminal::Input::Close);
                        }
                    }
                }
            }
        })
        .on_window_event(|window, event| {
            // WebviewWindow content routes native drops through WindowEvent.
            if let tauri::WindowEvent::DragDrop(event) = event {
                desktop_drag::native_drop(window, event);
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<Arc<State>>();
                state.deletes.discard_window_drafts(window.label());
                state.monitor.stop_window(window.label());
                state.logs.stop(window.label(), true);
                state.history.cancel_window(window.label());
                moves::window_closed(window.app_handle(), &state, window.label());
                state
                    .closed_windows
                    .lock()
                    .unwrap()
                    .insert(window.label().into());
                for terminal in state.terminals.lock().unwrap().values() {
                    if terminal.window == window.label() {
                        let _ = terminal.sender.send(terminal::Input::Close);
                    }
                }
                state.pending_windows.lock().unwrap().remove(window.label());
                state
                    .container_windows
                    .lock()
                    .unwrap()
                    .retain(|_, label| label != window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            window_chrome::window_chrome,
            logs::open_container_logs,
            logs::logs_info,
            logs::logs_start,
            logs::logs_stop,
            logs::logs_page,
            logs::logs_export,
            logs::logs_copy_range,
            logs::list_log_archives,
            logs::open_log_archives_window,
            logs::open_log_archive,
            logs::log_session_count,
            list_servers,
            reset_incompatible_database,
            save_server,
            delete_server,
            session_count,
            quit_app,
            connection::open_connection,
            connection::answer_connection,
            connection::close_connection,
            terminal_input,
            terminal_resize,
            terminal_clear_display,
            terminal_history::commands::terminal_history_sessions,
            terminal_history::commands::terminal_history_status,
            terminal_history::commands::terminal_history_page,
            terminal_history::commands::terminal_history_search,
            terminal_history::commands::terminal_history_cancel,
            terminal_history::commands::terminal_history_replay,
            terminal_history::commands::terminal_history_copy,
            terminal_history::commands::terminal_history_export,
            terminal_history::commands::open_terminal_history_window,
            remote_fs::list_directory,
            remote_fs::list_directory_chain,
            monitor::subscribe_monitor,
            monitor::update_monitor_interest,
            monitor::get_monitor_history,
            monitor::refresh_monitor,
            monitor::unsubscribe_monitor,
            remote_mounts::list_remote_mounts,
            remote_mounts::get_remote_mount_usage,
            detect_docker,
            docker_images,
            docker_containers,
            open_container,
            container_info,
            connect_container,
            moves::terminal_freeze,
            moves::terminal_attach,
            moves::terminal_resume,
            moves::move_workspace,
            moves::take_workspace,
            moves::workspace_ready,
            moves::cancel_workspace_move,
            moves::begin_tab_drag,
            moves::drop_tab,
            moves::finish_tab_drag,
            files::open_editor,
            files::editor_load,
            files::editor_save,
            files::editor_set_dirty,
            files::unsaved_editor_count,
            transfers::download::list_downloads,
            transfers::download::start_download,
            transfers::download::cancel_download,
            transfers::download::retry_download,
            transfers::list_transfers,
            transfers::cancel_transfer,
            transfers::retry_transfer,
            transfers::reconcile_transfer,
            transfers::cleanup_transfer,
            remote_fs::create_remote_entry,
            remote_delete::prepare_remote_delete,
            remote_delete::start_remote_delete,
            remote_delete::list_remote_deletes,
            remote_delete::prepare_delete_retry,
            file_tasks::list_file_tasks,
            file_tasks::get_file_task,
            file_tasks::cancel_file_task,
            file_tasks::reconcile_file_task,
            file_tasks::dismiss_file_task,
            file_tasks::clear_completed_file_tasks,
            remote_delete::cancel_remote_delete,
            remote_delete::reconcile_remote_delete,
            desktop_drag::begin_file_drag,
            desktop_drag::clipboard::set_remote_clipboard,
            desktop_drag::clipboard::get_remote_clipboard,
            desktop_drag::clipboard::clear_remote_clipboard,
            desktop_drag::clipboard::prepare_remote_paste,
            desktop_drag::begin_workspace_drag,
            desktop_drag::cancel_native_drag,
            desktop_drag::claim_file_drop,
            desktop_drag::pick_upload_files,
            desktop_drag::start_transfer,
            desktop_drag::set_transfer_endpoint,
            desktop_drag::targets::register_file_view,
            desktop_drag::targets::unregister_file_view,
            desktop_drag::targets::list_copy_targets,
            desktop_drag::targets::prepare_server_copy
        ])
        .run(tauri::generate_context!())
        .expect("无法启动桌面应用");
}
