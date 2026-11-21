mod checks;
mod commands;
mod publication;
mod rules;
mod runtime;
mod service;
mod socks5;
mod ui;
use serde_json::json;
use std::sync::{Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let holder = Arc::new(Mutex::new(None::<Arc<service::Service>>));
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    sodeck_plugin_client::run_with_commands(commands::HANDLERS, move |client, method, p| {
        if method == "initialize" {
            for capability in [
                "net.tcp.v1",
                "net.listenerLifecycle.v1",
                "ui.dialog.v1",
                "commands.v1",
                "sharedState.v1",
                "ui.background.v1",
                "ui.openView.v1",
            ] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == capability))
                {
                    return Err(format!("核心缺少 {capability}，请升级 SoDeck").into());
                }
            }
            let directory = p["dataDirectory"].as_str().ok_or("缺少数据目录")?;
            *holder.lock().unwrap() = Some(service::Service::new(directory.into(), client.clone()));
            return Ok(json!({"apiVersion":"1","settingsApiVersion":"1","uiApiVersion":"2"}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["defaults"]).map_err(Into::into);
        }
        let service = holder.lock().unwrap().clone().ok_or("插件尚未初始化")?;
        if method == "commands.handle" {
            commands::handle(&service, client, &p)
        } else {
            ui::handle(&service, client, method, &p).map_err(Into::into)
        }
    })
}
