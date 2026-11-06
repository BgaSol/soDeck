//! Plugin-declared preferences. Storage and basic constraints belong to the core;
//! business effects remain in the owning plugin and its UI.
pub mod declaration;
pub(crate) mod store;

use crate::{
    model::{PluginStatus, PluginView},
    AppCore,
};
use declaration::Page;
use serde_json::{json, Map, Value};
use std::{fs, sync::Arc};

fn runtime_snapshot(
    plugin: &str,
    page: &str,
    token: &str,
    revision: u64,
    values: &Map<String, Value>,
) -> Result<Value, String> {
    let snapshot = json!({"pluginId":plugin,"pageId":page,"definitionToken":token,"revision":revision.to_string(),"values":values});
    if serde_json::to_vec(&snapshot)
        .map_err(|e| e.to_string())?
        .len()
        > declaration::PAGE_BYTES
    {
        return Err("单页设置快照超过 24 KiB".into());
    }
    Ok(snapshot)
}

impl AppCore {
    fn settings_plugins(&self) -> Vec<PluginView> {
        // A stopped instance may still project an older package in the global snapshot.
        // Static settings use the installed record unless a live process owns that version.
        let records = self.store.lock().unwrap().plugins.clone();
        let projected = self.snapshot().plugins;
        records
            .into_iter()
            .filter_map(|record| {
                let mut p = projected
                    .iter()
                    .find(|p| p.manifest.id == record.manifest.id)?
                    .clone();
                if ![
                    PluginStatus::Running,
                    PluginStatus::Starting,
                    PluginStatus::Stopping,
                ]
                .contains(&p.status)
                {
                    p.manifest = record.manifest;
                    p.directory = record.directory;
                }
                Some(p)
            })
            .collect()
    }
    fn settings_definition(
        &self,
        plugin: &str,
        page_id: &str,
    ) -> Result<(PluginView, Page, String), String> {
        let p = self
            .settings_plugins()
            .into_iter()
            .find(|p| p.manifest.id == plugin)
            .ok_or("插件未安装")?;
        let raw = p
            .manifest
            .settings
            .as_ref()
            .ok_or("此版本尚未提供组件设置，请更新插件")?;
        let schema = raw["schemaVersion"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("设置 schemaVersion 无效")?;
        if raw["apiVersion"] != "1" {
            return Err("此插件设置需要更新版本的核心".into());
        }
        let pages = raw["pages"]
            .as_array()
            .filter(|p| !p.is_empty() && p.len() <= 8)
            .ok_or("设置页面数量无效")?;
        if pages.iter().filter(|v| v["id"] == page_id).count() != 1 {
            return Err("设置页面不存在或重复".into());
        }
        let page: Page =
            serde_json::from_value(pages.iter().find(|v| v["id"] == page_id).unwrap().clone())
                .map_err(|e| format!("设置声明无效：{e}"))?;
        page.validate()?;
        for view in &page.view_types {
            if pages
                .iter()
                .filter(|v| {
                    v["viewTypes"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(view)))
                })
                .count()
                != 1
            {
                return Err("一个视图只能关联一个设置页".into());
            }
            if !p
                .manifest
                .ui
                .as_ref()
                .is_some_and(|u| u.views.iter().any(|d| d.id == *view))
            {
                return Err("设置关联的视图不存在".into());
            }
        }
        let token = page.token(plugin, &p.manifest.version, schema);
        Ok((p, page, token))
    }
    pub fn settings_catalog(&self) -> Value {
        let mut entries = Vec::new();
        for p in self.settings_plugins() {
            let pages = p
                .manifest
                .settings
                .as_ref()
                .and_then(|v| v["pages"].as_array());
            if let Some(pages) = pages.filter(|a| !a.is_empty() && a.len() <= 8) {
                for raw in pages {
                    let id = raw["id"].as_str().unwrap_or("");
                    let error = self.settings_definition(&p.manifest.id, id).err();
                    entries.push(json!({"pluginId":p.manifest.id,"pluginName":p.manifest.name,"version":p.manifest.version,"pageId":id,"title":raw["title"].as_str().unwrap_or("组件设置"),"icon":raw["icon"],"order":raw["order"].as_u64().unwrap_or(1000),"enabled":p.enabled,"available":error.is_none(),"error":error}));
                }
            } else {
                entries.push(json!({"pluginId":p.manifest.id,"pluginName":p.manifest.name,"version":p.manifest.version,"pageId":"","title":p.manifest.name,"order":1000,"enabled":p.enabled,"available":false,"error":"此版本尚未提供可用组件设置，请更新插件"}));
            }
        }
        entries.sort_by_key(|v| {
            (
                v["order"].as_u64().unwrap_or(1000),
                v["pluginId"].to_string(),
                v["pageId"].to_string(),
            )
        });
        json!(entries)
    }
    pub(crate) fn settings_index(&self, plugin: &str) -> Value {
        let pages: Vec<Value> = self.settings_catalog().as_array().into_iter().flatten().filter(|p| p["pluginId"] == plugin && p["available"] == true).filter_map(|p| {
            self.settings_page(plugin, p["pageId"].as_str()?).ok().map(|s| json!({"pageId":s["pageId"],"definitionToken":s["definitionToken"],"revision":s["revision"]}))
        }).collect();
        json!(pages)
    }
    fn settings_snapshot(&self, plugin: &str, page_id: &str) -> Result<Value, String> {
        let (p, page, token) = self.settings_definition(plugin, page_id)?;
        let key = format!("{plugin}/{page_id}");
        let saved = self
            .settings
            .lock()
            .unwrap()
            .reconcile(&key, &page, &token)?;
        let (values, warnings) = page.effective(&saved.values);
        let surfaces = self.ui_data_surface_ids();
        let applications: Vec<Value> = self
            .settings
            .lock()
            .unwrap()
            .applications
            .values()
            .filter(|a| {
                a["pluginId"] == plugin
                    && a["pageId"] == page_id
                    && a["definitionToken"] == token
                    && a["revision"] == saved.revision.to_string()
                    && (a["surfaceId"]
                        .as_str()
                        .is_some_and(|s| surfaces.contains(s))
                        || (a["target"] == "backend"
                            && a["instanceId"].as_str() == p.instance_id.as_deref()))
            })
            .cloned()
            .collect();
        Ok(
            json!({"pluginId":plugin,"pageId":page_id,"version":p.manifest.version,"definitionToken":token,"revision":saved.revision.to_string(),"declaration":page,"values":values,"savedValues":saved.values,"warnings":warnings,"applications":applications,"application":if page.fields().all(|f| f.apply == "newInstance") { "newInstance" } else if p.status != PluginStatus::Running { "nextInstance" } else { "pending" }}),
        )
    }
    pub fn settings_page(&self, plugin: &str, page_id: &str) -> Result<Value, String> {
        // Startup holds mutations while awaiting initialize. A plugin may read settings
        // from initialize, so reads must never wait for that lifecycle lock.
        self.settings_snapshot(plugin, page_id)
    }
    pub fn save_settings(
        self: &Arc<Self>,
        plugin: &str,
        page_id: &str,
        input: Value,
    ) -> Result<Value, String> {
        let operation = self.mutations.lock().unwrap();
        let (_, page, token) = self.settings_definition(plugin, page_id)?;
        let key = format!("{plugin}/{page_id}");
        let request = input["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("保存请求 ID 无效")?;
        let values: Map<String, Value> = input["values"].as_object().ok_or("缺少设置值")?.clone();
        let mut s = self.settings.lock().unwrap();
        if let Some(receipt) = s
            .receipts
            .iter()
            .find(|r| r.key == key && r.request == request)
        {
            return if receipt.body == input {
                Ok(receipt.result.clone())
            } else {
                Err("相同请求 ID 的正文不同".into())
            };
        }
        page.validate_values(&values)?;
        runtime_snapshot(plugin, page_id, &token, u64::MAX, &values)?;
        let mut saved = s.reconcile(&key, &page, &token)?;
        if input["definitionToken"] != token || input["baseRevision"] != saved.revision.to_string()
        {
            drop(s);
            return Ok(
                json!({"persisted":false,"conflict":true,"snapshot":self.settings_snapshot(plugin, page_id)?}),
            );
        }
        // Archive invalid originals before an explicit save replaces effective fallbacks.
        if page.effective(&saved.values).1.len() > 0 {
            saved.history.insert(
                0,
                store::History {
                    structure: saved.structure.clone(),
                    values: saved.values.clone(),
                },
            );
            saved.history.truncate(2);
        }
        saved.values = values;
        saved.revision = saved.revision.checked_add(1).ok_or("设置修订已耗尽")?;
        let mut doc = s.document.clone();
        doc.pages.insert(key.clone(), saved);
        s.commit(doc)?;
        drop(s);
        let snapshot = self.settings_snapshot(plugin, page_id)?;
        let result = json!({"persisted":true,"snapshot":snapshot});
        self.settings.lock().unwrap().remember(store::Receipt {
            key,
            request: request.into(),
            body: input,
            result: result.clone(),
        });
        drop(operation);
        self.settings_broadcast(snapshot);
        Ok(result)
    }
    fn settings_broadcast(self: &Arc<Self>, snapshot: Value) {
        self.event("core-settings-changed", snapshot.clone());
        let instance = self
            .instances
            .lock()
            .unwrap()
            .get(snapshot["pluginId"].as_str().unwrap_or(""))
            .cloned();
        if let Some(instance) = instance.filter(|i| {
            i.settings_api.load(std::sync::atomic::Ordering::Acquire)
                && i.view().status == PluginStatus::Running
        }) {
            let core = self.clone();
            std::thread::spawn(move || {
                let instance_id = instance.view().instance_id;
                let result = instance.request("settings.changed", json!({"pageId":snapshot["pageId"],"definitionToken":snapshot["definitionToken"],"revision":snapshot["revision"],"values":snapshot["values"]}));
                if core
                    .instances
                    .lock()
                    .unwrap()
                    .get(snapshot["pluginId"].as_str().unwrap_or(""))
                    .is_some_and(|i| i.view().instance_id == instance_id)
                {
                    core.settings_report(json!({"pluginId":snapshot["pluginId"],"pageId":snapshot["pageId"],"definitionToken":snapshot["definitionToken"],"revision":snapshot["revision"],"target":"backend","instanceId":instance_id,"error":result.err()}));
                }
            });
        }
    }
    pub fn retry_settings_apply(
        self: &Arc<Self>,
        plugin: &str,
        page: &str,
        revision: &str,
    ) -> Result<Value, String> {
        let snapshot = self.settings_page(plugin, page)?;
        if snapshot["revision"] != revision {
            return Err("设置已变更，请刷新后重试".into());
        }
        self.settings_broadcast(snapshot.clone());
        Ok(snapshot)
    }
    pub(crate) fn settings_plugin_get(
        &self,
        plugin: &str,
        instance: &str,
        page: &str,
    ) -> Result<Value, String> {
        if !self
            .instances
            .lock()
            .unwrap()
            .get(plugin)
            .is_some_and(|i| i.view().instance_id.as_deref() == Some(instance))
        {
            return Err("插件实例已失效".into());
        }
        let snapshot = self.settings_page(plugin, page)?;
        runtime_snapshot(
            plugin,
            page,
            snapshot["definitionToken"].as_str().ok_or("缺少设置定义")?,
            snapshot["revision"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or("设置修订无效")?,
            snapshot["values"].as_object().ok_or("设置值无效")?,
        )
    }
    pub fn settings_for_surface(&self, window: &str, surface: &str) -> Result<Value, String> {
        let view = self.ui_data_view(window, surface)?;
        if !self
            .instances
            .lock()
            .unwrap()
            .get(&view.plugin_id)
            .is_some_and(|i| i.view().instance_id.as_ref() == Some(&view.plugin_instance_id))
        {
            return Err("组件实例已失效".into());
        }
        let kind = view.definition_key.split(':').nth(1).unwrap_or("");
        let p = self
            .settings_plugins()
            .into_iter()
            .find(|p| p.manifest.id == view.plugin_id)
            .ok_or("插件未安装")?;
        let page = p
            .manifest
            .settings
            .as_ref()
            .and_then(|s| s["pages"].as_array())
            .and_then(|pages| {
                pages.iter().find(|p| {
                    p["viewTypes"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(kind)))
                })
            });
        match page.and_then(|p| p["id"].as_str()) {
            Some(id) => self.settings_plugin_get(&view.plugin_id, &view.plugin_instance_id, id),
            None => Ok(Value::Null),
        }
    }
    pub fn settings_applied(
        &self,
        window: &str,
        surface: &str,
        input: Value,
    ) -> Result<Value, String> {
        let snapshot = self.settings_for_surface(window, surface)?;
        if snapshot.is_null()
            || ["pageId", "definitionToken", "revision"]
                .iter()
                .any(|k| input[*k] != snapshot[*k])
        {
            return Err("设置应用回报已过期".into());
        }
        let error = input["error"]
            .as_str()
            .map(|s| s.chars().take(1024).collect::<String>());
        self.settings_report(json!({"pluginId":snapshot["pluginId"],"pageId":snapshot["pageId"],"definitionToken":snapshot["definitionToken"],"revision":snapshot["revision"],"target":"surface","surfaceId":surface,"error":error}));
        Ok(json!({"accepted":true}))
    }
    fn settings_report(&self, report: Value) {
        let key = format!(
            "{}/{}",
            report["pluginId"].as_str().unwrap_or(""),
            report["pageId"].as_str().unwrap_or("")
        );
        let mut store = self.settings.lock().unwrap();
        if !store.document.pages.get(&key).is_some_and(|p| {
            report["definitionToken"] == p.token && report["revision"] == p.revision.to_string()
        }) {
            return;
        }
        let target = report["surfaceId"].as_str().unwrap_or("backend");
        store
            .applications
            .insert(format!("{key}/{target}"), report.clone());
        while store.applications.len() > 1024 {
            let first = store.applications.keys().next().cloned().unwrap();
            store.applications.remove(&first);
        }
        drop(store);
        self.event("core-settings-application", report);
    }
    pub fn settings_resource(
        &self,
        plugin: &str,
        token: &str,
        relative: &str,
    ) -> Result<(Vec<u8>, &'static str), String> {
        let p = self
            .settings_plugins()
            .into_iter()
            .find(|p| p.manifest.id == plugin)
            .ok_or("插件未安装")?;
        let pages = p
            .manifest
            .settings
            .as_ref()
            .and_then(|s| s["pages"].as_array())
            .ok_or("插件没有设置资源")?;
        for raw in pages {
            if let Ok((_, page, actual)) =
                self.settings_definition(plugin, raw["id"].as_str().unwrap_or(""))
            {
                if actual == token
                    && page
                        .fields()
                        .flat_map(|f| &f.presets)
                        .any(|font| font.regular == relative || font.bold == relative)
                {
                    let path = crate::component_ui::resource(&p.directory, relative)?;
                    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
                        return Err("字体资源超过 4 MiB".into());
                    }
                    return Ok((fs::read(path).map_err(|e| e.to_string())?, "font/woff2"));
                }
            }
        }
        Err("设置资源不存在或版本已变更".into())
    }
}
