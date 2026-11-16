use crate::{commands, resources, runtime};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Pull {
    ticket: Value,
    id: Option<String>,
    created: Instant,
    cancelled: bool,
    finished: bool,
    epoch: u64,
    reference: String,
}
pub struct Pulls {
    app: runtime::Shared,
    client: Client,
    items: Mutex<BTreeMap<String, Pull>>,
}
impl Pulls {
    pub fn new(app: runtime::Shared, client: Client) -> Arc<Self> {
        let service = Arc::new(Self {
            app,
            client,
            items: Mutex::new(BTreeMap::new()),
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
        let mut items = self.items.lock().unwrap();
        let mut a = self.app.lock().unwrap();
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
        if items.get(view).is_some_and(|p| !p.finished) {
            return Err("当前资源树已有拉取任务".into());
        }
        if items.values().filter(|p| !p.finished).count() >= 2 {
            return Err("插件已有两个镜像拉取任务，请稍后重试".into());
        }
        a.record_intent(view, input, input.clone())?;
        let v = a.views.get_mut(view).unwrap();
        v.sequence_for_stream();
        let check = commands::docker(&v.endpoint, &["info", "--format", "{{.ID}}"]);
        let pull = commands::docker(&v.endpoint, &["image", "pull", &reference]);
        let command=format!("actual=$({check}) || exit; [ \"$actual\" = {} ] || {{ printf '%s\\n' 'Docker Engine changed; pull not started' >&2; exit 1; }}; exec {pull}",commands::quote(&v.expected_engine));
        let ticket = json!({"viewId":view,"expectedContext":v.context,"operationId":format!("pull-{}-{}",v.epoch,v.command_sequence()),"sequence":v.command_sequence().to_string(),"command":command,"maxOutputBytes":4*1024*1024,"retention":"fixed"});
        v.pull = json!({"state":"opening","reference":reference,"context":v.context,"endpoint":v.endpoint,"engineId":v.expected_engine,"scopeEpoch":v.epoch.to_string(),"message":"正在启动公开镜像拉取","requestId":input["requestId"]});
        v.revision += 1;
        let epoch = v.epoch;
        drop(a);
        if let Some(old) = items.remove(view) {
            if let Some(id) = old.id {
                let _ = self
                    .client
                    .call("session.stream.release", json!({"streamId":id}));
            }
        }
        items.insert(
            view.into(),
            Pull {
                ticket,
                id: None,
                created: Instant::now(),
                cancelled: false,
                finished: false,
                epoch,
                reference,
            },
        );
        Ok(json!({"accepted":true}))
    }
    pub fn dispose(&self, view: &str) {
        if let Some(p) = self.items.lock().unwrap().remove(view) {
            if let Some(id) = p.id {
                let _ = self
                    .client
                    .call("session.stream.release", json!({"streamId":id}));
            }
        }
    }
    fn poll(&self) {
        let mut items = self.items.lock().unwrap();
        let closed = items
            .keys()
            .filter(|id| !self.app.lock().unwrap().views.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        for view in closed {
            if let Some(p) = items.remove(&view) {
                if let Some(id) = p.id {
                    let _ = self
                        .client
                        .call("session.stream.release", json!({"streamId":id}));
                }
            }
        }
        for (view, p) in items.iter_mut().filter(|(_, p)| !p.finished) {
            let expired = p.created.elapsed() > Duration::from_secs(15 * 60);
            if p.id.is_none() {
                let found = self
                    .client
                    .call_detailed("session.stream.lookup", p.ticket.clone());
                let result = match found {
                    Ok(r) if r["known"] == true => Ok(r),
                    Ok(_) if !p.cancelled && !expired => self
                        .client
                        .call_detailed("session.stream.open", p.ticket.clone()),
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
                        if let Some(v) = a.views.get_mut(view) {
                            v.pull["producedBytes"] = status["producedBytes"].clone();
                        }
                    }
                    if status["finished"] == true || expired {
                        p.finished = true;
                        let (state, message) = if status["finished"] == true
                            && status["remoteExitConfirmed"] == true
                            && status["exitCode"] == 0
                        {
                            ("succeeded", "镜像拉取成功；资源索引待刷新")
                        } else if status["remoteExitConfirmed"] == true {
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
                        if let Some(v) = a.views.get_mut(view) {
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
                    if let Some(v) = a.views.get_mut(view) {
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
        while items.len() > 8 {
            let oldest = items
                .iter()
                .filter(|(_, p)| p.finished)
                .min_by_key(|(_, p)| p.created)
                .map(|(id, _)| id.clone());
            let Some(view) = oldest else { break };
            if let Some(p) = items.remove(&view) {
                if let Some(id) = p.id {
                    let _ = self
                        .client
                        .call("session.stream.release", json!({"streamId":id}));
                }
            }
        }
    }
    fn finish(&self, view: &str, p: &Pull, state: &str, message: &str) {
        let mut a = self.app.lock().unwrap();
        if let Some(v) = a.views.get_mut(view) {
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
            if v.epoch == p.epoch && state == "succeeded" {
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
