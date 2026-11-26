//! Actual packaged WebView check, using only the isolated smoke-test data directory.
use crate::{model::PublishMessage, AppCore};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tauri::{Listener, Manager};

pub fn verify(handle: &tauri::AppHandle, core: &Arc<AppCore>) -> Result<(), String> {
    core.log(
        None,
        "info",
        "精密工作台原生日志验收",
        Some(json!({"nativeProbe":"logs-messages","literal":"<b>plain text</b>"})),
    );
    let message = core.publish(
        None,
        PublishMessage {
            title: "精密工作台原生消息验收".into(),
            body: "原生正文\n<b>plain text</b>".into(),
            level: "info".into(),
            notify: false,
            dedupe_key: Some("logs-messages-native".into()),
        },
    )?;
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = handle.listen("logs-messages-native-result", move |event| {
        let _ = tx.send(event.payload().to_string());
    });
    let script = include_str!("logs_messages.js").replace(
        "__MESSAGE_ID__",
        &serde_json::to_string(&message.id).map_err(|e| e.to_string())?,
    );
    let evaluated = handle
        .get_webview_window("main")
        .ok_or("日志消息原生验收主窗口不存在")?
        .eval(&script)
        .map_err(|e| e.to_string());
    let result = evaluated.and_then(|_| {
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "日志消息原生验收回执超时".to_string())
    });
    handle.unlisten(listener);
    let value: serde_json::Value = serde_json::from_str(&result?).map_err(|e| e.to_string())?;
    std::fs::write(
        core.snapshot()
            .data_directory
            .join("logs-messages-native.json"),
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if value["ok"] != true {
        return Err(format!("日志消息原生验收失败：{}", value["error"]));
    }
    Ok(())
}
