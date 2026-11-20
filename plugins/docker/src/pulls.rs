use crate::{cli, commands, connection::Snapshot, resources, runtime};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[derive(Clone)]
struct Pull {
    ticket: Option<Value>,
    target: Snapshot,
    id: Option<String>,
    created: Instant,
    cancelled: bool,
    finished: bool,
    epoch: u64,
    reference: String,
    request: String,
}
pub struct Pulls {
    app: runtime::Shared,
    client: Client,
    items: Mutex<BTreeMap<String, Pull>>,
    admission: Mutex<()>,
}
impl Pulls {
    pub fn new(app: runtime::Shared, client: Client) -> Arc<Self> {
        let service = Arc::new(Self {
            app,
            client,
            items: Mutex::new(BTreeMap::new()),
            admission: Mutex::new(()),
        });
        let weak = Arc::downgrade(&service);
        std::thread::spawn(move || loop {
            let Some(service) = weak.upgrade() else { break };
            if service.client.is_closed() {
                break;
            }
            service.poll();
            drop(service);
            std::thread::sleep(Duration::from_millis(500));
        });
        service
    }
    pub fn event(&self, view: &str, input: &Value) -> Result<Value, String> {
        if input["action"] == "pull.cancel" {
            let mut items = self.items.lock().unwrap();
            let p = items.get_mut(view).ok_or("没有拉取任务")?;
            p.cancelled = true;
            return Ok(json!({"accepted":true}));
        }
        if input["action"] != "pull.start" {
            return Err("未知拉取动作".into());
        }
        let reference =
            resources::pull_reference(input["reference"].as_str().ok_or("缺少镜像引用")?)?;
        // Reserve the view and capacity together without nesting App/items locks.
        let admission = self.admission.lock().unwrap();
        let a = self.app.lock().unwrap();
        let v = a.views.get(view).ok_or("页面已关闭")?;
        if let Some((_, _, old)) = v.intent_receipt(input["requestId"].as_str().unwrap_or("")) {
            if *old != *input {
                return Err("相同请求参数不同".into());
            }
            return Ok(json!({"accepted":true,"replayed":true}));
        }
        a.check_scope(view, input)?;
        if a.views[view].kind != "browser" {
            return Err("请从资源树拉取镜像".into());
        }
        drop(a);
        {
            let items = self.items.lock().unwrap();
            if items.get(view).is_some_and(|p| !p.finished) {
                return Err("当前资源树已有拉取任务".into());
            }
            if items.values().filter(|p| !p.finished).count() >= 2 {
                return Err("插件已有两个镜像拉取任务，请稍后重试".into());
            }
        }
        let mut a = self.app.lock().unwrap();
        a.check_scope(view, input)?;
        a.record_intent(view, input, input.clone())?;
        let v = a.views.get_mut(view).unwrap();
        let target = v.target.as_ref().ok_or("Docker 连接尚未识别")?.clone();
        v.pull = json!({"state":"opening","reference":reference,"context":v.context,"endpoint":v.endpoint,"engineId":v.expected_engine,"scopeEpoch":v.epoch.to_string(),"message":"正在启动公开镜像拉取","requestId":input["requestId"]});
        v.revision += 1;
        let epoch = v.epoch;
        drop(a);
        let mut items = self.items.lock().unwrap();
        let old_id = items.remove(view).and_then(|p| p.id);
        items.insert(
            view.into(),
            Pull {
                ticket: None,
                target,
                id: None,
                created: Instant::now(),
                cancelled: false,
                finished: false,
                epoch,
                reference,
                request: input["requestId"].as_str().unwrap().into(),
            },
        );
        drop(items);
        drop(admission);
        if let Some(id) = old_id {
            let _ = self
                .client
                .call("session.stream.release", json!({"streamId":id}));
        }
        Ok(json!({"accepted":true}))
    }
    pub fn dispose(&self, view: &str) {
        let removed = self.items.lock().unwrap().remove(view);
        if let Some(p) = removed {
            if let Some(id) = p.id {
                let _ = self
                    .client
                    .call("session.stream.release", json!({"streamId":id}));
            }
        }
    }
    fn poll(&self) {
        let mut items = self.items.lock().unwrap().clone();
        let alive = self
            .app
            .lock()
            .unwrap()
            .views
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        for view in items
            .keys()
            .filter(|id| !alive.contains(*id))
            .cloned()
            .collect::<Vec<_>>()
        {
            self.dispose(&view);
            items.remove(&view);
        }
        for (view, p) in items.iter_mut().filter(|(_, p)| !p.finished) {
            let expired = p.created.elapsed() > Duration::from_secs(15 * 60);
            let current = self.app.lock().unwrap().views.get(view).is_some_and(|v| {
                v.epoch == p.epoch
                    && v.pull["requestId"] == p.request
                    && v.target.as_ref().is_some_and(|t| t.same(&p.target))
            });
            if !current {
                p.cancelled = true;
            }
            if p.ticket.is_none() && !p.cancelled && !expired {
                let result = cli::prepare(
                    &self.client,
                    view,
                    &p.target,
                    &["image", "pull", &p.reference],
                    None,
                    false,
                )
                .map_err(|e| e.message)
                .and_then(|prepared| {
                    runtime::open_stream(
                        &self.app,
                        &self.client,
                        view,
                        &p.target,
                        "pull",
                        prepared,
                        4 * 1024 * 1024,
                        true,
                    )
                });
                match result {
                    Ok((ticket, reply)) => {
                        p.ticket = Some(ticket);
                        p.id = reply.and_then(|r| r["streamId"].as_str().map(str::to_string));
                    }
                    Err(error) => {
                        p.finished = true;
                        self.finish(view, p, "failed", &error);
                        continue;
                    }
                }
            }
            if p.ticket.is_none() {
                p.finished = true;
                self.finish(view, p, "failed", "拉取尚未发送，已取消或目标改变");
                continue;
            }
            if p.id.is_none() {
                let found = self
                    .client
                    .call_detailed("session.stream.lookup", p.ticket.as_ref().unwrap().clone());
                let result = match found {
                    Ok(r) if r["known"] == true => Ok(r),
                    Ok(_) if !p.cancelled && !expired => self
                        .client
                        .call_detailed("session.stream.open", p.ticket.as_ref().unwrap().clone()),
                    Ok(_) => {
                        p.finished = true;
                        self.finish(
                            view,
                            p,
                            "unknown",
                            "原拉取启动回执未确认；不会生成新的操作意图",
                        );
                        continue;
                    }
                    Err(e) => Err(e),
                };
                match result {
                    Ok(r) => p.id = r["streamId"].as_str().map(str::to_owned),
                    Err(e) if e.code == -32003 => continue,
                    Err(e) => {
                        p.finished = true;
                        self.finish(view, p, "failed", &e.message);
                        continue;
                    }
                }
            }
            let Some(id) = &p.id else { continue };
            if p.cancelled || expired {
                let _ = self
                    .client
                    .call("session.stream.close", json!({"streamId":id}));
            }
            match self
                .client
                .call("session.stream.status", json!({"streamId":id}))
            {
                Ok(status) => {
                    {
                        let mut a = self.app.lock().unwrap();
                        if let Some(v) = a.views.get_mut(view).filter(|v| {
                            v.epoch == p.epoch
                                && v.pull["requestId"] == p.request
                                && v.target.as_ref().is_some_and(|t| t.same(&p.target))
                        }) {
                            v.pull["producedBytes"] = status["producedBytes"].clone();
                        }
                    }
                    if status["finished"] == true || expired {
                        p.finished = true;
                        if let Some(message) = cli::startup_error(&status) {
                            self.finish(view, p, "failed", &message);
                            continue;
                        }
                        let (state, message) = if status["finished"] == true
                            && status["state"] == "completed"
                            && status["remoteExitConfirmed"] == true
                            && status["exitCode"] == 0
                        {
                            ("succeeded", "镜像拉取成功；资源索引待刷新")
                        } else if status["state"] == "completed"
                            && status["remoteExitConfirmed"] == true
                        {
                            (
                                "failed",
                                "拉取失败，请查看命令输出；当前仅管理公开源，不保存 Registry 凭据",
                            )
                        } else {
                            (
                                "unknown",
                                "取消、超时或连接中断，远端结果未确认；请刷新镜像，不会自动重发",
                            )
                        };
                        self.finish(view, p, state, message);
                    } else {
                        let mut a = self.app.lock().unwrap();
                        if let Some(v) = a.views.get_mut(view).filter(|v| {
                            v.epoch == p.epoch
                                && v.pull["requestId"] == p.request
                                && v.target.as_ref().is_some_and(|t| t.same(&p.target))
                        }) {
                            v.pull["state"] = json!("running");
                            v.pull["streamId"] = json!(id);
                            v.pull["producedBytes"] = status["producedBytes"].clone();
                            v.pull["message"] = json!("正在拉取 · Docker 尚未确认退出");
                            v.revision += 1;
                        }
                    }
                }
                Err(error) => {
                    let mut a = self.app.lock().unwrap();
                    if let Some(v) = a.views.get_mut(view).filter(|v| {
                        v.epoch == p.epoch
                            && v.pull["requestId"] == p.request
                            && v.target.as_ref().is_some_and(|t| t.same(&p.target))
                    }) {
                        v.pull["message"] = json!(commands::text(&error, 1024));
                        v.revision += 1;
                    }
                }
            }
            if expired {
                if let Some(id) = &p.id {
                    let _ = self
                        .client
                        .call("session.stream.release", json!({"streamId":id}));
                }
                p.id = None;
            }
        }
        let mut orphaned = vec![];
        {
            let mut registry = self.items.lock().unwrap();
            for (view, mut p) in items {
                if let Some(current) = registry
                    .get_mut(&view)
                    .filter(|old| old.created == p.created)
                {
                    p.cancelled |= current.cancelled;
                    *current = p;
                } else if let Some(id) = p.id {
                    orphaned.push(id);
                }
            }
            while registry.len() > 8 {
                let oldest = registry
                    .iter()
                    .filter(|(_, p)| p.finished)
                    .min_by_key(|(_, p)| p.created)
                    .map(|(id, _)| id.clone());
                let Some(view) = oldest else { break };
                if let Some(id) = registry.remove(&view).and_then(|p| p.id) {
                    orphaned.push(id);
                }
            }
        }
        for id in orphaned {
            let _ = self
                .client
                .call("session.stream.release", json!({"streamId":id}));
        }
    }
    fn finish(&self, view: &str, p: &Pull, state: &str, message: &str) {
        let mut a = self.app.lock().unwrap();
        if let Some(v) = a.views.get_mut(view).filter(|v| {
            v.epoch == p.epoch
                && v.pull["requestId"] == p.request
                && v.target.as_ref().is_some_and(|t| t.same(&p.target))
        }) {
            v.pull["state"] = json!(state);
            v.pull["streamId"] = json!(p.id);
            v.pull["message"] = json!(commands::text(message, 2048));
            v.pull["reference"] = json!(p.reference);
            let task = v.pull.clone();
            v.tasks.push_front(task);
            if v.tasks.len() > 64 {
                v.tasks.pop_back();
            }
            v.revision += 1;
            if v.epoch == p.epoch && v.pull["requestId"] == p.request && state == "succeeded" {
                a.refresh_loaded(view);
            }
        }
        drop(a);
        let _=self.client.call("logging.write",json!({"level":if state=="succeeded"{"info"}else{"warn"},"message":"Docker 镜像拉取完成","fields":{"viewId":view,"reference":p.reference,"result":state}}));
        if state != "succeeded" {
            let _=self.client.call("messages.publish",json!({"title":"Docker 镜像拉取未完成","body":format!("{}：请在 Docker 任务中查看结果",p.reference),"level":"warning","notify":false}));
        }
    }
}
