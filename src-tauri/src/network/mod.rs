//! Transport-neutral TCP resources. Proxy protocol and saved rules belong to plugins.
mod relay;
mod runtime;
use crate::connections::io::Stream;
use crate::{connections::Session, resource_owner::Owner, AppCore};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_util::sync::CancellationToken;

pub(crate) enum Command {
    Read(usize),
    Write(Vec<u8>),
    Take(oneshot::Sender<Stream>),
    Finish,
}
struct StreamState {
    state: &'static str,
    error: Option<String>,
    read: Option<Vec<u8>>,
    reading: bool,
    eof: bool,
    budget: usize,
    announced: bool,
    claimed: bool,
}
pub(crate) struct TcpStream {
    id: String,
    client: Option<String>,
    remote: bool,
    state: Mutex<StreamState>,
    commands: mpsc::Sender<Command>,
    cancel: CancellationToken,
    created: Instant,
    target: Mutex<Option<(String, u16)>>,
}
struct Status {
    state: &'static str,
    address: Value,
    error: Option<Value>,
}
pub(crate) struct Listener {
    id: String,
    owner: Owner,
    session: Arc<Session>,
    core: Weak<AppCore>,
    status: Mutex<Status>,
    streams: Mutex<HashMap<String, Arc<TcpStream>>>,
    cancel: CancellationToken,
    sent: AtomicU64,
    tasks: Arc<crate::resource_tasks::Tasks>,
    received: AtomicU64,
    samples: Mutex<VecDeque<(Instant, u64, u64)>>,
    path: Value,
    probes: Mutex<HashMap<std::net::SocketAddr, String>>,
    probe_clients: Mutex<HashMap<String, tokio::net::TcpStream>>,
    probe_operations: Mutex<HashSet<String>>,
}
struct Operation {
    fingerprint: String,
    id: String,
    at: Instant,
}
#[derive(Default)]
struct Registry {
    listeners: HashMap<String, Arc<Listener>>,
    operations: HashMap<(String, String), Operation>,
    scopes: HashMap<String, Scope>,
}
struct Scope {
    owner: Owner,
    last: u64,
    retiring: bool,
    entries: HashMap<u64, Receipt>,
}
struct Receipt {
    operation: String,
    bind: Value,
    listener: Option<String>,
    result: Value,
}
impl Registry {
    fn reap(&mut self) {
        let finished: Vec<_> = self
            .listeners
            .iter()
            .filter(|(_, l)| l.tasks.finished())
            .map(|(id, l)| (id.clone(), l.snapshot()))
            .collect();
        for (id, snapshot) in finished {
            for scope in self.scopes.values_mut() {
                for receipt in scope
                    .entries
                    .values_mut()
                    .filter(|r| r.listener.as_ref() == Some(&id))
                {
                    receipt.result = snapshot.clone();
                }
            }
            self.listeners.remove(&id);
        }
        self.scopes.retain(|_, scope| {
            !scope.retiring
                || scope
                    .entries
                    .values()
                    .any(|r| r.result["cleanupComplete"] != true)
        });
        self.operations.retain(|_, op| {
            self.listeners.contains_key(&op.id) || op.at.elapsed() < Duration::from_secs(60)
        });
    }
    fn observed(&self, scope: &str, sequence: u64, operation: &str) -> Result<Value, String> {
        let Some(scope) = self.scopes.get(scope) else {
            return Ok(json!({"state":"notObserved","cleanupComplete":false}));
        };
        if let Some(receipt) = scope.entries.get(&sequence) {
            if receipt.operation != operation {
                return Err("操作身份与序号不一致".into());
            }
            let snapshot = receipt
                .listener
                .as_ref()
                .and_then(|id| self.listeners.get(id))
                .map(|l| l.snapshot())
                .unwrap_or_else(|| receipt.result.clone());
            return Ok(
                json!({"state":"observed","listenerId":receipt.listener,"snapshot":snapshot,"cleanupComplete":snapshot["cleanupComplete"]==true}),
            );
        }
        Ok(if sequence <= scope.last {
            json!({"state":"retired","cleanupComplete":true})
        } else {
            json!({"state":"notObserved","cleanupComplete":false})
        })
    }
}
pub struct Network {
    registry: Mutex<Registry>,
    slots: Arc<Semaphore>,
}
impl Default for Network {
    fn default() -> Self {
        Self {
            registry: Mutex::new(Registry::default()),
            slots: Arc::new(Semaphore::new(256)),
        }
    }
}
impl Network {
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut registry = self.registry.lock().unwrap();
        for l in registry.listeners.values().filter(|l| matches(&l.owner)) {
            l.close();
        }
        for scope in registry.scopes.values_mut().filter(|s| matches(&s.owner)) {
            scope.retiring = true;
        }
        registry.reap();
        // The view/instance is retiring; resource admission prevents it being recreated.
        registry.scopes.retain(|_, scope| {
            !matches(&scope.owner)
                || scope
                    .entries
                    .values()
                    .any(|r| r.result["cleanupComplete"] != true)
        });
    }
    pub(crate) fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        let mut registry = self.registry.lock().unwrap();
        registry.reap();
        registry
            .listeners
            .values()
            .filter(|l| matches(&l.owner))
            .count()
    }
}
fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(|| format!("缺少或无效 {key}"))
}
impl AppCore {
    pub(crate) fn network_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, String> {
        let owned = |l: &Listener| -> Result<(), String> {
            if l.owner.plugin != plugin || l.owner.instance != instance {
                Err("网络资源不属于此插件实例".into())
            } else {
                Ok(())
            }
        };
        let lifecycle = [
            "net.listener.create",
            "net.listener.lookup",
            "net.listener.cancel",
            "net.listener.release",
        ]
        .contains(&method);
        let mut identity = None;
        if lifecycle {
            let view = text(p, "viewId")?;
            let operation = text(p, "operationId")?;
            let sequence = text(p, "sequence")?
                .parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or("操作序号无效")?;
            if p["expectedContext"].is_null() {
                return Err("缺少 expectedContext".into());
            }
            let scope = json!([plugin, instance, view, p["expectedContext"]]).to_string();
            let mut registry = self.network.registry.lock().unwrap();
            registry.reap();
            let observed = registry.observed(&scope, sequence, operation)?;
            if observed["state"] == "notObserved" && method != "net.listener.create" {
                drop(registry);
                let ui = self.ui_state();
                let retired = ui.resource_view(view).map_or(true, |v| {
                    v.plugin_id != plugin
                        || v.plugin_instance_id != instance
                        || serde_json::to_value(&v.context).ok().as_ref()
                            != Some(&p["expectedContext"])
                });
                let mut next = self.network.registry.lock().unwrap();
                next.reap();
                if retired
                    && !next.listeners.values().any(|l| {
                        l.owner.plugin == plugin
                            && l.owner.instance == instance
                            && l.owner.view == view
                            && serde_json::to_value(&l.owner.context).ok().as_ref()
                                == Some(&p["expectedContext"])
                    })
                {
                    return Ok(if method == "net.listener.release" {
                        json!({"released":true})
                    } else {
                        json!({"state":"retired","cleanupComplete":true})
                    });
                }
                drop(ui);
                registry = next;
            }
            let observed = registry.observed(&scope, sequence, operation)?;
            if method == "net.listener.lookup" {
                return Ok(observed);
            }
            if method == "net.listener.release" {
                if observed["cleanupComplete"] != true {
                    return Err("监听清理尚未确认".into());
                }
                if let Some(scope) = registry.scopes.get_mut(&scope) {
                    scope.entries.remove(&sequence);
                }
                return Ok(json!({"released":true}));
            }
            if method == "net.listener.cancel" {
                if let Some(id) = observed["listenerId"].as_str() {
                    if let Some(l) = registry.listeners.get(id) {
                        l.close();
                    }
                }
                if let Some(state) = registry.scopes.get_mut(&scope) {
                    state.last = state.last.max(sequence);
                } else {
                    drop(registry);
                    let ui = self.ui_state();
                    let v = ui.resource_view(view)?;
                    if v.plugin_id != plugin || v.plugin_instance_id != instance {
                        return Err("视图不属于此插件实例".into());
                    }
                    let owner = Owner {
                        plugin: plugin.into(),
                        instance: instance.into(),
                        view: view.into(),
                        context: serde_json::from_value(p["expectedContext"].clone())
                            .map_err(|e| e.to_string())?,
                    };
                    let mut registry = self.network.registry.lock().unwrap();
                    if !registry.scopes.contains_key(&scope) && registry.scopes.len() >= 4096 {
                        return Err("网络操作作用域已满".into());
                    }
                    let state = registry.scopes.entry(scope.clone()).or_insert(Scope {
                        owner,
                        last: 0,
                        retiring: false,
                        entries: HashMap::new(),
                    });
                    state.last = state.last.max(sequence);
                    if let Some(id) = state
                        .entries
                        .get(&sequence)
                        .and_then(|r| r.listener.clone())
                    {
                        if let Some(l) = registry.listeners.get(&id) {
                            l.close();
                        }
                    }
                    return registry.observed(&scope, sequence, operation);
                }
                return registry.observed(&scope, sequence, operation);
            }
            if observed["state"] == "observed" {
                if registry.scopes[&scope].entries[&sequence].bind != p["bind"] {
                    return Err("操作 ID 参数已变化".into());
                }
                return Ok(observed);
            }
            if observed["state"] == "retired" {
                return Err("操作序号已退役，不能重新启动".into());
            }
            identity = Some((scope, sequence, operation.to_owned()));
        }
        if method == "net.listener.open" || method == "net.listener.create" {
            let owner = self.resource_owner(plugin, instance, text(p, "viewId")?)?;
            if lifecycle
                && serde_json::to_value(&owner.context).map_err(|e| e.to_string())?
                    != p["expectedContext"]
            {
                return Err("连接上下文已变化".into());
            }
            let session = self.connection_session(&owner.context)?;
            let host = text(&p["bind"], "host")?
                .parse::<std::net::IpAddr>()
                .map_err(|_| "监听地址必须为本机 IP 地址")?;
            let port = p["bind"]["port"]
                .as_u64()
                .filter(|p| *p <= 65535)
                .ok_or("本地端口无效")? as u16;
            let key = (instance.to_owned(), text(p, "operationId")?.to_owned());
            let fingerprint =
                json!([method, owner.view, owner.context, host.to_string(), port]).to_string();
            let path = self
                .connection_views()
                .into_iter()
                .find(|v| v.context.as_ref() == Some(&owner.context))
                .map(|v| json!({"endpoint":v.endpoint,"nodeCount":v.node_count}))
                .unwrap_or(Value::Null);
            let admission = self.resource_admission(&owner)?;
            let mut registry = self.network.registry.lock().unwrap();
            registry.reap();
            if let Some((scope, sequence, operation)) = &identity {
                let old = registry.observed(scope, *sequence, operation)?;
                if old["state"] == "retired" {
                    return Err("操作序号已退役，不能重新启动".into());
                }
                if old["state"] == "observed" {
                    if registry.scopes[scope].entries[sequence].bind != p["bind"] {
                        return Err("操作 ID 参数已变化".into());
                    }
                    return Ok(old);
                }
            } else if let Some(op) = registry.operations.get(&key) {
                if op.fingerprint != fingerprint {
                    return Err("操作 ID 参数已变化".into());
                }
                return Ok(json!({"listenerId":op.id}));
            }
            if identity
                .as_ref()
                .is_some_and(|(scope, _, _)| !registry.scopes.contains_key(scope))
                && registry.scopes.len() >= 4096
            {
                return Err("网络操作作用域已满".into());
            }
            if registry.operations.len()
                + registry
                    .scopes
                    .values()
                    .map(|s| s.entries.len())
                    .sum::<usize>()
                >= 4096
            {
                return Err("网络操作已满，请先确认并释放旧记录".into());
            }
            let all = &registry.listeners;
            if all
                .values()
                .filter(|l| l.owner.instance == instance)
                .count()
                >= 32
            {
                return Err("插件监听数量达到上限 32".into());
            }
            let id = self.next_id();
            let l_path = json!({"nodes":session.path,"summary":path});
            let l = Arc::new(Listener {
                id: id.clone(),
                owner,
                session,
                core: Arc::downgrade(self),
                status: Mutex::new(Status {
                    state: "starting",
                    address: Value::Null,
                    error: None,
                }),
                streams: Mutex::new(HashMap::new()),
                cancel: CancellationToken::new(),
                tasks: Arc::new(crate::resource_tasks::Tasks::default()),
                sent: AtomicU64::new(0),
                received: AtomicU64::new(0),
                samples: Mutex::new(VecDeque::from([(Instant::now(), 0, 0)])),
                path: l_path,
                probes: Mutex::new(HashMap::new()),
                probe_clients: Mutex::new(HashMap::new()),
                probe_operations: Mutex::new(HashSet::new()),
            });
            let task = l.tasks.enter()?;
            registry.listeners.insert(id.clone(), l.clone());
            if let Some((scope, sequence, operation)) = &identity {
                let state = registry.scopes.entry(scope.clone()).or_insert(Scope {
                    owner: l.owner.clone(),
                    last: 0,
                    retiring: false,
                    entries: HashMap::new(),
                });
                state.last = *sequence;
                state.entries.insert(
                    *sequence,
                    Receipt {
                        operation: operation.clone(),
                        bind: p["bind"].clone(),
                        listener: Some(id.clone()),
                        result: Value::Null,
                    },
                );
            } else {
                registry.operations.insert(
                    key,
                    Operation {
                        fingerprint,
                        id: id.clone(),
                        at: Instant::now(),
                    },
                );
            }
            drop(registry);
            drop(admission);
            let slots = self.network.slots.clone();
            tauri::async_runtime::spawn(async move {
                let _task = task;
                runtime::listen(l, host, port, slots).await;
            });
            return Ok(if lifecycle {
                json!({"state":"observed","listenerId":id,"cleanupComplete":false,"snapshot":{"listenerId":id,"state":"starting","cleanupComplete":false}})
            } else {
                json!({"listenerId":id})
            });
        }
        if method == "net.poll" {
            let view = text(p, "viewId")?;
            let registry = self.network.registry.lock().unwrap();
            let all = &registry.listeners;
            let mut items = Vec::new();
            for l in all.values().filter(|l| {
                l.owner.plugin == plugin && l.owner.instance == instance && l.owner.view == view
            }) {
                for s in l.streams.lock().unwrap().values().filter(|s| !s.remote) {
                    let mut st = s.state.lock().unwrap();
                    if !st.announced && st.state == "ready" && items.len() < 16 {
                        st.announced = true;
                        items.push(json!({"listenerId":l.id,"streamId":s.id}));
                    }
                }
            }
            return Ok(json!({"items":items}));
        }
        let id = p["listenerId"]
            .as_str()
            .or_else(|| p["streamId"].as_str())
            .or_else(|| p["id"].as_str())
            .ok_or("缺少网络资源 ID")?;
        let resource = {
            let registry = self.network.registry.lock().unwrap();
            let all = &registry.listeners;
            if let Some(l) = all.get(id) {
                Some((l.clone(), None))
            } else {
                all.values().find_map(|l| {
                    l.streams
                        .lock()
                        .unwrap()
                        .get(id)
                        .cloned()
                        .map(|s| (l.clone(), Some(s)))
                })
            }
        };
        let Some((l, stream)) = resource else {
            // Resource IDs are never reused. Repeated cleanup stays successful.
            return if method == "net.close" {
                Ok(json!({}))
            } else {
                Err("网络资源已释放".into())
            };
        };
        owned(&l)?;
        if method == "net.close" {
            if let Some(s) = stream {
                s.cancel.cancel();
                if !s.remote {
                    for child in l.streams.lock().unwrap().values() {
                        if child.client.as_deref() == Some(&s.id) {
                            child.cancel.cancel();
                        }
                    }
                }
            } else {
                l.close();
            }
            return Ok(json!({}));
        }
        if method == "net.listener.snapshot" {
            return Ok(l.snapshot());
        }
        if l.cancel.is_cancelled() || l.session.is_closed() {
            return Err("网络运行已停止".into());
        }
        if method == "net.listener.probe" {
            let operation = text(p, "operationId")?.to_owned();
            let address = l.status.lock().unwrap().address.clone();
            let host = address["host"]
                .as_str()
                .ok_or("监听尚未就绪")?
                .parse::<std::net::IpAddr>()
                .map_err(|_| "监听地址无效")?;
            let host = if host.is_unspecified() {
                if host.is_ipv4() {
                    "127.0.0.1".parse().unwrap()
                } else {
                    "::1".parse().unwrap()
                }
            } else {
                host
            };
            let port = address["port"].as_u64().ok_or("监听尚未就绪")? as u16;
            let mut probes = l.probe_operations.lock().unwrap();
            if probes.contains(&operation) {
                return Ok(json!({"probeId":operation}));
            }
            if probes.len() >= 4 {
                return Err("正在检查，请稍后再试".into());
            }
            let task = l.tasks.enter()?;
            probes.insert(operation.clone());
            drop(probes);
            let listener = l.clone();
            tauri::async_runtime::spawn(async move {
                let _task = task;
                runtime::probe(listener, host, port, operation).await;
            });
            return Ok(json!({"probeId":p["operationId"]}));
        }
        if method == "net.tcp.open" {
            let host = text(p, "host")?.to_owned();
            let port = p["port"]
                .as_u64()
                .filter(|p| *p > 0 && *p <= 65535)
                .ok_or("目标端口无效")? as u16;
            if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("目标主机无效".into());
            }
            let client = text(p, "clientStreamId")?.to_owned();
            let operation = text(p, "operationId")?;
            let mut streams = l.streams.lock().unwrap();
            let local = streams
                .get(&client)
                .filter(|s| !s.remote)
                .ok_or("本地连接已释放")?;
            if local.cancel.is_cancelled() {
                return Err("本地连接已结束".into());
            }
            let remote_id = format!("{client}:{operation}");
            if let Some(s) = streams.get(&remote_id) {
                if s.target.lock().unwrap().as_ref() != Some(&(host.clone(), port)) {
                    return Err("操作 ID 参数已变化".into());
                }
                return Ok(json!({"streamId":s.id}));
            }
            if streams
                .values()
                .any(|s| s.client.as_deref() == Some(&client))
            {
                return Err("本地连接已经分配目标".into());
            }
            let (s, rx) = runtime::new_stream(remote_id.clone(), &l, true, Some(client.clone()));
            *s.target.lock().unwrap() = Some((host.clone(), port));
            streams.insert(remote_id.clone(), s.clone());
            drop(streams);
            let listener = l.clone();
            let task = l.tasks.enter()?;
            tauri::async_runtime::spawn(async move {
                let _task = task;
                runtime::connect(listener, s, rx, host, port).await;
            });
            return Ok(json!({"streamId":remote_id}));
        }
        if method == "net.stream.splice" {
            let task = l.tasks.enter()?;
            let left = text(p, "leftId")?;
            let right = text(p, "rightId")?;
            let streams = l.streams.lock().unwrap();
            let a = streams.get(left).cloned().ok_or("本地流已释放")?;
            let b = streams.get(right).cloned().ok_or("远端流已释放")?;
            if a.remote || !b.remote || b.client.as_deref() != Some(left) {
                return Err("流归属不一致".into());
            }
            let mut x = a.state.lock().unwrap();
            let mut y = b.state.lock().unwrap();
            if x.claimed && y.claimed {
                return Ok(json!({}));
            }
            if x.state != "ready"
                || y.state != "ready"
                || x.reading
                || y.reading
                || x.read.is_some()
                || y.read.is_some()
            {
                return Err("流尚未就绪".into());
            }
            x.claimed = true;
            y.claimed = true;
            x.state = "relaying";
            y.state = "relaying";
            drop(x);
            drop(y);
            drop(streams);
            tauri::async_runtime::spawn(async move {
                let _task = task;
                runtime::splice(l, a, b).await;
            });
            return Ok(json!({}));
        }
        let s = stream.ok_or("需要 streamId")?;
        let mut st = s.state.lock().unwrap();
        match method {
            "net.stream.status" => Ok(json!({"state":st.state,"error":st.error})),
            "net.stream.read" => {
                if st.claimed {
                    return Err("流已交给原生转发".into());
                }
                let max = p["maxBytes"]
                    .as_u64()
                    .filter(|v| *v > 0 && *v <= 512)
                    .ok_or("协商读取上限为 512 字节")? as usize;
                if let Some(bytes) = st.read.take() {
                    return Ok(json!({"data":bytes,"eof":st.eof}));
                }
                if st.eof {
                    return Ok(json!({"data":[],"eof":true}));
                }
                if st.state == "failed" || st.state == "closed" {
                    return Err(st.error.clone().unwrap_or("连接已关闭".into()));
                }
                if !st.reading {
                    if st.budget + max > 1024 {
                        return Err("协商数据超过上限".into());
                    }
                    s.commands
                        .try_send(Command::Read(max))
                        .map_err(|_| "网络控制队列已满")?;
                    st.reading = true;
                }
                Ok(json!({"pending":true}))
            }
            "net.stream.write" => {
                let bytes: Vec<u8> =
                    serde_json::from_value(p["bytes"].clone()).map_err(|_| "协商数据无效")?;
                if st.claimed
                    || st.state != "ready"
                    || bytes.len() > 512
                    || st.budget + bytes.len() > 1024
                {
                    return Err("流状态或协商写入长度无效".into());
                }
                s.commands
                    .try_send(Command::Write(bytes.clone()))
                    .map_err(|_| "网络控制队列已满")?;
                st.budget += bytes.len();
                Ok(json!({"queued":true}))
            }
            "net.stream.finish" => {
                if st.claimed || st.state != "ready" {
                    return Err("流已移交或已结束".into());
                }
                s.commands
                    .try_send(Command::Finish)
                    .map_err(|_| "网络控制队列已满")?;
                Ok(json!({}))
            }
            _ => Err("未知网络接口".into()),
        }
    }
}
impl Listener {
    fn close(&self) {
        self.tasks.close();
        self.cancel.cancel();
    }
    fn snapshot(&self) -> Value {
        let st = self.status.lock().unwrap();
        let streams = self.streams.lock().unwrap();
        let sent = self.sent.load(Ordering::Relaxed);
        let received = self.received.load(Ordering::Relaxed);
        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap();
        while samples.len() > 1
            && samples
                .front()
                .is_some_and(|s| now.duration_since(s.0).as_secs_f64() > 3.0)
        {
            samples.pop_front();
        }
        let (up, down) = samples
            .front()
            .map(|s| {
                let t = now.duration_since(s.0).as_secs_f64().max(0.25);
                ((sent - s.1) as f64 / t, (received - s.2) as f64 / t)
            })
            .unwrap_or_default();
        if samples
            .back()
            .is_none_or(|s| now.duration_since(s.0).as_millis() > 250)
        {
            samples.push_back((now, sent, received));
        }
        let children:Vec<_>=streams.values().filter(|s|!s.remote).take(64).map(|s|{let state=s.state.lock().unwrap();let remote=streams.values().find(|r|r.client.as_deref()==Some(&s.id)).map(|r|{let x=r.state.lock().unwrap();json!({"state":x.state,"error":x.error})});json!({"id":s.id,"state":state.state,"probeId":s.id.split_once("@probe:").map(|(_,p)|p),"target":remote})}).collect();
        json!({"listenerId":self.id,"cleanupComplete":self.tasks.finished(),"state":st.state,"address":st.address,"error":st.error,"path":self.path,"sent":sent.to_string(),"received":received.to_string(),"sentPerSecond":up,"receivedPerSecond":down,"connections":streams.values().filter(|s|!s.remote&&matches!(s.state.lock().unwrap().state,"ready"|"relaying")).count(),"clients":children})
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            owner: Owner {
                plugin: "plugin".into(),
                instance: "instance".into(),
                view: "view".into(),
                context: crate::workspaces::Context {
                    workspace_id: "workspace".into(),
                    server_id: "server".into(),
                    connection_id: "connection".into(),
                    generation: 1,
                },
            },
            last: 1,
            retiring: false,
            entries: HashMap::new(),
        }
    }
    #[test]
    fn cancelled_sequence_is_retired_but_absence_is_not_cleanup() {
        let mut registry = Registry::default();
        assert_eq!(
            registry.observed("scope", 1, "open").unwrap()["cleanupComplete"],
            false
        );
        registry.scopes.insert("scope".into(), scope());
        assert_eq!(
            registry.observed("scope", 1, "open").unwrap()["state"],
            "retired"
        );
        assert_eq!(
            registry.observed("scope", 2, "next").unwrap()["state"],
            "notObserved"
        );
    }
    #[test]
    fn unfinished_receipts_survive_reaping_and_reject_changed_identity() {
        let mut registry = Registry::default();
        let mut scope = scope();
        scope.retiring = true;
        scope.entries.insert(
            1,
            Receipt {
                operation: "open".into(),
                bind: json!({"host":"127.0.0.1","port":0}),
                listener: None,
                result: json!({"state":"closed","cleanupComplete":false}),
            },
        );
        registry.scopes.insert("scope".into(), scope);
        registry.reap();
        assert_eq!(
            registry.observed("scope", 1, "open").unwrap()["cleanupComplete"],
            false
        );
        assert!(registry.observed("scope", 1, "other").is_err());
        registry
            .scopes
            .get_mut("scope")
            .unwrap()
            .entries
            .get_mut(&1)
            .unwrap()
            .result["cleanupComplete"] = json!(true);
        registry.reap();
        assert!(registry.scopes.is_empty());
    }
}
