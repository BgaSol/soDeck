//! Opt-in native-package validation. Uses the actual WebView-created surfaces.
use crate::{workspaces::Context, AppCore};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
async fn wait<T>(mut read: impl FnMut() -> Option<T>) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(v) = read() {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "原生插件界面未完成挂载或数据回传".into())
}
fn view(core: &AppCore, context: &Context, key: &str) -> Option<String> {
    core.ui_snapshot("main").ok()?["workspaces"][&context.workspace_id]["views"]
        .as_array()?
        .iter()
        .find(|v| v["definitionKey"] == key)?["id"]
        .as_str()
        .map(str::to_string)
}
async fn file_job(core: &Arc<AppCore>, token: &str, id: &str) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let v = core
                .files_data("main", token, json!({"id":id,"action":"status"}))
                .await?;
            match v["state"].as_str() {
                Some("completed") => return Ok(v["result"].clone()),
                Some("failed") => return Err(v["error"].as_str().unwrap_or("文件任务失败").into()),
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    })
    .await
    .map_err(|_| "原生文件任务超时".to_string())?
}
pub(super) async fn roundtrip(
    core: &Arc<AppCore>,
    context: &Context,
    remote: &str,
) -> Result<(), String> {
    let (Ok(terminal), Ok(files)) = (
        std::env::var("SODECK_TEST_TERMINAL_PLUGIN"),
        std::env::var("SODECK_TEST_FILES_PLUGIN"),
    ) else {
        return Ok(());
    };
    for (id, path) in [("sodeck.terminal", terminal), ("sodeck.files", files)] {
        if !core
            .snapshot()
            .plugins
            .iter()
            .any(|p| p.manifest.id == id && p.status == crate::model::PluginStatus::Running)
        {
            core.load(std::path::Path::new(&path))?;
        }
    }
    core.ui_snapshot("main")?;
    core.ui_action(
        "main",
        context,
        json!({"op":"open","key":"sodeck.terminal:terminal","requestId":"native-terminal-open"}),
    )?;
    core.workspace_changed();
    let tv = wait(|| view(core, context, "sodeck.terminal:terminal"))
        .await
        .map_err(|e| format!("terminal view: {e}"))?;
    let (window, token) = wait(|| core.ui_ready_surface(&tv)).await.map_err(|e| {
        format!(
            "terminal surface: {e}; {}",
            core.ui_snapshot("main").unwrap_or_default()
        )
    })?;
    let (id, output, sequence) = core
        .terminals
        .validation_surface(&tv, &token)
        .ok_or("终端原语未创建")?;
    let command = if cfg!(windows) {
        b"echo sodeck-native-pty-ok\r".to_vec()
    } else {
        b"printf 'sodeck-native-pty-ok\\n'\r".to_vec()
    };
    core.terminal_data(
        &window,
        &token,
        crate::terminal::DataRequest::Write {
            id,
            sequence,
            data: command,
        },
        None,
    )
    .await
    .map_err(|e| format!("terminal native checkpoint: {e}"))?;
    // Only the shipped xterm UI can create this checkpoint: PTY -> Tauri Channel ->
    // iframe -> xterm -> serialized display -> native IPC. No browser transport mock.
    wait(|| {
        output
            .checkpoint_bytes()
            .windows(b"sodeck-native-pty-ok".len())
            .any(|s| s == b"sodeck-native-pty-ok")
            .then_some(())
    })
    .await?;
    let opened=core.ui_action("main",context,json!({"op":"open","key":"sodeck.files:editor","requestId":"native-file-open","resourceKey":remote,"params":{"path":remote}}))?;
    let fv = opened["viewId"].as_str().ok_or("没有编辑器视图")?;
    let (_, token) = wait(|| core.ui_ready_surface(fv)).await?;
    let id = core.files.validation_handle(fv).ok_or("文件原语未创建")?;
    // The iframe reads its document itself over the real native data endpoint.
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let d = core
                .files_data("main", &token, json!({"id":id,"action":"describe"}))
                .await?;
            if d["size"].as_u64().is_some() {
                break Ok::<_, String>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "原生文件正文没有读入".to_string())??;
    let bytes = b"sodeck-native-file-save\n";
    core.files_data(
        "main",
        &token,
        json!({"id":id,"action":"stage","data":bytes,"begin":true,"done":true,"offset":0}),
    )
    .await?;
    core.ui_event("main", &token, json!({"action":"staged","revision":1}))?;
    core.files_data("main", &token, json!({"id":id,"action":"save"}))
        .await?;
    file_job(core, &token, &id).await?;
    let local = core
        .snapshot()
        .data_directory
        .join("native-file-download.txt");
    core.files_download(
        "main",
        &token,
        json!({"id":id,"path":remote}),
        local.clone(),
    )
    .await?;
    file_job(core, &token, &id).await?;
    if std::fs::read(&local).map_err(|e| e.to_string())? != bytes {
        return Err("实际包保存/下载内容不一致".into());
    }
    std::fs::remove_file(local).map_err(|e| e.to_string())?;
    core.ui_event("main", &token, json!({"action":"saved","revision":1}))?;
    Ok(())
}
