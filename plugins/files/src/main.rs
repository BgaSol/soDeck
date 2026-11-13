use serde_json::json;
use sodeck_plugin_client::file_editor::EditorHost;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
mod file_clipboard;
mod tasks;
struct View {
    id: String,
    title: String,
    path: String,
    dirty: bool,
    busy: bool,
    navigation: serde_json::Value,
    context: serde_json::Value,
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
    let editors = EditorHost::new("editor");
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    let clipboard = Mutex::new(serde_json::Value::Null);
    let selections = Mutex::new(HashMap::<String, Selection>::new());
    let task_manager: Arc<Mutex<Option<Arc<tasks::Tasks>>>> = Arc::new(Mutex::new(None));
    let task_manager_for_handler = task_manager.clone();
    sodeck_plugin_client::run(move |client, method, p| {
        if method == "initialize" {
            editors.check_capabilities(&p)?;
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
            let manager = Arc::new(tasks::Tasks::new(client.clone(), directory)?);
            *task_manager_for_handler.lock().unwrap() = Some(manager);
            return Ok(json!({"apiVersion":"1","settingsApiVersion":"1","uiApiVersion":"2"}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["browser", "editor"]);
        }
        let tasks = task_manager_for_handler
            .lock()
            .unwrap()
            .clone()
            .ok_or("文件任务尚未初始化")?;
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.event"
            && p["data"]["action"]
                .as_str()
                .is_some_and(|a| a.starts_with("fileEditor."))
        {
            let result = editors.source_event(client, &p)?;
            return Ok(json!({"data":result}));
        }
        let editor_event = method == "ui.event"
            && [
                "save",
                "editorState",
                "dirty",
                "staged",
                "saved",
                "busy",
                "position",
                "error",
            ]
            .contains(&p["data"]["action"].as_str().unwrap_or(""));
        if (method == "ui.createView" && p["viewType"] == "editor")
            || (editors.contains(view) && (method != "ui.event" || editor_event))
        {
            if method == "ui.disposeView"
                || (method == "ui.beforeClose" && p["data"]["cancelTasks"] == true)
            {
                tasks.cancel_view(view);
            }
            if method == "ui.save" && tasks.waiting_conflict(view) {
                return Err(
                    "文件任务正在等待同名处理；请取消关闭并处理冲突，或选择放弃并关闭以取消任务"
                        .into(),
                );
            }
            let mut response = editors.handle(client, &method, &p, |context, path| {
                // Serialize this short check with file-batch acceptance; no SDK RPC holds it.
                let _business = views.lock().unwrap();
                if tasks.blocks(context, path) {
                    Err("此路径有文件任务正在修改，请等待任务结束后再操作".into())
                } else {
                    Ok(())
                }
            })?;
            if response["metadata"].is_object() {
                response["metadata"]["busy"] =
                    json!(response["metadata"]["busy"] == true || tasks.busy(view));
            }
            if method == "ui.createView" {
                let editor = editors.view(view)?;
                views.lock().unwrap().insert(
                    view.into(),
                    View {
                        id: editor.id,
                        title: editor.title,
                        path: editor.path,
                        dirty: editor.dirty,
                        busy: editor.busy,
                        navigation: serde_json::Value::Null,
                        context: p["context"].clone(),
                    },
                );
            }
            if method == "ui.disposeView" {
                views.lock().unwrap().remove(view);
            }
            if ["ui.createView", "ui.getView"].contains(&method) {
                response["data"]["tasks"] = tasks.snapshot(&p["context"]);
            }
            return Ok(response);
        }
        if method == "ui.disposeView" {
            tasks.cancel_view(view);
            editors.dispose_source(client, view)?;
            if let Some(v) = views.lock().unwrap().remove(view) {
                client.call("files.request", json!({"action":"close","id":v.id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.createView" && !views.lock().unwrap().contains_key(view) {
            let result = client.call("files.request", json!({"action":"open","viewId":view}))?;
            let path = p["params"]["path"].as_str().unwrap_or("").to_string();
            let title = "文件".to_string();
            views.lock().unwrap().entry(view.into()).or_insert(View {
                id: result["id"].as_str().ok_or("没有文件资源 ID")?.into(),
                title,
                path,
                dirty: false,
                busy: false,
                navigation: serde_json::Value::Null,
                context: p["context"].clone(),
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
                for (id, editor) in editors.entries(&p["context"]) {
                    if tasks::affected_paths(&parameters)
                        .iter()
                        .any(|path| tasks::within(&editor.path, path))
                    {
                        affected.push(id);
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
        if editors.contains(view) {
            let editor = editors.view(view)?;
            v.dirty = editor.dirty;
            v.busy = editor.busy;
        }
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
            json!({"data":{"fileId":v.id,"path":v.path,"navigation":v.navigation,"tasks":tasks.snapshot(&p["context"])},"metadata":{"title":v.title,"dirty":v.dirty,"busy":busy}}),
        )
    })
}
