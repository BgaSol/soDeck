#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sodeck::{
    desktop_updates::CoreUpdater,
    model::*,
    updates::{ReleaseClient, UpdateView},
    AppCore,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU8, Ordering},
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
}

#[tauri::command]
fn snapshot(app: State<Application>) -> Snapshot {
    app.core.snapshot()
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
    if app.core.updates_busy() {
        return Err("请等待更新检查或下载完成再重启".into());
    }
    app.exiting
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "应用正在退出或重启")?;
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
fn frontend_ready(handle: tauri::AppHandle, app: State<Application>) {
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
    let persisted = app.verify_persisted;
    std::thread::spawn(move || {
        let validation = (|| {
            if let Some(bundle) = update_bundle {
                sodeck::validation::core_update_bundle(&handle, &bundle)?;
            }
            if let Some(directory) = plugin {
                sodeck::validation::plugin_roundtrip(&core, &directory, persisted)?;
            }
            Ok::<(), String>(())
        })();
        let stopped = core.shutdown();
        let result = validation.and(stopped);
        if result.is_ok() {
            let status = core.snapshot();
            let report = serde_json::json!({"completed":true,"version":status.version,"target":status.target});
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
            });
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
                    begin_exit(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            add_plugin,
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
            frontend_ready
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
            begin_exit(handle);
        }
    });
}
