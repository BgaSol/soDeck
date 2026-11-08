use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
mod integration;
mod session;
use session::Session;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sessions = Mutex::new(HashMap::<String, Arc<Session>>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            for capability in ["terminal.session.v1", "viewData.v1", "sharedState.v1"] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|value| value == capability))
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
        if method == "ui.mount" {
            let session = {
                let mut all = sessions.lock().unwrap();
                let number = all.len() + 1;
                all.entry(view.into())
                    .or_insert_with(|| Session::new(view, p["context"].clone(), number))
                    .clone()
            };
            session.start(client);
            return Ok(
                json!({"data":session.data(client),"metadata":{"title":session.title(),"dirty":false,"busy":false}}),
            );
        }
        let session = sessions
            .lock()
            .unwrap()
            .get(view)
            .cloned()
            .ok_or("终端会话不存在")?;
        match method {
            "ui.event" if p["data"]["type"] == "terminal.status" => {
                Ok(json!({"data":session.data(client)}))
            }
            "ui.event"
                if p["data"]["type"] == "terminal.action" && p["data"]["actionId"] == "reopen" =>
            {
                let data = session.data(client);
                if data["runtime"]["state"] != "closed"
                    && data["session"]["preparation"] != "failed"
                {
                    return Err("终端尚未结束".into());
                }
                let state = client.call("session.state", json!({"viewId":view}))?;
                if state["state"] != "connected" {
                    return Err("连接已断开，请先在工作台重新连接".into());
                }
                Ok(json!({"data":{"viewType":"terminal","params":Value::Null}}))
            }
            "ui.disposeView" => {
                session.close(client)?;
                sessions.lock().unwrap().remove(view);
                Ok(json!({}))
            }
            "ui.beforeClose" | "ui.save" => Ok(json!({"metadata":{"dirty":false,"busy":false}})),
            _ => Err("未知终端插件方法".into()),
        }
    })
}
