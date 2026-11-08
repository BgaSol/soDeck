use crate::integration;
use serde_json::{json, Value};
use sodeck_plugin_client::{shared_state, Client};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Session {
    view: String,
    context: Value,
    descriptor: Mutex<Value>,
    cancel: AtomicBool,
    started: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
    publishing: Mutex<()>,
    closing: Mutex<()>,
}
impl Session {
    pub fn new(view: &str, context: Value, number: usize) -> Arc<Self> {
        Arc::new(Self {
            view: view.into(),
            context: context.clone(),
            cancel: AtomicBool::new(false),
            started: AtomicBool::new(false),
            worker: Mutex::new(None),
            publishing: Mutex::new(()),
            closing: Mutex::new(()),
            descriptor: Mutex::new(
                json!({"schemaVersion":1,"revision":"1","viewId":view,"context":context,"ptyId":null,"title":format!("终端 {number}"),"targetLabel":"服务器终端","details":[],"preparation":"preparing","targetReadiness":"pending","capabilities":{"input":true,"resize":true,"location":{"supported":false}},"actions":[{"id":"reopen","label":"新建终端","enabled":true}],"integration":{"mode":"preparing"}}),
            ),
        })
    }
    pub fn start(self: &Arc<Self>, client: &Client) {
        let mut worker = self.worker.lock().unwrap();
        if self.started.swap(true, Ordering::AcqRel) || self.cancel.load(Ordering::Acquire) {
            return;
        }
        let session = self.clone();
        let client = client.clone();
        *worker = Some(thread::spawn(move || {
            let _ = session.publish(&client);
            if let Err(error) = session.prepare(&client) {
                let mut d = session.descriptor.lock().unwrap();
                d["preparation"] = json!("failed");
                d["targetReadiness"] = json!("failed");
                d["error"] = json!({"code":if session.cancel.load(Ordering::Acquire){"cancelled"}else{"prepareFailed"},"message":error,"phase":"prepare"});
                bump(&mut d);
                drop(d);
                let _ = session.publish(&client);
            }
        }));
    }
    fn prepare(&self, client: &Client) -> Result<(), String> {
        let preferences = client
            .call("settings.get", json!({"pageId":"appearance"}))
            .unwrap_or(Value::Null);
        self.check_cancel()?;
        let mode = preferences["values"]["directoryMode"]
            .as_str()
            .unwrap_or("auto");
        let (launch, integration) = if mode == "auto" {
            match integration::prepare(client, &self.view, &self.context, &self.cancel) {
                Ok((command, state)) => (json!({"kind":"command","command":command}), state),
                Err(reason) => {
                    self.check_cancel()?;
                    (
                        json!({"kind":"loginShell"}),
                        json!({"mode":"unavailable","reasonCode":reason}),
                    )
                }
            }
        } else {
            (json!({"kind":"loginShell"}), json!({"mode":mode}))
        };
        self.check_cancel()?;
        let runtime=client.call("terminal.session.open",json!({"viewId":self.view,"expectedContext":self.context,"launch":launch,"cols":80,"rows":24}))?;
        {
            let mut d = self.descriptor.lock().unwrap();
            d["ptyId"] = runtime["id"].clone();
            d["integration"] = integration.clone();
            d["capabilities"]["location"] = json!({"supported":mode=="manual"||integration["mode"]=="auto","reason":if mode=="off"{"终端目录识别已关闭。"}else{"当前远端 Shell 无法提供自动目录识别，可在终端设置中选择兼容模式。"}});
            d["details"] = integration["shell"]
                .as_str()
                .map_or(json!([]), |shell| json!([shell]));
            d["preparation"] = json!("prepared");
            d["targetReadiness"] = json!("ready");
            bump(&mut d);
        }
        self.check_cancel()?;
        // Publication is recoverable through terminal.status; it does not undo a live PTY.
        let _ = self.publish(client);
        if mode == "off" || integration["mode"] == "unavailable" {
            let _ = self.publish_unknown_location(
                client,
                &runtime["id"],
                if mode == "off" {
                    "disabled"
                } else {
                    integration["reasonCode"]
                        .as_str()
                        .unwrap_or("prepare-failed")
                },
            );
        }
        Ok(())
    }
    fn check_cancel(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Acquire) {
            Err("终端准备已取消".into())
        } else {
            Ok(())
        }
    }
    fn publish(&self, client: &Client) -> Result<(), String> {
        let _publishing = self.publishing.lock().unwrap();
        let value = self.descriptor.lock().unwrap().clone();
        let record = shared_state::request(
            client,
            &self.view,
            &self.context,
            "get",
            json!({"topic":"terminal.session.v1","key":"current"}),
        )
        .map_err(|e| e.message)?;
        shared_state::request(client,&self.view,&self.context,"put",json!({"topic":"terminal.session.v1","key":"current","expectedRevision":record["record"]["revision"],"value":value})).map_err(|e|e.message)?;
        Ok(())
    }
    fn publish_unknown_location(
        &self,
        client: &Client,
        id: &Value,
        reason: &str,
    ) -> Result<(), String> {
        let record = shared_state::request(
            client,
            &self.view,
            &self.context,
            "get",
            json!({"topic":"terminal.location.v1","key":"current"}),
        )
        .map_err(|e| e.message)?;
        shared_state::request(client,&self.view,&self.context,"put",json!({"topic":"terminal.location.v1","key":"current","expectedRevision":record["record"]["revision"],"value":{"state":"unknown","path":null,"activity":"unavailable","reasonCode":reason,"terminalId":id,"outputOffset":"0"}})).map_err(|e|e.message)?;
        Ok(())
    }
    pub fn data(&self, client: &Client) -> Value {
        let descriptor = self.descriptor.lock().unwrap().clone();
        let runtime = descriptor["ptyId"]
            .as_str()
            .and_then(|id| {
                client
                    .call("terminal.session.status", json!({"id":id}))
                    .ok()
            })
            .unwrap_or(Value::Null);
        json!({"session":descriptor,"runtime":runtime,"streamId":descriptor["ptyId"],"title":descriptor["title"],"openedContext":self.context,"integration":descriptor["integration"]})
    }
    pub fn title(&self) -> Value {
        self.descriptor.lock().unwrap()["title"].clone()
    }
    pub fn close(&self, client: &Client) -> Result<(), String> {
        let _closing = self.closing.lock().unwrap();
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.lock().unwrap().take() {
            worker.join().map_err(|_| "终端准备线程异常退出")?;
        }
        let id = self.descriptor.lock().unwrap()["ptyId"].clone();
        if id.is_null() {
            return Ok(());
        }
        let mut state = client.call("terminal.session.close", json!({"id":id}))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while state["state"] != "closed" {
            if Instant::now() >= deadline {
                return Err("终端仍在清理，请等待原关闭任务完成".into());
            }
            thread::sleep(Duration::from_millis(20));
            state = client.call("terminal.session.status", json!({"id":id}))?;
        }
        Ok(())
    }
}
fn bump(value: &mut Value) {
    let next = value["revision"].as_str().unwrap().parse::<u64>().unwrap() + 1;
    value["revision"] = json!(next.to_string());
}
