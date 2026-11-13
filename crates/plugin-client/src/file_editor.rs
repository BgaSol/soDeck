//! Reusable remote file editor lifecycle. Bodies remain on the core data bridge.
use crate::Client;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct EditorView {
    pub id: String,
    pub title: String,
    pub path: String,
    pub context: Value,
    pub dirty: bool,
    pub busy: bool,
    revision: u64,
    saved: u64,
    edit_revision: u64,
    staged_edit_revision: u64,
    editor_state: Value,
    cursor: u64,
    scroll: u64,
    observed: u64,
    query: u64,
    opening: bool,
    facts: Value,
}
impl EditorView {
    fn new(path: String, context: Value) -> Self {
        Self {
            id: String::new(),
            title: path.rsplit('/').next().unwrap_or("文件").into(),
            path,
            context,
            dirty: false,
            busy: false,
            revision: 0,
            saved: 0,
            edit_revision: 0,
            staged_edit_revision: 0,
            editor_state: Value::Null,
            cursor: 0,
            scroll: 0,
            observed: 0,
            query: 0,
            opening: false,
            facts: Value::Null,
        }
    }
    fn update_dirty(&mut self) {
        self.dirty = self.revision != self.saved || self.edit_revision != self.staged_edit_revision;
    }
    fn project(&mut self, facts: &Value, query: u64) {
        if query < self.observed {
            return;
        }
        self.observed = query;
        let d = &facts["draft"];
        let number =
            |v: &Value, default| v.as_str().and_then(|s| s.parse().ok()).unwrap_or(default);
        self.revision = number(&d["draftRef"]["revision"], self.revision);
        self.saved = number(&d["savedRevision"], self.saved);
        self.staged_edit_revision = number(&d["source"]["editRevision"], self.staged_edit_revision);
        self.edit_revision = self.edit_revision.max(self.staged_edit_revision);
        self.busy = ["running", "committing"].contains(&facts["state"].as_str().unwrap_or(""))
            || ["waitingInput", "writing"].contains(&d["exportState"].as_str().unwrap_or(""));
        self.facts = facts.clone();
        self.update_dirty();
    }
    fn response(&self, data: Option<Value>) -> Value {
        json!({"data":data.unwrap_or_else(||json!({"fileId":self.id,"path":self.path,"cursor":self.cursor,"scroll":self.scroll,"revision":self.revision,"editRevision":self.edit_revision,"editorState":self.editor_state})),"metadata":{"title":self.title,"dirty":self.dirty,"busy":self.busy}})
    }
    fn require_staged(&self) -> Result<(), String> {
        if self.edit_revision != self.staged_edit_revision {
            Err("当前正文尚未完整暂存，请等待后重试".into())
        } else {
            Ok(())
        }
    }
}

pub fn validate_editor_state(state: &Value) -> Result<(), String> {
    if state["version"] != 1
        || state.to_string().len() > 8192
        || ["anchor", "active", "topLine"]
            .iter()
            .any(|key| state[*key].as_u64().is_none_or(|v| v > 1048577))
        || state["topOffset"]
            .as_f64()
            .is_none_or(|v| !v.is_finite() || v.abs() > 100000000.0)
        || state["left"]
            .as_f64()
            .is_none_or(|v| !v.is_finite() || !(0.0..=100000000.0).contains(&v))
        || state["language"].as_str().is_none_or(|v| v.len() > 64)
        || !state["search"]["open"].is_boolean()
        || !state["search"]["matchCase"].is_boolean()
        || state["search"]["query"]
            .as_str()
            .is_none_or(|v| v.encode_utf16().count() > 1024 || v.contains(['\r', '\n']))
    {
        return Err("文件编辑器状态无效或过大".into());
    }
    Ok(())
}

type Entry = Arc<Mutex<EditorView>>;
pub struct EditorHost {
    view_type: String,
    views: Mutex<HashMap<String, Entry>>,
    sources: Mutex<HashMap<String, Option<String>>>,
}
impl EditorHost {
    pub fn new(view_type: &str) -> Self {
        Self {
            view_type: view_type.into(),
            views: Mutex::new(HashMap::new()),
            sources: Mutex::new(HashMap::new()),
        }
    }
    pub fn check_capabilities(&self, p: &Value) -> Result<(), String> {
        for cap in [
            "files.v1",
            "files.editor.v1",
            "viewData.v1",
            "ui.workbench.v2",
            "ui.openView.v1",
        ] {
            if !p["capabilities"]
                .as_array()
                .is_some_and(|v| v.iter().any(|c| c == cap))
            {
                return Err(format!("核心缺少 {cap}，请升级 SoDeck"));
            }
        }
        Ok(())
    }
    pub fn contains(&self, view: &str) -> bool {
        self.views.lock().unwrap().contains_key(view)
    }
    pub fn view(&self, view: &str) -> Result<EditorView, String> {
        Ok(self.entry(view)?.lock().unwrap().clone())
    }
    pub fn entries(&self, context: &Value) -> Vec<(String, EditorView)> {
        self.views
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, v)| {
                let v = v.lock().unwrap();
                (v.context["workspaceId"] == context["workspaceId"])
                    .then(|| (id.clone(), v.clone()))
            })
            .collect()
    }
    fn entry(&self, view: &str) -> Result<Entry, String> {
        self.views
            .lock()
            .unwrap()
            .get(view)
            .cloned()
            .ok_or("编辑视图已关闭".into())
    }
    pub fn projection(&self, view: &str) -> Result<Value, String> {
        Ok(self.entry(view)?.lock().unwrap().facts.clone())
    }
    pub fn refresh(&self, client: &Client, view: &str) -> Result<Value, String> {
        let entry = self.entry(view)?;
        let (id, query) = {
            let mut v = entry.lock().unwrap();
            v.query = v.query.checked_add(1).ok_or("查询序号已用尽")?;
            (v.id.clone(), v.query)
        };
        let facts = client.call("files.editor", json!({"action":"save.status","id":id}))?;
        let mut state = entry.lock().unwrap();
        state.project(&facts, query);
        Ok(state.facts.clone())
    }
    /// Handles source-page events without taking over that page's lifecycle.
    pub fn source_event(&self, client: &Client, p: &Value) -> Result<Value, String> {
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        let d = &p["data"];
        if d["expectedContext"] != p["context"] {
            return Err("文件打开上下文已变化".into());
        }
        match d["action"].as_str() {
            Some("fileEditor.open") => {
                if d["viewType"] != self.view_type {
                    return Err("未知编辑视图类型".into());
                }
                let path = d["path"]
                    .as_str()
                    .filter(|v| {
                        !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control)
                    })
                    .ok_or("文件路径无效")?;
                client.call("ui.openView",json!({"requestId":d["requestId"],"expectedContext":p["context"],"sourceViewId":view,"viewType":self.view_type,"resourceKey":path,"params":{"path":path}}))
            }
            Some("fileEditor.resource") => {
                {
                    let mut sources = self.sources.lock().unwrap();
                    if let Some(id) = sources.get(view) {
                        return id
                            .as_ref()
                            .map(|id| json!({"id":id}))
                            .ok_or("文件资源正在创建".into());
                    }
                    sources.insert(view.into(), None);
                }
                let result =
                    match client.call("files.request", json!({"action":"open","viewId":view})) {
                        Ok(result) => result,
                        Err(error) => {
                            self.sources.lock().unwrap().remove(view);
                            return Err(error);
                        }
                    };
                let Some(id) = result["id"].as_str().map(str::to_string) else {
                    self.sources.lock().unwrap().remove(view);
                    return Err("没有文件资源 ID".into());
                };
                let accepted = {
                    let mut sources = self.sources.lock().unwrap();
                    if let Some(slot) = sources.get_mut(view) {
                        *slot = Some(id.clone());
                        true
                    } else {
                        false
                    }
                };
                if !accepted {
                    client.call("files.request", json!({"action":"close","id":id}))?;
                    return Err("来源页面已关闭".into());
                }
                Ok(result)
            }
            _ => Err("未知编辑器来源事件".into()),
        }
    }
    pub fn dispose_source(&self, client: &Client, view: &str) -> Result<(), String> {
        let id = self.sources.lock().unwrap().remove(view).flatten();
        if let Some(id) = id {
            if let Err(error) = client.call("files.request", json!({"action":"close","id":id})) {
                self.sources
                    .lock()
                    .unwrap()
                    .entry(view.into())
                    .or_insert(Some(id));
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn handle(
        &self,
        client: &Client,
        method: &str,
        p: &Value,
        before_write: impl Fn(&Value, &str) -> Result<(), String>,
    ) -> Result<Value, String> {
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.createView" {
            let path = p["params"]["path"]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
                .ok_or("文件路径无效")?;
            let entry = {
                let mut views = self.views.lock().unwrap();
                views
                    .entry(view.into())
                    .or_insert_with(|| {
                        Arc::new(Mutex::new(EditorView::new(
                            path.into(),
                            p["context"].clone(),
                        )))
                    })
                    .clone()
            };
            {
                let v = entry.lock().unwrap();
                if v.path != path || v.context != p["context"] {
                    return Err("编辑视图的路径或上下文不一致".into());
                }
            }
            if let Err(error) = before_write(&p["context"], path) {
                let unopened = {
                    let v = entry.lock().unwrap();
                    v.id.is_empty() && !v.opening
                };
                if unopened {
                    self.views.lock().unwrap().remove(view);
                }
                return Err(error);
            }
            let needs_open = {
                let mut v = entry.lock().unwrap();
                if v.opening {
                    return Err("编辑资源正在创建，请重试".into());
                }
                let needed = v.id.is_empty();
                v.opening = needed;
                needed
            };
            if needs_open {
                let result = client.call("files.request", json!({"action":"open","viewId":view}));
                let id = match result.and_then(|r| {
                    r["id"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or("没有文件资源 ID".into())
                }) {
                    Ok(id) => id,
                    Err(error) => {
                        entry.lock().unwrap().opening = false;
                        return Err(error);
                    }
                };
                if self.entry(view).is_err() {
                    client.call("files.request", json!({"action":"close","id":id}))?;
                    return Err("编辑页面已关闭".into());
                }
                let mut v = entry.lock().unwrap();
                v.id = id;
                v.opening = false;
            }
        }
        let entry = self.entry(view)?;
        if method == "ui.disposeView" {
            let id = entry.lock().unwrap().id.clone();
            if !id.is_empty() {
                client.call("files.request", json!({"action":"close","id":id}))?;
            }
            self.views.lock().unwrap().remove(view);
            return Ok(json!({}));
        }
        entry.lock().unwrap().context = p["context"].clone();
        let mut result = None;
        match method {
            "ui.createView" | "ui.getView" => {}
            "ui.beforeClose" | "ui.save" => {
                let was_busy = entry.lock().unwrap().busy;
                let job = self.refresh(client, view)?;
                if was_busy
                    && ["notCommitted", "unknown"].contains(&job["state"].as_str().unwrap_or(""))
                {
                    return Err(job["error"]
                        .as_str()
                        .unwrap_or("保存未确认，草稿保留")
                        .into());
                }
                let v = entry.lock().unwrap().clone();
                if method == "ui.save" && v.dirty && !v.busy {
                    v.require_staged()?;
                    before_write(&p["context"], &v.path)?;
                    if job["state"] == "unknown" {
                        return Err("原保存结果未知，请核对服务器；可以导出草稿".into());
                    }
                    if job["request"]["requestId"].is_string() {
                        client.call("files.editor",json!({"action":"save.release","id":v.id,"requestId":job["request"]["requestId"]}))?;
                    }
                    let sequence = job["draft"]["saveSequence"]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("保存序号已用尽")?;
                    client.call("files.editor",json!({"action":"save.start","id":v.id,"requestId":format!("close/{view}/{sequence}"),"sequence":sequence.to_string(),"expectedDraftRef":job["draft"]["draftRef"],"expectedContext":p["context"]}))?;
                    self.refresh(client, view)?;
                }
            }
            "ui.event" => {
                let d = &p["data"];
                match d["action"].as_str() {
                    Some("save") => {
                        let v = entry.lock().unwrap().clone();
                        before_write(&p["context"], &v.path)?;
                        let mut command = d.clone();
                        command["action"] = json!("save.start");
                        command["id"] = json!(v.id);
                        result = Some(client.call("files.editor", command)?);
                        self.refresh(client, view)?;
                    }
                    Some("editorState" | "dirty" | "staged" | "saved" | "busy") => {
                        if let Some(r) = d["editRevision"].as_u64() {
                            let mut v = entry.lock().unwrap();
                            v.edit_revision = v.edit_revision.max(r);
                            v.update_dirty();
                        }
                        self.refresh(client, view)?;
                    }
                    Some("position") => {
                        if let Some(state) = d.get("editorState") {
                            validate_editor_state(state)?;
                        }
                        let mut v = entry.lock().unwrap();
                        if let Some(state) = d.get("editorState") {
                            v.editor_state = state.clone();
                        }
                        v.cursor = d["cursor"].as_u64().unwrap_or(0);
                        v.scroll = d["scroll"].as_u64().unwrap_or(0);
                    }
                    Some("error") => {
                        client.call("logging.write",json!({"level":"error","message":d["message"].as_str().unwrap_or("文件编辑失败"),"fields":{"viewId":view}}))?;
                    }
                    _ => return Err("未知编辑器交互".into()),
                }
            }
            _ => return Err("未知编辑器方法".into()),
        }
        let response = entry.lock().unwrap().response(result);
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_snapshot_cannot_clear_new_input_or_apply_stale_observation() {
        let mut v = EditorView::new("/file".into(), Value::Null);
        v.edit_revision = 4;
        let facts = json!({"state":"committed","draft":{"draftRef":{"revision":"3"},"savedRevision":"3","source":{"editRevision":"3"}}});
        v.project(&facts, 2);
        assert!(v.dirty);
        assert!(v.require_staged().is_err());
        v.project(&json!({"state":"running"}), 1);
        assert!(!v.busy);
        assert_eq!(v.facts, facts);
        v.project(&json!({"state":"committed","draft":{"draftRef":{"revision":"4"},"savedRevision":"4","source":{"editRevision":"4"}}}),3);
        assert!(!v.dirty);
    }
    #[test]
    fn editor_state_is_bounded() {
        let mut s = json!({"version":1,"anchor":0,"active":12,"topLine":2,"topOffset":4.5,"left":0,"language":"auto","search":{"open":true,"query":"配置","matchCase":false}});
        validate_editor_state(&s).unwrap();
        s["search"]["query"] = json!("🙂".repeat(513));
        assert!(validate_editor_state(&s).is_err());
        s["search"]["query"] = json!("line\nnext");
        assert!(validate_editor_state(&s).is_err());
    }
}
