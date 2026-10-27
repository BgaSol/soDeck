mod model;
mod route;
mod route_store;
mod route_services;
mod route_v2;
mod secrets;
mod ssh;
mod store;
mod server_catalog;
mod server_catalog_store;
pub mod store_bootstrap;
mod remote_read;
mod pty_observe;
mod pty_start;
mod network;
pub mod contract;
#[path="workbench_ui_contract.generated.rs"]
pub mod workbench_ui_contract;
#[path="../../../contracts/capabilities.rs"]
pub mod capabilities;
pub use remote_read::WorkspaceScope;

use base64::{engine::general_purpose::STANDARD, Engine};
use model::{field, id, number};
pub use model::{Caller, CoreError, Result, Server};
use secrets::{Credential, Secrets};
use serde_json::{json, Value};
use ssh::{Connection, PtyState};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use store::Store;

struct Lease {
    connection: Arc<Connection>,
    instance_id: String,
}
struct Pty {
    id: String,
    connection: Arc<Connection>,
    instance_id: String,
    generation: u64,
    state: Arc<Mutex<PtyState>>,
}
#[derive(Default)]
struct Registry {
    connections: HashMap<String, Arc<Connection>>,
    leases: HashMap<String, Lease>,
    ptys: HashMap<String, Arc<Pty>>,
    revoked: HashSet<String>,
    pending_ptys: usize,
    pending_connections: HashMap<String, (String, Arc<AtomicBool>)>,
    shutdown_epoch: u64,
}

struct Operation {
    canceled: Arc<AtomicBool>,
    active: usize,
    touched: Instant,
}
struct ConnectionAdmission { registry:Arc<Mutex<Registry>>, id:String }
impl Drop for ConnectionAdmission {
    fn drop(&mut self){self.registry.lock().unwrap().pending_connections.remove(&self.id);}
}

pub struct Core {
    shared: Arc<CoreState>,
    owns_lifetime: bool,
}
#[doc(hidden)]
pub struct CoreState {
    configuration: Mutex<()>,
    route_attempts: Mutex<HashMap<String, route_services::Attempt>>,
    store: Store,
    secrets: Secrets,
    registry: Arc<Mutex<Registry>>,
    next_generation: AtomicU64,
    connect_locks: Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>,
    credential_revisions: Mutex<HashMap<String, u64>>,
    host_challenges: Mutex<HashMap<String, Value>>,
    log_faults: Mutex<VecDeque<Value>>,
    log_dropped: AtomicU64,
    call_count: AtomicU64,
    operations: Mutex<HashMap<(String, String), Operation>>,
    pty_starts: Arc<Mutex<HashMap<(String,String),pty_start::Receipt>>>,
    remote: Arc<Mutex<remote_read::RemoteRegistry>>,
    remote_admission: Mutex<()>,
    remote_budget: Arc<Mutex<ssh::remote::IoBudget>>,
    transport_budget: Arc<Mutex<ssh::TransportBudget>>,
    network: Mutex<network::Registry>,
    network_jobs: Mutex<Option<std::sync::mpsc::SyncSender<network::Job>>>,
    network_stopped: AtomicBool,
}
impl Core {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(data_dir.as_ref())?;
        let store = Store::open(data_dir.as_ref())?;
        store.maintain()?;
        let core = Self { shared: Arc::new(CoreState {
            configuration: Mutex::new(()),
            route_attempts: Mutex::new(HashMap::new()),
            store,
            secrets: Secrets::open(data_dir.as_ref())?,
            registry: Arc::new(Mutex::new(Registry::default())),
            next_generation: AtomicU64::new(model::now_ms()),
            connect_locks: Mutex::new(HashMap::new()),
            credential_revisions: Mutex::new(HashMap::new()),
            host_challenges: Mutex::new(HashMap::new()),
            log_faults: Mutex::new(VecDeque::new()),
            log_dropped: AtomicU64::new(0),
            call_count: AtomicU64::new(0),
            operations: Mutex::new(HashMap::new()),
            pty_starts: Arc::new(Mutex::new(HashMap::new())),
            remote: Arc::new(Mutex::new(remote_read::RemoteRegistry::default())),
            remote_admission: Mutex::new(()),
            remote_budget: Arc::new(Mutex::new(ssh::remote::IoBudget::default())),
            transport_budget: Arc::new(Mutex::new(ssh::TransportBudget::default())),
            network: Mutex::new(network::Registry::default()),
            network_jobs: Mutex::new(None),
            network_stopped: AtomicBool::new(false),
        }), owns_lifetime: true };
        core.start_remote_maintenance();
        core.start_network();
        Ok(core)
    }
    /// Native hosts bind caller identity before entering this boundary. Params never
    /// override source attribution. Run network methods on a background executor.
    pub fn call(&self, caller: &Caller, method: &str, params: Value) -> Result<Value> {
        self.record(caller, method, &params, || {
            if self
                .registry
                .lock()
                .unwrap()
                .revoked
                .contains(&caller.instance_id)
            {
                return Err(CoreError::new("INSTANCE_EXPIRED", "插件实例已经结束"));
            }
            self.dispatch(caller, method, &params)
        })
    }
    /// Desktop-owned public APIs (workbench/clipboard/plugin manager) share the
    /// same audit barrier without putting their implementation in this crate.
    pub fn record(
        &self,
        caller: &Caller,
        method: &str,
        params: &Value,
        operation: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        let available_without_log = matches!(
            method,
            "pty.close"
                | "connections.release"
                | "resources.stop"
                | "app.exit"
                | "workbench.close"
                | "diagnostics.get"
                | "operations.cancel"
                | "workbench.cancelOpen"
                | "state.close" | "pty.observe.close" | "directories.unregister" | "files.close" | "files.closeDirectory" | "exec.cancel" | "exec.close" | "remote.operations.get" | "remote.operations.forget"
                | "net.groups.close" | "net.routes.release" | "net.listeners.close" | "net.streams.close"
                | "net.operations.get" | "net.operations.cancel" | "net.resources.list" | "net.resources.stop"
        );
        let record = self.store.call_start(caller, method, params);
        if record.is_err() {
            self.fault(method, "start-not-persisted");
            if !available_without_log {
                return Err(CoreError::new(
                    "LOG_UNAVAILABLE",
                    "调用记录存储不可用，未执行本次操作",
                ));
            }
        }
        let expired = self.registry.lock().unwrap().revoked.contains(&caller.instance_id);
        let result = if expired {Err(CoreError::new("INSTANCE_EXPIRED", "插件实例已经结束"))} else {operation()};
        if let Ok(record) = record {
            if self.store.call_finish(&record, &result).is_err() {
                self.fault(method, "result-not-persisted");
                if result.is_ok() && !available_without_log {
                    return Err(CoreError::new(
                        "RESULT_UNKNOWN",
                        "操作已执行但结果未能持久记录，请查询状态，不要盲目重试",
                    )
                    .details(json!({"recordId":record})));
                }
            }
        }
        if self.call_count.fetch_add(1, Ordering::Relaxed) % 1000 == 999 {
            if self.store.maintain().is_err() {
                self.fault("calls.maintenance", "retention-failed");
            }
        }
        result
    }
    fn fault(&self, method: &str, reason: &str) {
        let mut queue = self.log_faults.lock().unwrap();
        if queue.len() == 1024 {
            queue.pop_front();
            self.log_dropped.fetch_add(1, Ordering::Relaxed);
        }
        queue.push_back(json!({"time":model::now_ms(),"method":method,"reason":reason}));
    }
    pub fn diagnostics(&self) -> Value {
        json!({"logFaults":*self.log_faults.lock().unwrap(),"droppedLogFaults":self.log_dropped.load(Ordering::Relaxed)})
    }
    fn dispatch(&self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        if method.starts_with("servers.catalog.") || method.starts_with("serverGroups.") {return self.catalog_dispatch(caller,method,p);}
        if method.starts_with("net.") { return self.network_dispatch(caller, method, p); }
        if method.starts_with("servers.routes.v2.") || method.starts_with("connections.route.v2.") || method == "auth.challenge.v2.get" { return self.route_v2_dispatch(caller,method,p); }
        if method.starts_with("servers.routes.") || matches!(method,"connections.route.get"|"connections.routes.list"|"auth.challenge.get") { return self.route_dispatch(caller,method,p); }
        if method.starts_with("files.") || method.starts_with("exec.") || method.starts_with("remote.operations.") || matches!(method,"connections.observe"|"connections.acquireExisting") {return self.remote_dispatch(caller,method,p);}
        match method {
            "capabilities.list" => Ok(
                json!({"apiVersion":1,"capabilities":capabilities::CAPABILITIES}),
            ),
            "servers.list" => {
                let mut servers = self.store.servers()?;
                for server in &mut servers {
                    server.has_secret = self.secrets.remembered(&server.id);
                }
                Ok(json!({"servers":servers}))
            }
            "servers.get" => {
                let mut server = self.store.server(field(p, "serverId")?)?;
                server.has_secret = self.secrets.remembered(&server.id);
                Ok(serde_json::to_value(server)?)
            }
            "servers.create" | "servers.update" => self.save_server_configuration(p, method == "servers.update", p.get("connectionRoute").is_none()),
            "servers.delete" => {
                let _config_guard=self.configuration.lock().unwrap();
                let server_id = field(p, "serverId")?;
                self.store.server(server_id)?;
                let dependents=self.store.dependents(server_id)?;
                if !dependents.is_empty(){return Err(CoreError::new("SERVER_DEPENDENTS","此服务器被用作跳板，请先修改依赖服务器的连接方式").details(json!({"servers":dependents})));}
                let active: Vec<String> = self
                    .registry
                    .lock()
                    .unwrap()
                    .connections
                    .values()
                    .filter(|c| c.route.contains(server_id))
                    .map(|c| c.id.clone())
                    .collect();
                if !active.is_empty()
                    && !p
                        .get("stopSessions")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                {
                    return Err(CoreError::new(
                        "RESOURCE_IN_USE",
                        "服务器仍有运行会话，请确认结束后删除",
                    )
                    .details(json!({"connectionIds":active})));
                }
                for connection in active {
                    self.stop_resource(&connection)?;
                }
                self.secrets.remove(server_id)?;
                self.store.delete_server(server_id)?;
                Ok(json!({"deleted":true}))
            }
            "auth.set" => {
                let _config_guard=self.configuration.lock().unwrap();
                let server_id = field(p, "serverId")?;
                let server = self.store.server(server_id)?;
                if let Some(id)=p["challengeId"].as_str(){let challenge=self.checked_challenge(id)?;if challenge["serverId"]!=server_id {return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证对象不匹配"));}}
                let plan=self.route_plan(server_id)?;
                let key = if server.auth_type == "key" {
                    let path = field(p, "privateKeyPath")?;
                    let metadata = std::fs::metadata(path)?;
                    if metadata.len() > 1024 * 1024 {
                        return Err(CoreError::new("LIMIT_EXCEEDED", "私钥文件超过 1 MiB"));
                    }
                    std::fs::read_to_string(path)?
                } else {
                    String::new()
                };
                let credential = Credential {
                    context: Some(plan.credential_context(plan.nodes.len()-1)),
                    password: p
                        .get("password")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    private_key: key,
                    passphrase: p
                        .get("passphrase")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                };
                self.secrets.set(
                    server_id,
                    credential,
                    p.get("remember").and_then(Value::as_bool).unwrap_or(false),
                )?;
                *self
                    .credential_revisions
                    .lock()
                    .unwrap()
                    .entry(server_id.into())
                    .or_default() += 1;
                Ok(json!({"stored":true,"remembered":self.secrets.remembered(server_id)}))
            }
            "hosts.trust" => {
                let _guard=self.configuration.lock().unwrap();
                let server_id=field(p,"serverId")?;let fingerprint=field(p,"fingerprint")?;
                let challenge_id=if let Some(id)=p["challengeId"].as_str(){id.to_string()}else{
                    self.host_challenges.lock().unwrap().iter().find(|(_,v)|v["serverId"]==server_id&&v["fingerprint"]==fingerprint).map(|(id,_)|id.clone()).ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","请重新连接以获取当前主机身份"))?
                };
                let challenge=self.checked_challenge(&challenge_id)?;
                if challenge["serverId"]!=server_id||challenge["fingerprint"]!=fingerprint {
                    return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","确认的主机身份与当前握手不一致"));
                }
                if challenge["previousFingerprint"].is_string() {
                    *self.credential_revisions.lock().unwrap().entry(server_id.into()).or_default() += 1;
                    // A replaced gateway can expose another private network.
                    // Reconfirm downstream credentials before using the new path.
                    for dependent in self.store.dependents(server_id)? {
                        self.secrets.remove(&dependent.id)?;
                        *self.credential_revisions.lock().unwrap().entry(dependent.id).or_default()+=1;
                    }
                }
                self.store.trust(field(&challenge,"trustKey")?,fingerprint,field(&challenge,"algorithm")?)?;
                self.host_challenges.lock().unwrap().remove(&challenge_id);
                Ok(json!({"trusted":true}))
            }
            "connections.acquire" => {
                self.with_operation(caller, p, |token| self.acquire(caller, p, token))
            }
            "operations.cancel" => {
                let key = Self::operation_key(caller, p)?;
                let mut operations = self.operations.lock().unwrap();
                Self::prune_operations(&mut operations);
                if !operations.contains_key(&key) && operations.len() >= 10_000 {
                    return Err(CoreError::new("LIMIT_EXCEEDED", "操作取消记录已满"));
                }
                let operation = operations.entry(key).or_insert_with(|| Operation {
                    canceled: Arc::new(AtomicBool::new(false)),
                    active: 0,
                    touched: Instant::now(),
                });
                operation.canceled.store(true, Ordering::SeqCst);
                operation.touched = Instant::now();
                Ok(json!({"canceled":true}))
            }
            "connections.release" => {
                let lease_id = field(p, "leaseId")?;
                let mut registry = self.registry.lock().unwrap();
                if let Some(lease) = registry.leases.get(lease_id) {
                    if lease.instance_id != caller.instance_id {
                        return Err(CoreError::new(
                            "STALE_HANDLE",
                            "租约不属于当前实例，请取得自己的引用",
                        ));
                    }
                }
                registry.leases.remove(lease_id);
                Self::reap(&mut registry);
                drop(registry);
                self.remote_release(lease_id);
                Ok(json!({"released":true}))
            }
            "pty.open" => {
                if p.get("command").is_some()||p.get("observe").is_some(){return Err(CoreError::new("INVALID_PARAMS","自定义启动请使用 pty.openCommand"));}
                self.with_operation(caller, p, |token| self.open_pty(caller, p, token))
            }
            "pty.openCommand" => {
                pty_observe::validate("pty.openCommand","x-methods",p)?;
                let command=field(p,"command")?;
                if command.len()>32768||command.contains('\0'){return Err(CoreError::new("INVALID_PARAMS","PTY 启动命令无效或超长"));}
                self.start_pty(caller,p)
            }
            "pty.start.get"=>self.pty_start_result(caller,p),
            "pty.observe.open"|"pty.observe.read"|"pty.observe.close"=>self.observe_pty(caller,method,p).and_then(|result|{pty_observe::validate(method,"x-results",&result)?;Ok(result)}),
            "pty.read" => {
                let pty = self.pty(p)?;
                self.check_binding(caller, &pty)?;
                let result = pty
                    .state
                    .lock()
                    .unwrap()
                    .read(p.get("after").and_then(Value::as_u64).unwrap_or(0));
                result
            }
            "pty.ack" => {
                let pty = self.pty(p)?;
                self.check_binding(caller, &pty)?;
                pty.state.lock().unwrap().ack(number(p, "sequence")?)?;
                Ok(json!({"acknowledged":number(p,"sequence")?}))
            }
            "pty.write" => {
                let pty = self.pty(p)?;
                self.check_binding(caller, &pty)?;
                let bytes = STANDARD
                    .decode(field(p, "bytes")?)
                    .map_err(|_| CoreError::new("INVALID_PARAMS", "输入不是有效的 base64"))?;
                pty.connection
                    .write(&pty.id, &pty.state, number(p, "inputSeq")?, bytes)
            }
            "pty.resize" => {
                let pty = self.pty(p)?;
                self.check_binding(caller, &pty)?;
                let (cols, rows) = ssh::dimensions(number(p, "cols")?, number(p, "rows")?)?;
                pty.connection.resize(&pty.id, cols, rows)?;
                Ok(json!({"cols":cols,"rows":rows}))
            }
            "pty.bind" => {
                let pty = self.pty(p)?;
                let mut state = pty.state.lock().unwrap();
                let next = number(p, "mountGeneration")?;
                if state.mount_generation.is_some_and(|current| {
                    next < current
                        || (next == current
                            && state.view_id.as_deref() != p.get("viewId").and_then(Value::as_str))
                }) {
                    return Err(CoreError::new("STALE_MOUNT", "挂载代次必须前进"));
                }
                state.view_id = Some(field(p, "viewId")?.into());
                state.mount_generation = Some(next);
                Ok(
                    json!({"bound":true,"mountGeneration":next,"lastInputSeq":state.last_input,"acknowledged":state.acknowledged}),
                )
            }
            "pty.close" => {
                let pty_id = field(p, "ptyId")?;
                let mut registry = self.registry.lock().unwrap();
                if let Some(pty) = registry.ptys.get(pty_id) {
                    if number(p, "generation")? != pty.generation {
                        return Err(CoreError::new("STALE_HANDLE", "终端代次已经失效"));
                    }
                    pty.connection.close_pty(&pty.state);
                }
                registry.ptys.remove(pty_id);
                Self::reap(&mut registry);
                Ok(json!({"closed":true}))
            }
            "resources.list" => Ok(self.resources()),
            "resources.stop" => self.stop_resource(field(p, "resourceId")?),
            "settings.get" => Ok(
                json!({"value":self.store.get_setting(caller.plugin_id.as_deref().unwrap_or("shell"),field(p,"key")?)?}),
            ),
            "settings.set" => {
                self.store.set_setting(
                    caller.plugin_id.as_deref().unwrap_or("shell"),
                    field(p, "key")?,
                    p.get("value").unwrap_or(&Value::Null),
                )?;
                Ok(json!({"saved":true}))
            }
            "calls.list" => self.store.calls(p),
            "diagnostics.get" => Ok(self.diagnostics()),
            _ => Err(CoreError::new("METHOD_NOT_FOUND", "核心尚未实现此方法")),
        }
    }
    fn operation_key(caller: &Caller, p: &Value) -> Result<(String, String)> {
        let operation = field(p, "operationId")?;
        if operation.is_empty() || operation.len() > 128 || operation.chars().any(char::is_control)
        {
            return Err(CoreError::new("INVALID_PARAMS", "操作标识无效"));
        }
        Ok((caller.instance_id.clone(), operation.into()))
    }
    fn prune_operations(operations: &mut HashMap<(String, String), Operation>) {
        operations.retain(|_, operation| {
            operation.active > 0 || operation.touched.elapsed() < Duration::from_secs(600)
        });
    }
    fn with_operation(
        &self,
        caller: &Caller,
        p: &Value,
        run: impl FnOnce(&AtomicBool) -> Result<Value>,
    ) -> Result<Value> {
        if p.get("operationId").is_none() {
            return run(&AtomicBool::new(false));
        }
        let key = Self::operation_key(caller, p)?;
        let token = {
            let mut operations = self.operations.lock().unwrap();
            Self::prune_operations(&mut operations);
            if !operations.contains_key(&key) && operations.len() >= 10_000 {
                return Err(CoreError::new("LIMIT_EXCEEDED", "操作记录已满"));
            }
            let operation = operations.entry(key.clone()).or_insert_with(|| Operation {
                canceled: Arc::new(AtomicBool::new(false)),
                active: 0,
                touched: Instant::now(),
            });
            if operation.canceled.load(Ordering::SeqCst) {
                return Err(CoreError::new("CANCELED", "操作已经取消"));
            }
            operation.active += 1;
            operation.canceled.clone()
        };
        let result = run(&token);
        if let Some(operation) = self.operations.lock().unwrap().get_mut(&key) {
            operation.active -= 1;
            operation.touched = Instant::now();
        }
        result
    }
    fn acquire(&self, caller: &Caller, p: &Value, operation: &AtomicBool) -> Result<Value> {
        self.acquire_for(caller, p, operation, "session", &caller.instance_id)
    }
    fn acquire_for(&self, caller: &Caller, p: &Value, operation: &AtomicBool, purpose: &str, lease_owner: &str) -> Result<Value> {
        let initial_epoch = self.registry.lock().unwrap().shutdown_epoch;
        let plan={let _guard=self.configuration.lock().unwrap();self.route_plan(field(p,"serverId")?)?};
        if p.get("expectedRouteRevision").and_then(Value::as_str).is_some_and(|r| r != plan.revision) {
            return Err(CoreError::new("ROUTE_CHANGED", "连接路径已变化，请应用配置并重新启动"));
        }
        let server=plan.target().clone();
        if p.get("configRevision")
            .and_then(Value::as_u64)
            .is_some_and(|revision| revision != server.revision)
        {
            return Err(CoreError::new(
                "REVISION_CONFLICT",
                "请使用当前服务器配置修订",
            ));
        }
        let workspace = field(p, "workspaceId")?.to_owned();
        let state_key = format!("{}:{}:{}", server.id, plan.revision, workspace);
        let key = format!("{state_key}:{purpose}");
        let gate = {
            let mut gates=self.connect_locks.lock().unwrap();
            gates.retain(|_,gate|gate.strong_count()>0);
            if gates.len()>=512&&!gates.contains_key(&key){return Err(CoreError::new("LIMIT_EXCEEDED","等待连接过多"));}
            if let Some(gate)=gates.get(&key).and_then(std::sync::Weak::upgrade){gate}
            else {let gate=Arc::new(Mutex::new(()));gates.insert(key.clone(),Arc::downgrade(&gate));gate}
        };
        let _guard = loop {
            if operation.load(Ordering::SeqCst) {
                return Err(CoreError::new("CANCELED", "连接已取消"));
            }
            {
                let registry = self.registry.lock().unwrap();
                if registry.revoked.contains(&caller.instance_id)
                    || registry.shutdown_epoch != initial_epoch
                {
                    return Err(CoreError::new("CANCELED", "连接消费者已经结束"));
                }
            }
            match gate.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(CoreError::new("INTERNAL", "连接调度不可用"))
                }
            }
        };
        if self.registry.lock().unwrap().shutdown_epoch != initial_epoch {
            return Err(CoreError::new("CANCELED", "应用正在清理连接"));
        }

        let existing = self
            .registry
            .lock()
            .unwrap()
            .connections
            .values()
            .find(|c| {
                c.server.id == server.id
                    && c.purpose == purpose
                    && c.server.revision == server.revision
                    && c.workspace_id == workspace
                    && c.route.revision == plan.revision
                    && *c.status.lock().unwrap() == "active"
            })
            .cloned();
        let mut _admission=None;
        let connection = if let Some(connection) = existing {
            connection
        } else {
            let pending_id = id();
            let canceled = Arc::new(AtomicBool::new(false));
            {
                let mut registry = self.registry.lock().unwrap();
                if registry.revoked.contains(&caller.instance_id)
                    || registry.shutdown_epoch != initial_epoch
                {
                    return Err(CoreError::new("CANCELED", "连接消费者已经结束"));
                }
                if registry.connections.len()+registry.pending_connections.len()>=64 {
                    return Err(CoreError::new("LIMIT_EXCEEDED","目标连接已达到 64 个上限"));
                }
                registry.pending_connections.insert(
                    pending_id.clone(),
                    (caller.instance_id.clone(), canceled.clone()),
                );
                _admission=Some(ConnectionAdmission{registry:self.registry.clone(),id:pending_id.clone()});
            }
            if purpose == "session" {self.connection_state(&state_key,"connecting");}
            let result = Connection::connect_for(
                purpose.to_owned(), plan.clone(), workspace.clone(), self.next_generation.fetch_add(1,Ordering::SeqCst),
                canceled,operation,self.transport_budget.clone(),
                |index,fingerprint,algorithm,verified|self.verify_route_node(&plan,index,fingerprint,algorithm,verified),
                |index,stage|{
                    self.route_progress(&plan,&workspace,index,stage,None);
                    if self.store.route_stage(caller,p,&plan,index,stage,None).is_err(){self.fault("connections.route.stage","stage-not-persisted");}
                },
            );
            match result {
                Ok(connection) => {if purpose == "session" {self.connection_state(&state_key,"active");}connection},
                Err(mut error) => {
                    let index=error.details.as_ref().and_then(|d|d["hopIndex"].as_u64()).unwrap_or(0) as usize;
                    if self.store.route_stage(caller,p,&plan,index,"failed",Some(&error)).is_err(){self.fault("connections.route.stage","stage-not-persisted");}
                    if purpose == "session" {self.connection_state(&state_key,if matches!(error.code.as_str(),"HOST_KEY_REQUIRED"|"HOST_KEY_CHANGED"|"AUTH_REQUIRED"|"AUTH_FAILED"){"authRequired"}else if error.code=="CANCELED"{"idle"}else{"failed"});}
                    if matches!(error.code.as_str(),"HOST_KEY_REQUIRED"|"HOST_KEY_CHANGED"|"AUTH_REQUIRED"|"AUTH_FAILED") {
                        self.route_challenge(caller,p,&plan,&mut error)?;
                    }
                    self.route_progress(&plan,&workspace,error.details.as_ref().and_then(|d|d["hopIndex"].as_u64()).unwrap_or(0) as usize,if error.code=="CANCELED"{"canceled"}else{"failed"},Some(&error));
                    return Err(error);
                }
            }
        };
        let _config_guard=self.configuration.lock().unwrap();
        if self.route_plan(&server.id).map(|p|p.revision).ok().as_deref()!=Some(plan.revision.as_str()) {
            if !self.registry.lock().unwrap().connections.contains_key(&connection.id){connection.stop();}
            return Err(CoreError::new("REVISION_CONFLICT","连接路径已变化，请重新连接"));
        }
        self.route_progress(&plan,&workspace,plan.nodes.len()-1,"active",None);
        if self.store.route_stage(caller,p,&plan,plan.nodes.len()-1,"active",None).is_err(){self.fault("connections.route.stage","stage-not-persisted");}
        let mut registry = self.registry.lock().unwrap();
        if operation.load(Ordering::SeqCst) {
            if !registry.connections.contains_key(&connection.id) {
                connection.stop();
            }
            return Err(CoreError::new("CANCELED", "连接已取消"));
        }
        if registry.revoked.contains(&caller.instance_id) {
            if !registry.connections.contains_key(&connection.id) {
                connection.stop();
            }
            return Err(CoreError::new("INSTANCE_EXPIRED", "连接消费者已经结束"));
        }
        if registry.shutdown_epoch != initial_epoch {
            connection.stop();
            return Err(CoreError::new("CANCELED", "应用正在清理连接"));
        }
        if registry.leases.len()>=1024 {
            if !registry.connections.contains_key(&connection.id){connection.stop();}
            return Err(CoreError::new("LIMIT_EXCEEDED","连接使用关系已达到上限"));
        }
        let lease_id = id();
        registry
            .connections
            .insert(connection.id.clone(), connection.clone());
        registry.leases.insert(
            lease_id.clone(),
            Lease {
                connection: connection.clone(),
                instance_id: lease_owner.to_owned(),
            },
        );
        Ok(
            json!({"leaseId":lease_id,"connectionId":connection.id,"generation":connection.generation}),
        )
    }
    fn open_pty(&self, caller: &Caller, p: &Value, operation: &AtomicBool) -> Result<Value> {
        let (cols, rows) = ssh::dimensions(
            p.get("cols").and_then(Value::as_u64).unwrap_or(80),
            p.get("rows").and_then(Value::as_u64).unwrap_or(24),
        )?;
        let lease_id = field(p, "leaseId")?;
        let (connection, epoch) = {
            let mut registry = self.registry.lock().unwrap();
            if registry.ptys.len() + registry.pending_ptys >= 8 {
                return Err(CoreError::new("LIMIT_EXCEEDED", "同时最多运行 8 个终端"));
            }
            let lease = registry
                .leases
                .get(lease_id)
                .ok_or_else(|| CoreError::new("STALE_HANDLE", "连接租约已经释放"))?;
            if lease.instance_id != caller.instance_id {
                return Err(CoreError::new("STALE_HANDLE", "请使用当前实例的连接租约"));
            }
            let connection = lease.connection.clone();
            registry.pending_ptys += 1;
            (connection, registry.shutdown_epoch)
        };
        let pty_id = id();
        let command=p.get("command").and_then(Value::as_str).map(str::to_owned);
        let observe=p["observe"]==true;
        let result = connection.open(&pty_id, cols, rows, operation, command, observe);
        let mut registry = self.registry.lock().unwrap();
        registry.pending_ptys -= 1;
        let state = result?;
        if registry.revoked.contains(&caller.instance_id) {
            connection.close_pty(&state);
            return Err(CoreError::new("INSTANCE_EXPIRED", "终端消费者已经结束"));
        }
        // Waiting for SSH must not resurrect a resource after its consumer,
        // connection, or application lifetime ended on another thread.
        if operation.load(Ordering::SeqCst)
            || registry.shutdown_epoch != epoch
            || !registry.connections.contains_key(&connection.id)
            || !registry.leases.get(lease_id).is_some_and(|lease| {
                lease.instance_id == caller.instance_id && lease.connection.id == connection.id
            })
        {
            connection.close_pty(&state);
            return Err(CoreError::new("CANCELED", "终端所属连接或租约已经结束"));
        }
        {
            let mut current = state.lock().unwrap();
            current.view_id = caller.view_id.clone();
            current.mount_generation = caller.mount_generation;
        }
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let observer=state.lock().unwrap().observer.as_ref().map(|o|json!({"observerId":o.id,"cursor":"0"}));
        registry.ptys.insert(
            pty_id.clone(),
            Arc::new(Pty {
                id: pty_id.clone(),
                connection: connection.clone(),
                instance_id: caller.instance_id.clone(),
                generation,
                state,
            }),
        );
        let mut result=json!({"ptyId":pty_id,"streamId":pty_id,"connectionId":connection.id,"generation":generation});
        if let Some(observer)=observer {result["observer"]=observer;}
        Ok(result)
    }
    fn pty(&self, p: &Value) -> Result<Arc<Pty>> {
        let registry = self.registry.lock().unwrap();
        let pty = registry
            .ptys
            .get(field(p, "ptyId")?)
            .ok_or_else(|| CoreError::new("STALE_HANDLE", "终端已经关闭"))?;
        if number(p, "generation")? != pty.generation {
            return Err(CoreError::new("STALE_HANDLE", "终端代次已失效"));
        }
        Ok(pty.clone())
    }
    fn check_binding(&self, caller: &Caller, pty: &Pty) -> Result<()> {
        let state = pty.state.lock().unwrap();
        if state.view_id.is_some()
            && (state.view_id != caller.view_id
                || state.mount_generation != caller.mount_generation)
        {
            return Err(CoreError::new("STALE_MOUNT", "终端已移动到其他显示表面"));
        }
        Ok(())
    }
    fn reap(registry: &mut Registry) {
        let ids: Vec<_> = registry
            .connections
            .keys()
            .filter(|id| {
                !registry
                    .leases
                    .values()
                    .any(|lease| &lease.connection.id == *id)
                    && !registry.ptys.values().any(|pty| &pty.connection.id == *id)
            })
            .cloned()
            .collect();
        for id in ids {
            if let Some(connection) = registry.connections.remove(&id) {
                connection.stop();
            }
        }
    }
    /// Validate the entire batch before changing any input binding. Keeping the
    /// registry and all affected PTY locks excludes close and partial commits.
    pub fn bind_view_resources(&self, instance: &str, entries: &[Value]) -> Result<Value> {
        self.bind_owned_view_resources(&entries.iter().map(|e| (instance.to_owned(), e.clone())).collect::<Vec<_>>())
    }
    /// Host-only transaction: each view carries its trusted plugin instance.
    /// All owners and resources are checked before changing any binding.
    pub fn bind_owned_view_resources(&self, entries: &[(String, Value)]) -> Result<Value> {
        let registry = self.registry.lock().unwrap();
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        for (instance, entry) in entries {
            if registry.revoked.contains(instance) {
                return Err(CoreError::new("INSTANCE_EXPIRED", "插件实例已结束"));
            }
            let view = field(entry, "viewId")?;
            let workspace = field(entry, "workspaceId")?;
            let mount = number(entry, "mountGeneration")?;
            let resources = entry["resources"]
                .as_array()
                .ok_or_else(|| CoreError::new("INVALID_PARAMS", "资源引用无效"))?;
            for reference in resources {
                let id = field(reference, "id")?;
                match field(reference, "kind")? {
                    "pty" => {
                        let pty = registry
                            .ptys
                            .get(id)
                            .ok_or_else(|| CoreError::new("STALE_HANDLE", "PTY 已结束"))?;
                        if &pty.instance_id != instance
                            || pty.connection.workspace_id != workspace
                            || pty.generation != number(reference, "generation")?
                            || !seen.insert(id.to_string())
                        {
                            return Err(CoreError::new(
                                "STALE_HANDLE",
                                "资源归属、代次或重复绑定无效",
                            ));
                        }
                        pending.push((pty.clone(), view.to_string(), mount));
                    }
                    "lease" => {
                        let lease = registry
                            .leases
                            .get(id)
                            .ok_or_else(|| CoreError::new("STALE_HANDLE", "租约已释放"))?;
                        if &lease.instance_id != instance
                            || lease.connection.workspace_id != workspace
                        {
                            return Err(CoreError::new("STALE_HANDLE", "租约归属不匹配"));
                        }
                    }
                    _ => return Err(CoreError::new("UNSUPPORTED_API", "不支持的视图资源类型")),
                }
            }
        }
        pending.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        let mut states = pending
            .iter()
            .map(|(pty, _, _)| pty.state.lock().unwrap())
            .collect::<Vec<_>>();
        for ((_, view, mount), state) in pending.iter().zip(states.iter()) {
            if *mount==0 || state.view_id.as_deref().is_some_and(|id|id!=view) || state.mount_generation.is_some_and(|old| *mount < old) {
                return Err(CoreError::new("STALE_MOUNT", "挂载代次必须前进"));
            }
        }
        let mut cursors = serde_json::Map::new();
        for ((_, view, mount), state) in pending.iter().zip(states.iter_mut()) {
            state.view_id = Some(view.clone());
            state.mount_generation = Some(*mount);
            cursors.insert(
                view.clone(),
                json!({"acknowledged":state.acknowledged,"lastInputSeq":state.last_input}),
            );
        }
        Ok(Value::Object(cursors))
    }
    pub fn release_view_resources(&self, instance: &str, references: &[Value]) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        // Reject a bad later reference before releasing any earlier resource.
        for reference in references {
            let id=field(reference,"id")?;
            match field(reference,"kind")? {
                "pty"=>{let generation=number(reference,"generation")?;if registry.ptys.get(id).is_some_and(|p|p.instance_id!=instance||p.generation!=generation){return Err(CoreError::new("STALE_HANDLE","资源归属不匹配"));}}
                "lease"=>if registry.leases.get(id).is_some_and(|l|l.instance_id!=instance){return Err(CoreError::new("STALE_HANDLE","租约归属不匹配"));},
                _=>return Err(CoreError::new("UNSUPPORTED_API","不支持的资源类型")),
            }
        }
        // Never close a shared connection directly: release only this view's uses.
        for reference in references {
            let id = field(reference, "id")?;
            if reference["kind"] == "pty" {
                if let Some(pty) = registry.ptys.get(id) {
                    if pty.instance_id != instance
                        || pty.generation != number(reference, "generation")?
                    {
                        return Err(CoreError::new("STALE_HANDLE", "资源归属不匹配"));
                    }
                    pty.connection.close_pty(&pty.state);
                }
                registry.ptys.remove(id);
            } else if reference["kind"] == "lease" {
                if registry
                    .leases
                    .get(id)
                    .is_some_and(|l| l.instance_id != instance)
                {
                    return Err(CoreError::new("STALE_HANDLE", "租约归属不匹配"));
                }
                registry.leases.remove(id);
            } else {
                return Err(CoreError::new("UNSUPPORTED_API", "不支持的资源类型"));
            }
        }
        Self::reap(&mut registry);
        drop(registry);
        for reference in references {
            if reference["kind"] == "lease" { self.remote_release(field(reference,"id")?); }
        }
        Ok(())
    }
    pub fn resources(&self) -> Value {
        let remote = self.remote_resources();
        let registry = self.registry.lock().unwrap();
        json!({"remote":remote,"connections":registry.connections.values().map(|c|json!({"id":c.id,"serverId":c.server.id,"workspaceId":c.workspace_id,"generation":c.generation,"state":*c.status.lock().unwrap()})).collect::<Vec<_>>(),"ptys":registry.ptys.values().map(|p|{let state=p.state.lock().unwrap();json!({"id":p.id,"serverId":p.connection.server.id,"connectionId":p.connection.id,"generation":p.generation,"instanceId":p.instance_id,"state":state.state,"reason":state.reason,"bufferedBytes":state.buffered,"queuedInputBytes":state.queued_input,"viewId":state.view_id})}).collect::<Vec<_>>(),"leases":registry.leases.iter().map(|(id,l)|json!({"id":id,"connectionId":l.connection.id,"instanceId":l.instance_id})).collect::<Vec<_>>()})
    }
    fn stop_resource(&self, resource_id: &str) -> Result<Value> {
        let mut registry = self.registry.lock().unwrap();
        if let Some(connection) = registry.connections.remove(resource_id) {
            connection.stop();
            let affected: Vec<_> = registry
                .ptys
                .values()
                .filter(|p| p.connection.id == resource_id)
                .map(|p| p.id.clone())
                .collect();
            registry
                .leases
                .retain(|_, l| l.connection.id != resource_id);
            registry.ptys.retain(|_, p| p.connection.id != resource_id);
            return Ok(json!({"stopped":true,"affectedPtyIds":affected}));
        }
        if let Some(pty) = registry.ptys.remove(resource_id) {
            pty.connection.close_pty(&pty.state);
            Self::reap(&mut registry);
            return Ok(json!({"stopped":true,"affectedPtyIds":[resource_id]}));
        }
        Ok(json!({"stopped":true,"alreadyClosed":true}))
    }
    pub fn close_instance(&self, instance_id: &str) {
        self.network_stop_instance(instance_id);
        self.remote_stop_instance(instance_id);
        for ((instance, _), operation) in self.operations.lock().unwrap().iter() {
            if instance == instance_id {
                operation.canceled.store(true, Ordering::SeqCst);
            }
        }
        let mut registry = self.registry.lock().unwrap();
        registry.revoked.insert(instance_id.into());
        for (instance, canceled) in registry.pending_connections.values() {
            if instance == instance_id {
                canceled.store(true, Ordering::SeqCst);
            }
        }
        let ptys: Vec<_> = registry
            .ptys
            .values()
            .filter(|p| p.instance_id == instance_id)
            .cloned()
            .collect();
        for pty in ptys {
            pty.connection.close_pty(&pty.state);
            registry.ptys.remove(&pty.id);
        }
        registry.leases.retain(|_, l| l.instance_id != instance_id);
        Self::reap(&mut registry);
    }
    pub fn shutdown(&self) {
        self.network_stop();
        for operation in self.operations.lock().unwrap().values() {
            operation.canceled.store(true, Ordering::SeqCst);
        }
        let mut registry = self.registry.lock().unwrap();
        registry.shutdown_epoch = registry.shutdown_epoch.wrapping_add(1);
        for (_, canceled) in registry.pending_connections.values() {
            canceled.store(true, Ordering::SeqCst);
        }
        for connection in registry.connections.values() {
            connection.stop();
        }
        registry.ptys.clear();
        registry.leases.clear();
        registry.connections.clear();
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        if self.owns_lifetime { self.shutdown(); }
    }
}
impl std::ops::Deref for Core {
    type Target = CoreState;
    fn deref(&self) -> &Self::Target { &self.shared }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_admission_is_scoped_and_survives_log_failure() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        let caller = Caller::shell();
        core.store.fail_writes(true);
        core.call(
            &caller,
            "operations.cancel",
            json!({"operationId":"pre-canceled"}),
        )
        .unwrap();
        core.store.fail_writes(false);
        let params = json!({"operationId":"pre-canceled","serverId":"absent","workspaceId":"test"});
        assert_eq!(
            core.call(&caller, "connections.acquire", params.clone())
                .unwrap_err()
                .code,
            "CANCELED"
        );
        let other = Caller {
            instance_id: "other-instance".into(),
            ..caller.clone()
        };
        assert_eq!(
            core.call(&other, "connections.acquire", params)
                .unwrap_err()
                .code,
            "NOT_FOUND"
        );
        assert!(core.resources()["connections"]
            .as_array()
            .unwrap()
            .is_empty());
        let logs = core
            .call(
                &caller,
                "calls.list",
                json!({"method":"connections.acquire"}),
            )
            .unwrap();
        assert!(logs["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["errorCode"] == "CANCELED"));
    }

    #[test]
    fn log_failure_stops_new_work_but_cleanup_remains_available() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        let caller = Caller::shell();
        core.store.fail_writes(true);
        let error = core
            .call(
                &caller,
                "servers.create",
                json!({"name":"must-not-create","host":"127.0.0.1","username":"u"}),
            )
            .unwrap_err();
        assert_eq!(error.code, "LOG_UNAVAILABLE");
        assert!(core.store.servers().unwrap().is_empty());
        assert!(core
            .call(
                &caller,
                "resources.stop",
                json!({"resourceId":"already-closed"})
            )
            .is_ok());
        let diagnostics = core.call(&caller, "diagnostics.get", json!({})).unwrap();
        assert_eq!(diagnostics["logFaults"].as_array().unwrap().len(), 3);
        assert_eq!(diagnostics["logFaults"][2]["method"], "diagnostics.get");
        core.store.fail_writes(false);
        let error = core
            .record(&caller, "test.sideEffect", &json!({}), || {
                core.store.fail_writes(true);
                Ok(json!({"executed":true}))
            })
            .unwrap_err();
        assert_eq!(error.code, "RESULT_UNKNOWN");
        core.store.fail_writes(false);
        drop(core);
        let reopened = Core::open(directory.path()).unwrap();
        let logs = reopened
            .call(&caller, "calls.list", json!({"method":"test.sideEffect"}))
            .unwrap();
        assert_eq!(logs["records"][0]["status"], "interrupted");
    }
    #[test]
    fn persistence_validation_and_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        let caller = Caller::shell();
        assert!(core
            .call(
                &caller,
                "servers.create",
                json!({"name":"bad","host":"localhost","username":"u","port":0})
            )
            .is_err());
        let server = core
            .call(
                &caller,
                "servers.create",
                json!({"name":"local","host":"127.0.0.1","username":"tester","port":2222}),
            )
            .unwrap();
        assert_eq!(server["revision"], 1);
        assert!(core.call(&caller,"servers.update",json!({"serverId":server["id"],"expectedRevision":0,"name":"new","host":"127.0.0.1","username":"tester"})).is_err());
        drop(core);
        let core = Core::open(directory.path()).unwrap();
        assert_eq!(
            core.call(&caller, "servers.list", json!({})).unwrap()["servers"][0]["id"],
            server["id"]
        );
    }
    #[test]
    fn every_failure_is_logged_without_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        let caller = Caller::shell();
        let marker = "DO-NOT-LOG-SECRET-BODY";
        assert!(core
            .call(
                &caller,
                "not.a.method",
                json!({"password":marker,"bytes":marker})
            )
            .is_err());
        let logs = core.call(&caller, "calls.list", json!({})).unwrap();
        assert!(!logs.to_string().contains(marker));
        assert!(logs["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["errorCode"] == "METHOD_NOT_FOUND"));
    }
    #[test]
    fn revoked_instance_cannot_reuse_identity() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        let caller = Caller {
            instance_id: "terminated".into(),
            ..Caller::shell()
        };
        core.close_instance("terminated");
        assert_eq!(
            core.call(&caller, "servers.list", json!({}))
                .unwrap_err()
                .code,
            "INSTANCE_EXPIRED"
        );
    }
    #[test]
    fn plugin_settings_are_namespaced_without_capability_acl() {
        let directory = tempfile::tempdir().unwrap();
        let core = Core::open(directory.path()).unwrap();
        for plugin in ["a", "b"] {
            let caller = Caller {
                plugin_id: Some(plugin.into()),
                instance_id: plugin.into(),
                source: "plugin".into(),
                ..Caller::shell()
            };
            core.call(
                &caller,
                "settings.set",
                json!({"key":"fontSize","value":14}),
            )
            .unwrap();
            assert_eq!(
                core.call(&caller, "settings.get", json!({"key":"fontSize"}))
                    .unwrap()["value"],
                14
            );
            assert!(core.call(&caller, "resources.list", json!({})).is_ok());
        }
        let records = core
            .call(&Caller::shell(), "calls.list", json!({"pluginId":"a"}))
            .unwrap();
        let records = records["records"].as_array().unwrap();
        assert_eq!(records.len(), 3);
        assert!(records
            .iter()
            .all(|record| record["caller"]["pluginId"] == "a"));
    }
}
