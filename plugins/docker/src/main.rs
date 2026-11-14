mod commands;
mod runtime;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let holder = Arc::new(Mutex::new(None::<runtime::Shared>));
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            for cap in [
                "commands.v1",
                "session.exec.v1",
                "ui.workbench.v2",
                "ui.openView.v1",
                "settings.v1",
            ] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|c| c == cap))
                {
                    return Err(format!("核心缺少 {cap}，请升级 SoDeck"));
                }
            }
            let directory = p["dataDirectory"].as_str().ok_or("缺少插件数据目录")?;
            let mut state = runtime::App::new(directory.into())?;
            let settings = client.call("settings.get", json!({"pageId":"display"}))?;
            apply_settings(&mut state, &settings["values"])?;
            let app = Arc::new(Mutex::new(state));
            runtime::start(app.clone(), client.clone());
            *holder.lock().unwrap() = Some(app);
            return Ok(json!({"apiVersion":"1","uiApiVersion":"2","settingsApiVersion":"1"}));
        }
        let app = holder
            .lock()
            .unwrap()
            .clone()
            .ok_or("Docker 插件未初始化")?;
        if method == "settings.changed" {
            let mut a = app.lock().unwrap();
            if p["pageId"] != "display" {
                return Err("未知设置页".into());
            }
            let revision = p["revision"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("设置修订无效")?;
            if revision > a.settings_revision {
                apply_settings(&mut a, &p["values"])?;
                a.settings_revision = revision;
                for v in a.views.values_mut() {
                    v.revision += 1;
                }
            }
            return Ok(json!({"accepted":true,"revision":revision.to_string()}));
        }
        let id = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.event"
            && matches!(
                p["data"]["action"].as_str(),
                Some("openContainer" | "openLogs")
            )
        {
            let d = &p["data"];
            let (context, resource) = {
                let a = app.lock().unwrap();
                let v = a.views.get(id).ok_or("视图已关闭")?;
                if v.frozen {
                    return Err("页面正在关闭".into());
                }
                let container = d["id"].as_str().ok_or("缺少容器 ID")?;
                commands::id(container)?;
                if v.expected_engine.is_empty() {
                    return Err("请先刷新 Docker 状态".into());
                }
                for (key, expected) in [
                    ("engineId", json!(v.expected_engine)),
                    ("endpoint", json!(v.endpoint)),
                    ("expectedContext", v.context.clone()),
                ] {
                    if d.get(key).is_some_and(|value| value != &expected) {
                        return Err("容器连接身份已改变，请刷新后重新选择".into());
                    }
                }
                (
                    v.context.clone(),
                    json!({"id":container,"engineId":v.expected_engine}).to_string(),
                )
            };
            let request = d["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("请求 ID 无效")?;
            let result=client.call("ui.openView",json!({"expectedContext":context,"requestId":request,
                "viewType":if d["action"]=="openLogs"{"logs"}else{"container"},"resourceKey":resource,"sourceViewId":id}))?;
            return app.lock().unwrap().response(id, result);
        }
        let mut a = app.lock().unwrap();
        match method {
            "ui.createView" => a.create(&p)?,
            "ui.disposeView" => {
                a.views.remove(id);
                return Ok(json!({}));
            }
            "ui.getView" => {
                if let Some(v) = a
                    .views
                    .get_mut(id)
                    .filter(|v| !p["context"].is_null() && v.context != p["context"])
                {
                    v.clear_terminal();
                    v.context = p["context"].clone();
                    v.cancelled = true;
                    v.busy = false;
                    v.data["state"] = json!("stale");
                    v.data["error"] = json!("连接代次改变，请刷新");
                    if matches!(
                        v.data["operation"]["state"].as_str(),
                        Some("checking" | "running")
                    ) {
                        v.data["operation"]["state"] =
                            json!(if v.data["operation"]["state"] == "running" {
                                "unknown"
                            } else {
                                "failed"
                            });
                        v.data["operation"]["message"] =
                            json!("连接上下文改变；操作不会自动重发，请刷新状态");
                    }
                    v.revision += 1;
                }
            }
            "ui.beforeClose" => {
                a.views.get_mut(id).ok_or("视图已关闭")?.frozen = true;
            }
            "ui.save" => {
                if a.views.get(id).ok_or("视图已关闭")?.busy {
                    return Err("请等待 Docker 请求结束，或取消请求后关闭".into());
                }
            }
            "ui.event" => {
                let data = a.event(id, &p["data"])?;
                return a.response(id, data);
            }
            _ => return Err("未知 Docker 方法".into()),
        }
        let data = a.snapshot(id)?;
        a.response(id, data)
    })
}
fn apply_settings(a: &mut runtime::App, values: &Value) -> Result<(), String> {
    let read = |key: &str, default: u64| {
        values[key]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| values[key].as_u64())
            .unwrap_or(default)
    };
    let refresh = read("refreshSeconds", 5);
    let tail = read("logTail", 200);
    if ![5, 10, 30].contains(&refresh) || ![50, 200, 500].contains(&tail) {
        return Err("Docker 设置值无效".into());
    }
    a.refresh_seconds = refresh;
    a.tail = tail;
    Ok(())
}
