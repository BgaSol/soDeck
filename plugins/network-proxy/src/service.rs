use crate::{
    checks,
    rules::{Rule, Rules},
    runtime::{self, App},
};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Service {
    pub state: Arc<Mutex<App>>,
    changes: Mutex<()>,
    wake: SyncSender<()>,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl Service {
    pub fn new(directory: PathBuf, client: Client) -> Arc<Self> {
        let (wake, receive) = mpsc::sync_channel(1);
        let service = Arc::new(Self {
            state: Arc::new(Mutex::new(App {
                rules: Rules::open(directory),
                views: HashMap::new(),
                runs: HashMap::new(),
                checks: HashMap::new(),
                sequence: 0,
                write_view: None,
            })),
            changes: Mutex::new(()),
            wake,
        });
        let worker = service.clone();
        let runtime_client = client.clone();
        std::thread::spawn(move || {
            while !runtime_client.is_closed() {
                runtime::tick(&worker.state, &runtime_client);
                worker.auto_checks(&runtime_client);
                worker.changed();
                std::thread::sleep(Duration::from_millis(35));
            }
        });
        crate::publication::start(service.clone(), client, receive);
        service
    }
    pub fn changed(&self) {
        let _ = self.wake.try_send(());
    }
    pub fn response(&self, id: &str) -> Result<Value, String> {
        let a = self.state.lock().unwrap();
        let v = a.views.get(id).ok_or("视图已关闭")?;
        Ok(
            json!({"data":{"kind":v.kind,"draft":v.draft,"draftRevision":v.draft_revision.to_string(),"navigation":v.navigation,"navigationRevision":v.navigation_revision.to_string(),"dirty":v.dirty,"writePending":a.write_view.as_deref()==Some(id),"catalog":a.snapshot(&v.server,v.navigation["offset"].as_u64().unwrap_or(0) as usize)},"metadata":{"dirty":v.dirty,"busy":a.write_view.as_deref()==Some(id)}}),
        )
    }
    pub fn rule_for(&self, view: &str, id: &str) -> Result<Rule, String> {
        let a = self.state.lock().unwrap();
        let v = a.views.get(view).ok_or("视图已关闭")?;
        a.rules
            .items
            .iter()
            .find(|r| r.id == id && r.server == v.server)
            .cloned()
            .ok_or("规则不属于此服务器".into())
    }
    pub fn begin_edit(&self, view: &str, mut rule: Value) -> Result<(), String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "配置正在保存，请稍后重试")?;
        let mut a = self.state.lock().unwrap();
        let v = a.views.get(view).ok_or("视图已关闭")?;
        if v.dirty {
            return Err("请先保存或放弃当前草稿".into());
        }
        if rule["serverId"] != v.server || !rule["id"].is_string() {
            return Err("规则不属于当前服务器".into());
        }
        if let Some(existing) = a.rules.items.iter().find(|r| rule["id"] == r.id) {
            if existing.server != v.server || rule["revision"].as_u64() != Some(existing.revision) {
                return Err("规则已变化，请刷新后编辑".into());
            }
            rule = existing.value();
        } else if rule["revision"] != 0 {
            return Err("规则已删除，请刷新".into());
        }
        if serde_json::to_vec(&rule).map_err(|e| e.to_string())?.len() > 4096 {
            return Err("草稿过大".into());
        }
        let v = a.views.get_mut(view).unwrap();
        v.draft_revision = v.draft_revision.checked_add(1).ok_or("草稿序号已用尽")?;
        v.draft_base = rule.clone();
        v.draft = rule;
        v.dirty = false;
        Ok(())
    }
    pub fn update_draft(&self, view: &str, rule: Value, expected: &Value) -> Result<(), String> {
        if serde_json::to_vec(&rule).map_err(|e| e.to_string())?.len() > 4096 {
            return Err("草稿过大".into());
        }
        let mut a = self.state.lock().unwrap();
        let v = a.views.get_mut(view).ok_or("视图已关闭")?;
        if expected.as_str() != Some(&v.draft_revision.to_string()) || v.draft.is_null() {
            return Err("草稿已变化，请重新读取".into());
        }
        for field in ["id", "serverId", "revision"] {
            if rule[field] != v.draft[field] {
                return Err("草稿身份不能改变".into());
            }
        }
        if v.draft["revision"] != 0 && rule["type"] != v.draft_base["type"] {
            return Err("已有规则不能改变类型".into());
        }
        v.draft_revision = v.draft_revision.checked_add(1).ok_or("草稿序号已用尽")?;
        v.dirty = rule != v.draft_base;
        v.draft = rule;
        Ok(())
    }
    pub fn cancel_edit(&self, view: &str, expected: &Value) -> Result<(), String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "配置正在保存，请稍后重试")?;
        let mut a = self.state.lock().unwrap();
        let v = a.views.get_mut(view).ok_or("视图已关闭")?;
        if expected.as_str() != Some(&v.draft_revision.to_string()) {
            return Err("草稿已变化，请重新读取".into());
        }
        v.draft_revision = v.draft_revision.checked_add(1).ok_or("草稿序号已用尽")?;
        v.draft = Value::Null;
        v.draft_base = Value::Null;
        v.dirty = false;
        Ok(())
    }
    pub fn save(&self, view: &str) -> Result<Value, String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "changeBusy：已有规则变更正在保存，请稍后重试")?;
        let (mut candidate, server, draft, revision) = {
            let mut a = self.state.lock().unwrap();
            let v = a.views.get(view).ok_or("视图已关闭")?;
            if v.draft.is_null() {
                return Ok(v.draft.clone());
            }
            v.draft_revision.checked_add(1).ok_or("草稿序号已用尽")?;
            v.navigation_revision
                .checked_add(1)
                .ok_or("导航序号已用尽")?;
            let values = (
                a.rules.clone(),
                v.server.clone(),
                v.draft.clone(),
                v.draft_revision,
            );
            a.write_view = Some(view.into());
            values
        };
        let result = candidate.save(&server, draft);
        let mut a = self.state.lock().unwrap();
        a.write_view = None;
        if let Ok(saved) = &result {
            a.rules = candidate;
            let offset = a
                .rules
                .items
                .iter()
                .filter(|r| r.server == server)
                .position(|r| saved["id"] == r.id)
                .unwrap_or(0)
                / 20
                * 20;
            if let Some(v) = a
                .views
                .get_mut(view)
                .filter(|v| v.draft_revision == revision)
            {
                v.draft = Value::Null;
                v.draft_base = Value::Null;
                v.dirty = false;
                v.draft_revision += 1;
                v.navigate(json!({"selected":saved["id"],"offset":offset}))?;
            } else if let Some(v) = a
                .views
                .get_mut(view)
                .filter(|v| v.draft["id"] == saved["id"])
            {
                // Keep edits made during commit on the newly saved rule revision.
                v.draft["revision"] = saved["revision"].clone();
                v.draft_base = saved.clone();
                v.dirty = v.draft != v.draft_base;
            }
        }
        drop(a);
        self.changed();
        result
    }
    pub fn delete(&self, view: &str, id: &str, revision: u64) -> Result<(), String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "changeBusy：已有规则变更正在保存，请稍后重试")?;
        let (mut candidate, server) = {
            let mut a = self.state.lock().unwrap();
            let v = a.views.get(view).ok_or("视图已关闭")?;
            if a.runs.get(id).is_some_and(|r| {
                r.snapshot["cleanupComplete"] != true || !r.released || r.query_error.is_some()
            }) {
                return Err("请先停止并确认清理，再删除规则".into());
            }
            let values = (a.rules.clone(), v.server.clone());
            a.write_view = Some(view.into());
            values
        };
        let result = candidate.delete(&server, id, revision);
        let mut a = self.state.lock().unwrap();
        a.write_view = None;
        if result.is_ok() {
            a.rules = candidate;
            a.runs.remove(id);
            a.checks.remove(id);
        }
        drop(a);
        self.changed();
        result
    }
    pub fn start(&self, view: &str, id: &str) -> Result<(), String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "changeBusy：已有规则变更正在保存，请稍后重试")?;
        self.state.lock().unwrap().start(view, id)?;
        self.changed();
        Ok(())
    }
    pub fn stop(&self, id: &str) {
        let _ = self.state.lock().unwrap().stop(id);
        self.changed();
    }
    pub fn check(
        self: &Arc<Self>,
        client: &Client,
        view: &str,
        id: &str,
        target: Value,
    ) -> Result<(), String> {
        let (listener, snapshot, rule, request) = {
            let mut a = self.state.lock().unwrap();
            let v = a.views.get(view).ok_or("工作区已关闭")?;
            let run = a.runs.get(id).ok_or("请先启动隧道")?;
            if run.view != view
                || run.request["expectedContext"] != v.context
                || run.snapshot["state"] != "running"
                || run.stop_requested
                || run.query_error.is_some()
            {
                return Err("隧道尚未就绪".into());
            }
            if a.checks.get(id).is_some_and(|v| v["checking"] == true) {
                return Err("这条规则正在检查，请稍后再试".into());
            }
            if a.checks.values().filter(|v| v["checking"] == true).count() >= 4 {
                return Err("检查任务已满".into());
            }
            let values = (
                run.listener.clone(),
                run.snapshot.clone(),
                run.rule.value(),
                run.request.clone(),
            );
            a.checks.insert(
                id.into(),
                json!({"checking":true,"listenerId":values.0,"runId":values.3["operationId"]}),
            );
            a.runs.get_mut(id).unwrap().auto_check = false;
            values
        };
        let service = self.clone();
        let client = client.clone();
        let id = id.to_owned();
        self.changed();
        std::thread::spawn(move || {
            let result = checks::check(&client, &listener, &snapshot, &rule, &target);
            let mut a = service.state.lock().unwrap();
            if a.runs
                .get(&id)
                .is_some_and(|r| r.request == request && !r.stop_requested)
                && a.checks
                    .get(&id)
                    .is_some_and(|v| v["listenerId"] == listener)
            {
                let (ok, message, failure) = match result {
                    Ok(value) => (
                        true,
                        value["message"].as_str().unwrap_or("TCP 可达").to_owned(),
                        None,
                    ),
                    Err(error) => (false, error.message.clone(), Some(error)),
                };
                a.checks.insert(id,json!({"checking":false,"listenerId":listener,"runId":request["operationId"],"observedAt":now(),"ok":ok,"message":message,"failure":failure,"target":if rule["type"]=="tcpForward"{&rule["target"]}else{&target}}));
            }
            drop(a);
            service.changed();
        });
        Ok(())
    }
    fn auto_checks(self: &Arc<Self>, client: &Client) {
        let ready = {
            let a = self.state.lock().unwrap();
            a.runs
                .iter()
                .filter(|(_, r)| {
                    r.auto_check
                        && !r.stop_requested
                        && r.query_error.is_none()
                        && r.snapshot["state"] == "running"
                })
                .map(|(id, r)| (id.clone(), r.view.clone()))
                .collect::<Vec<_>>()
        };
        for (id, view) in ready {
            let _ = self.check(client, &view, &id, Value::Null);
        }
    }
    pub fn dispose(&self, id: &str) {
        let mut a = self.state.lock().unwrap();
        for r in a.runs.values_mut().filter(|r| r.view == id) {
            r.stop_requested = true;
            r.auto_check = false;
        }
        a.views.remove(id);
        drop(a);
        self.changed();
    }
    pub fn context(&self, id: &str, context: &Value) {
        let mut a = self.state.lock().unwrap();
        if a.views
            .get(id)
            .is_some_and(|v| !context.is_null() && v.context != *context)
        {
            for r in a.runs.values_mut().filter(|r| r.view == id) {
                r.stop_requested = true;
                r.auto_check = false;
            }
            let v = a.views.get_mut(id).unwrap();
            v.context = context.clone();
        }
        drop(a);
        self.changed();
    }
    pub fn ensure(
        &self,
        context: &Value,
        target: crate::rules::Target,
        name: &str,
        selected: &Value,
    ) -> Result<Value, String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "changeBusy：已有规则变更正在保存，请稍后重试")?;
        let (manager, mut candidate, create) = {
            let mut a = self.state.lock().unwrap();
            let manager = a
                .views
                .iter()
                .find(|(_, v)| v.kind == "manager" && v.context == *context)
                .map(|(id, _)| id.clone())
                .ok_or("managerNotReady：网络隧道后台尚未就绪")?;
            let decision = crate::commands::decide(&a, context, &target, selected)?;
            match decision {
                crate::commands::Decision::Select(count) => {
                    return Ok(
                        json!({"version":1,"outcome":"selectionRequired","candidateCount":count,"target":target.value()}),
                    )
                }
                crate::commands::Decision::Use(id) => {
                    let active = a.runs.get(&id).is_some_and(|r| {
                        r.snapshot["cleanupComplete"] != true
                            || !r.released
                            || r.query_error.is_some()
                    });
                    let disposition = if active {
                        if a.runs[&id].snapshot["state"] == "running" {
                            "reused"
                        } else {
                            "joined"
                        }
                    } else {
                        a.start(&manager, &id)?;
                        a.runs.get_mut(&id).unwrap().auto_check = true;
                        "started"
                    };
                    return Ok(crate::commands::accepted(&a, &id, disposition));
                }
                crate::commands::Decision::Create => {}
            }
            if let Some(e) = &a.rules.error {
                return Err(format!("storageUnavailable：{e}"));
            }
            a.sequence = a
                .sequence
                .checked_add(1)
                .ok_or("resourceLimit：操作序号已用尽")?;
            let id = format!("tcp-{}-{}-{}", now(), std::process::id(), a.sequence);
            if a.rules.items.iter().any(|r| r.id == id) {
                return Err("resourceLimit：规则标识冲突".into());
            }
            let value = json!({"id":id,"serverId":context["serverId"],"revision":0,"name":name,"type":"tcpForward","bind":{"host":"127.0.0.1","port":0},"target":target.value()});
            let candidate = a.rules.clone();
            a.write_view = Some(manager.clone());
            (manager, candidate, value)
        };
        let saved = candidate.save(
            context["serverId"]
                .as_str()
                .ok_or("invalidParams：缺少服务器")?,
            create,
        );
        let mut a = self.state.lock().unwrap();
        a.write_view = None;
        let saved = saved.map_err(|e| format!("storageUnavailable：{e}"))?;
        let id = saved["id"].as_str().unwrap();
        a.rules = candidate;
        if a.views.get(&manager).is_none_or(|v| v.context != *context) {
            return Ok(
                json!({"version":1,"outcome":"savedNotStarted","ruleId":id,"ruleRevision":saved["revision"],"reasonCode":"workspaceClosing","message":"规则已保存，工作区已结束，尚未启动"}),
            );
        }
        if let Err(error) = a.start(&manager, id) {
            return Ok(
                json!({"version":1,"outcome":"savedNotStarted","ruleId":id,"ruleRevision":saved["revision"],"reasonCode":"startRejected","message":error}),
            );
        }
        a.runs.get_mut(id).unwrap().auto_check = true;
        let result = crate::commands::accepted(&a, id, "created");
        drop(a);
        self.changed();
        Ok(result)
    }
}
#[cfg(test)]
mod editor_tests {
    use super::*;
    use crate::runtime::View;
    fn service(directory: PathBuf) -> Service {
        let (wake, _) = mpsc::sync_channel(1);
        let context =
            serde_json::json!({"serverId":"server","workspaceId":"workspace","generation":1});
        Service {
            state: Arc::new(Mutex::new(App {
                rules: Rules::open(directory),
                runs: HashMap::new(),
                checks: HashMap::new(),
                sequence: 0,
                write_view: None,
                views: HashMap::from([(
                    "manager".into(),
                    View {
                        server: "server".into(),
                        kind: "manager".into(),
                        context,
                        draft: Value::Null,
                        draft_base: Value::Null,
                        dirty: false,
                        navigation: Value::Null,
                        draft_revision: 0,
                        navigation_revision: 0,
                    },
                )]),
            })),
            changes: Mutex::new(()),
            wake,
        }
    }
    #[test]
    fn one_draft_uses_identity_and_revision_without_owning_the_listener_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let s = service(directory.path().into());
        let rule = serde_json::json!({"id":"first","serverId":"server","revision":0,"name":"","type":"socks5","bind":{"host":"127.0.0.1","port":0}});
        s.begin_edit("manager", rule.clone()).unwrap();
        let revision = s.response("manager").unwrap()["data"]["draftRevision"].clone();
        assert_eq!(s.response("manager").unwrap()["metadata"]["dirty"], false);
        let mut changed = rule.clone();
        changed["name"] = json!("changed");
        s.update_draft("manager", changed.clone(), &revision)
            .unwrap();
        assert!(s.update_draft("manager", rule.clone(), &revision).is_err());
        assert!(s.begin_edit("manager", rule.clone()).is_err());
        s.save("manager").unwrap();
        let response = s.response("manager").unwrap();
        assert!(response["data"]["draft"].is_null());
        assert_eq!(response["data"]["catalog"]["total"], 1);
        assert_eq!(response["data"]["navigation"]["selected"], "first");
        assert!(s.state.lock().unwrap().runs.is_empty());
        let saved = response["data"]["catalog"]["rules"][0]["rule"].clone();
        s.begin_edit("manager", saved).unwrap();
        let revision = s.response("manager").unwrap()["data"]["draftRevision"].clone();
        s.cancel_edit("manager", &revision).unwrap();
        assert!(s.update_draft("manager", changed, &revision).is_err());
        assert_eq!(s.state.lock().unwrap().rules.items.len(), 1);
    }
    #[test]
    fn save_failure_keeps_draft_and_dirty_and_navigation_is_monotonic() {
        let directory = tempfile::tempdir().unwrap();
        let s = service(directory.path().into());
        let rule = serde_json::json!({"id":"first","serverId":"server","revision":0,"name":"","type":"tcpForward","bind":{"host":"127.0.0.1","port":0},"target":{"host":"","port":0}});
        s.begin_edit("manager", rule.clone()).unwrap();
        let revision = s.response("manager").unwrap()["data"]["draftRevision"].clone();
        let mut changed = rule;
        changed["name"] = json!("unfinished");
        s.update_draft("manager", changed.clone(), &revision)
            .unwrap();
        assert!(s.save("manager").is_err());
        let response = s.response("manager").unwrap();
        assert_eq!(response["metadata"]["dirty"], true);
        assert_eq!(response["metadata"]["busy"], false);
        assert_eq!(response["data"]["draft"], changed);
        let mut a = s.state.lock().unwrap();
        let v = a.views.get_mut("manager").unwrap();
        v.navigate(json!({"selected":"first","offset":0})).unwrap();
        v.navigate(json!({"selected":"first","offset":0})).unwrap();
        assert_eq!(v.navigation_revision, 2);
    }
}
