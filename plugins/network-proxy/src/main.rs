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
    sodeck_plugin_client::run_with_commands(commands::HANDLERS, move |client, callback| {
        let (method, p) = callback.into_parts();
        if method == "initialize" {
            let directory = p["dataDirectory"].as_str().ok_or("缺少数据目录")?;
            *holder.lock().unwrap() = Some(service::Service::new(directory.into(), client.clone()));
            return Ok(json!({}));
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
