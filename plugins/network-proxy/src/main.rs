mod checks;
mod manager;
mod rules;
mod socks5;
use manager::{App, View};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Arc::new(Mutex::new(None::<App>));
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    sodeck_plugin_client::run(move |c, method, p| {
        if method == "initialize" {
            for capability in ["net.tcp.v1", "net.listenerLifecycle.v1", "ui.dialog.v1"] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v == capability))
                {
                    return Err(format!("核心缺少 {capability}，请升级 SoDeck"));
                }
            }
            let directory = PathBuf::from(p["dataDirectory"].as_str().ok_or("缺少数据目录")?);
            *app.lock().unwrap() = Some(App {
                rules: rules::Rules::open(directory),
                views: HashMap::new(),
                runs: HashMap::new(),
                checks: HashMap::new(),
                sequence: 0,
            });
            let state = app.clone();
            let client = c.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(35));
                if client.is_closed() {
                    break;
                }
                manager::tick(&state, &client);
            });
            return Ok(json!({"apiVersion":"1","settingsApiVersion":"1","uiApiVersion":"2"}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["defaults"]);
        }
        let id = p["viewId"].as_str().ok_or("缺少 viewId")?;
        let mut state = app.lock().unwrap();
        let a = state.as_mut().ok_or("插件尚未初始化")?;
        if method == "ui.getView" {
            if !a.views.contains_key(id) {
                return Err("视图尚未创建".into());
            }
            return Ok(view_data(a, id));
        }
        if method == "ui.createView" {
            let server = p["context"]["serverId"]
                .as_str()
                .ok_or("缺少服务器")?
                .to_owned();
            let kind = p["viewType"].as_str().ok_or("缺少视图类型")?.to_owned();
            if !["manager", "rule-editor"].contains(&kind.as_str()) {
                return Err("未知视图".into());
            }
            let draft = if kind == "rule-editor" {
                p["params"]["rule"].clone()
            } else {
                Value::Null
            };
            let v = a.views.entry(id.into()).or_insert(View {
                server,
                kind,
                context: p["context"].clone(),
                draft,
                dirty: false,
                navigation: Value::Null,
            });
            v.context = p["context"].clone();
            return Ok(view_data(a, id));
        }
        if method == "ui.disposeView" {
            if a.views.get(id).is_some_and(|v| v.kind == "manager") {
                let ids: Vec<_> = a
                    .runs
                    .iter()
                    .filter(|(_, r)| r.view == id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for run in ids {
                    let _ = a.stop(c, &run);
                }
            }
            a.views.remove(id);
            return Ok(json!({}));
        }
        let server = a.views.get(id).ok_or("视图已关闭")?.server.clone();
        match method {
            "ui.beforeClose" => return Ok(view_data(a, id)),
            "ui.save" => {
                save(a, id)?;
                return Ok(view_data(a, id));
            }
            "ui.event" => {}
            _ => return Err("未知网络插件方法".into()),
        }
        let d = &p["data"];
        match d["action"].as_str().unwrap_or("") {
            "snapshot" => {}
            "draft" => {
                if a.views[id].kind != "rule-editor" {
                    return Err("不是编辑表单".into());
                }
                let bytes = serde_json::to_vec(&d["rule"]).map_err(|e| e.to_string())?;
                if bytes.len() > 4096 {
                    return Err("草稿过大".into());
                }
                let v = a.views.get_mut(id).unwrap();
                v.draft = d["rule"].clone();
                v.dirty = true;
            }
            "navigation" => {
                if serde_json::to_vec(&d["state"])
                    .map_err(|e| e.to_string())?
                    .len()
                    > 2048
                {
                    return Err("导航状态过大".into());
                }
                a.views.get_mut(id).unwrap().navigation = d["state"].clone();
            }
            "save" => save(a, id)?,
            "start" | "stop" | "delete" | "check" => {
                if a.views[id].kind != "manager" {
                    return Err("需要管理视图".into());
                }
                let rule_id = d["id"].as_str().ok_or("缺少规则 ID")?;
                let _rule = a
                    .rules
                    .items
                    .iter()
                    .find(|r| r["id"] == rule_id && r["serverId"] == server)
                    .ok_or("规则不属于此服务器")?
                    .clone();
                match d["action"].as_str().unwrap() {
                    "start" => a.start(c, id, rule_id)?,
                    "stop" => {
                        a.stop(c, rule_id)?;
                    }
                    "delete" => {
                        if a.runs.get(rule_id).is_some_and(|r| {
                            r.snapshot["cleanupComplete"] != true
                                || !r.released
                                || r.query_error.is_some()
                        }) {
                            return Err("请先停止并确认清理，再删除规则".into());
                        }
                        a.rules.delete(
                            &server,
                            rule_id,
                            d["revision"].as_u64().ok_or("缺少修订")?,
                        )?;
                        a.runs.remove(rule_id);
                    }
                    "check" => {
                        let run = a.runs.get(rule_id).ok_or("请先启动隧道")?;
                        if run.snapshot["state"] != "running"
                            || run.stop_requested
                            || run.query_error.is_some()
                        {
                            return Err("隧道尚未就绪".into());
                        }
                        let listener = run.listener.clone();
                        let snapshot = run.snapshot.clone();
                        let rule = run.rule.clone();
                        let client = c.clone();
                        if a.checks.get(rule_id).is_some_and(|v| v["checking"] == true) {
                            return Err("这条规则正在检查，请稍后再试".into());
                        }
                        if a.checks.values().filter(|v| v["checking"] == true).count() >= 4 {
                            return Err("检查任务已满".into());
                        }
                        a.checks.insert(
                            rule_id.into(),
                            json!({"checking":true,"listenerId":listener}),
                        );
                        let target = d["target"].clone();
                        let rule_id = rule_id.to_owned();
                        let shared = app.clone();
                        std::thread::spawn(move || {
                            let result =
                                checks::check(&client, &listener, &snapshot, &rule, &target);
                            if let Some(a) = shared.lock().unwrap().as_mut() {
                                if a.checks
                                    .get(&rule_id)
                                    .is_some_and(|v| v["listenerId"] == listener)
                                {
                                    a.checks.insert(rule_id,json!({"checking":false,"listenerId":listener,"ok":result.is_ok(),"message":result.map(|v|v["message"].as_str().unwrap_or("TCP 可达").to_owned()).unwrap_or_else(|e|e)}));
                                }
                            }
                        });
                    }
                    _ => unreachable!(),
                }
            }
            _ => return Err("未知网络操作".into()),
        }
        Ok(view_data(a, id))
    })
}
fn save(a: &mut App, id: &str) -> Result<(), String> {
    let v = a.views.get(id).ok_or("视图已关闭")?;
    if v.kind != "rule-editor" || !v.dirty {
        return Ok(());
    }
    let rule = a.rules.save(&v.server, v.draft.clone())?;
    let v = a.views.get_mut(id).unwrap();
    v.draft = rule;
    v.dirty = false;
    Ok(())
}
fn view_data(a: &App, id: &str) -> Value {
    let v = &a.views[id];
    json!({"data":{"kind":v.kind,"draft":v.draft,"navigation":v.navigation,"dirty":v.dirty,"catalog":a.snapshot(&v.server,v.navigation["offset"].as_u64().unwrap_or(0)as usize)},"metadata":{"dirty":v.dirty,"busy":false}})
}
