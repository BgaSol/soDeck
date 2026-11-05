use serde_json::json;
use std::{collections::HashMap, sync::Mutex};
struct View {
    id: String,
    title: String,
    path: String,
    dirty: bool,
    busy: bool,
    revision: u64,
    saved: u64,
    cursor: u64,
    scroll: u64,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let views = Mutex::new(HashMap::<String, View>::new());
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            for capability in ["files.v1", "viewData.v1"] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|v| v == capability))
                {
                    return Err(format!("核心缺少 {capability}，请先升级 SoDeck"));
                }
            }
            return Ok(json!({"apiVersion":"1"}));
        }
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.disposeView" {
            if let Some(v) = views.lock().unwrap().remove(view) {
                client.call("files.request", json!({"action":"close","id":v.id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.mount" && !views.lock().unwrap().contains_key(view) {
            let result = client.call("files.request", json!({"action":"open","viewId":view}))?;
            let path = p["params"]["path"].as_str().unwrap_or("").to_string();
            let title = if p["viewType"] == "editor" {
                path.rsplit('/').next().unwrap_or("文件").to_string()
            } else {
                "文件".into()
            };
            views.lock().unwrap().entry(view.into()).or_insert(View {
                id: result["id"].as_str().ok_or("没有文件资源 ID")?.into(),
                title,
                path,
                dirty: false,
                busy: false,
                revision: 0,
                saved: 0,
                cursor: 0,
                scroll: 0,
            });
        }
        let mut all = views.lock().unwrap();
        let v = all.get_mut(view).ok_or("文件视图尚未创建")?;
        if ["ui.beforeClose", "ui.save"].contains(&method) {
            let job = client.call("files.request", json!({"action":"status","id":v.id}))?;
            if job["state"] == "failed" && v.busy {
                v.busy = false;
                return Err(job["error"]
                    .as_str()
                    .unwrap_or("文件任务失败，草稿保留")
                    .into());
            }
            v.busy = job["state"] == "running";
            if job["state"] == "completed" {
                if let Some(revision) = job["result"]["revision"].as_u64() {
                    v.saved = revision;
                    v.dirty = v.revision != revision;
                }
            }
            if method == "ui.save" && v.dirty && !v.busy {
                client.call("files.request", json!({"action":"save","id":v.id}))?;
                v.busy = true;
            }
        } else if method == "ui.event" {
            let d = &p["data"];
            match d["action"].as_str() {
                Some("dirty") => {
                    v.dirty = true;
                    v.revision = d["revision"].as_u64().unwrap_or(v.revision);
                }
                Some("staged") => {
                    v.revision = d["revision"].as_u64().ok_or("缺少 revision")?;
                    v.dirty = v.revision != v.saved;
                }
                Some("saved") => {
                    v.saved = d["revision"].as_u64().ok_or("缺少 revision")?;
                    v.dirty = v.revision != v.saved;
                    v.busy = false;
                }
                Some("busy") => v.busy = d["busy"] == true,
                Some("position") => {
                    v.cursor = d["cursor"].as_u64().unwrap_or(0);
                    v.scroll = d["scroll"].as_u64().unwrap_or(0);
                }
                Some("directory") => v.path = d["path"].as_str().ok_or("缺少 path")?.into(),
                _ => return Err("未知文件交互".into()),
            }
        } else if method != "ui.mount" {
            return Err("未知文件插件方法".into());
        }
        Ok(
            json!({"data":{"fileId":v.id,"path":v.path,"cursor":v.cursor,"scroll":v.scroll,"revision":v.revision},"metadata":{"title":v.title,"dirty":v.dirty,"busy":v.busy}}),
        )
    })
}
