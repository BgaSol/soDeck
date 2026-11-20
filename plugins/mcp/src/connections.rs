use crate::{
    app::App,
    targets::{Context, Descriptor, Observation},
    tasks::{failure, text, WaitSlots},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tokio::sync::{watch, Notify, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub server_id: String,
    pub expected_profile_token: String,
    pub request_id: String,
}

#[derive(Clone, PartialEq)]
struct Ready {
    id: String,
    context: Context,
    descriptor: Descriptor,
    capabilities: Vec<String>,
}
#[derive(Clone, PartialEq)]
enum State {
    Admitting,
    Connecting {
        stage: String,
        interaction: Option<Value>,
    },
    Preparing(Context),
    Cancelling,
    Ready(Ready),
    Failed(Value),
    Cancelled,
}
impl State {
    fn terminal(&self) -> bool {
        matches!(self, Self::Ready(_) | Self::Failed(_) | Self::Cancelled)
    }
}
struct Record {
    revision: u64,
    state: State,
    last_error: Option<Value>,
}
pub struct Request {
    id: String,
    input: Input,
    record: Mutex<Record>,
    changes: watch::Sender<u64>,
    refreshed: watch::Sender<u64>,
    control: Notify,
    cancel: CancellationToken,
    retry: AtomicBool,
    waiters: Arc<Semaphore>,
}
#[derive(Default)]
struct Registry {
    by_request: HashMap<String, Arc<Request>>,
    by_id: HashMap<String, Arc<Request>>,
}
pub struct Opens {
    registry: Mutex<Registry>,
    active: Arc<Semaphore>,
}
impl Default for Opens {
    fn default() -> Self {
        Self {
            registry: Mutex::new(Registry::default()),
            active: Arc::new(Semaphore::new(32)),
        }
    }
}

impl Request {
    fn snapshot(&self) -> (u64, State) {
        let r = self.record.lock().unwrap();
        (r.revision, r.state.clone())
    }
    fn update(&self, mut state: State) {
        let mut r = self.record.lock().unwrap();
        if r.state.terminal()
            || matches!(r.state, State::Cancelling)
                && !matches!(state, State::Cancelled | State::Cancelling)
        {
            return;
        }
        if let (State::Preparing(context), State::Failed(error)) = (&r.state, &mut state) {
            if error.is_object() {
                error["context"] = json!(context);
            }
        }
        if r.state != state {
            if state.terminal() {
                r.last_error = None;
            }
            r.state = state;
            r.revision += 1;
            self.changes.send_replace(r.revision);
        }
    }
    fn cancelling(&self) -> bool {
        matches!(self.record.lock().unwrap().state, State::Cancelling)
    }
    fn observed_error(&self, error: Option<Value>) {
        let mut r = self.record.lock().unwrap();
        if !r.state.terminal() && r.last_error != error {
            r.last_error = error;
            r.revision += 1;
            self.changes.send_replace(r.revision);
        }
    }
    fn begin_cancel(&self) {
        self.update(State::Cancelling);
        if self.cancelling() {
            self.cancel.cancel();
            self.control.notify_one();
        }
    }
    async fn refresh(&self, app: &App, ct: &CancellationToken) -> Result<(), Value> {
        let mut refreshed = self.refreshed.subscribe();
        if self.snapshot().1.terminal() {
            return Ok(());
        }
        self.control.notify_one();
        tokio::select! {r=refreshed.changed()=>r.map_err(|_|failure("unavailable","连接观察已结束")),_=ct.cancelled()=>Err(failure("requestCancelled","本次读取已取消，原请求继续")),_=app.shutdown.cancelled()=>Err(failure("unavailable","插件正在退出"))}
    }
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum Receipt {
    #[serde(rename_all = "camelCase")]
    Attached {
        attempt_id: String,
    },
    Accepted {
        context: Context,
    },
    Failed {
        error: Value,
    },
    Cancelled,
}
#[derive(Deserialize)]
struct Native {
    receipt: Receipt,
    view: Option<Value>,
    current: Option<Value>,
    background: Option<Value>,
}

fn unknown(e: &Value) -> bool {
    e["code"] == -32003
}
fn business(e: Value) -> Value {
    if e["details"]["code"].is_string() {
        json!({"code":e["details"]["code"],"message":e["message"]})
    } else {
        e
    }
}

async fn rpc(
    app: Arc<App>,
    method: &'static str,
    p: Value,
    permit: Option<OwnedSemaphorePermit>,
) -> Result<Value, Value> {
    let permit = match permit {
        Some(p) => p,
        None => {
            tokio::select! {p=app.target_reads.clone().acquire_owned()=>p.map_err(|_|failure("unavailable","RPC 读取已关闭"))?,_=app.shutdown.cancelled()=>return Err(failure("unavailable","插件正在退出"))}
        }
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        app.rpc(method, p)
    })
    .await
    .map_err(|e| failure("unavailable", e.to_string()))?
}

fn advance(app: &App, request: &Request, value: Value) -> Result<(), Value> {
    let native: Native =
        serde_json::from_value(value).map_err(|e| failure("invalidResult", e.to_string()))?;
    match native.receipt {
        Receipt::Attached { attempt_id } => {
            if attempt_id.is_empty() {
                return Err(failure("invalidResult", "缺少原尝试身份"));
            }
            let view = native
                .view
                .ok_or_else(|| failure("invalidResult", "原尝试信息缺失"))?;
            let interaction = if view["question"].is_object() {
                let q = &view["question"];
                Some(
                    json!({"kind":q["kind"],"name":q["name"],"nodeIndex":q["nodeIndex"],"endpoint":q["endpoint"],"message":q["message"],"windowId":view["windowId"]}),
                )
            } else {
                None
            };
            request.update(State::Connecting {
                stage: view["stage"].as_str().unwrap_or("connecting").into(),
                interaction,
            });
        }
        Receipt::Accepted { context } => {
            if let State::Preparing(old) = request.snapshot().1 {
                if old != context {
                    return Err(failure("staleContext", "原请求被错误地绑定到新连接"));
                }
            }
            request.update(State::Preparing(context.clone()));
            let Some(current) = native.current else {
                return Err(failure("invalidResult", "接纳会话事实缺失"));
            };
            if current["error"].is_object() {
                return Err(current["error"].clone());
            }
            let observation: Observation = serde_json::from_value(current)
                .map_err(|e| failure("invalidResult", e.to_string()))?;
            if !observation.validate() || observation.context != context {
                return Err(failure("staleContext", "会话事实不是原连接"));
            }
            if observation.availability() != "ready" {
                return Err(failure("staleContext", "原会话已断开或正在关闭"));
            }
            let Some(background) = native.background else {
                return Ok(());
            };
            if background["state"] == "failed" {
                return Err(
                    json!({"code":"targetInitializationFailed","message":background["error"],"context":context}),
                );
            }
            if background["state"] == "stale" {
                return Err(failure("staleContext", "原后台视图已更换"));
            }
            if background["state"] != "created" {
                return Ok(());
            }
            let Some(view) = background["viewId"].as_str() else {
                return Err(failure("invalidResult", "后台视图身份缺失"));
            };
            // Registration and terminal publication share this lock order.
            let views = app.views.lock().unwrap();
            if let Some(target) = views
                .get(view)
                .filter(|t| t.context == context && !t.closed.load(Ordering::Acquire))
            {
                let descriptor = observation
                    .target
                    .ok_or_else(|| failure("invalidResult", "真实连接摘要缺失"))?;
                request.update(State::Ready(Ready {
                    id: target.id.clone(),
                    context,
                    descriptor,
                    capabilities: observation.capabilities,
                }));
            }
        }
        Receipt::Failed { error } => request.update(State::Failed(error)),
        Receipt::Cancelled => request.update(State::Cancelled),
    }
    Ok(())
}

async fn drive(
    app: Arc<App>,
    request: Arc<Request>,
    _active: OwnedSemaphorePermit,
    first: OwnedSemaphorePermit,
) {
    let mut core = app.core_changes.subscribe();
    let mut targets = app.target_changes.subscribe();
    let mut first = Some(first);
    let mut initial = true;
    loop {
        if app.shutdown.is_cancelled() {
            break;
        }
        let cancel = request.cancelling();
        let retry = request.retry.swap(false, Ordering::AcqRel)
            && matches!(request.snapshot().1, State::Admitting);
        let (method, p) = if cancel {
            (
                "connections.cancel",
                json!({"requestId":request.input.request_id}),
            )
        } else if initial || retry {
            (
                "connections.open",
                json!({"requestId":request.input.request_id,"serverId":request.input.server_id,"expectedProfileToken":request.input.expected_profile_token}),
            )
        } else {
            (
                "connections.get",
                json!({"requestId":request.input.request_id,"viewType":"status"}),
            )
        };
        initial = false;
        match rpc(app.clone(), method, p, first.take()).await {
            Ok(value) => {
                request.observed_error(None);
                if method == "connections.cancel" {
                    match serde_json::from_value::<Native>(value) {
                        Ok(_) => request.update(State::Cancelled),
                        Err(e) => {
                            request.observed_error(Some(failure("invalidResult", e.to_string())))
                        }
                    }
                } else if !request.cancelling() {
                    if let Err(error) = advance(&app, &request, value) {
                        request.update(State::Failed(error));
                    }
                }
            }
            Err(error) => {
                if unknown(&error) || error["details"]["code"] == "connectionRequestNotFound" {
                    request.observed_error(Some(error.clone()));
                    if cancel {
                        request.update(State::Cancelling);
                    } else if matches!(request.snapshot().1, State::Admitting) {
                        request.update(State::Admitting);
                    }
                    // Confirmed receipt/target facts are retained during a communication failure.
                } else {
                    request.update(State::Failed(business(error)));
                }
            }
        }
        request.refreshed.send_modify(|n| *n += 1);
        if request.snapshot().1.terminal() {
            break;
        }
        if method == "connections.open" && matches!(request.snapshot().1, State::Preparing(_)) {
            request.control.notify_one();
        }
        tokio::select! {
            _=app.shutdown.cancelled()=>break,
            _=request.control.notified()=>{},
            _=core.changed()=>{},
            _=targets.changed()=>{},
            _=request.cancel.cancelled(),if !request.cancelling()=>{},
        }
    }
    request.refreshed.send_modify(|n| *n += 1);
}

pub struct Started {
    request: Arc<Request>,
    slots: Option<WaitSlots>,
    wait: u64,
    started: Instant,
}
pub fn start(app: &Arc<App>, p: &Value, ct: &CancellationToken) -> Result<Started, Value> {
    let mut input = p.clone();
    let wait = input
        .as_object_mut()
        .ok_or_else(|| failure("invalidParams", "参数必须为对象"))?
        .remove("waitMs")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| failure("invalidParams", "waitMs 必须为整数"))
        })
        .transpose()?
        .unwrap_or(1000);
    if wait > 20000
        || p.as_object()
            .is_some_and(|o| o.values().any(Value::is_null))
    {
        return Err(failure("invalidParams", "连接等待参数无效"));
    }
    let input: Input =
        serde_json::from_value(input).map_err(|e| failure("invalidParams", e.to_string()))?;
    for s in [
        &input.server_id,
        &input.expected_profile_token,
        &input.request_id,
    ] {
        if s.is_empty() || s.len() > 256 || s.contains('\0') {
            return Err(failure("invalidParams", "连接身份无效"));
        }
    }
    if input.request_id.len() > 128 {
        return Err(failure("invalidParams", "requestId 超限"));
    }
    let mut registry = app.opens.registry.lock().unwrap();
    if let Some(request) = registry.by_request.get(&input.request_id) {
        if request.input != input {
            return Err(failure("requestConflict", "同一 requestId 参数不同"));
        }
        let request = request.clone();
        drop(registry);
        let slots = slots(app, &request, wait)?;
        request.retry.store(true, Ordering::Release);
        request.control.notify_one();
        return Ok(Started {
            request,
            slots,
            wait,
            started: Instant::now(),
        });
    }
    if ct.is_cancelled() || app.shutdown.is_cancelled() {
        return Err(failure("requestCancelled", "未接纳新的打开请求"));
    }
    if registry.by_id.len() >= 4096 {
        return Err(failure("resourceBusy", "打开请求记录已满"));
    }
    let active = app
        .opens
        .active
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "未完成打开请求已满"))?;
    let first = app
        .target_reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "核心读取位置已满"))?;
    let number = app.numbers.fetch_add(1, Ordering::Relaxed);
    let request = Arc::new(Request {
        id: format!("open-{}-{number}", app.run),
        input,
        record: Mutex::new(Record {
            revision: 1,
            state: State::Admitting,
            last_error: None,
        }),
        changes: watch::channel(1).0,
        refreshed: watch::channel(0).0,
        control: Notify::new(),
        cancel: CancellationToken::new(),
        retry: AtomicBool::new(false),
        waiters: Arc::new(Semaphore::new(2)),
    });
    let slots = slots(app, &request, wait)?;
    if ct.is_cancelled() {
        return Err(failure("requestCancelled", "未接纳新的打开请求"));
    }
    registry
        .by_request
        .insert(request.input.request_id.clone(), request.clone());
    registry.by_id.insert(request.id.clone(), request.clone());
    drop(registry);
    app.runtime
        .spawn(drive(app.clone(), request.clone(), active, first));
    Ok(Started {
        request,
        slots,
        wait,
        started: Instant::now(),
    })
}
fn resolve(app: &App, p: &Value) -> Result<Arc<Request>, Value> {
    let id = text(p, "connectionRequestId", 256)?;
    app.opens
        .registry
        .lock()
        .unwrap()
        .by_id
        .get(id)
        .cloned()
        .ok_or_else(|| failure("connectionRequestNotFound", "回执不属于本次运行"))
}
fn slots(app: &App, request: &Request, wait: u64) -> Result<Option<WaitSlots>, Value> {
    if wait == 0 || request.snapshot().1.terminal() {
        return Ok(None);
    }
    let global=app.waiting.clone().try_acquire_owned().map_err(|_|json!({"code":"resourceBusy","message":"等待位置已满","connectionRequestId":request.id}))?;
    let local=request.waiters.clone().try_acquire_owned().map_err(|_|json!({"code":"resourceBusy","message":"原请求等待位置已满","connectionRequestId":request.id}))?;
    Ok(Some(WaitSlots {
        _global: global,
        _local: local,
    }))
}

fn response(app: &App, request: &Request, detail: bool) -> Value {
    let (revision, state, last_error) = {
        let r = request.record.lock().unwrap();
        (r.revision, r.state.clone(), r.last_error.clone())
    };
    let mut target = Value::Null;
    let (status, stage, interaction, error) = match state {
        State::Admitting => ("pending", "admitting".to_string(), None, None),
        State::Connecting { stage, interaction } => (
            if interaction.is_some() {
                "needsUser"
            } else {
                "pending"
            },
            stage,
            interaction,
            None,
        ),
        State::Preparing(_) => ("pending", "preparingTarget".into(), None, None),
        State::Cancelling => ("pending", "cancelling".into(), None, None),
        State::Ready(ready) => {
            target = json!({"targetId":ready.id,"serverId":ready.context.server_id,"workspaceId":ready.context.workspace_id,"target":ready.descriptor,"targetSource":"current","availability":"ready","capabilities":ready.capabilities});
            if detail {
                target["context"] = json!(ready.context);
            }
            ("ready", "ready".into(), None, None)
        }
        State::Failed(error) => ("failed", "failed".into(), None, Some(error)),
        State::Cancelled => ("cancelled", "cancelled".into(), None, None),
    };
    let mut connection = json!({"connectionRequestId":request.id,"requestId":request.input.request_id,"serverId":request.input.server_id,"state":status,"stage":stage,"revision":revision.to_string(),"interaction":interaction,"error":error.or(last_error).map(|e|if detail{e}else{crate::response::brief_error(&e)})});
    if detail {
        connection["input"] = json!({"serverId":request.input.server_id,"expectedProfileToken":request.input.expected_profile_token,"requestId":request.input.request_id});
    }
    json!({"runId":app.run,"connection":connection,"target":target})
}

async fn read(
    app: Arc<App>,
    mut start: Started,
    since: Option<u64>,
    until_ready: bool,
    detail: bool,
    ct: CancellationToken,
) -> Result<Value, Value> {
    let request = &start.request;
    let mut changes = request.changes.subscribe();
    let baseline = since.unwrap_or(request.snapshot().0);
    if baseline > request.snapshot().0 {
        return Err(failure("invalidParams", "sinceRevision 超过当前版本"));
    }
    if !until_ready {
        request.refresh(&app, &ct).await?;
    }
    let deadline = tokio::time::Instant::from_std(
        start.started + std::time::Duration::from_millis(start.wait),
    );
    let reason = loop {
        if ct.is_cancelled() {
            return Err(
                json!({"code":"requestCancelled","message":"本次读取已取消","connectionRequestId":request.id}),
            );
        }
        let (revision, state) = request.snapshot();
        if start.wait == 0 {
            break "";
        }
        if state.terminal() {
            break match state {
                State::Ready(_) => "ready",
                State::Cancelled => "cancelled",
                _ => "failed",
            };
        }
        if until_ready
            && matches!(
                state,
                State::Connecting {
                    interaction: Some(_),
                    ..
                }
            )
        {
            break "needsUser";
        }
        if !until_ready && revision > baseline {
            break "changed";
        }
        if tokio::time::Instant::now() >= deadline {
            request.refresh(&app, &ct).await?;
            break match request.snapshot().1 {
                State::Ready(_) => "ready",
                State::Failed(_) => "failed",
                State::Cancelled => "cancelled",
                _ => "timeout",
            };
        }
        if start.slots.is_none() {
            start.slots = slots(&app, request, start.wait)?;
        }
        tokio::select! {_=ct.cancelled()=>{},_=app.shutdown.cancelled()=>return Err(failure("unavailable","插件正在退出")),_=changes.changed()=>{},_=tokio::time::sleep_until(deadline)=>{}}
    };
    drop(start.slots.take());
    let mut value = response(&app, request, detail);
    if !reason.is_empty() {
        value["wait"] = json!({"reason":reason,"elapsedMs":start.started.elapsed().as_millis()});
    }
    Ok(value)
}
pub async fn after_start(
    app: Arc<App>,
    start: Started,
    ct: CancellationToken,
) -> Result<Value, Value> {
    read(app, start, None, true, false, ct).await
}
pub async fn get(app: Arc<App>, p: &Value, ct: CancellationToken) -> Result<Value, Value> {
    let request = resolve(&app, p)?;
    let wait = p
        .get("waitMs")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| failure("invalidParams", "等待必须为整数"))
        })
        .transpose()?
        .unwrap_or(0);
    let since = p
        .get("sinceRevision")
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| failure("invalidParams", "revision 必须是十进制字符串"))
        })
        .transpose()?;
    let detail = p
        .get("detail")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| failure("invalidParams", "detail 必须为布尔值"))
        })
        .transpose()?
        .unwrap_or(false);
    if wait > 20000 {
        return Err(failure("invalidParams", "等待超限"));
    }
    read(
        app,
        Started {
            request,
            slots: None,
            wait,
            started: Instant::now(),
        },
        since,
        false,
        detail,
        ct,
    )
    .await
}
pub async fn cancel(app: Arc<App>, p: &Value, ct: CancellationToken) -> Result<Value, Value> {
    let request = resolve(&app, p)?;
    let finished = request.snapshot().1.terminal();
    if !finished {
        request.begin_cancel();
        request.refresh(&app, &ct).await?;
    }
    let mut value = response(&app, &request, false);
    value["cancelResult"] = json!(if finished
        || matches!(
            value["connection"]["state"].as_str(),
            Some("ready" | "failed")
        ) {
        "alreadyFinished"
    } else if value["connection"]["state"] == "cancelled" {
        "cancelled"
    } else {
        "pending"
    });
    Ok(value)
}
