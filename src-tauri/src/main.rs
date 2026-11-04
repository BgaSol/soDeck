#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sodeck::{model::*, AppCore};
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
async fn load_plugin(
    app: State<'_, Application>,
    directory: PathBuf,
) -> Result<PluginView, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.load(&directory))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn unload_plugin(app: State<'_, Application>, id: String) -> Result<PluginView, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.unload(&id))
        .await
        .map_err(|e| e.to_string())?
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
    let persisted = app.verify_persisted;
    std::thread::spawn(move || {
        let validation = (|| {
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
                core,
                exiting: AtomicU8::new(0),
                smoke,
                plugin_test: smoke.then(|| argument("--plugin-test")).flatten(),
                verify_persisted: smoke && std::env::args().any(|a| a == "--verify-persisted"),
            });
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
            load_plugin,
            unload_plugin,
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
