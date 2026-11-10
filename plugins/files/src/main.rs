use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[cfg(test)]
mod editor_tests;
mod file_clipboard;
mod tasks;
struct View {
    id: String,
    title: String,
    path: String,
    dirty: bool,
    busy: bool,
    revision: u64,
    saved: u64,
    edit_revision: u64,
    staged_edit_revision: u64,
    editor_state: serde_json::Value,
    cursor: u64,
    scroll: u64,
    navigation: serde_json::Value,
    context: serde_json::Value,
    editor: bool,
}
impl View {
    fn update_dirty(&mut self) {
        self.dirty = self.revision != self.saved || self.edit_revision != self.staged_edit_revision;
    }
    fn project(&mut self, facts: &serde_json::Value) {
        let d = &facts["draft"];
        self.revision = d["draftRef"]["revision"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.revision);
        self.saved = d["savedRevision"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.saved);
        self.staged_edit_revision = d["source"]["editRevision"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.staged_edit_revision);
        self.edit_revision = self.edit_revision.max(self.staged_edit_revision);
        self.busy = ["running", "committing"].contains(&facts["state"].as_str().unwrap_or(""))
            || ["waitingInput", "writing"].contains(&d["exportState"].as_str().unwrap_or(""));
        self.update_dirty();
    }
    fn require_staged(&self) -> Result<(), String> {
        if self.edit_revision != self.staged_edit_revision {
            return Err("当前正文尚未完整暂存，请等待后重试".into());
        }
        Ok(())
    }
}
fn validate_editor_state(state: &serde_json::Value) -> Result<(), String> {
    if state["version"] != 1
        || state.to_string().len() > 8192
        || ["anchor", "active", "topLine"]
            .iter()
            .any(|key| state[*key].as_u64().is_none_or(|value| value > 1048577))
        || state["topOffset"]
            .as_f64()
            .is_none_or(|value| !value.is_finite() || value.abs() > 100000000.0)
        || state["left"]
            .as_f64()
            .is_none_or(|value| !value.is_finite() || !(0.0..=100000000.0).contains(&value))
        || state["language"]
            .as_str()
            .is_none_or(|value| value.len() > 64)
        || !state["search"]["open"].is_boolean()
        || !state["search"]["matchCase"].is_boolean()
        || state["search"]["query"]
            .as_str()
            .is_none_or(|value| value.encode_utf16().count() > 1024 || value.contains(['\r', '\n']))
    {
        return Err("文件编辑器状态无效或过大".into());
    }
    Ok(())
}
struct Selection {
    view: String,
    context: serde_json::Value,
    roots: Vec<serde_json::Value>,
    at: std::time::Instant,
}
fn selection_parameters(
    selections: &HashMap<String, Selection>,
    view: &str,
    context: &serde_json::Value,
    parameters: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut value = parameters.clone();
    if let Some(id) = parameters["selectionId"].as_str() {
        let selection = selections
            .get(id)
            .filter(|s| s.view == view && s.context == *context && s.at.elapsed().as_secs() < 60)
            .ok_or("选择集合已失效，请重新选择")?;
        value["roots"] = json!(selection.roots);
    }
    Ok(value)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let views = Mutex::new(HashMap::<String, View>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    let clipboard = Mutex::new(serde_json::Value::Null);
    let selections = Mutex::new(HashMap::<String, Selection>::new());
    let task_manager: Arc<Mutex<Option<tasks::Tasks>>> = Arc::new(Mutex::new(None));
    let task_manager_for_handler = task_manager.clone();
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            for capability in [
                "files.v1",
                "files.directoryCursors.v1",
                "files.operations.v1",
                "files.operations.v2",
                "files.editor.v1",
                "files.localInputs.v1",
                "viewData.v1",
            ] {
                if !p["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|v| v == capability))
                {
                    return Err(format!("核心缺少 {capability}，请先升级 SoDeck"));
                }
            }
            let directory = p["dataDirectory"]
                .as_str()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("sodeck-files"));
            let manager = tasks::Tasks::new(client.clone(), directory)?;
            *task_manager_for_handler.lock().unwrap() = Some(manager);
            return Ok(json!({"apiVersion":"1","settingsApiVersion":"1","uiApiVersion":"2"}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["browser", "editor"]);
        }
        let manager = task_manager_for_handler.lock().unwrap();
        let tasks = manager.as_ref().ok_or("文件任务尚未初始化")?;
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.disposeView" {
            tasks.cancel_view(view);
            if let Some(v) = views.lock().unwrap().remove(view) {
                client.call("files.request", json!({"action":"close","id":v.id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.createView" && !views.lock().unwrap().contains_key(view) {
            if p["viewType"] == "editor"
                && tasks.blocks(&p["context"], p["params"]["path"].as_str().unwrap_or(""))
            {
                return Err("此路径有文件任务正在修改，请等待任务结束后打开".into());
            }
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
                edit_revision: 0,
                staged_edit_revision: 0,
                editor_state: serde_json::Value::Null,
                cursor: 0,
                scroll: 0,
                navigation: serde_json::Value::Null,
                context: p["context"].clone(),
                editor: p["viewType"] == "editor",
            });
        }
        let mut all = views.lock().unwrap();
        if method == "ui.event"
            && p["data"]["action"]
                .as_str()
                .is_some_and(|s| s.starts_with("selection."))
        {
            let d = &p["data"];
            let id = d["selectionId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("选择集合 ID 无效")?;
            let mut selections = selections.lock().unwrap();
            selections.retain(|_, s| s.at.elapsed().as_secs() < 60);
            match d["action"].as_str() {
                Some("selection.begin") => {
                    if selections.len() >= 16 || selections.contains_key(id) {
                        return Err("选择集合已满或重复".into());
                    }
                    selections.insert(
                        id.into(),
                        Selection {
                            view: view.into(),
                            context: p["context"].clone(),
                            roots: Vec::new(),
                            at: std::time::Instant::now(),
                        },
                    );
                }
                Some("selection.append") => {
                    let s = selections
                        .get_mut(id)
                        .filter(|s| s.view == view && s.context == p["context"])
                        .ok_or("选择集合已失效")?;
                    let roots = d["roots"].as_array().ok_or("选择集合无效")?;
                    if s.roots.len() + roots.len() > 4096
                        || roots
                            .iter()
                            .any(|r| r.as_str().is_none_or(|r| r.is_empty() || r.len() > 4096))
                    {
                        return Err("选择集合超过限制".into());
                    }
                    s.roots.extend(roots.iter().cloned());
                    s.at = std::time::Instant::now();
                }
                Some("selection.release") => {
                    if selections.get(id).is_some_and(|s| s.view == view) {
                        selections.remove(id);
                    }
                }
                _ => return Err("未知选择操作".into()),
            }
            return Ok(json!({"data":{}}));
        }
        if method == "ui.event"
            && ["prepareBatch", "startBatch"].contains(&p["data"]["action"].as_str().unwrap_or(""))
        {
            let parameters = selection_parameters(
                &selections.lock().unwrap(),
                view,
                &p["context"],
                &p["data"]["parameters"],
            )?;
            let kind = parameters["kind"].as_str().unwrap_or("");
            if ["move", "rename"].contains(&kind) {
                return Err("当前后端不支持原子无覆盖移动/改名".into());
            }
            let mut affected = Vec::new();
            if kind != "download" {
                for (id, editor) in all.iter().filter(|(_, v)| {
                    v.editor && v.context["workspaceId"] == p["context"]["workspaceId"]
                }) {
                    if tasks::affected_paths(&parameters)
                        .iter()
                        .any(|path| tasks::within(&editor.path, path))
                    {
                        affected.push(id.clone());
                    }
                }
            }
            let v = all.get(view).ok_or("文件视图尚未创建")?;
            if p["data"]["action"] == "prepareBatch" {
                return Ok(
                    json!({"data":{"closeViews":affected},"metadata":{"title":v.title,"dirty":v.dirty,"busy":v.busy||tasks.busy(view)}}),
                );
            }
            if !affected.is_empty() {
                return Err("目标包含已打开的编辑器，请先处理保存并关闭后再操作".into());
            }
        }
        let v = all.get_mut(view).ok_or("文件视图尚未创建")?;
        v.context = p["context"].clone();
        if ["ui.beforeClose", "ui.save"].contains(&method) {
            if method == "ui.beforeClose" && p["data"]["cancelTasks"] == true {
                tasks.cancel_view(view);
            }
            if method == "ui.save" && tasks.waiting_conflict(view) {
                return Err(
                    "文件任务正在等待同名处理；请取消关闭并处理冲突，或选择放弃并关闭以取消任务"
                        .into(),
                );
            }
            if v.editor {
                let job = client.call("files.editor", json!({"action":"save.status","id":v.id}))?;
                let was_busy = v.busy;
                v.project(&job);
                if was_busy
                    && ["notCommitted", "unknown"].contains(&job["state"].as_str().unwrap_or(""))
                {
                    return Err(job["error"]
                        .as_str()
                        .unwrap_or("保存未确认，草稿保留")
                        .into());
                }
                if method == "ui.save" && v.dirty && !v.busy {
                    v.require_staged()?;
                    if tasks.blocks(&p["context"], &v.path) {
                        return Err("此路径有文件任务正在修改，请等待后保存".into());
                    }
                    if job["state"] == "unknown" {
                        return Err("原保存结果未知，请核对服务器；可以导出草稿".into());
                    }
                    if job["request"]["requestId"].is_string() {
                        client.call("files.editor", json!({"action":"save.release", "id":v.id,"requestId":job["request"]["requestId"]}))?;
                    }
                    let sequence = job["draft"]["saveSequence"]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("保存序号已用尽")?;
                    let started = client.call("files.editor", json!({"action":"save.start","id":v.id,"requestId":format!("close/{view}/{sequence}"),"sequence":sequence.to_string(),"expectedDraftRef":job["draft"]["draftRef"],"expectedContext":p["context"]}))?;
                    v.project(&started);
                }
            }
        } else if method == "ui.event" {
            let d = &p["data"];
            let task_result = match d["action"].as_str() {
                Some("copyEntries") => {
                    if d["cut"] == true {
                        return Err("当前后端不支持原子无覆盖移动；请使用复制".into());
                    }
                    let copied =
                        selection_parameters(&selections.lock().unwrap(), view, &p["context"], d)?;
                    if copied["roots"]
                        .as_array()
                        .is_none_or(|v| v.len() > 4096 || v.iter().any(|p| !p.is_string()))
                    {
                        return Err("复制选择无效".into());
                    }
                    *clipboard.lock().unwrap() = json!({"id":d["selectionId"],"roots":copied["roots"],"context":p["context"],"revision":d["revision"],"cut":d["cut"]==true});
                    Some(json!({"copied":true}))
                }
                Some("clipboardStatus") => {
                    let copied = clipboard.lock().unwrap();
                    let consumed = copied["batchId"]
                        .as_str()
                        .is_some_and(|id| tasks.succeeded(id));
                    Some(file_clipboard::status(
                        &copied,
                        &p["context"],
                        &d["revision"],
                        d["targetPath"].as_str().ok_or("缺少粘贴目标")?,
                        consumed,
                    ))
                }
                Some("pasteEntries") => {
                    let mut copied = clipboard.lock().unwrap();
                    if let Some(expected) = d["clipboardId"].as_str() {
                        if copied["id"] != expected {
                            return Err("剪贴板已变化，请重新粘贴".into());
                        }
                    }
                    if copied["batchId"]
                        .as_str()
                        .is_some_and(|id| tasks.succeeded(id))
                    {
                        *copied = serde_json::Value::Null;
                    }
                    if copied.is_null() || copied["revision"] != d["revision"] {
                        *copied = serde_json::Value::Null;
                        Some(json!({"local":true}))
                    } else if copied["context"] != p["context"] {
                        return Err(
                            "复制来源不属于当前连接；请回到原服务器操作，或重新复制本地文件".into(),
                        );
                    } else {
                        let id = d["selectionId"].as_str().ok_or("缺少目标选择集合")?;
                        let mut selections = selections.lock().unwrap();
                        selections.retain(|_, s| s.at.elapsed().as_secs() < 60);
                        if selections.len() >= 16 {
                            return Err("选择集合已满，请稍后重试".into());
                        }
                        selections.insert(
                            id.into(),
                            Selection {
                                view: view.into(),
                                context: p["context"].clone(),
                                roots: copied["roots"].as_array().cloned().unwrap_or_default(),
                                at: std::time::Instant::now(),
                            },
                        );
                        Some(
                            json!({"selectionId":id,"kind":if copied["cut"]==true{"move"}else{"copy"}}),
                        )
                    }
                }
                Some("startBatch") => {
                    let mut parameters = selection_parameters(
                        &selections.lock().unwrap(),
                        view,
                        &p["context"],
                        &d["parameters"],
                    )?;
                    if let Some(object) = parameters.as_object_mut() {
                        object.remove("retryJournal");
                        object.remove("renewInput");
                    }
                    let cut = parameters["clipboard"] == true && parameters["kind"] == "move";
                    let result = tasks.start(view, &v.id, p["context"].clone(), parameters)?;
                    if cut {
                        clipboard.lock().unwrap()["batchId"] = result["batchId"].clone();
                    }
                    Some(result)
                }
                Some("readTasks") => Some(
                    tasks.snapshot_page(&p["context"], d["offset"].as_u64().unwrap_or(0) as usize),
                ),
                Some(
                    "confirmBatch" | "cancelBatch" | "resolveConflict" | "readItems"
                    | "retryFailed" | "clearBatch",
                ) => {
                    let mut command = d.clone();
                    command["context"] = p["context"].clone();
                    command["fileId"] = json!(v.id);
                    Some(tasks.command(view, &command)?)
                }
                Some("save") => {
                    if tasks.blocks(&p["context"], &v.path) {
                        return Err("此路径有文件任务正在修改，请等待后保存".into());
                    }
                    let mut command = d.clone();
                    command["action"] = json!("save.start");
                    command["id"] = json!(v.id);
                    let job = client.call("files.editor", command)?;
                    v.project(&job);
                    Some(job)
                }
                _ => None,
            };
            if let Some(result) = task_result {
                return Ok(
                    json!({"data":result,"metadata":{"title":v.title,"dirty":v.dirty,"busy":v.busy||tasks.busy(view)}}),
                );
            }
            match d["action"].as_str() {
                Some("error") => {
                    client.call("logging.write", json!({
                        "level":"error", "message":d["message"].as_str().unwrap_or("文件操作失败"),
                        "fields":{"viewId":view}
                    }))?;
                }
                Some("dirty" | "staged" | "saved" | "busy" | "editorState") => {
                    if let Some(revision) = d["editRevision"].as_u64() {
                        v.edit_revision = v.edit_revision.max(revision);
                    }
                    let facts =
                        client.call("files.editor", json!({"action":"save.status","id":v.id}))?;
                    v.project(&facts);
                }
                Some("position") => {
                    if let Some(state) = d.get("editorState") {
                        validate_editor_state(state)?;
                        v.editor_state = state.clone();
                    }
                    v.cursor = d["cursor"].as_u64().unwrap_or(0);
                    v.scroll = d["scroll"].as_u64().unwrap_or(0);
                }
                Some("directory") => v.path = d["path"].as_str().ok_or("缺少 path")?.into(),
                Some("navigation") => {
                    let state = &d["state"];
                    if state.to_string().len() > 12000
                        || !state["expanded"].as_array().is_some_and(|a| {
                            a.len() <= 128
                                && a.iter()
                                    .all(|p| p.as_str().is_some_and(|s| s.len() <= 4096))
                        })
                        || !state["root"].is_string()
                        || !state["selected"].is_string()
                    {
                        return Err("文件导航状态无效或过大".into());
                    }
                    v.path = state["selected"].as_str().unwrap().into();
                    v.navigation = state.clone();
                }
                _ => return Err("未知文件交互".into()),
            }
        } else if !["ui.createView", "ui.getView"].contains(&method) {
            return Err("未知文件插件方法".into());
        }
        let busy = v.busy || tasks.busy(view);
        Ok(
            json!({"data":{"fileId":v.id,"path":v.path,"cursor":v.cursor,"scroll":v.scroll,"revision":v.revision,"editRevision":v.edit_revision,"editorState":v.editor_state,"navigation":v.navigation,"tasks":tasks.snapshot(&p["context"])},"metadata":{"title":v.title,"dirty":v.dirty,"busy":busy}}),
        )
    })
}
