#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sodeck::{
    desktop_updates::CoreUpdater,
    model::*,
    updates::{ReleaseClient, UpdateView},
    windows::{self, DesktopWindows},
    AppCore,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
};
use tauri::{Emitter, Manager, State};

struct Application {
    core: Arc<AppCore>,
    updater: CoreUpdater,
    exiting: AtomicU8,
    smoke: bool,
    plugin_test: Option<PathBuf>,
    verify_persisted: bool,
    frontend_loaded: AtomicBool,
}

#[tauri::command]
async fn component_data(
    app: State<'_, Application>,
    window: tauri::WebviewWindow,
    surface_id: String,
    request: serde_json::Value,
    channel: tauri::ipc::Channel<sodeck::streams::Frame>,
) -> Result<serde_json::Value, String> {
    if request["op"] == "files" {
        if request["action"] == "download" {
            use tauri_plugin_dialog::DialogExt;
            app.core
                .validate_file_download(window.label(), &surface_id, &request)?;
            let filename = request["path"]
                .as_str()
                .unwrap_or("download")
                .rsplit('/')
                .next()
                .unwrap_or("download")
                .to_string();
            let dialog = window.dialog().file().set_file_name(filename);
            let chosen = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
                .await
                .map_err(|e| e.to_string())?;
            let Some(chosen) = chosen else {
                return Ok(serde_json::json!({"cancelled":true}));
            };
            let local = chosen.into_path().map_err(|e| e.to_string())?;
            return app
                .core
                .files_download(window.label(), &surface_id, request, local)
                .await;
        }
        return app
            .core
            .files_data(window.label(), &surface_id, request)
            .await;
    }
    let sink = Some({
        Arc::new(move |frame| channel.send(frame).map_err(|e| e.to_string()))
            as sodeck::streams::Sink
    });
    app.core
        .terminal_data(
            window.label(),
            &surface_id,
            serde_json::from_value(request).map_err(|e| e.to_string())?,
            sink,
        )
        .await
        .map(|_| serde_json::Value::Null)
}

#[tauri::command]
fn network_snapshot(app: State<Application>) -> serde_json::Value {
    app.core.network_snapshot()
}

#[tauri::command]
fn snapshot(app: State<Application>) -> Snapshot {
    app.core.snapshot()
}
#[tauri::command]
fn close_app(handle: tauri::AppHandle) {
    begin_exit(&handle);
}

#[tauri::command]
fn request_main_action(handle: tauri::AppHandle, action: String) -> Result<(), String> {
    if !matches!(action.as_str(), "restart" | "install") {
        return Err("未知应用操作".into());
    }
    let main = handle.get_webview_window("main").ok_or("主窗口不可用")?;
    let _ = main.set_focus();
    main.emit("core-main-action", action)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn server_catalog(app: State<Application>) -> serde_json::Value {
    app.core.server_catalog()
}
#[tauri::command]
async fn change_catalog(
    app: State<'_, Application>,
    revision: u64,
    change: sodeck::servers::CatalogChange,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.change_catalog(revision, change))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn recover_catalog(app: State<'_, Application>) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.recover_catalog())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn official_plugins() -> &'static [sodeck::official_plugins::OfficialPlugin] {
    sodeck::official_plugins::OFFICIAL_PLUGINS
}

#[tauri::command]
async fn install_official_plugin(
    app: State<'_, Application>,
    id: String,
) -> Result<String, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.install_official_plugin(&id, &ReleaseClient::github()?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn add_plugin(app: State<'_, Application>, directory: PathBuf) -> Result<String, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.add_plugin(&directory))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_plugin_enabled(
    app: State<'_, Application>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.set_enabled(&id, enabled))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn update_status(app: State<Application>) -> std::collections::HashMap<String, UpdateView> {
    app.core.update_views()
}

#[tauri::command]
async fn set_plugin_update_source(
    app: State<'_, Application>,
    id: String,
    source: Option<UpdateSource>,
) -> Result<(), String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.set_update_source(&id, source))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_update(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    id: String,
) -> Result<(), String> {
    if id == sodeck::updates::CORE_ID {
        return app.updater.check(&handle, &app.core).await;
    }
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.check_component(&id, &ReleaseClient::github()?)
            .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn download_update(app: State<'_, Application>, id: String) -> Result<bool, String> {
    if id == sodeck::updates::CORE_ID {
        app.updater.download(&app.core).await?;
        return Ok(true);
    }
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.download_plugin(&id, &ReleaseClient::github()?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn restart_app(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    install_core: bool,
) -> Result<(), String> {
    if install_core && !app.updater.ready() {
        return Err("请先下载核心更新".into());
    }
    app.exiting
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "应用正在退出或重启")?;
    if let Err(error) = app.core.prepare_restart() {
        app.exiting.store(0, Ordering::Release);
        return Err(error);
    }
    let core = app.core.clone();
    let stopped = tauri::async_runtime::spawn_blocking(move || core.shutdown())
        .await
        .map_err(|e| e.to_string())?;
    if let Err(error) = stopped {
        app.exiting.store(0, Ordering::Release);
        return Err(error);
    }
    app.exiting.store(2, Ordering::Release);
    if install_core {
        if let Err(error) = app.updater.install() {
            app.exiting.store(0, Ordering::Release);
            app.core.resume_after_failed_install();
            return Err(format!("安装未完成，更新包已保留，可重试：{error}"));
        }
    }
    handle.request_restart();
    Ok(())
}

#[tauri::command]
fn forget_plugin(app: State<Application>, id: String) -> Result<(), String> {
    app.core.forget(&id)
}
#[tauri::command]
fn set_theme(app: State<Application>, theme: String) -> Result<(), String> {
    app.core.set_theme(&theme)
}
#[tauri::command]
async fn read_logs(
    app: State<'_, Application>,
    filter: LogFilter,
) -> Result<Vec<LogRecord>, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.logs(&filter))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn read_messages(app: State<Application>) -> Vec<Message> {
    app.core.messages()
}
#[tauri::command]
fn mark_read(app: State<Application>, id: Option<String>) -> Result<(), String> {
    app.core.mark_read(id.as_deref())
}

#[tauri::command]
fn frontend_ready(handle: tauri::AppHandle, window: tauri::WebviewWindow, app: State<Application>) {
    if window.label() != "main" {
        return;
    }
    app.frontend_loaded.store(true, Ordering::Release);
    if !app.smoke
        || app
            .exiting
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    let core = app.core.clone();
    let plugin = app.plugin_test.clone();
    let update_bundle = argument("--update-bundle-test");
    let ui_plugin = argument("--ui-plugin-test");
    let ui_test_requested = ui_plugin.is_some();
    let ssh_config = argument("--ssh-test-config");
    let ssh_test_requested = ssh_config.is_some();
    let persisted = app.verify_persisted;
    std::thread::spawn(move || {
        let validation = (|| {
            sodeck::validation::server_roundtrip(&core, persisted)?;
            sodeck::validation::workspace_roundtrip(&handle, &core)?;
            if let Some(bundle) = update_bundle {
                sodeck::validation::core_update_bundle(&handle, &bundle)?;
            }
            if let Some(directory) = plugin {
                sodeck::validation::plugin_roundtrip(&core, &directory, persisted)?;
            }
            if let Some(directory) = ui_plugin {
                sodeck::validation::component_ui_roundtrip(&handle, &core, &directory)?;
            }
            if let Some(config) = ssh_config {
                tauri::async_runtime::block_on(sodeck::validation::ssh_roundtrip(
                    &handle, &core, &config,
                ))?;
            }
            Ok::<(), String>(())
        })();
        let stopped = core.shutdown();
        let result = validation.and(stopped);
        if result.is_ok() {
            let status = core.snapshot();
            let report = serde_json::json!({"completed":true,"workspaceNative":true,"componentUiNative":ui_test_requested,"sshNative":ssh_test_requested,"version":status.version,"target":status.target});
            let _ = std::fs::write(
                status.data_directory.join("smoke-result.json"),
                report.to_string(),
            );
        }
        let code = if let Err(error) = result {
            let path = core.snapshot().data_directory.join("smoke-error.txt");
            let _ = std::fs::write(path, error);
            1
        } else {
            0
        };
        handle
            .state::<Application>()
            .exiting
            .store(2, Ordering::Release);
        handle.exit(code);
    });
}

fn argument(name: &str) -> Option<PathBuf> {
    let mut args = std::env::args_os();
    args.find(|a| a == name)
        .and_then(|_| args.next())
        .map(PathBuf::from)
}

fn request_exit(handle: &tauri::AppHandle) {
    let app = handle.state::<Application>();
    if app.frontend_loaded.load(Ordering::Acquire) && !app.smoke {
        if let Some(main) = handle.get_webview_window("main") {
            let _ = main.set_focus();
            let _ = main.emit("core-request-close", ());
        }
    } else {
        begin_exit(handle);
    }
}

fn begin_exit(handle: &tauri::AppHandle) {
    let app = handle.state::<Application>();
    if app
        .exiting
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let core = app.core.clone();
    let handle = handle.clone();
    std::thread::spawn(move || match core.shutdown() {
        Ok(()) => {
            handle
                .state::<Application>()
                .exiting
                .store(2, Ordering::Release);
            handle.exit(0);
        }
        Err(error) => {
            handle
                .state::<Application>()
                .exiting
                .store(0, Ordering::Release);
            let _ = handle.emit("core-exit-error", error);
        }
    });
}

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let app = tauri::Builder::default()
        .register_uri_scheme_protocol("sodeck-plugin", |context, request| {
            let path = request.uri().path().trim_start_matches('/');
            let result = path.split_once('/').ok_or("缺少插件资源路径".to_string()).and_then(|(plugin, relative)| {
                context.app_handle().state::<Application>().core.ui_resource(plugin, relative)
            });
            let (status, bytes, mime) = match result {
                Ok((bytes, mime)) => (200, bytes, mime),
                Err(e) => (404, e.into_bytes(), "text/plain; charset=utf-8"),
            };
            tauri::http::Response::builder().status(status)
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'self' http://tauri.localhost tauri: http://localhost:1420")
                .body(bytes).unwrap()
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let directory = if smoke {
                argument("--test-data-dir").unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("sodeck-smoke-{}", std::process::id()))
                })
            } else {
                app.path().app_data_dir()?
            };
            let core = AppCore::open(directory, sodeck::app_version())?;
            let handle = app.handle().clone();
            core.set_listener(Arc::new(move |name, value| {
                let _ = handle.emit(name, value);
            }));
            app.manage(Application {
                core: core.clone(),
                updater: CoreUpdater::default(),
                exiting: AtomicU8::new(0),
                smoke,
                plugin_test: smoke.then(|| argument("--plugin-test")).flatten(),
                verify_persisted: smoke && std::env::args().any(|a| a == "--verify-persisted"),
                frontend_loaded: AtomicBool::new(false),
            });
            app.manage(DesktopWindows::new(core.clone()));
            if !smoke {
                std::thread::spawn(move || core.start_enabled());
            }
            #[cfg(target_os = "macos")]
            {
                let window = app.get_webview_window("main").unwrap();
                window.set_decorations(true)?;
                window.set_title_bar_style(tauri::TitleBarStyle::Overlay)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .state::<Application>()
                    .exiting
                    .load(Ordering::Acquire)
                    != 2
                {
                    api.prevent_close();
                    if window.label() == "main" {
                        request_exit(window.app_handle());
                    } else {
                        let desktop = window.state::<DesktopWindows>();
                        let empty = desktop.core.workspace_projection(window.label()).is_ok_and(|p| desktop.core.component_ui.lock().unwrap().require_closed(&p.window.workspace_ids).is_ok());
                        if empty {
                            let handle = window.app_handle().clone(); let label = window.label().to_owned();
                            tauri::async_runtime::spawn_blocking(move || {
                                if let Err(error) = handle.state::<DesktopWindows>().close_secondary(&handle, &label) { let _ = handle.emit("core-exit-error", error); }
                            });
                        } else { let _ = window.emit("workspace-request-close", ()); }
                    }
                }
            }
            if let tauri::WindowEvent::Focused(true) = event {
                window.state::<DesktopWindows>().focused(window.label());
            }
            if let tauri::WindowEvent::Destroyed = event {
                let desktop = window.state::<DesktopWindows>();
                if let Ok(p) = desktop.core.workspace_projection(window.label()) {
                    for id in p.window.workspace_ids { desktop.core.cancel_workspace_connection(&id); }
                }
                desktop.core.cancel_window_connections(window.label());
                desktop
                    .core
                    .workspaces
                    .lock()
                    .unwrap()
                    .lost_window(window.label());
                desktop.remove_surface(window.label());
                desktop.core.workspace_changed();
            }
        })
        .invoke_handler(tauri::generate_handler![
            component_data,
            network_snapshot,
            sodeck::connections::desktop::connection,
            server_catalog,
            change_catalog,
            recover_catalog,
            snapshot,
            close_app,
            add_plugin,
            official_plugins,
            install_official_plugin,
            set_plugin_enabled,
            set_plugin_update_source,
            update_status,
            check_update,
            download_update,
            restart_app,
            forget_plugin,
            set_theme,
            read_logs,
            read_messages,
            mark_read,
            frontend_ready,
            request_main_action,
            windows::workspace_snapshot,
            windows::workspace_ready,
            windows::workspace_surface,
            windows::workspace_activate,
            windows::workspace_layout,
            windows::workspace_move,
            windows::workspace_cancel_move,
            windows::workspace_close,
            windows::workspace_close_window,
            windows::workspace_drag_start,
            windows::workspace_drag_sample,
            windows::workspace_drag_finish
            ,sodeck::component_ui::component_ui
        ])
        .build(tauri::generate_context!())
        .expect("failed to start SoDeck");
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if handle
                .state::<Application>()
                .exiting
                .load(Ordering::Acquire)
                == 2
            {
                return;
            }
            api.prevent_exit();
            request_exit(handle);
        }
    });
}
