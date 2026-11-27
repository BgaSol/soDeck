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
            json!({"data":{"kind":v.kind,"draft":v.draft,"navigation":v.navigation,"dirty":v.dirty,"catalog":a.snapshot(&v.server,v.navigation["offset"].as_u64().unwrap_or(0) as usize)},"metadata":{"dirty":v.dirty,"busy":a.write_view.as_deref()==Some(id)}}),
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
    pub fn save(&self, view: &str) -> Result<Value, String> {
        let _gate = self
            .changes
            .try_lock()
            .map_err(|_| "changeBusy：已有规则变更正在保存，请稍后重试")?;
        let (mut candidate, server, draft, revision) = {
            let mut a = self.state.lock().unwrap();
            let v = a.views.get(view).ok_or("视图已关闭")?;
            if v.kind != "rule-editor" || !v.dirty {
                return Ok(v.draft.clone());
            }
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
            if let Some(v) = a
                .views
                .get_mut(view)
                .filter(|v| v.draft_revision == revision)
            {
                v.draft = saved.clone();
                v.dirty = false;
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
