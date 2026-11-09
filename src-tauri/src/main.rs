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
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Emitter, Manager, State};

struct Application {
    core: Arc<AppCore>,
    updater: CoreUpdater,
    exit_handoff: AtomicBool,
    smoke: bool,
    interactive_test: bool,
    verify_persisted: bool,
    frontend_loaded: AtomicBool,
}
#[tauri::command]
fn close_register_host(
    window: tauri::WebviewWindow,
    app: State<Application>,
) -> Result<(), String> {
    app.core.register_close_host(window.label())
}

#[tauri::command]
async fn settings_catalog(app: State<'_, Application>) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.settings_catalog())
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn settings_page(
    app: State<'_, Application>,
    plugin_id: String,
    page_id: String,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.settings_page(&plugin_id, &page_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_settings(
    window: tauri::WebviewWindow,
    app: State<'_, Application>,
    plugin_id: String,
    page_id: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    let window = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        core.save_settings_for_window(&window, &plugin_id, &page_id, input)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn retry_settings_apply(
    app: State<'_, Application>,
    plugin_id: String,
    page_id: String,
    revision: String,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.retry_settings_apply(&plugin_id, &page_id, &revision)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn settings_for_surface(
    app: State<'_, Application>,
    window: tauri::WebviewWindow,
    surface_id: String,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || core.settings_for_surface(&label, &surface_id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn settings_applied(
    app: State<'_, Application>,
    window: tauri::WebviewWindow,
    surface_id: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || core.settings_applied(&label, &surface_id, input))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn save_settings_for_surface(
    app: State<'_, Application>,
    window: tauri::WebviewWindow,
    surface_id: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let core = app.core.clone();
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        core.save_settings_for_surface(&label, &surface_id, input)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clipboard_text(
    handle: tauri::AppHandle,
    operation: String,
    text: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    // Native reads must stay off the event loop (in particular on Linux).
    tauri::async_runtime::spawn_blocking(move || match operation.as_str() {
        "read" => {
            let text = match handle.clipboard().read_text() {
                Ok(text) => text,
                // The pinned official plugin erases arboard's error variants.
                Err(tauri_plugin_clipboard_manager::Error::Clipboard(message))
                    if message == "The clipboard contents were not available in the requested format or the clipboard is empty." => String::new(),
                Err(error) => return Err(format!("无法读取系统剪贴板：{error}")),
            };
            if text.len() > 256 * 1024 {
                return Err("剪贴板超过 256 KiB，请分段粘贴；本次未发送".into());
            }
            Ok(Some(text))
        }
        "write" => {
            let text = text.ok_or("缺少剪贴板文本")?;
            if text.len() > 256 * 1024 {
                return Err("剪贴板文本超过 256 KiB".into());
            }
            handle
                .clipboard()
                .write_text(text)
                .map_err(|e| format!("无法写入系统剪贴板：{e}"))?;
            Ok(None)
        }
        _ => Err("未知剪贴板操作".into()),
    })
    .await
    .map_err(|e| e.to_string())?
}

// Explicit isolated smoke mode only; restore the previous text even after a failed assertion.
async fn clipboard_roundtrip(handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let original = {
        let handle = handle.clone();
        tauri::async_runtime::spawn_blocking(move || handle.clipboard().read_text())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("剪贴板烟测需要已有文本以便恢复：{e}"))?
    };
    if original.len() > 256 * 1024 {
        return Err("剪贴板烟测的原有文本超过恢复上限".into());
    }
    let sample = "SoDeck 原生剪贴板 🙂\r\nsecond line\n".repeat(300);
    let result = async {
        clipboard_text(handle.clone(), "write".into(), Some(sample.clone())).await?;
        if clipboard_text(handle.clone(), "read".into(), None).await? != Some(sample) {
            return Err("原生剪贴板文本往返不一致".into());
        }
        if clipboard_text(handle.clone(), "write".into(), Some("中".repeat(90_000)))
            .await
            .is_ok()
        {
            return Err("原生剪贴板未拒绝超限文本".into());
        }
        Ok(())
    }
    .await;
    let restored = clipboard_text(handle, "write".into(), Some(original)).await;
    result.and(restored.map(|_| ()))
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
        let action = request["action"].as_str().unwrap_or("");
        let file = request["id"].as_str().unwrap_or("").to_string();
        if action == "input.reveal" {
            let path = app.core.files_local_directory(
                window.label(),
                &surface_id,
                &file,
                request["inputId"].as_str().ok_or("缺少下载目标")?,
            )?;
            sodeck::desktop_files::reveal_directory(window.app_handle().clone(), path).await?;
            return Ok(serde_json::json!({}));
        }
        if action == "input.revision" || action == "input.clipboard" {
            app.core.validate_file_download(
                window.label(),
                &surface_id,
                &serde_json::json!({"id":file,"path":"/"}),
            )?;
            let snapshot = sodeck::desktop_files::clipboard(
                window.app_handle().clone(),
                action == "input.clipboard",
            )
            .await?;
            if action == "input.revision" {
                return Ok(sodeck::desktop_files::clipboard_info(&snapshot));
            }
            if snapshot.paths.is_empty() {
                return Ok(serde_json::json!({"empty":true,"revision":snapshot.revision}));
            }
            let mut value = app.core.files_register_input(
                window.label(),
                &surface_id,
                &file,
                snapshot.paths,
                false,
            )?;
            value["revision"] = serde_json::json!(snapshot.revision);
            return Ok(value);
        }
        if action == "input.drop" {
            app.core.validate_file_download(
                window.label(),
                &surface_id,
                &serde_json::json!({"id":file,"path":"/"}),
            )?;
            let paths = app.core.desktop_files.claim(
                window.label(),
                request["dropId"].as_str().ok_or("缺少拖入来源")?,
            )?;
            return app
                .core
                .files_register_input(window.label(), &surface_id, &file, paths, false);
        }
        if action == "input.chooseUpload" || action == "input.chooseDownload" {
            use tauri_plugin_dialog::DialogExt;
            let started = app.core.files_begin_input(
                window.label(),
                &surface_id,
                &file,
                request["action"] == "input.chooseDownload",
            )?;
            let operation = started["id"].as_str().unwrap().to_owned();
            let core = app.core.clone();
            let window = window.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let dialog = window.dialog().file();
                let result = (|| {
                    let paths = if request["action"] == "input.chooseUpload" {
                        if request["directory"] == true {
                            dialog.blocking_pick_folders()
                        } else {
                            dialog.blocking_pick_files()
                        }
                    } else if request["single"] == true {
                        dialog
                            .set_file_name(request["name"].as_str().unwrap_or("download"))
                            .blocking_save_file()
                            .map(|p| vec![p])
                    } else {
                        dialog.blocking_pick_folder().map(|p| vec![p])
                    };
                    paths
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| p.into_path().map_err(|e| e.to_string()))
                        .collect::<Result<Vec<_>, _>>()
                })();
                core.files_finish_input(window.label(), &surface_id, &file, &operation, result);
            });
            return Ok(started);
        }
        if request["action"] == "download" || request["action"] == "exportDraft" {
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
            if request["action"] == "exportDraft" {
                return app
                    .core
                    .files_export(window.label(), &surface_id, request, local)
                    .await;
            }
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
    if !matches!(action.as_str(), "exit" | "restart" | "install") {
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
async fn official_plugins(
    app: State<'_, Application>,
    refresh: bool,
) -> Result<sodeck::official_plugins::OfficialCatalog, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.official_plugins(&core.release_client()?, refresh)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn install_official_plugin(
    app: State<'_, Application>,
    id: String,
) -> Result<String, String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.install_official_plugin(&id, &core.release_client()?)
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
fn update_overview(app: State<Application>) -> sodeck::updates::UpdateOverview {
    app.core.update_overview()
}

#[tauri::command]
fn set_automatic_updates(app: State<Application>, enabled: bool) -> Result<(), String> {
    app.core.set_automatic_updates(enabled)
}

#[tauri::command]
fn set_update_network(
    app: State<Application>,
    expected_revision: u64,
    preferences: sodeck::update_network::Preferences,
) -> Result<sodeck::update_network::Preferences, String> {
    app.core.set_update_network(expected_revision, preferences)
}

#[tauri::command]
async fn test_update_mirror(
    app: State<'_, Application>,
    prefix: Option<String>,
) -> Result<Vec<sodeck::update_network::Diagnostic>, String> {
    let client = app.core.release_client()?;
    tauri::async_runtime::spawn_blocking(move || client.diagnose(prefix.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_update_release(handle: tauri::AppHandle) -> Result<(), String> {
    sodeck::updates::open_release_page(handle).await
}

#[tauri::command]
async fn open_update_link(handle: tauri::AppHandle, url: String) -> Result<(), String> {
    sodeck::updates::open_web_page(handle, &url).await
}

#[tauri::command]
async fn open_usage_help(handle: tauri::AppHandle) -> Result<(), String> {
    sodeck::updates::open_web_page(
        handle,
        "https://github.com/BgaSol/soDeck/blob/main/docs/usage.md",
    )
    .await
}

async fn check_all(handle: tauri::AppHandle, automatic: bool) -> Result<(), String> {
    let app = handle.state::<Application>();
    let Some(mut batch) = app.core.begin_update_check(automatic) else {
        return Ok(());
    };
    let client = Arc::new(app.core.release_client()?);
    for id in app.core.update_check_ids() {
        let result = if id == sodeck::updates::CORE_ID {
            app.updater
                .check_with_client(&handle, &app.core, client.clone())
                .await
        } else {
            let core = app.core.clone();
            let client = client.clone();
            tauri::async_runtime::spawn_blocking(move || {
                core.check_component(&id, &client).map(|_| ())
            })
            .await
            .map_err(|e| e.to_string())?
        };
        if result.is_ok() {
            batch.succeeded();
        }
    }
    Ok(())
}

#[tauri::command]
async fn check_updates(handle: tauri::AppHandle, automatic: bool) -> Result<(), String> {
    check_all(handle, automatic).await
}

async fn download_reserved(
    handle: tauri::AppHandle,
    task: (String, sodeck::updates::Operation),
    client: Arc<ReleaseClient>,
) {
    let (id, operation) = task;
    let app = handle.state::<Application>();
    let result = if id == sodeck::updates::CORE_ID {
        app.updater
            .download_reserved_with_client(&app.core, operation, client)
            .await
    } else {
        let core = app.core.clone();
        let plugin_id = id.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            core.download_plugin_reserved(&plugin_id, &client, operation)
                .map(|_| ())
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(error.to_string()),
        }
    };
    if let Err(error) = result {
        app.core.fail_update(&id, error);
    }
}

#[tauri::command]
async fn download_updates(handle: tauri::AppHandle, ids: Vec<String>) -> Result<(), String> {
    let client = Arc::new(handle.state::<Application>().core.release_client()?);
    let tasks = handle
        .state::<Application>()
        .core
        .reserve_update_downloads(&ids)?;
    let mut tasks = std::collections::VecDeque::from(tasks);
    while let Some(first) = tasks.pop_front() {
        let second = tasks.pop_front();
        let other = handle.clone();
        let second_client = client.clone();
        tokio::join!(
            download_reserved(handle.clone(), first, client.clone()),
            async move {
                if let Some(task) = second {
                    download_reserved(other, task, second_client).await;
                }
            }
        );
    }
    Ok(())
}

async fn background_updates(handle: tauri::AppHandle) {
    let copy = handle.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let app = copy.state::<Application>();
        app.updater.restore(&copy, &app.core)
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    loop {
        if handle.state::<Application>().core.is_closing() {
            break;
        }
        let _ = check_all(handle.clone(), true).await;
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
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
        core.check_component(&id, &core.release_client()?)
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
    tauri::async_runtime::spawn_blocking(move || core.download_plugin(&id, &core.release_client()?))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn close_prepare(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    app: State<'_, Application>,
    request_id: String,
    target: sodeck::lifecycle::Target,
) -> Result<sodeck::lifecycle::Operation, String> {
    if matches!(
        target,
        sodeck::lifecycle::Target::Application {
            action: sodeck::lifecycle::Action::InstallAndRestart
        }
    ) {
        let verify = handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let app = verify.state::<Application>();
            app.updater.verify_for_close(&app.core)
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    app.core.close_prepare(window.label(), &request_id, target)
}
#[tauri::command]
fn close_status(
    window: tauri::WebviewWindow,
    app: State<Application>,
    operation_id: String,
) -> Result<sodeck::lifecycle::Operation, String> {
    app.core.close_status(window.label(), &operation_id)
}
#[tauri::command]
fn close_operations(
    window: tauri::WebviewWindow,
    app: State<Application>,
) -> Vec<sodeck::lifecycle::Operation> {
    app.core.close_operations(window.label())
}
#[tauri::command]
fn close_report_inputs(
    window: tauri::WebviewWindow,
    app: State<Application>,
    operation_id: String,
    input_round: u64,
    report: serde_json::Value,
) -> Result<sodeck::lifecycle::Operation, String> {
    app.core
        .close_report_inputs(window.label(), &operation_id, input_round, report)
}
#[tauri::command]
fn close_decide(
    window: tauri::WebviewWindow,
    app: State<Application>,
    operation_id: String,
    decision_revision: u64,
    decision: String,
) -> Result<sodeck::lifecycle::Operation, String> {
    app.core
        .close_decide(window.label(), &operation_id, decision_revision, &decision)
}
#[tauri::command]
fn close_retry(
    window: tauri::WebviewWindow,
    app: State<Application>,
    operation_id: String,
) -> Result<sodeck::lifecycle::Operation, String> {
    app.core.close_retry(window.label(), &operation_id)
}
#[tauri::command]
fn close_exit_after_failure(
    window: tauri::WebviewWindow,
    app: State<Application>,
    operation_id: String,
) -> Result<sodeck::lifecycle::Operation, String> {
    app.core
        .close_exit_after_failure(window.label(), &operation_id)
}
#[tauri::command]
async fn restart_app(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    install_core: bool,
) -> Result<(), String> {
    if install_core {
        let verify = handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            verify.state::<Application>().updater.verify_ready()
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    app.core.close_native_prepare(
        "main",
        sodeck::lifecycle::Target::Application {
            action: if install_core {
                sodeck::lifecycle::Action::InstallAndRestart
            } else {
                sodeck::lifecycle::Action::Restart
            },
        },
    )?;
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
    let first = !app.frontend_loaded.swap(true, Ordering::AcqRel);
    if first && !app.smoke && !app.interactive_test {
        tauri::async_runtime::spawn(background_updates(handle.clone()));
    }
    if !app.smoke || !first || app.interactive_test {
        return;
    }
    let core = app.core.clone();
    let update_bundle = argument("--update-bundle-test");
    let ssh_config = argument("--ssh-test-config");
    let ssh_test_requested = ssh_config.is_some();
    let clipboard_test_requested = std::env::args().any(|a| a == "--clipboard-test");
    let desktop_files_probe = argument("--desktop-files-test");
    let desktop_files_requested = desktop_files_probe.is_some();
    let persisted = app.verify_persisted;
    std::thread::spawn(move || {
        let validation = (|| {
            if clipboard_test_requested {
                tauri::async_runtime::block_on(clipboard_roundtrip(handle.clone()))?;
            }
            if let Some(probe) = desktop_files_probe {
                tauri::async_runtime::block_on(sodeck::validation::desktop_files_roundtrip(
                    &handle, &probe,
                ))?;
            }
            sodeck::validation::server_roundtrip(&core, persisted)?;
            sodeck::validation::workspace_roundtrip(&handle, &core)?;
            if let Some(bundle) = update_bundle {
                sodeck::validation::core_update_bundle(&handle, &bundle)?;
            }
            if let Some(config) = ssh_config {
                tauri::async_runtime::block_on(sodeck::validation::ssh_roundtrip(
                    &handle, &core, &config,
                ))?;
            }
            Ok::<(), String>(())
        })();
        let stopped = core
            .shutdown()
            .and_then(|_| sodeck::desktop_files::shutdown(&handle));
        let result = validation.and(stopped);
        if result.is_ok() {
            let status = core.snapshot();
            let report = serde_json::json!({"completed":true,"workspaceNative":true,"sshNative":ssh_test_requested,"clipboardNative":clipboard_test_requested,"desktopFilesNative":desktop_files_requested,"version":status.version,"target":status.target});
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
            .exit_handoff
            .store(true, Ordering::Release);
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
    match app.core.close_native_prepare(
        "main",
        sodeck::lifecycle::Target::Application {
            action: sodeck::lifecycle::Action::Exit,
        },
    ) {
        Ok(op) => {
            app.core.close_unhosted(&op.id);
            if let Some(main) = handle.get_webview_window("main") {
                let _ = main.set_focus();
            }
        }
        Err(error) => {
            let _ = handle.emit("core-exit-error", error);
        }
    }
}
fn begin_exit(handle: &tauri::AppHandle) {
    request_exit(handle);
}

fn attach_native_finalizer(handle: &tauri::AppHandle, core: &Arc<AppCore>) {
    use sodeck::lifecycle::{Action, Target};
    let (finalize, requests) =
        std::sync::mpsc::sync_channel::<(Target, std::sync::mpsc::Sender<Result<(), String>>)>(4);
    core.set_finalizer(Arc::new(move |target| {
        let (reply, result) = std::sync::mpsc::channel();
        finalize
            .send((target, reply))
            .map_err(|_| "原生收尾通道已关闭")?;
        result.recv().map_err(|_| "原生收尾结果未确认")?
    }));
    let native = handle.clone();
    std::thread::spawn(move || {
        while let Ok((target, reply)) = requests.recv() {
            let global = matches!(target, Target::Application { .. });
            let result = (|| match target {
                Target::Window { id } => {
                    native.state::<DesktopWindows>().finish_window(&native, &id)
                }
                Target::Application { action } => {
                    let app = native.state::<Application>();
                    if action == Action::InstallAndRestart {
                        if let Err(error) = app.updater.install() {
                            app.core.failed_core_install(error.clone());
                            return Err(error);
                        }
                    }
                    sodeck::desktop_files::shutdown(&native)?;
                    app.exit_handoff.store(true, Ordering::Release);
                    if action == Action::Exit {
                        native.exit(0);
                    } else {
                        native.request_restart();
                    }
                    Ok(())
                }
                _ => Ok(()),
            })();
            let ended = global && result.is_ok();
            let _ = reply.send(result);
            if ended {
                break;
            }
        }
    });
}

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let interactive_test = std::env::args().any(|arg| arg == "--interactive-test");
    let app = tauri::Builder::default()
        .plugin(sodeck::desktop_behavior::plugin())
        .register_uri_scheme_protocol("sodeck-plugin", |context, request| {
            let path = request.uri().path().trim_start_matches('/');
            let result = path.split_once('/').ok_or("缺少插件资源路径".to_string()).and_then(|(plugin, relative)| {
                let core = &context.app_handle().state::<Application>().core;
                if let Some(path) = relative.strip_prefix("settings/") {
                    let (token, asset) = path.split_once('/').ok_or("缺少设置资源版本")?;
                    core.settings_resource(plugin, token, asset)
                } else { core.ui_resource(plugin, relative) }
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_webview_event(|webview,event| {
            if let tauri::WebviewEvent::DragDrop(event)=event {
                if let Some(app)=webview.try_state::<Application>() {app.core.desktop_files.event(webview,event);}
            }
        })
        .setup(move |app| {
            let directory = if interactive_test {
                argument("--test-data-dir").ok_or("交互验收必须指定隔离的 --test-data-dir")?
            } else if smoke {
                argument("--test-data-dir").unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("sodeck-smoke-{}", std::process::id()))
                })
            } else {
                app.path().app_data_dir()?
            };
            let core = AppCore::open(directory, sodeck::app_version())?;
            let handle = app.handle().clone();
            core.set_listener(Arc::new(move |name, value| {
                if name == "component-state-changed" {
                    if let Some(window)=value["windowId"].as_str() {let _=handle.emit_to(window,name,value.clone());}
                } else {let _ = handle.emit(name, value);}
            }));
            app.manage(Application {
                core: core.clone(),
                updater: CoreUpdater::default(),
                exit_handoff: AtomicBool::new(false),
                smoke,
                interactive_test,
                verify_persisted: smoke && std::env::args().any(|a| a == "--verify-persisted"),
                frontend_loaded: AtomicBool::new(false),
            });
            app.manage(DesktopWindows::new(core.clone()));
            attach_native_finalizer(app.handle(), &core);
            if !smoke {
                std::thread::spawn(move || core.start_enabled());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .state::<Application>()
                    .exit_handoff
                    .load(Ordering::Acquire) == false
                {
                    api.prevent_close();
                    if window.label() == "main" {
                        request_exit(window.app_handle());
                    } else {
                        let desktop = window.state::<DesktopWindows>();
                        if let Err(error)=desktop.close_secondary(window.app_handle(),window.label()){let _=window.emit("core-exit-error",error);}
                    }
                }
            }
            if let tauri::WindowEvent::Focused(true) = event {
                window.state::<DesktopWindows>().focused(window.label());
            }
            if matches!(event,tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                if let Some(webview)=window.app_handle().get_webview_window(window.label()) {window.state::<DesktopWindows>().record_bounds(&webview);}
            }
            if let tauri::WindowEvent::Destroyed = event {
                let desktop = window.state::<DesktopWindows>();
                desktop.core.close_host_lost(window.label());
                desktop.remove_surface(window.label());
                desktop.core.workspace_changed();
                if window.label()=="main" && !desktop.core.is_closing() {
                    let native=window.app_handle().clone();let recreate=native.clone();
                    let _=native.run_on_main_thread(move || {
                        let _=recreate.state::<DesktopWindows>().restore_main_display(&recreate);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            clipboard_text,
            settings_catalog,
            settings_page,
            save_settings,
            retry_settings_apply,
            settings_for_surface,
            settings_applied,
            save_settings_for_surface,
            component_data,
            network_snapshot,
            sodeck::connections::desktop::connection,
            server_catalog,
            change_catalog,
            recover_catalog,
            snapshot,
            close_app,
            close_register_host,
            close_prepare,close_status,close_operations,close_report_inputs,close_decide,close_retry,close_exit_after_failure,
            add_plugin,
            official_plugins,
            install_official_plugin,
            set_plugin_enabled,
            set_plugin_update_source,
            update_status,
            update_overview,
            check_updates,
            download_updates,
            set_automatic_updates,
            set_update_network,
            test_update_mirror,
            open_update_release,
            open_update_link,
            open_usage_help,
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
            windows::workbench_move,
            windows::workbench_return_window,
            windows::workbench_restore_layout,
            windows::workbench_locate_view,
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
        if matches!(event, tauri::RunEvent::Exit) {
            handle.state::<Application>().core.detach_host();
        }
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if handle
                .state::<Application>()
                .exit_handoff
                .load(Ordering::Acquire)
            {
                return;
            }
            api.prevent_exit();
            if handle.get_webview_window("main").is_none()
                && !handle.state::<Application>().core.is_closing()
            {
                let _ = handle
                    .state::<DesktopWindows>()
                    .restore_main_display(handle);
            } else {
                request_exit(handle);
            }
        }
    });
}
