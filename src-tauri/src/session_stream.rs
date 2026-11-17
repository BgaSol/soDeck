//! Non-interactive owned command streams. Plugins interpret commands and output.
use crate::{
    resource_owner::Owner,
    resource_tasks::Tasks,
    streams::{capture::Capture, Frame, Sink},
    workspaces::Context,
    AppCore,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Open {
    view_id: String,
    expected_context: Context,
    operation_id: String,
    sequence: String,
    command: String,
    max_output_bytes: usize,
    retention: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}
struct Stream {
    owner: Owner,
    request: Open,
    capture: Mutex<Capture>,
    status: Mutex<Value>,
    cancel: CancellationToken,
    worker_done: CancellationToken,
    tasks: Arc<Tasks>,
    changed: Notify,
    readers: Mutex<HashMap<String, Arc<Reader>>>,
    released: Mutex<bool>,
}
struct Reader {
    ack: Mutex<(u64, u64)>,
    changed: Notify,
    cancel: CancellationToken,
}
#[derive(Default)]
struct Registry {
    streams: HashMap<String, Arc<Stream>>,
    sequences: HashMap<String, (Owner, u64)>,
}
#[derive(Default)]
pub(crate) struct Streams(Mutex<Registry>);
fn key(owner: &Owner) -> String {
    format!(
        "{}/{}/{}/{}",
        owner.instance, owner.view, owner.context.connection_id, owner.context.generation
    )
}
fn integer(p: &Value, name: &str) -> Result<u64, String> {
    p[name]
        .as_str()
        .ok_or_else(|| format!("缺少{name}"))?
        .parse()
        .map_err(|_| format!("{name}无效"))
}
impl Streams {
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut r = self.0.lock().unwrap();
        r.sequences.retain(|_, (owner, _)| !matches(owner));
        r.streams.retain(|_, s| {
            if matches(&s.owner) {
                *s.released.lock().unwrap() = true;
                s.cancel.cancel();
                for reader in s.readers.lock().unwrap().values() {
                    reader.cancel.cancel();
                }
                s.tasks.close();
            }
            !*s.released.lock().unwrap() || !s.tasks.finished()
        });
    }
    pub(crate) fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        let mut r = self.0.lock().unwrap();
        r.streams
            .retain(|_, s| !*s.released.lock().unwrap() || !s.tasks.finished());
        r.streams
            .values()
            .filter(|s| matches(&s.owner) && s.tasks.active() > 0)
            .count()
    }
}
impl AppCore {
    pub(crate) fn stream_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, String> {
        if method == "session.stream.lookup" {
            let request: Open = serde_json::from_value(p.clone()).map_err(|e| e.to_string())?;
            let ui = self.ui_state();
            let view = ui.resource_view(&request.view_id)?;
            if view.plugin_id != plugin || view.plugin_instance_id != instance {
                return Err("票据不属于此实例".into());
            }
            let registry = self.command_streams.0.lock().unwrap();
            for (id, s) in &registry.streams {
                if s.owner.instance == instance
                    && s.request.view_id == request.view_id
                    && s.request.expected_context == request.expected_context
                    && s.request.operation_id == request.operation_id
                {
                    if s.request != request {
                        return Err("operationConflict: 同一票据参数不同".into());
                    }
                    return Ok(json!({"known":true,"streamId":id,"status":status(s)}));
                }
            }
            return Ok(json!({"known":false}));
        }
        if method == "session.stream.open" {
            let request: Open = serde_json::from_value(p.clone()).map_err(|e| e.to_string())?;
            let sequence = request.sequence.parse::<u64>().map_err(|_| "序号无效")?;
            if request.operation_id.is_empty()
                || request.operation_id.len() > 128
                || request.command.is_empty()
                || request.command.len() > 16 * 1024
                || request.command.contains('\0')
                || !matches!(request.retention.as_str(), "fixed" | "rolling")
                || !(1024..=64 * 1024 * 1024).contains(&request.max_output_bytes)
                || request
                    .timeout_ms
                    .is_some_and(|n| !(1000..=3_600_000).contains(&n))
            {
                return Err("流式命令参数无效".into());
            }
            let owner = self.resource_owner(plugin, instance, &request.view_id)?;
            if owner.context != request.expected_context {
                return Err("连接代次已改变".into());
            }
            let session = self.connection_session(&owner.context)?;
            let admission = self.resource_admission(&owner)?;
            let mut registry = self.command_streams.0.lock().unwrap();
            registry
                .streams
                .retain(|_, s| !*s.released.lock().unwrap() || !s.tasks.finished());
            let own_key = key(&owner);
            for (id, s) in &registry.streams {
                if key(&s.owner) == own_key && s.request.operation_id == request.operation_id {
                    if s.request != request {
                        return Err("operationConflict: 同一票据参数不同".into());
                    }
                    return Ok(json!({"streamId":id,"state":s.status.lock().unwrap()["state"]}));
                }
            }
            if registry
                .sequences
                .get(&own_key)
                .is_some_and(|(_, n)| sequence <= *n)
            {
                return Err("operationExpired: 旧流式命令票据".into());
            }
            let live = |s: &&Arc<Stream>| s.status.lock().unwrap()["finished"] != true;
            if registry.streams.values().filter(live).count() >= 24
                || registry
                    .streams
                    .values()
                    .filter(|s| s.owner.instance == instance)
                    .filter(live)
                    .count()
                    >= 12
                || registry
                    .streams
                    .values()
                    .filter(|s| s.owner.view == owner.view)
                    .filter(live)
                    .count()
                    >= 3
                || registry
                    .streams
                    .values()
                    .filter(|s| s.owner.instance == instance)
                    .count()
                    >= 64
                || registry.sequences.len() >= 4096 && !registry.sequences.contains_key(&own_key)
            {
                return Err("resourceBusy: 流式命令资源已满".into());
            }
            for retention in ["fixed", "rolling"] {
                let sum: usize = registry
                    .streams
                    .values()
                    .filter(|s| s.owner.view == owner.view)
                    .map(|s| {
                        if s.request.retention == retention {
                            s.request.max_output_bytes
                        } else if retention == "fixed" {
                            s.capture.lock().unwrap().pinned_bytes() as usize
                        } else {
                            0
                        }
                    })
                    .sum();
                if request.retention == retention
                    && sum + request.max_output_bytes > 64 * 1024 * 1024
                {
                    return Err("当前页面流式捕获额度已满".into());
                }
            }
            let id = self.next_id();
            let path = self
                .store
                .lock()
                .unwrap()
                .root
                .join("command-output")
                .join(format!(
                    "{}-{}-{}",
                    std::process::id(),
                    crate::core::now(),
                    id
                ));
            let capture = Capture::new(
                path,
                request.max_output_bytes,
                request.retention == "rolling",
            )?;
            let s = Arc::new(Stream {
                owner,
                request,
                capture: Mutex::new(capture),
                status: Mutex::new(
                    json!({"state":"opening","execSent":false,"remoteExitConfirmed":false,"finished":false}),
                ),
                cancel: CancellationToken::new(),
                worker_done: CancellationToken::new(),
                tasks: Arc::new(Tasks::default()),
                changed: Notify::new(),
                readers: Mutex::new(HashMap::new()),
                released: Mutex::new(false),
            });
            let task = s.tasks.enter()?;
            registry
                .sequences
                .insert(own_key, (s.owner.clone(), sequence));
            registry.streams.insert(id.clone(), s.clone());
            drop(registry);
            drop(admission);
            let core = Arc::downgrade(self);
            tauri::async_runtime::spawn(async move {
                run(core, s.clone(), session).await;
                drop(task);
                s.worker_done.cancel();
                s.changed.notify_waiters();
            });
            return Ok(json!({"streamId":id,"state":"opening"}));
        }
        let id = p["streamId"].as_str().ok_or("缺少streamId")?;
        let s = self
            .command_streams
            .0
            .lock()
            .unwrap()
            .streams
            .get(id)
            .cloned()
            .ok_or("流式命令已释放")?;
        if s.owner.plugin != plugin || s.owner.instance != instance {
            return Err("资源不属于此实例".into());
        }
        match method {
            "session.stream.status" => Ok(status(&s)),
            "session.stream.poll" => {
                let offset = integer(p, "offset")?;
                let capture = s.capture.lock().unwrap();
                let watermark = capture.produced;
                let (chunks, next) = capture.read_blocks(offset, 48 * 1024, watermark)?;
                let earliest = capture.earliest;
                drop(capture);
                Ok(
                    json!({"chunks":chunks,"nextOffset":next.to_string(),"watermark":watermark.to_string(),"earliestAvailableOffset":earliest.to_string(),"status":status(&s)}),
                )
            }
            "session.stream.close" | "session.stream.release" => {
                s.cancel.cancel();
                s.changed.notify_waiters();
                if method.ends_with("release") {
                    *s.released.lock().unwrap() = true;
                    s.tasks.close();
                    for r in s.readers.lock().unwrap().values() {
                        r.cancel.cancel()
                    }
                }
                Ok(
                    json!({"accepted":true,"released":method.ends_with("release"),"state":s.status.lock().unwrap()["state"]}),
                )
            }
            _ => Err("未知流式命令方法".into()),
        }
    }
    pub async fn session_stream_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
        sink: Option<Sink>,
    ) -> Result<Value, String> {
        let view = self.ui_data_view(window, token)?;
        let id = p["id"].as_str().ok_or("缺少id")?.to_string();
        let s = self
            .command_streams
            .0
            .lock()
            .unwrap()
            .streams
            .get(&id)
            .cloned()
            .ok_or("流式命令已关闭")?;
        if s.owner.view != view.id
            || s.owner.plugin != view.plugin_id
            || s.owner.instance != view.plugin_instance_id
            || *s.released.lock().unwrap()
        {
            return Err("数据流不属于此页面".into());
        }
        match p["action"].as_str().unwrap_or("") {
            "read" => {
                let start = integer(&p, "offset")?;
                let end = integer(&p, "watermark")?;
                let s = s.clone();
                let data = tauri::async_runtime::spawn_blocking(move || {
                    s.capture.lock().unwrap().read(start, 16 * 1024, end)
                })
                .await
                .map_err(|e| e.to_string())??;
                Ok(json!({"data":data,"nextOffset":(start+data.len() as u64).to_string()}))
            }
            "pin" => {
                let _admission = self.surface_write_admission(window, token)?;
                let registry = self.command_streams.0.lock().unwrap();
                let bytes: u64 = registry
                    .streams
                    .values()
                    .filter(|other| other.owner.view == view.id)
                    .map(|other| {
                        if other.request.retention == "fixed" {
                            other.request.max_output_bytes as u64
                        } else {
                            other.capture.lock().unwrap().pinned_bytes()
                        }
                    })
                    .sum();
                let start = integer(&p, "start")?;
                let end = integer(&p, "end")?;
                let additional = if s.request.retention == "rolling" {
                    end.saturating_sub(start)
                } else {
                    0
                };
                if end <= start || bytes.saturating_add(additional) > 64 * 1024 * 1024 {
                    return Err("当前页面固定阅读窗口达到64MiB容量".into());
                }
                s.capture.lock().unwrap().pin(self.next_id(), start, end)
            }
            "unpin" => {
                let _admission = self.surface_write_admission(window, token)?;
                s.capture
                    .lock()
                    .unwrap()
                    .unpin(p["pinId"].as_str().ok_or("缺少pinId")?);
                Ok(json!({}))
            }
            "ack" => {
                let reader = s
                    .readers
                    .lock()
                    .unwrap()
                    .get(token)
                    .cloned()
                    .ok_or("数据订阅已失效")?;
                let packet = p["packet"].as_u64().ok_or("缺少packet")?;
                let mut ack = reader.ack.lock().unwrap();
                if packet == ack.1 {
                    return Ok(json!({}));
                }
                if packet != ack.0 || packet != ack.1 + 1 {
                    return Err("数据确认序号无效".into());
                }
                ack.1 = packet;
                reader.changed.notify_one();
                Ok(json!({}))
            }
            "detach" => {
                if let Some(r) = s.readers.lock().unwrap().remove(token) {
                    r.cancel.cancel()
                }
                Ok(json!({}))
            }
            "attach" => {
                let sink = sink.ok_or("缺少正文通道")?;
                let reader = Arc::new(Reader {
                    ack: Mutex::new((0, 0)),
                    changed: Notify::new(),
                    cancel: CancellationToken::new(),
                });
                let offset = p["offset"]
                    .as_str()
                    .map(str::parse::<u64>)
                    .transpose()
                    .map_err(|_| "偏移无效")?
                    .unwrap_or(s.capture.lock().unwrap().earliest);
                if !s.capture.lock().unwrap().boundary(offset) {
                    return Err("偏移不在完整输出块边界".into());
                }
                let mut readers = s.readers.lock().unwrap();
                if readers.contains_key(token) {
                    return Ok(json!({}));
                }
                let task = s.tasks.enter()?;
                readers.insert(token.into(), reader.clone());
                drop(readers);
                let core = Arc::downgrade(self);
                let window = window.to_owned();
                let token = token.to_owned();
                tauri::async_runtime::spawn(async move {
                    let _task = task;
                    subscribe(core, s, id, window, token, reader, sink, offset).await;
                });
                Ok(json!({}))
            }
            _ => Err("未知正文操作".into()),
        }
    }
}
fn status(s: &Stream) -> Value {
    let mut value = s.status.lock().unwrap().clone();
    let c = s.capture.lock().unwrap();
    value["producedBytes"] = json!(c.produced.to_string());
    value["earliestAvailableOffset"] = json!(c.earliest.to_string());
    value["workerFinished"] = json!(s.worker_done.is_cancelled());
    value
}
async fn run(core: Weak<AppCore>, s: Arc<Stream>, session: Arc<crate::connections::Session>) {
    let parent = session.cancellation();
    let mut channel = None;
    let deadline = async {
        match s.request.timeout_ms {
            Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
            None => std::future::pending::<()>().await,
        }
    };
    let result: Result<(), String> = tokio::select! {biased;_=s.cancel.cancelled()=>Err("cancelled".into()),_=parent.cancelled()=>Err("sessionLost".into()),_=deadline=>Err("deadlineExceeded".into()),r=async{
        let opening=async{channel=Some(session.open_channel().await?);s.status.lock().unwrap()["execSent"]=json!(true);let c=channel.as_ref().unwrap();c.exec(true,s.request.command.as_str()).await.map_err(|e|e.to_string())?;c.eof().await.map_err(|e|e.to_string())?;Ok::<_,String>(())};
        tokio::time::timeout(Duration::from_secs(30),opening).await.map_err(|_|"打开流式命令超时")??;
        s.status.lock().unwrap()["state"]=json!("running");s.changed.notify_waiters();
        let mut sequence=0u64;
        while let Some(message)=channel.as_mut().unwrap().wait().await {
            let source=if matches!(&message,russh::ChannelMsg::ExtendedData{..}){"stderr"}else{"stdout"};
            match message {
                russh::ChannelMsg::Data{data}|russh::ChannelMsg::ExtendedData{data,ext:1}=>{
                    if let Some(c)=core.upgrade(){c.record_business(&s.owner,0,data.len() as u64)}
                    for bytes in data.chunks(8192){sequence+=1;let mut envelope=serde_json::to_vec(&json!({"v":1,"sequence":sequence.to_string(),"source":source,"receivedAt":crate::core::now(),"dataBase64":STANDARD.encode(bytes)})).unwrap();envelope.push(b'\n');let output=s.clone();tauri::async_runtime::spawn_blocking(move||output.capture.lock().unwrap().append(&envelope)).await.map_err(|e|e.to_string())??;s.changed.notify_waiters();}
                },
                russh::ChannelMsg::Failure=>return Err("远端拒绝执行命令".into()),
                russh::ChannelMsg::ExitStatus{exit_status}=>{let mut state=s.status.lock().unwrap();state["exitCode"]=json!(exit_status);state["remoteExitConfirmed"]=json!(true);},
                russh::ChannelMsg::ExitSignal{signal_name,..}=>{let mut state=s.status.lock().unwrap();state["exitSignal"]=json!(format!("{signal_name:?}"));state["remoteExitConfirmed"]=json!(true);},_=>{}
            }
        }
        if s.status.lock().unwrap()["remoteExitConfirmed"]!=true{return Err("missingExitStatus".into())}Ok(())
    }=>r};
    if let Some(channel) = channel {
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            if result.is_err() {
                let _ = channel.signal(russh::Sig::TERM).await;
            }
            let _ = channel.close().await;
        })
        .await;
    }
    let mut state = s.status.lock().unwrap();
    state["state"] = json!(if result.is_ok() {
        "completed"
    } else if s.cancel.is_cancelled() {
        "cancelled"
    } else {
        "failed"
    });
    state["finished"] = json!(true);
    state["outputComplete"] = json!(result.is_ok());
    if let Err(e) = result {
        let code = match e.split(':').next().unwrap_or("") {
            "cancelled" => "cancelled",
            "sessionLost" => "sessionLost",
            "deadlineExceeded" => "deadlineExceeded",
            "missingExitStatus" => "missingExitStatus",
            "outputLimit" => "outputLimit",
            _ => "commandFailed",
        };
        state["error"] = json!({"code":code,"message":e})
    }
    drop(state);
    s.changed.notify_waiters();
}
async fn subscribe(
    core: Weak<AppCore>,
    s: Arc<Stream>,
    id: String,
    window: String,
    token: String,
    r: Arc<Reader>,
    sink: Sink,
    mut cursor: u64,
) {
    let initial = s.capture.lock().unwrap().produced;
    let mut caught = false;
    let mut first = true;
    'subscription: loop {
        let changed = s.changed.notified();
        if r.cancel.is_cancelled()
            || !core
                .upgrade()
                .is_some_and(|c| c.ui_data_view(&window, &token).is_ok())
        {
            break;
        }
        let (earliest, produced) = {
            let c = s.capture.lock().unwrap();
            (c.earliest, c.produced)
        };
        let finished = s.status.lock().unwrap()["finished"] == true;
        let (kind, data, metadata) = if first {
            first = false;
            (
                "reset",
                vec![],
                json!({"streamId":id,"earliestAvailableOffset":earliest.to_string()}),
            )
        } else if cursor < earliest
            && !s
                .capture
                .lock()
                .unwrap()
                .available(cursor, (cursor + 1).min(produced))
        {
            cursor = earliest;
            (
                "gap",
                vec![],
                json!({"streamId":id,"message":"较早输出已退出最近缓冲"}),
            )
        } else if !caught && cursor >= initial {
            caught = true;
            ("caughtUp", vec![], json!({"streamId":id}))
        } else if cursor < produced {
            let output = s.clone();
            let start = cursor;
            let result = tauri::async_runtime::spawn_blocking(move || {
                output
                    .capture
                    .lock()
                    .unwrap()
                    .read(start, 16 * 1024, produced)
            })
            .await;
            match result {
                Ok(Ok(data)) => {
                    cursor += data.len() as u64;
                    ("data", data, json!({"streamId":id}))
                }
                _ => {
                    cursor = s.capture.lock().unwrap().earliest;
                    (
                        "gap",
                        vec![],
                        json!({"streamId":id,"message":"输出保留范围已变化"}),
                    )
                }
            }
        } else if finished {
            ("end", vec![], json!({"streamId":id,"status":status(&s)}))
        } else {
            tokio::select! {_=changed=>{},_=r.cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(2))=>{}};
            continue;
        };
        let packet = {
            let mut a = r.ack.lock().unwrap();
            a.0 += 1;
            a.0
        };
        if sink(Frame {
            kind,
            packet,
            offset: cursor,
            data,
            metadata,
        })
        .is_err()
        {
            break;
        }
        loop {
            let wake = r.changed.notified();
            if r.ack.lock().unwrap().1 == packet {
                break;
            }
            tokio::select! {_=wake=>{},_=r.cancel.cancelled()=>break 'subscription,_=tokio::time::sleep(Duration::from_secs(2))=>{if !core.upgrade().is_some_and(|c|c.ui_data_view(&window,&token).is_ok()){r.cancel.cancel();break 'subscription}}}
        }
        if kind == "end" {
            break;
        }
    }
    let mut readers = s.readers.lock().unwrap();
    if readers
        .get(&token)
        .is_some_and(|current| Arc::ptr_eq(current, &r))
    {
        readers.remove(&token);
    }
}
