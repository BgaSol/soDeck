#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sodeck::{
    desktop_shell::{self, DesktopShell},
    desktop_updates::UpdateService,
    model::*,
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
    updates: Arc<UpdateService>,
    login_startup: Arc<sodeck::login_startup::Service>,
    smoke: bool,
    interactive_test: bool,
    verify_persisted: bool,
    smoke_started: AtomicBool,
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
    purpose: Option<String>,
) -> Result<Option<String>, String> {
    let limit = match purpose.as_deref() {
        None => 256 * 1024,
        Some("serverShare") => sodeck::servers::SHARE_LIMIT,
        _ => return Err("剪贴板用途无效".into()),
    };
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
            if text.len() > limit {
                return Err(format!("剪贴板文本超过 {} KiB", limit / 1024));
            }
            Ok(Some(text))
        }
        "write" => {
            let text = text.ok_or("缺少剪贴板文本")?;
            if text.len() > limit {
                return Err(format!("剪贴板文本超过 {} KiB", limit / 1024));
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
        clipboard_text(handle.clone(), "write".into(), Some(sample.clone()), None).await?;
        if clipboard_text(handle.clone(), "read".into(), None, None).await? != Some(sample) {
            return Err("原生剪贴板文本往返不一致".into());
        }
        if clipboard_text(
            handle.clone(),
            "write".into(),
            Some("中".repeat(90_000)),
            None,
        )
        .await
        .is_ok()
        {
            return Err("原生剪贴板未拒绝超限文本".into());
        }
        Ok(())
    }
    .await;
    let restored = clipboard_text(handle, "write".into(), Some(original), None).await;
    result.and(restored.map(|_| ()))
}

#[tauri::command]
async fn component_data(
    app: State<'_, Application>,
    window: tauri::WebviewWindow,
    surface_id: String,
    request: serde_json::Value,
    channel: tauri::ipc::Channel<sodeck::streams::Frame>,
) -> Result<serde_json::Value, sodeck_plugin_protocol::ApiError> {
    if request["op"] == "pluginData" && request["action"] == "reveal" {
        let relative: Vec<String> =
            serde_json::from_value(request["relative"].clone()).map_err(|e| e.to_string())?;
        let path = app
            .core
            .plugin_data_directory(window.label(), &surface_id, &relative)?;
        sodeck::desktop_files::reveal_directory(window.app_handle().clone(), path).await?;
        return Ok(serde_json::json!({}));
    }
    if request["op"] == "processOutput" {
        let sink = Arc::new(move |frame| channel.send(frame).map_err(|e| e.to_string()))
            as sodeck::streams::Sink;
        return app
            .core
            .process_output_data(window.label(), &surface_id, request, Some(sink))
            .await
            .map_err(Into::into);
    }
    if request["op"] == "contentExport" {
        if request["action"] == "begin" {
            use tauri_plugin_dialog::DialogExt;
            let desktop = window.state::<DesktopShell>();
            let dialog_guard = desktop.present_modal(window.app_handle(), window.label())?;
            let started = app
                .core
                .content_export_begin(window.label(), &surface_id, &request)?;
            if started["started"] == true {
                let core = app.core.clone();
                let id = started["id"].as_str().unwrap().to_owned();
                window
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_file_name(request["name"].as_str().unwrap_or("logs.txt"))
                    .save_file(move |chosen| {
                        drop(dialog_guard);
                        let selected = chosen
                            .map(|p| p.into_path().map_err(|e| e.to_string()))
                            .transpose();
                        tauri::async_runtime::spawn_blocking(move || {
                            core.content_export_choose(&id, selected)
                        });
                    });
            }
            return Ok(started);
        }
        return app
            .core
            .content_export_data(window.label(), &surface_id, request)
            .await
            .map_err(Into::into);
    }
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
            app.core.validate_file_input(
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
            app.core.validate_file_input(
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
                .files_register_input(window.label(), &surface_id, &file, paths, false)
                .map_err(Into::into);
        }
        if action == "input.chooseUpload" || action == "input.chooseDownload" {
            use tauri_plugin_dialog::DialogExt;
            let desktop = window.state::<DesktopShell>();
            let dialog_guard = desktop.present_modal(window.app_handle(), window.label())?;
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
                let _dialog_guard = dialog_guard;
                let dialog = window.dialog().file().set_parent(&window);
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
                core.files_finish_input(
                    window.label(),
                    &surface_id,
                    &file,
                    &operation,
                    result.map_err(Into::into),
                );
            });
            return Ok(started);
        }
        if request["action"] == "export.start" {
            use tauri_plugin_dialog::DialogExt;
            let desktop = window.state::<DesktopShell>();
            let dialog_guard = desktop.present_modal(window.app_handle(), window.label())?;
            let (receipt, task) =
                app.core
                    .files_export_begin(window.label(), &surface_id, &request)?;
            if let Some(task) = task {
                let core = app.core.clone();
                let filename = request["path"]
                    .as_str()
                    .unwrap_or("draft.txt")
                    .rsplit('/')
                    .next()
                    .unwrap_or("draft.txt");
                let dialog = window
                    .dialog()
                    .file()
                    .set_parent(&window)
                    .set_file_name(filename);
                tauri::async_runtime::spawn(async move {
                    let chosen = tauri::async_runtime::spawn_blocking(move || {
                        let _dialog_guard = dialog_guard;
                        dialog.blocking_save_file()
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|v| {
                        v.map(|p| p.into_path().map_err(|e| e.to_string()))
                            .transpose()
                    });
                    core.files_export_finish(task, chosen).await;
                });
            }
            return Ok(receipt);
        }

        return app
            .core
            .files_data(window.label(), &surface_id, request)
            .await
            .map_err(Into::into);
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
        .map_err(Into::into)
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
async fn login_startup_state(
    app: State<'_, Application>,
) -> Result<sodeck::login_startup::Status, String> {
    let service = app.login_startup.clone();
    tauri::async_runtime::spawn_blocking(move || service.refresh())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_login_startup(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    enabled: bool,
) -> Result<sodeck::login_startup::Status, String> {
    if app.core.is_closing() {
        return Err("应用正在退出或重启，不能修改启动设置".into());
    }
    let service = app.login_startup.clone();
    let result = tauri::async_runtime::spawn_blocking(move || service.set(enabled))
        .await
        .map_err(|e| e.to_string())?;
    app.core.record_login_startup(enabled, &result);
    let _ = handle.emit("login-startup-changed", ());
    result
}
#[tauri::command]
async fn export_server_share(
    app: State<'_, Application>,
    revision: u64,
    server_id: Option<String>,
) -> Result<serde_json::Value, sodeck::servers::ShareError> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.export_server_share(revision, server_id))
        .await
        .map_err(|_| sodeck::servers::ShareError {
            code: "internal",
            message: "生成分享码失败，请重试".into(),
        })?
}
#[tauri::command]
async fn preview_server_share(
    app: State<'_, Application>,
    code: String,
) -> Result<serde_json::Value, sodeck::servers::ShareError> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.preview_server_share(&code))
        .await
        .map_err(|_| sodeck::servers::ShareError {
            code: "internal",
            message: "解析分享码失败，请重试".into(),
        })?
}
#[tauri::command]
async fn import_server_share(
    app: State<'_, Application>,
    revision: u64,
    code: String,
) -> Result<serde_json::Value, sodeck::servers::ShareError> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || core.import_server_share(revision, &code))
        .await
        .map_err(|_| sodeck::servers::ShareError {
            code: "internal",
            message: "导入结果未能返回，请刷新目录核对后再操作".into(),
        })?
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
    app.updates.install_official(id).await
}

#[tauri::command]
async fn retry_official_preparation(app: State<'_, Application>) -> Result<(), String> {
    app.updates.retry_official_preparation().await
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
fn update_snapshot(app: State<Application>) -> sodeck::updates::UpdateSnapshot {
    app.core.update_snapshot()
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

#[tauri::command]
async fn check_updates(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
) -> Result<(), String> {
    app.updates.check(&handle, None, false).await
}

#[tauri::command]
async fn download_updates(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    ids: Vec<String>,
) -> Result<(), String> {
    app.updates.download(&handle, ids).await
}

#[tauri::command]
async fn set_plugin_update_source(
    app: State<'_, Application>,
    id: String,
    source: Option<UpdateSource>,
    expected: Option<sodeck::model::UpdateSourceBaseline>,
) -> Result<(), String> {
    let core = app.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.set_update_source_checked(&id, source, expected)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_update(
    handle: tauri::AppHandle,
    app: State<'_, Application>,
    id: String,
) -> Result<(), String> {
    app.updates.check(&handle, Some(id), false).await
}

#[tauri::command]
async fn close_prepare(
    handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    request_id: String,
    target: sodeck::lifecycle::Target,
) -> Result<sodeck::lifecycle::Operation, String> {
    handle
        .state::<DesktopShell>()
        .prepare_close(&handle, window.label(), &request_id, target)
        .await
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
    let first = !app.smoke_started.swap(true, Ordering::AcqRel);
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
    let desktop_open_probe = argument("--desktop-open-test");
    let desktop_open_requested = desktop_open_probe.is_some();
    let persisted = app.verify_persisted;
    let package_only = std::env::args().any(|arg| arg == "--package-only");
    std::thread::spawn(move || {
        let restart_probe = std::env::args_os().any(|arg| arg == "--startup-restart-test");
        if restart_probe && !std::env::args_os().any(|arg| arg == "--restart-from") {
            let preparation = (|| {
                sodeck::desktop_startup::verify_native(&handle, &core)?;
                let path = core
                    .snapshot()
                    .data_directory
                    .join("startup-restart-origin.json");
                std::fs::write(path,serde_json::json!({"pid":std::process::id(),"login":std::env::args_os().any(|arg|arg=="--login-start")}).to_string()).map_err(|e|e.to_string())?;
                let operation = core.close_native_prepare(
                    "main",
                    sodeck::lifecycle::Target::Application {
                        action: sodeck::lifecycle::Action::Restart,
                    },
                )?;
                core.close_unhosted(&operation.id);
                Ok::<(), String>(())
            })();
            if let Err(error) = preparation {
                let _ = std::fs::write(
                    core.snapshot().data_directory.join("smoke-error.txt"),
                    error,
                );
                handle.exit(1);
            }
            return;
        }
        let validation = (|| {
            if restart_probe {
                let original: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(
                        core.snapshot()
                            .data_directory
                            .join("startup-restart-origin.json"),
                    )
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if original["pid"] == std::process::id()
                    || original["login"] != true
                    || std::env::args_os().any(|arg| arg == "--login-start")
                {
                    return Err("登录启动后的普通重启来源或进程身份错误".into());
                }
            }
            if package_only {
                sodeck::login_startup::verify_native(&core.snapshot().data_directory)?;
                sodeck::desktop_startup::verify_native(&handle, &core)?;
                DesktopShell::verify_native(&handle)?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while !handle.state::<Application>().updates.initialized() {
                    if std::time::Instant::now() >= deadline {
                        return Err("应用级更新服务未随原生应用启动".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                sodeck::validation::plugin_icons_roundtrip(&handle, &core)?;
                sodeck::validation::logs_messages_roundtrip(&handle, &core)?;
            }
            if clipboard_test_requested {
                tauri::async_runtime::block_on(clipboard_roundtrip(handle.clone()))?;
            }
            if let Some(probe) = desktop_files_probe {
                tauri::async_runtime::block_on(sodeck::validation::desktop_files_roundtrip(
                    &handle, &probe,
                ))?;
            }
            if let Some(probe) = desktop_open_probe {
                tauri::async_runtime::block_on(sodeck::validation::desktop_open_roundtrip(
                    &handle, &probe,
                ))?;
            }
            sodeck::validation::server_roundtrip(&core, persisted)?;
            if package_only {
                sodeck::validation::server_sharing_roundtrip(&handle, &core, persisted)?;
            }
            if !package_only {
                sodeck::validation::workspace_roundtrip(&handle, &core)?;
            }
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
            let report = serde_json::json!({"completed":true,"pid":std::process::id(),"workspaceNative":!package_only,"packageNative":true,"pluginIconsNative":package_only,"logsMessagesNative":package_only,"serverSharingNative":package_only,"desktopShellNative":package_only,"desktopStartupNative":package_only,"loginRegistrationIoNative":package_only,"loginRestartNative":restart_probe,"macosApplicationPolicyNative":package_only && cfg!(target_os = "macos"),"sshNative":ssh_test_requested,"clipboardNative":clipboard_test_requested,"desktopFilesNative":desktop_files_requested,"desktopOpenNative":desktop_open_requested,"version":status.version,"target":status.target});
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
        handle.state::<DesktopShell>().handoff();
        handle.exit(code);
    });
}

fn argument(name: &str) -> Option<PathBuf> {
    let mut args = std::env::args_os();
    args.find(|a| a == name)
        .and_then(|_| args.next())
        .map(PathBuf::from)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let text: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "SoDeck 无法启动".encode_utf16().chain(Some(0)).collect();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    let launch = sodeck::desktop_startup::Launch::parse(&args)?;
    let setup_launch = launch.clone();
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let interactive_test = std::env::args().any(|arg| arg == "--interactive-test");
    #[allow(unused_mut)]
    let mut app = tauri::Builder::default()
        .manage(sodeck::desktop_startup::Launch::restart_environment(&args))
        .plugin(sodeck::desktop_behavior::plugin())
        .register_uri_scheme_protocol("sodeck-plugin", |context, request| {
            let path = request.uri().path().trim_start_matches('/');
            let result = path.split_once('/').ok_or("缺少插件资源路径".to_string()).and_then(|(plugin, relative)| {
                let core = &context.app_handle().state::<Application>().core;
                if let Some(path) = relative.strip_prefix("icons/") {
                    let (version, name) = path.split_once('/').ok_or("缺少图标资源版本")?;
                    core.icon_resource(plugin, version, name)
                } else if let Some(path) = relative.strip_prefix("settings/") {
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
            let native_login = sodeck::login_startup::native_login_launch();
            if std::env::var("SODECK_EXPECT_NATIVE_LOGIN").as_deref() == Ok("1") && !native_login {
                return Err("原生登录启动事件未在 setup 中识别".into());
            }
            let lease = app.state::<sodeck::desktop_startup::Prepared>().0.lock().unwrap().take().ok_or("应用启动资格已被消费")?;
            let core = AppCore::open_locked(lease, sodeck::app_version())?;
            let login_startup = Arc::new(sodeck::login_startup::Service::new(smoke || interactive_test));
            app.manage(login_startup.clone());
            let handle = app.handle().clone();
            core.set_listener(Arc::new(move |name, value| {
                if handle.try_state::<DesktopShell>().is_some() { DesktopShell::on_core_event(&handle, name, &value); }
                if name == "component-state-changed" {
                    if let Some(window)=value["windowId"].as_str() {let _=handle.emit_to(window,name,value.clone());}
                } else {let _ = handle.emit(name, value);}
            }));
            let updates = Arc::new(UpdateService::new(core.clone()));
            app.manage(Application {
                core: core.clone(),
                updates: updates.clone(),
                login_startup,
                smoke,
                interactive_test,
                verify_persisted: smoke && std::env::args().any(|a| a == "--verify-persisted"),
                smoke_started: AtomicBool::new(false),
            });
            app.manage(DesktopWindows::new(core.clone()));
            app.manage(DesktopShell::new(core.clone(), updates.clone()));
            DesktopShell::attach_finalizer(app.handle(), &core);
            if let Err(error) = app.state::<DesktopShell>().install(app.handle()) {
                app.state::<DesktopShell>().report(app.handle(), format!("系统托盘创建失败：{error}"));
            }
            app.state::<DesktopWindows>().ensure_main_display(app.handle())?;
            if setup_launch.starts_background(native_login, app.state::<sodeck::desktop_startup::Activation>().take_pending())
                && (cfg!(target_os = "linux") || app.state::<DesktopShell>().tray_available())
            {
                if let Err(error) = app.state::<DesktopShell>().start_background(app.handle()) {
                    app.state::<DesktopShell>().present(app.handle(), "main", false)?;
                    app.state::<DesktopShell>().report(app.handle(), error);
                }
            } else {
                app.state::<DesktopShell>().present(app.handle(), "main", false)?;
            }
            let preparation = if !smoke && !interactive_test { updates.reserve_default_installation() } else { None };
            if !interactive_test {
                if smoke { core.set_automatic_updates(false)?; }
                updates.start(app.handle().clone());
            }
            if !smoke {
                std::thread::spawn(move || {
                    core.start_enabled();
                    if let Some(preparation) = preparation {
                        let _ = tauri::async_runtime::block_on(updates.prepare_official(preparation));
                    }
                });
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(shell) = webview.try_state::<DesktopShell>() { shell.page_started(webview.label()); }
            }
        })
        .on_window_event(|window, event| {
            let shell = window.state::<DesktopShell>();
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !shell.handed_off() { api.prevent_close(); shell.close_requested(window.app_handle(), window.label()); }
            }
            if let tauri::WindowEvent::Focused(true) = event { window.state::<DesktopWindows>().focused(window.label()); }
            if matches!(event, tauri::WindowEvent::Focused(_) | tauri::WindowEvent::Resized(_)) { DesktopShell::changed(window.app_handle()); }
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) { window.state::<DesktopWindows>().record_bounds(&webview); }
            }
            if let tauri::WindowEvent::Destroyed = event {
                shell.page_started(window.label());
                let desktop = window.state::<DesktopWindows>();
                desktop.core.close_host_lost(window.label()); desktop.remove_surface(window.app_handle(), window.label()); desktop.core.workspace_changed();
                if window.label() == "main" && !desktop.core.is_closing() && !shell.handed_off() {
                    let native = window.app_handle().clone(); let recreate = native.clone();
                    let _ = native.run_on_main_thread(move || {
                        if let Err(error) = recreate.state::<DesktopShell>().recover_main(&recreate) { recreate.state::<DesktopShell>().report(&recreate, error); }
                    });
                }
                shell.reconcile(window.app_handle());
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
            export_server_share,
            preview_server_share,
            import_server_share,
            change_catalog,
            recover_catalog,
            snapshot,
            login_startup_state,
            set_login_startup,
            close_register_host,
            close_prepare,close_status,close_operations,close_report_inputs,close_decide,close_retry,close_exit_after_failure,
            add_plugin,
            official_plugins,
            install_official_plugin,
            retry_official_preparation,
            set_plugin_enabled,
            set_plugin_update_source,
            update_snapshot,
            check_updates,
            download_updates,
            set_automatic_updates,
            set_update_network,
            test_update_mirror,
            open_update_release,
            open_update_link,
            open_usage_help,
            check_update,
            forget_plugin,
            set_theme,
            read_logs,
            read_messages,
            mark_read,
            frontend_ready,
            desktop_shell::desktop_present, desktop_shell::desktop_state, desktop_shell::desktop_ready, desktop_shell::desktop_navigation_done,
            desktop_shell::desktop_background_done, desktop_shell::set_close_behavior, desktop_shell::choose_local_path,
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
            windows::workspace_drag_release,
            windows::workspace_drag_finish
            ,sodeck::component_ui::component_ui
        ])
        .build(tauri::generate_context!())
        .map_err(|e| e.to_string())?;
    match sodeck::desktop_startup::prepare(app.handle(), &launch) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(error) => {
            use tauri_plugin_dialog::DialogExt;
            app.dialog()
                .message(&error)
                .title("SoDeck 无法启动")
                .blocking_show();
            std::process::exit(1);
        }
    }
    #[cfg(target_os = "macos")]
    // Defer activation until setup resolves the native login event and presents a window.
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            handle.state::<Application>().core.detach_host();
        }
        #[cfg(target_os = "macos")]
        if matches!(event, tauri::RunEvent::Reopen { .. }) {
            DesktopShell::menu(handle, "desktop.show");
        }
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if handle.state::<DesktopShell>().handed_off() {
                return;
            }
            api.prevent_exit();
            if handle.get_webview_window("main").is_none()
                && !handle.state::<Application>().core.is_closing()
            {
                let _ = handle.state::<DesktopShell>().recover_main(handle);
            } else {
                DesktopShell::request_exit(handle);
            }
        }
    });
    Ok(())
}
