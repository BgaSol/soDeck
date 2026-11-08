use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
mod integration;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terminals = Mutex::new(HashMap::<String, TerminalView>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    let can_launch = AtomicBool::new(false);
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            can_launch.store(
                ["terminal.launch.v1", "session.exec.v1"]
                    .iter()
                    .all(|wanted| {
                        p["capabilities"]
                            .as_array()
                            .is_some_and(|c| c.iter().any(|v| v == wanted))
                    }),
                Ordering::Relaxed,
            );
            for capability in ["terminal.v1", "viewData.v1"] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|v| v == capability))
                {
                    return Err(format!("核心缺少 {capability}，请先升级 SoDeck"));
                }
            }
            return Ok(json!({"apiVersion":"1","settingsApiVersion":"1"}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["appearance"]);
        }
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.disposeView" {
            let terminal = terminals.lock().unwrap().remove(view);
            if let Some(terminal) = terminal {
                client.call("terminal.close", json!({"id":terminal.id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.mount" {
            let mut all = terminals.lock().unwrap();
            let existing = all.get(view).cloned();
            let terminal = if let Some(existing) = existing {
                existing
            } else {
                let preferences = client
                    .call("settings.get", json!({"pageId":"appearance"}))
                    .unwrap_or(Value::Null);
                let mode = preferences["values"]["directoryMode"]
                    .as_str()
                    .unwrap_or("auto");
                let mut params = json!({"viewId":view,"cols":80,"rows":24});
                let integration = if mode != "auto" {
                    json!({"mode":mode})
                } else if !can_launch.load(Ordering::Relaxed) {
                    json!({"mode":"unavailable","reasonCode":"old-core"})
                } else {
                    match integration::prepare(client, view, &p["context"]) {
                        Ok((command, state)) => {
                            params["command"] = json!(command);
                            state
                        }
                        Err(reason) => json!({"mode":"unavailable","reasonCode":reason}),
                    }
                };
                let result = client.call("terminal.open", params)?;
                let id = result["id"]
                    .as_str()
                    .ok_or("核心没有返回终端 ID")?
                    .to_string();
                let next = all.len() + 1;
                all.entry(view.into())
                    .or_insert_with(|| TerminalView {
                        id,
                        title: format!("终端 {next}"),
                        context: p["context"].clone(),
                        integration,
                    })
                    .clone()
            };
            return Ok(
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
            );
        }
        match method {
            "ui.beforeClose" | "ui.save" => Ok(json!({"metadata":{"dirty":false,"busy":false}})),
            _ => Err("未知终端插件方法".into()),
        }
    })
}
#[derive(Clone)]
struct TerminalView {
    id: String,
    title: String,
    context: Value,
    integration: Value,
}
