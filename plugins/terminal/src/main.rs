use serde_json::json;
use std::{collections::HashMap, sync::Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terminals = Mutex::new(HashMap::<String, (String, String)>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
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
            if let Some((id, _)) = terminal {
                client.call("terminal.close", json!({"id":id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.mount" {
            let existing = terminals.lock().unwrap().get(view).cloned();
            let (id, title) = if let Some(existing) = existing {
                existing
            } else {
                let result =
                    client.call("terminal.open", json!({"viewId":view,"cols":80,"rows":24}))?;
                let id = result["id"]
                    .as_str()
                    .ok_or("核心没有返回终端 ID")?
                    .to_string();
                let mut all = terminals.lock().unwrap();
                let next = all.len() + 1;
                all.entry(view.into())
                    .or_insert_with(|| (id, format!("终端 {next}")))
                    .clone()
            };
            return Ok(
                json!({"data":{"streamId":id,"title":title},"metadata":{"title":title,"dirty":false,"busy":false}}),
            );
        }
        match method {
            "ui.beforeClose" | "ui.save" => Ok(json!({"metadata":{"dirty":false,"busy":false}})),
            _ => Err("未知终端插件方法".into()),
        }
    })
}
