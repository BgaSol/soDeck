mod evidence;
mod mcp;
mod tasks;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};
fn token(root: &Path) -> Result<String, String> {
    let path = root.join("access-token");
    match fs::read_to_string(&path) {
        Ok(value) => {
            if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(value);
            }
            return Err("接入 Token 文件无效".into());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let value = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(value.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(value)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    let state = Arc::new(Mutex::new(None::<Arc<tasks::App>>));
    let app_state = state.clone();
    let settings_cache = sodeck_plugin_client::settings::SettingsCache::default();
    let result = sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            for cap in [
                "session.target.v1",
                "session.stream.process.v1",
                "files.pluginData.v1",
                "files.operations.v2",
                "ui.background.v1",
                "settings.v1",
            ] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps.iter().any(|v| v == cap))
                {
                    return Err(format!("核心缺少 {cap}，请升级 SoDeck"));
                }
            }
            let root =
                std::path::PathBuf::from(p["dataDirectory"].as_str().ok_or("缺少插件数据目录")?);
            fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            fs::create_dir_all(root.join("evidence")).map_err(|e| e.to_string())?;
            let settings = client.call("settings.get", json!({"pageId":"connection"}))?;
            let port = settings["values"]["port"]
                .as_u64()
                .or_else(|| {
                    settings["values"]["port"]
                        .as_str()
                        .and_then(|v| v.parse().ok())
                })
                .unwrap_or(19846);
            if !(1024..=65535).contains(&port) {
                return Err("端口无效".into());
            }
            let app = Arc::new(tasks::App::new(
                client.clone(),
                root.clone(),
                token(&root)?,
                port as u16,
                handle.clone(),
            ));
            *app_state.lock().unwrap() = Some(app.clone());
            handle.spawn(async move {
                if let Err(e) = mcp::serve(app.clone()).await {
                    *app.http_state.lock().unwrap() = e.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        app.client
                            .call("logging.write", json!({"level":"error","message":e}))
                    })
                    .await;
                }
            });
            return Ok(json!({"apiVersion":"1","uiApiVersion":"2","settingsApiVersion":"1"}));
        }
        let app = app_state.lock().unwrap().clone().ok_or("插件未初始化")?;
        if method == "settings.changed" {
            return settings_cache.changed(&p, &["connection"]);
        }
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        match method {
            "ui.createView" => {
                if p["viewType"] != "status" {
                    return Err("未知视图".into());
                }
                app.mount(view, &p["context"])?;
            }
            "ui.disposeView" => {
                app.dispose(view);
                return Ok(json!({}));
            }
            "ui.beforeClose" | "ui.save" => return Ok(json!({"allow":true})),
            "ui.getView" => {}
            "ui.event" => match p["data"]["action"].as_str().unwrap_or("") {
                "snapshot" => {}
                "config" => return Ok(json!({"data":{"config":app.config()}})),
                "cancel" => {
                    app.cancel(&p["data"]).map_err(|e| e.to_string())?;
                }
                _ => return Err("未知操作".into()),
            },
            _ => return Err("未知方法".into()),
        }
        Ok(
            json!({"metadata":{"title":"AI 排障接入","dirty":false,"busy":false},"data":app.view(view)?}),
        )
    });
    if let Some(app) = state.lock().unwrap().as_ref() {
        app.shutdown.cancel();
    }
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    result
}
