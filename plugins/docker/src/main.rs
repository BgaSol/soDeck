use sodeck_plugin_client::protocol::requests as api;
mod browser;
mod cli;
mod collaboration;
mod commands;
mod connection;
mod logs;
mod object;
mod ports;
mod pulls;
mod resources;
mod runtime;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let holder = Arc::new(Mutex::new(None::<runtime::Shared>));
    let log_holder = Arc::new(Mutex::new(None::<Arc<logs::Logs>>));
    let pull_holder = Arc::new(Mutex::new(None::<Arc<pulls::Pulls>>));
    sodeck_plugin_client::run(move |client, callback| {
        let (method, p) = callback.into_parts();
        if method == "initialize" {
            let directory = p["dataDirectory"].as_str().ok_or("缺少插件数据目录")?;
            let mut state = runtime::App::new(directory.into())?;
            let settings = client.request(api::call::Settings(api::Page {
                page_id: "display".to_string(),
            }))?;
            apply_settings(&mut state, &settings["values"])?;
            let app = Arc::new(Mutex::new(state));
            runtime::start(app.clone(), client.clone());
            *pull_holder.lock().unwrap() = Some(pulls::Pulls::new(app.clone(), client.clone()));
            *log_holder.lock().unwrap() = Some(logs::Logs::new(client.clone(), app.clone()));
            *holder.lock().unwrap() = Some(app);
            return Ok(json!({}));
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
        if method == "ui.disposeView" {
            if let Some(pulls) = pull_holder.lock().unwrap().as_ref() {
                pulls.dispose(id);
            }
            if let Some(logs) = log_holder.lock().unwrap().as_ref() {
                logs.dispose(id)
            }
        }
        if method == "ui.event"
            && p["data"]["action"]
                .as_str()
                .is_some_and(|s| s.starts_with("pull."))
        {
            let service = pull_holder
                .lock()
                .unwrap()
                .clone()
                .ok_or("拉取服务未就绪")?;
            let result = service.event(id, &p["data"])?;
            return app.lock().unwrap().response(id, result).map_err(Into::into);
        }
        if method == "ui.event"
            && p["data"]["action"]
                .as_str()
                .is_some_and(|s| s.starts_with("logs."))
        {
            let target = {
                let a = app.lock().unwrap();
                let v = a.views.get(id).ok_or("日志页已关闭")?;
                if v.kind != "logs" {
                    return Err("不是日志页面".into());
                }
                if v.frozen && p["data"]["action"] != "logs.status" {
                    return Err("日志页面正在关闭".into());
                }
                a.snapshot(id)?
            };
            let service = log_holder
                .lock()
                .unwrap()
                .clone()
                .ok_or("日志服务尚未就绪")?;
            let result = service.event(id, target, &p["data"])?;
            return app.lock().unwrap().response(id, result).map_err(Into::into);
        }
        if method == "ui.event" && p["data"]["action"] == "openLogs" {
            let d = &p["data"];
            let (context, resource) = {
                let a = app.lock().unwrap();
                a.check_scope(id, d)?;
                let v = a.views.get(id).ok_or("视图已关闭")?;
                let container = d["id"].as_str().ok_or("缺少容器 ID")?;
                commands::id(container)?;
                if v.kind != "browser" || !v.container_rows().iter().any(|r| r["id"] == container) {
                    return Err("请从资源树选择容器".into());
                }
                (
                    v.context.clone(),
                    json!({"id":container,"engineId":v.expected_engine,"endpoint":v.endpoint})
                        .to_string(),
                )
            };
            let request = d["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("请求 ID 无效")?;
            let result=serde_json::from_value::<api::OpenView>(json!({"expectedContext":context,"requestId":request,"viewType":"logs","resourceKey":resource,"sourceViewId":id})).map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效")).and_then(|params| client.request(api::call::OpenView(params))).map_err(|e|e.message)?;
            return app
                .lock()
                .unwrap()
                .response(id, json!({"viewId":result.view_id}))
                .map_err(Into::into);
        }
        let mut a = app.lock().unwrap();
        match method {
            "ui.createView" => a.create(&p)?,
            "ui.disposeView" => {
                a.views.remove(id);
                a.prune_bindings();
                return Ok(json!({}));
            }
            "ui.getView" => {
                if let Some(v) = a
                    .views
                    .get_mut(id)
                    .filter(|v| !p["context"].is_null() && v.context != p["context"])
                {
                    v.clear_commands();
                    v.target = None;
                    v.epoch += 1;
                    if let Some(b) = &mut v.browser {
                        *b = browser::BrowserState::default();
                    }
                    if v.kind == "browser" {
                        v.expected_engine.clear();
                        v.endpoint.clear();
                    }
                    v.data["engine"] = Value::Null;
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
                return a.response(id, data).map_err(Into::into);
            }
            _ => return Err("未知 Docker 方法".into()),
        }
        let mut data = a.snapshot(id)?;
        a.prune_bindings();
        if data["kind"] == "logs" {
            if let Some(logs) = log_holder.lock().unwrap().as_ref() {
                data["logCapture"] = logs.snapshot(id)?;
            }
        }
        a.response(id, data).map_err(Into::into)
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

    if ![5, 10, 30].contains(&refresh) {
        return Err("Docker 设置值无效".into());
    }
    a.refresh_seconds = refresh;
    a.show_log_minimap = values["showLogMinimap"].as_bool().unwrap_or(true);
    Ok(())
}
