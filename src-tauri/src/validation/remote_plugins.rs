//! Opt-in native-package validation. Uses the actual WebView-created surfaces.
use crate::{workspaces::Context, AppCore};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
async fn wait<T>(stage: &str, mut read: impl FnMut() -> Option<T>) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(v) = read() {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| format!("原生插件等待超时（{stage}，15 秒）"))
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
async fn terminal_settings(
    core: &Arc<AppCore>,
    view_id: &str,
    surface: &str,
) -> Result<(), String> {
    // Older independently released plugins retain the original smoke protocol.
    if !core
        .snapshot()
        .plugins
        .iter()
        .any(|p| p.manifest.id == "sodeck.terminal" && p.manifest.settings.is_some())
    {
        return Ok(());
    }
    let initial = core.settings_page("sodeck.terminal", "appearance")?;
    if std::env::args().any(|a| a == "--verify-persisted")
        && (initial["values"]["fontSize"] != 18
            || initial["values"]["paddingBottom"] != 40
            || initial["values"]["font"]["id"] != "ibm-plex-mono")
    {
        return Err("实际包重开后未恢复终端外观设置".into());
    }
    let terminal_id = core
        .terminals
        .validation_surface(view_id)
        .ok_or("设置修改前终端不存在")?
        .0;
    let mut fonts = vec![
        json!({"source":"preset","id":"jetbrains-mono"}),
        json!({"source":"preset","id":"source-code-pro"}),
    ];
    if cfg!(windows) {
        fonts.push(json!({"source":"local","family":"Consolas"}));
    }
    fonts.push(json!({"source":"preset","id":"ibm-plex-mono"}));
    for font_value in fonts {
        let font = font_value["id"]
            .as_str()
            .or_else(|| font_value["family"].as_str())
            .unwrap();
        let current = core.settings_page("sodeck.terminal", "appearance")?;
        let mut values = current["values"].clone();
        values["font"] = font_value.clone();
        values["fontSize"] = json!(18);
        values["paddingBottom"] = json!(40);
        let saved = core.save_settings(
            "sodeck.terminal",
            "appearance",
            json!({"definitionToken":current["definitionToken"],"baseRevision":current["revision"],"requestId":format!("native-font-{font}"),"values":values}),
        )?;
        if saved["persisted"] != true {
            return Err("实际包终端设置保存冲突".into());
        }
        // This report comes from the shipped iframe after native FontFace.load and PTY resize.
        // No injected browser transport or font mock participates in this validation.
        wait(&format!("终端字体 {font} 应用"), || {
            core.settings_page("sodeck.terminal", "appearance").ok()?["applications"]
                .as_array()?
                .iter()
                .find(|a| {
                    a["surfaceId"] == surface && a["revision"] == saved["snapshot"]["revision"]
                })
                .map(|a| match a["error"].as_str() {
                    Some(error) => Err(format!("实际包字体 {font} 应用失败：{error}")),
                    None => Ok(()),
                })
        })
        .await??;
        if core
            .terminals
            .validation_surface(view_id)
            .is_none_or(|(id, _)| id != terminal_id)
        {
            return Err("修改外观重新创建了 PTY".into());
        }
    }
    Ok(())
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
    let tv = wait("终端视图创建", || {
        view(core, context, "sodeck.terminal:terminal")
    })
    .await
    .map_err(|e| format!("terminal view: {e}"))?;
    let (window, token) = wait("终端界面挂载", || core.ui_ready_surface(&tv))
        .await
        .map_err(|e| {
            format!(
                "terminal surface: {e}; {}",
                core.ui_snapshot("main").unwrap_or_default()
            )
        })?;
    let (id, output) = core
        .terminals
        .validation_surface(&tv)
        .ok_or("终端原语未创建")?;
    terminal_settings(core, &tv, &token).await?;
    let command = if cfg!(windows) {
        b"echo sodeck-native-pty-ok\r".to_vec()
    } else {
        b"printf 'sodeck-native-pty-ok\\n'\r".to_vec()
    };
    core.terminal_validation_write(&window, &token, &id, command)
        .await
        .map_err(|e| format!("terminal native checkpoint: {e}"))?;
    // Only the shipped xterm UI can create this checkpoint: PTY -> Tauri Channel ->
    // iframe -> xterm -> serialized display -> native IPC. No browser transport mock.
    wait("终端显示检查点", || {
        output
            .checkpoint_bytes()
            .windows(b"sodeck-native-pty-ok".len())
            .any(|s| s == b"sodeck-native-pty-ok")
            .then_some(())
    })
    .await?;
    let opened=core.ui_action("main",context,json!({"op":"open","key":"sodeck.files:editor","requestId":"native-file-open","resourceKey":remote,"params":{"path":remote}}))?;
    let fv = opened["viewId"].as_str().ok_or("没有编辑器视图")?;
    let (_, token) = wait("文件编辑器界面挂载", || core.ui_ready_surface(fv)).await?;
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
    if core
        .snapshot()
        .plugins
        .iter()
        .any(|p| p.manifest.id == "sodeck.files" && p.manifest.settings.is_some())
    {
        let settings = core.settings_page("sodeck.files", "editor")?;
        // Mounting and describe do not prove that the iframe consumed all chunks.
        // The current editor reports its applied preferences after reading the document.
        wait("文件编辑器设置应用", || {
            core.settings_page("sodeck.files", "editor").ok()?["applications"]
                .as_array()?
                .iter()
                .find(|a| a["surfaceId"] == token && a["revision"] == settings["revision"])
                .map(|a| match a["error"].as_str() {
                    Some(error) => Err(format!("实际包文件编辑器应用设置失败：{error}")),
                    None => Ok(()),
                })
        })
        .await??;
    }
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
