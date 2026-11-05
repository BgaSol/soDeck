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
pub struct Network {
    listeners: Mutex<HashMap<String, Arc<Listener>>>,
    operations: Mutex<HashMap<(String, String), Operation>>,
    slots: Arc<Semaphore>,
}
impl Default for Network {
    fn default() -> Self {
        Self {
            listeners: Mutex::new(HashMap::new()),
            operations: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(256)),
        }
    }
}
impl Network {
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        self.listeners.lock().unwrap().retain(|_, l| {
            if matches(&l.owner) {
                l.cancel.cancel();
                false
            } else {
                true
            }
        });
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
        if method == "net.listener.open" {
            let owner = self.resource_owner(plugin, instance, text(p, "viewId")?)?;
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
            let mut ops = self.network.operations.lock().unwrap();
            ops.retain(|_, v| v.at.elapsed() < Duration::from_secs(60));
            if let Some(op) = ops.get(&key) {
                if op.fingerprint != fingerprint {
                    return Err("操作 ID 参数已变化".into());
                }
                return Ok(json!({"listenerId":op.id}));
            }
            if ops.len() >= 4096 {
                return Err("网络操作过多，请稍后再试".into());
            }
            let mut all = self.network.listeners.lock().unwrap();
            all.retain(|_, l| {
                l.owner.instance != instance
                    || !matches!(l.status.lock().unwrap().state, "closed" | "failed")
            });
            if all
                .values()
                .filter(|l| l.owner.instance == instance)
                .count()
                >= 32
            {
                return Err("插件监听数量达到上限 32".into());
            }
            let id = self.next_id();
            let path = self
                .connection_views()
                .into_iter()
                .find(|v| v.context.as_ref() == Some(&owner.context))
                .map(|v| json!({"endpoint":v.endpoint,"nodeCount":v.node_count}))
                .unwrap_or(Value::Null);
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
                sent: AtomicU64::new(0),
                received: AtomicU64::new(0),
                samples: Mutex::new(VecDeque::from([(Instant::now(), 0, 0)])),
                path: l_path,
                probes: Mutex::new(HashMap::new()),
                probe_clients: Mutex::new(HashMap::new()),
                probe_operations: Mutex::new(HashSet::new()),
            });
            all.insert(id.clone(), l.clone());
            ops.insert(
                key,
                Operation {
                    fingerprint,
                    id: id.clone(),
                    at: Instant::now(),
                },
            );
            drop(all);
            drop(ops);
            if let Err(e) = self.resource_owner(plugin, instance, &l.owner.view) {
                l.cancel.cancel();
                self.network.listeners.lock().unwrap().remove(&id);
                return Err(e);
            }
            let slots = self.network.slots.clone();
            tauri::async_runtime::spawn(runtime::listen(l, host, port, slots));
            return Ok(json!({"listenerId":id}));
        }
        if method == "net.poll" {
            let view = text(p, "viewId")?;
            let all = self.network.listeners.lock().unwrap();
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
            let all = self.network.listeners.lock().unwrap();
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
                l.cancel.cancel();
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
            probes.insert(operation.clone());
            drop(probes);
            let listener = l.clone();
            tauri::async_runtime::spawn(async move {
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
            tauri::async_runtime::spawn(async move {
                runtime::connect(listener, s, rx, host, port).await;
            });
            return Ok(json!({"streamId":remote_id}));
        }
        if method == "net.stream.splice" {
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
            tauri::async_runtime::spawn(runtime::splice(l, a, b));
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
        json!({"listenerId":self.id,"state":st.state,"address":st.address,"error":st.error,"path":self.path,"sent":sent.to_string(),"received":received.to_string(),"sentPerSecond":up,"receivedPerSecond":down,"connections":streams.values().filter(|s|!s.remote&&matches!(s.state.lock().unwrap().state,"ready"|"relaying")).count(),"clients":children})
    }
}
