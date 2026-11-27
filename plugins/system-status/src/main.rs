use sodeck_plugin_client::protocol::requests as api;
#[cfg(test)]
mod collection_tests;
mod collectors;
mod filesystem;
mod metrics;
mod model;
mod mounts;
mod scheduler;
mod shared_state;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Arc::new(Mutex::new(None::<Arc<Mutex<scheduler::App>>>));
    sodeck_plugin_client::run(move |client, callback| {
        let (method, p) = callback.into_parts();
        if method == "initialize" {
            let settings = client.request(api::call::Settings(api::Page {
                page_id: "sampling".to_string(),
            }))?;
            let interval = interval(&settings["values"]["dynamicIntervalSeconds"])?;
            let state = Arc::new(Mutex::new(scheduler::App::new(interval)));
            scheduler::start(state.clone(), client.clone());
            *app.lock().unwrap() = Some(state);
            return Ok(json!({}));
        }
        let state = app.lock().unwrap().clone().ok_or("插件未初始化")?;
        let mut a = state.lock().unwrap();
        if method == "settings.changed" {
            if p["pageId"] != "sampling" {
                return Err("未知设置页".into());
            }
            let revision = p["revision"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("设置修订无效")?;
            let n = interval(&p["values"]["dynamicIntervalSeconds"])?;
            if revision > a.settings_revision {
                a.interval = n;
                a.settings_revision = revision;
                for v in a.views.values_mut() {
                    v.revision += 1;
                }
            }
            return Ok(json!({"accepted":true,"revision":revision.to_string()}));
        }
        let id = p["viewId"].as_str().ok_or("缺少视图")?;
        match method {
            "ui.createView" => {
                if p["viewType"] != "overview" {
                    return Err("未知视图".into());
                }
                a.mount(id, &p["context"])?;
            }
            "ui.disposeView" => {
                a.views.remove(id);
                return Ok(json!({}));
            }
            "ui.getView" | "ui.beforeClose" | "ui.save" => {}
            "ui.event" => {
                let d = &p["data"];
                let data = match d["action"].as_str().unwrap_or("") {
                    "snapshot" => a.snapshot(id, d["knownRevision"].as_u64())?,
                    "list" => a.list(id, d)?,
                    "history" => {
                        let v = a.views.get(id).ok_or("视图已关闭")?;
                        let key = d["seriesKey"]
                            .as_str()
                            .filter(|s| s.len() <= 320)
                            .ok_or("曲线标识无效")?;
                        json!({"revision":v.revision,"points":v.history.get(key).cloned().unwrap_or_default()})
                    }
                    action => a.control(id, action, d)?,
                };
                if serde_json::to_vec(&data).map_err(|e| e.to_string())?.len() > 24 * 1024 {
                    return Err("数据超出单次展示范围".into());
                }
                return Ok(
                    json!({"metadata":{"title":"系统状态","dirty":false,"busy":false},"data":data}),
                );
            }
            _ => return Err("未知系统状态方法".into()),
        }
        Ok(
            json!({"metadata":{"title":"系统状态","dirty":false,"busy":false},"data":a.snapshot(id,None)?}),
        )
    })
}
fn interval(v: &Value) -> Result<u64, String> {
    let n = v
        .as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(5);
    if ![3, 5, 10, 30].contains(&n) {
        return Err("采样间隔无效".into());
    }
    Ok(n)
}
