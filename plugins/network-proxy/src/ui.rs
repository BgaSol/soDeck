use crate::{runtime::View, service::Service};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::sync::Arc;
pub fn handle(
    service: &Arc<Service>,
    client: &Client,
    method: &str,
    p: &Value,
) -> Result<Value, String> {
    let id = p["viewId"].as_str().ok_or("缺少 viewId")?;
    if method == "ui.createView" {
        let context = &p["context"];
        let server = context["serverId"].as_str().ok_or("缺少服务器")?.to_owned();
        let kind = p["viewType"]
            .as_str()
            .filter(|s| ["manager", "rule-editor"].contains(s))
            .ok_or("未知视图")?;
        let mut a = service.state.lock().unwrap();
        a.views.entry(id.into()).or_insert(View {
            server,
            kind: kind.into(),
            context: context.clone(),
            draft: if kind == "rule-editor" {
                p["params"]["rule"].clone()
            } else {
                Value::Null
            },
            dirty: false,
            navigation: Value::Null,
            draft_revision: 0,
        });
        drop(a);
        service.context(id, context);
        return service.response(id);
    }
    if method == "ui.disposeView" {
        service.dispose(id);
        return Ok(json!({}));
    }
    if method == "ui.getView" {
        service.context(id, &p["context"]);
        return service.response(id);
    }
    if method == "ui.beforeClose" {
        // The core owns close reservations, including cancellation for a view
        // that has never had a Surface. Do not keep a second frozen flag here.
        return service.response(id);
    }
    if method == "ui.save" {
        service.save(id)?;
        return service.response(id);
    }
    if method != "ui.event" {
        return Err("未知网络插件方法".into());
    }
    let d = &p["data"];
    let action = d["action"].as_str().ok_or("缺少操作")?;
    if matches!(action, "resumeInput" | "snapshot") {
        return service.response(id);
    }
    if !service.state.lock().unwrap().views.contains_key(id) {
        return Err("视图已关闭".into());
    }
    match action {
        "draft" => {
            let bytes = serde_json::to_vec(&d["rule"]).map_err(|e| e.to_string())?;
            if bytes.len() > 4096 {
                return Err("草稿过大".into());
            }
            let mut a = service.state.lock().unwrap();
            let v = a.views.get_mut(id).unwrap();
            if v.kind != "rule-editor" {
                return Err("不是编辑表单".into());
            }
            v.draft = d["rule"].clone();
            v.dirty = true;
            v.draft_revision = v.draft_revision.checked_add(1).ok_or("草稿序号已用尽")?;
        }
        "navigation" => {
            if serde_json::to_vec(&d["state"])
                .map_err(|e| e.to_string())?
                .len()
                > 2048
            {
                return Err("导航状态过大".into());
            }
            service
                .state
                .lock()
                .unwrap()
                .views
                .get_mut(id)
                .unwrap()
                .navigation = d["state"].clone();
        }
        "save" => {
            service.save(id)?;
        }
        "start" | "stop" | "delete" | "check" => {
            let rule_id = d["id"].as_str().ok_or("缺少规则 ID")?;
            service.rule_for(id, rule_id)?;
            if service.state.lock().unwrap().views[id].kind != "manager" {
                return Err("需要管理视图".into());
            }
            match action {
                "start" => service.start(id, rule_id)?,
                "stop" => service.stop(rule_id),
                "delete" => {
                    service.delete(id, rule_id, d["revision"].as_u64().ok_or("缺少修订")?)?
                }
                "check" => service.check(client, id, rule_id, d["target"].clone())?,
                _ => unreachable!(),
            }
        }
        _ => return Err("未知网络操作".into()),
    }
    service.changed();
    service.response(id)
}
