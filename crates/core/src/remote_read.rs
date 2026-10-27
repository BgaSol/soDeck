use super::*;
use crate::ssh::remote::{ExecState, Job};
#[path = "remote_read_contract.generated.rs"]
mod generated;
fn contract(method: &str, kind: &str, value: &Value) -> Result<()> {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../contracts/remote-read-v1.schema.json"
        ))
        .expect("remote-read schema")
    });
    let name = schema[kind][method]
        .as_str()
        .ok_or_else(|| CoreError::new("UNSUPPORTED_API", "未知远端读取方法"))?;
    crate::contract::check(schema, &schema["$defs"][name], value, method).map_err(|e| {
        CoreError::new(
            if kind == "x-methods" {
                "INVALID_PARAMS"
            } else {
                "PROTOCOL_ERROR"
            },
            e,
        )
    })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceScope {
    pub workspace_id: String,
    pub workspace_generation: String,
}
struct Resource {
    instance: String,
    scope: WorkspaceScope,
    lease: String,
    connection: Arc<Connection>,
    alive: Arc<AtomicBool>,
    kind: String,
    exec: Option<Arc<Mutex<ExecState>>>,
    claimed: bool,
}
struct Receipt {
    method: String,
    params: Value,
    scope: WorkspaceScope,
    canceled: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<Value>>>>,
    completed_at: Option<Instant>,
    resource: Option<String>,
    lease: Option<String>,
    claimed: bool,
}
#[derive(Default)]
pub(crate) struct RemoteRegistry {
    scopes: HashMap<String, String>,
    leases: HashMap<String, (WorkspaceScope, Arc<AtomicBool>, String)>,
    resources: HashMap<String, Resource>,
    receipts: HashMap<(String, String), Receipt>,
    expired: HashMap<(String, String), Instant>,
    connection_states: HashMap<String, (String, u64)>,
    observations: HashMap<String, (String, u64)>,
    revision: u64,
}
impl RemoteRegistry {
    // A receipt is also the recovery owner when the caller loses its reply.
    // Only explicit acknowledgement or actual use transfers that ownership.
    fn claim(&mut self, key: &(String, String)) {
        if let Some(receipt) = self.receipts.get_mut(key) {
            receipt.claimed = true;
            if let Some(resource) = receipt
                .resource
                .as_ref()
                .and_then(|id| self.resources.get_mut(id))
            {
                resource.claimed = true;
            }
        }
    }
    fn claim_lease(&mut self, lease: &str) {
        for receipt in self.receipts.values_mut() {
            if receipt.lease.as_deref() == Some(lease) {
                receipt.claimed = true;
            }
        }
    }
    fn sweep(&mut self, registry: &mut Registry) {
        for receipt in self.receipts.values_mut() {
            if receipt.completed_at.is_none() && receipt.result.lock().unwrap().is_some() {
                receipt.completed_at = Some(Instant::now());
            }
        }
        let expired: Vec<_> = self
            .receipts
            .iter()
            .filter(|(_, r)| {
                r.completed_at
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(60))
                    && r.result.lock().unwrap().is_some()
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in expired {
            let receipt = self.receipts.remove(&key).unwrap();
            self.expired.insert(key, Instant::now());
            if let Some(id) = receipt.resource {
                if self.resources.get(&id).is_some_and(|r| !r.claimed) {
                    if let Some(resource) = self.resources.remove(&id) {
                        resource.alive.store(false, Ordering::SeqCst);
                    }
                }
            }
            if !receipt.claimed {
                if let Some(id) = receipt.lease {
                    if let Some((_, alive, _)) = self.leases.remove(&id) {
                        alive.store(false, Ordering::SeqCst);
                    }
                    registry.leases.remove(&id);
                }
            }
        }
        self.resources.retain(|_, r| r.alive.load(Ordering::SeqCst));
        self.expired
            .retain(|_, t| t.elapsed() < Duration::from_secs(600));
        Core::reap(registry);
    }
}
impl Core {
    fn remote_token(&self, caller: &Caller, operation: &str) -> Result<Arc<AtomicBool>> {
        let key = (caller.instance_id.clone(), operation.to_string());
        let mut operations = self.operations.lock().unwrap();
        Self::prune_operations(&mut operations);
        if !operations.contains_key(&key) && operations.len() >= 10000 {
            return Err(CoreError::new("LIMIT_EXCEEDED", "操作记录已满"));
        }
        let operation = operations.entry(key).or_insert_with(|| Operation {
            canceled: Arc::new(AtomicBool::new(false)),
            active: 0,
            touched: Instant::now(),
        });
        if operation.canceled.load(Ordering::SeqCst) {
            return Err(CoreError::new("CANCELED", "读取已取消"));
        }
        operation.touched = Instant::now();
        Ok(operation.canceled.clone())
    }
    pub(crate) fn remote_resources(&self) -> Value {
        let remote = self.remote.lock().unwrap();
        let registry = self.registry.lock().unwrap();
        json!({"handles":remote.resources.iter().map(|(id,r)|json!({"id":id,"kind":r.kind,"instanceId":r.instance,"scope":r.scope,"leaseId":r.lease,"connectionId":r.connection.id,"claimed":r.claimed,"revoked":!r.alive.load(Ordering::SeqCst)})).collect::<Vec<_>>(),"receipts":remote.receipts.len(),"connections":registry.connections.values().map(|c|json!({"connectionId":c.id,"io":*c.remote_metrics.lock().unwrap()})).collect::<Vec<_>>()})
    }
    pub(crate) fn start_remote_maintenance(&self) {
        let remote = Arc::downgrade(&self.remote);
        let starts = Arc::downgrade(&self.pty_starts);
        let registry = Arc::downgrade(&self.registry);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let (Some(remote), Some(registry)) = (remote.upgrade(), registry.upgrade()) else {
                break;
            };
            if let Some(starts)=starts.upgrade(){pty_start::sweep(&mut starts.lock().unwrap(),&mut registry.lock().unwrap());}
            remote.lock().unwrap().sweep(&mut registry.lock().unwrap());
        });
    }
    pub(crate) fn connection_state(&self, key: &str, state: &str) {
        let mut r = self.remote.lock().unwrap();
        if r.connection_states.len()>=512&&!r.connection_states.contains_key(key){
            if let Some(old)=r.connection_states.iter().min_by_key(|(_,v)|v.1).map(|(k,_)|k.clone()){r.connection_states.remove(&old);}
        }
        r.revision += 1;
        let revision = r.revision;
        r.connection_states
            .insert(key.into(), (state.into(), revision));
    }
    pub fn register_workspace(&self, id: &str, generation: &str) {
        self.network_scope(id, generation, true);
        self.remote
            .lock()
            .unwrap()
            .scopes
            .insert(id.into(), generation.into());
    }
    pub fn revoke_workspace(&self, id: &str, generation: &str) {
        self.network_scope(id, generation, false);
        let mut remote = self.remote.lock().unwrap();
        if remote.scopes.get(id).is_some_and(|g| g == generation) {
            remote.scopes.remove(id);
        }
        let leases = remote
            .leases
            .iter()
            .filter(|(_, (scope, _, _))| {
                scope.workspace_id == id && scope.workspace_generation == generation
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for lease in &leases {
            if let Some((_, alive, _)) = remote.leases.remove(lease) {
                alive.store(false, Ordering::SeqCst);
            }
        }
        for r in remote.receipts.values() {
            if r.scope.workspace_id == id && r.scope.workspace_generation == generation {
                r.canceled.store(true, Ordering::SeqCst);
            }
        }
        remote.receipts.retain(|_, r| {
            r.scope.workspace_id != id || r.scope.workspace_generation != generation
        });
        remote.resources.retain(|_, r| {
            if r.scope.workspace_id == id && r.scope.workspace_generation == generation {
                r.alive.store(false, Ordering::SeqCst);
                false
            } else {
                true
            }
        });
        drop(remote);
        let mut registry = self.registry.lock().unwrap();
        for lease in leases {
            registry.leases.remove(&lease);
        }
        Self::reap(&mut registry);
    }
    fn scope(&self, p: &Value) -> Result<WorkspaceScope> {
        let s: WorkspaceScope = serde_json::from_value(p["scope"].clone())?;
        if self.remote.lock().unwrap().scopes.get(&s.workspace_id) != Some(&s.workspace_generation)
        {
            return Err(CoreError::new("STALE_WORKSPACE", "工作区创建身份已失效"));
        }
        Ok(s)
    }
    pub(crate) fn remote_release(&self, lease: &str) {
        let mut remote = self.remote.lock().unwrap();
        if let Some((_, alive, _)) = remote.leases.remove(lease) {
            alive.store(false, Ordering::SeqCst);
        }
        remote.resources.retain(|_, r| {
            if r.lease == lease {
                r.alive.store(false, Ordering::SeqCst);
                false
            } else {
                true
            }
        });
    }
    pub(crate) fn remote_stop_instance(&self, instance: &str) {
        let mut remote = self.remote.lock().unwrap();
        remote.leases.retain(|_, (_, alive, owner)| {
            if owner == instance {
                alive.store(false, Ordering::SeqCst);
                false
            } else {
                true
            }
        });
        for ((owner, _), r) in &remote.receipts {
            if owner == instance {
                r.canceled.store(true, Ordering::SeqCst);
            }
        }
        remote.receipts.retain(|(owner, _), _| owner != instance);
        remote.resources.retain(|_, r| {
            if r.instance == instance {
                r.alive.store(false, Ordering::SeqCst);
                false
            } else {
                true
            }
        });
    }
    pub(crate) fn remote_dispatch(
        &self,
        caller: &Caller,
        method: &str,
        p: &Value,
    ) -> Result<Value> {
        contract(method, "x-methods", p)?;
        self.remote
            .lock()
            .unwrap()
            .sweep(&mut self.registry.lock().unwrap());
        let value = self.remote_execute(caller, method, p)?;
        contract(method, "x-results", &value)?;
        Ok(value)
    }
    fn remote_execute(&self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        if method == "connections.observe" || method == "connections.acquireExisting" {
            return self.connection_access(caller, method, p);
        }
        if method == "remote.operations.get" || method == "remote.operations.forget" {
            let key = (
                caller.instance_id.clone(),
                field(p, "operationId")?.to_owned(),
            );
            let mut remote = self.remote.lock().unwrap();
            let r = remote
                .receipts
                .get(&key)
                .ok_or_else(|| CoreError::new("RESULT_EXPIRED", "操作回执不可用"))?;
            let result = r.result.lock().unwrap().clone();
            if method.ends_with("forget") {
                if result.is_none() {
                    return Err(CoreError::new("BUSY", "操作尚未完成"));
                }
                remote.claim(&key);
                remote.receipts.remove(&key);
                return Ok(json!({"forgotten":true}));
            }
            if result.as_ref().is_some_and(|r| r.is_ok()) {
                remote.claim(&key);
            }
            return Ok(match result {
                None => json!({"state":"running"}),
                Some(Ok(value)) => json!({"state":"completed","result":value}),
                Some(Err(error)) => {
                    json!({"state":if error.code=="CANCELED"{"canceled"}else{"failed"},"error":error})
                }
            });
        }
        let scope = self.scope(p)?;
        let id_field = if method.starts_with("exec.") {
            "execId"
        } else if method.contains("Directory") {
            "directoryId"
        } else {
            "fileId"
        };
        if ["exec.read", "exec.ack", "exec.cancel", "exec.close"].contains(&method) {
            let mut remote = self.remote.lock().unwrap();
            let id = field(p, id_field)?;
            let Some(r) = remote.resources.get_mut(id) else {
                if method == "exec.close" {
                    return Ok(json!({"closed":true}));
                }
                return Err(CoreError::new("STALE_HANDLE", "执行句柄已结束"));
            };
            if r.instance != caller.instance_id
                || r.scope != scope
                || p["connectionGeneration"].as_u64() != Some(r.connection.generation)
                || p["leaseId"].as_str() != Some(r.lease.as_str())
            {
                return Err(CoreError::new("STALE_HANDLE", "执行句柄归属不匹配"));
            }
            let exec = r.exec.clone().unwrap();
            r.claimed = true;
            if method == "exec.read" {
                return exec.lock().unwrap().read(
                    p["afterSequence"].as_u64().unwrap_or(0),
                    p["maxBytes"].as_u64().unwrap_or(65536).min(65536) as usize,
                );
            }
            if method == "exec.ack" {
                return exec.lock().unwrap().acknowledge(number(p, "sequence")?);
            }
            exec.lock().unwrap().canceled = true;
            if method == "exec.close" {
                r.alive.store(false, Ordering::SeqCst);
                remote.resources.remove(id);
            }
            return Ok(json!({"closed":true}));
        }
        if method == "files.close" || method == "files.closeDirectory" {
            let mut remote = self.remote.lock().unwrap();
            let id = field(p, id_field)?;
            if let Some(r) = remote.resources.get(id) {
                if r.instance != caller.instance_id
                    || r.scope != scope
                    || p["connectionGeneration"].as_u64() != Some(r.connection.generation)
                    || p["leaseId"].as_str() != Some(r.lease.as_str())
                {
                    return Err(CoreError::new("STALE_HANDLE", "资源归属不匹配"));
                }
                r.alive.store(false, Ordering::SeqCst);
                remote.resources.remove(id);
            }
            return Ok(json!({"closed":true}));
        }
        validate_params(method, p)?;
        let admission = self.remote_admission.lock().unwrap();
        let operation_id = field(p, "operationId")?.to_owned();
        let key = (caller.instance_id.clone(), operation_id.clone());
        if self.remote.lock().unwrap().expired.contains_key(&key) {
            return Err(CoreError::new(
                "RESULT_EXPIRED",
                "操作回执已过期，请使用新的操作 ID",
            ));
        }
        let existing = {
            let remote = self.remote.lock().unwrap();
            remote
                .receipts
                .get(&key)
                .map(|r| {
                    if r.method != method || r.params != *p {
                        Err(CoreError::new(
                            "REQUEST_CONFLICT",
                            "相同操作 ID 的参数不一致",
                        ))
                    } else {
                        Ok(r.result.clone())
                    }
                })
                .transpose()?
        };
        if let Some(result) = existing {
            drop(admission);
            return self.wait_remote(caller, &operation_id, &result);
        }
        let canceled = self.remote_token(caller, &operation_id)?;
        let ticket = crate::ssh::remote::IoTicket::reserve(self.remote_budget.clone())?;
        let (lease_id, connection, lease_alive, resource_id, resource_alive, exec) = {
            let mut remote = self.remote.lock().unwrap();
            remote
                .resources
                .retain(|_, r| r.alive.load(Ordering::SeqCst));
            let scoped_running = remote
                .receipts
                .values()
                .filter(|r| r.scope == scope && r.result.lock().unwrap().is_none())
                .count();
            let count = remote
                .receipts
                .keys()
                .filter(|(owner, _)| owner == &caller.instance_id)
                .count();
            let bytes: usize = remote
                .receipts
                .iter()
                .filter(|((owner, _), _)| owner == &caller.instance_id)
                .filter_map(|(_, r)| {
                    r.result.lock().unwrap().as_ref().map(|v| match v {
                        Ok(v) => v.to_string().len(),
                        Err(_) => 128,
                    })
                })
                .sum();
            if scoped_running >= 4 || count >= 128 || bytes >= 4 * 1024 * 1024 {
                return Err(CoreError::new(
                    "LIMIT_EXCEEDED",
                    "远端读取容量已满，请稍后重试",
                ));
            }
            let resource = if method == "files.readDirectory" || method == "files.read" {
                let id = field(p, id_field)?;
                let r = remote
                    .resources
                    .get(id)
                    .ok_or_else(|| CoreError::new("STALE_HANDLE", "资源已释放"))?;
                if r.instance != caller.instance_id
                    || r.scope != scope
                    || !r.alive.load(Ordering::SeqCst)
                {
                    return Err(CoreError::new("STALE_HANDLE", "资源归属或代次不匹配"));
                }
                if p["leaseId"].as_str() != Some(r.lease.as_str()) {
                    return Err(CoreError::new("STALE_HANDLE", "租约与资源不匹配"));
                }
                Some((r.lease.clone(), r.connection.clone()))
            } else {
                None
            };
            let (lease_id, connection) = if let Some(value) = resource {
                value
            } else {
                let lease_id = field(p, "leaseId")?.to_owned();
                let registry = self.registry.lock().unwrap();
                let lease = registry
                    .leases
                    .get(&lease_id)
                    .ok_or_else(|| CoreError::new("STALE_HANDLE", "连接租约已结束"))?;
                if lease.instance_id != caller.instance_id
                    || lease.connection.workspace_id != scope.workspace_id
                {
                    return Err(CoreError::new("STALE_HANDLE", "租约归属不匹配"));
                }
                (lease_id, lease.connection.clone())
            };
            if p["connectionGeneration"].as_u64() != Some(connection.generation)
                || *connection.status.lock().unwrap() != "active"
            {
                return Err(CoreError::new("CONNECTION_LOST", "连接代次已失效"));
            }
            let lease_alive = remote.leases.entry(lease_id.clone()).or_insert_with(|| {
                (
                    scope.clone(),
                    Arc::new(AtomicBool::new(true)),
                    caller.instance_id.clone(),
                )
            });
            if lease_alive.0 != scope || !lease_alive.1.load(Ordering::SeqCst) {
                return Err(CoreError::new("STALE_WORKSPACE", "租约创建身份已失效"));
            }
            let lease_alive = lease_alive.1.clone();
            remote.claim_lease(&lease_id);
            let creating =
                ["files.openDirectory", "files.openRead", "exec.start"].contains(&method);
            let resource_id = creating.then(id);
            let alive = remote
                .resources
                .get_mut(p[id_field].as_str().unwrap_or(""))
                .map(|r| {
                    r.claimed = true;
                    r.alive.clone()
                })
                .unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
            let exec = (method == "exec.start").then(|| Arc::new(Mutex::new(ExecState::default())));
            if let Some(id) = &resource_id {
                let kind = if method == "exec.start" {
                    "exec"
                } else {
                    "file"
                };
                let max = if kind == "exec" { 2 } else { 8 };
                if remote
                    .resources
                    .values()
                    .filter(|r| {
                        r.scope == scope && r.kind == kind && r.alive.load(Ordering::SeqCst)
                    })
                    .count()
                    >= max
                {
                    return Err(CoreError::new("LIMIT_EXCEEDED", "打开的远端句柄过多"));
                }
                remote.resources.insert(
                    id.clone(),
                    Resource {
                        instance: caller.instance_id.clone(),
                        scope: scope.clone(),
                        lease: lease_id.clone(),
                        connection: connection.clone(),
                        alive: alive.clone(),
                        kind: kind.into(),
                        exec: exec.clone(),
                        claimed: false,
                    },
                );
            }
            (lease_id, connection, lease_alive, resource_id, alive, exec)
        };
        let mut params = p.clone();
        if let Some(id) = &resource_id {
            params["resourceId"] = json!(id);
        }
        let mut job = Job::new(
            method,
            params,
            lease_id,
            lease_alive,
            canceled.clone(),
            ticket,
        );
        job.exec = exec;
        job.resource_alive = Some(resource_alive.clone());
        let result = job.result.clone();
        {
            let mut remote = self.remote.lock().unwrap();
            if remote.scopes.get(&scope.workspace_id) != Some(&scope.workspace_generation) {
                resource_alive.store(false, Ordering::SeqCst);
                return Err(CoreError::new("STALE_WORKSPACE", "工作区已关闭"));
            }
            remote.receipts.insert(
                key,
                Receipt {
                    method: method.into(),
                    params: p.clone(),
                    scope,
                    canceled,
                    result: result.clone(),
                    completed_at: None,
                    resource: resource_id.clone(),
                    lease: None,
                    claimed: false,
                },
            );
        }
        if let Err(error) = connection.remote(job) {
            *result.lock().unwrap() = Some(Err(error));
        }
        drop(admission);
        let answer = self.wait_remote(caller, &operation_id, &result);
        if answer.as_ref().is_err_and(|e| e.code != "RESULT_UNKNOWN") {
            resource_alive.store(false, Ordering::SeqCst);
            if let Some(id) = &resource_id {
                self.remote.lock().unwrap().resources.remove(id);
            }
        }
        if method == "files.readDirectory" && answer.as_ref().is_ok_and(|v| v["eof"] == true) {
            self.remote
                .lock()
                .unwrap()
                .resources
                .remove(p["directoryId"].as_str().unwrap());
        }
        answer
    }
    fn wait_remote(
        &self,
        caller: &Caller,
        operation: &str,
        result: &Arc<Mutex<Option<Result<Value>>>>,
    ) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(31);
        loop {
            if let Some(result) = result.lock().unwrap().clone() {
                return result;
            }
            if self
                .operations
                .lock()
                .unwrap()
                .get(&(caller.instance_id.clone(), operation.into()))
                .is_some_and(|o| o.canceled.load(Ordering::SeqCst))
            {
                let error = CoreError::new("CANCELED", "读取已取消");
                *result.lock().unwrap() = Some(Err(error.clone()));
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(
                    CoreError::new("RESULT_UNKNOWN", "读取结果尚未交付，请查询操作状态")
                        .details(json!({"operationId":operation})),
                );
            }
            if self
                .registry
                .lock()
                .unwrap()
                .revoked
                .contains(&caller.instance_id)
            {
                return Err(CoreError::new("INSTANCE_EXPIRED", "插件实例已结束"));
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    fn connection_access(&self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        let deadline =
            Instant::now() + Duration::from_millis(p["waitMs"].as_u64().unwrap_or(0).min(1000));
        loop {
            let scope = self.scope(p)?;
            let config_guard=self.configuration.lock().unwrap();
            let plan=self.route_plan(field(p,"serverId")?)?;
            let server=plan.target();
            if p["configRevision"].as_u64()!=Some(server.revision){return Err(CoreError::new("REVISION_CONFLICT","服务器配置已变化"));}
            let key=format!("{}:{}:{}",server.id,plan.revision,scope.workspace_id);
            // Global lock order is remote -> registry. Acquisition and scope
            // revocation cannot race between matching a socket and adding a lease.
            let mut remote = self.remote.lock().unwrap();
            let mut registry = self.registry.lock().unwrap();
            if remote.scopes.get(&scope.workspace_id) != Some(&scope.workspace_generation)
                || registry.revoked.contains(&caller.instance_id)
            {
                return Err(CoreError::new("STALE_WORKSPACE", "工作区或插件已结束"));
            }
            let connection = registry
                .connections
                .values()
                .find(|c| {
                    c.server.id == server.id
                        && c.purpose == "session"
                        && c.workspace_id == scope.workspace_id
                        && c.server.revision == server.revision
                        && c.route.revision == plan.revision
                        && *c.status.lock().unwrap() == "active"
                })
                .cloned();
            if method == "connections.observe" {
                let state = if connection.is_some() {
                    "active"
                } else {
                    remote
                        .connection_states
                        .get(&key)
                        .map(|s| s.0.as_str())
                        .filter(|s| *s != "active")
                        .unwrap_or("idle")
                }
                .to_owned();
                let fingerprint = format!(
                    "{}:{}",
                    state,
                    connection.as_ref().map(|c| c.generation).unwrap_or(0)
                );
                if remote
                    .observations
                    .get(&key)
                    .is_none_or(|s| s.0 != fingerprint)
                {
                    if remote.observations.len()>=512&&!remote.observations.contains_key(&key){
                        if let Some(old)=remote.observations.iter().min_by_key(|(_,v)|v.1).map(|(k,_)|k.clone()){remote.observations.remove(&old);}
                    }
                    remote.revision += 1;
                    let revision = remote.revision;
                    remote
                        .observations
                        .insert(key.clone(), (fingerprint, revision));
                }
                let revision = remote.observations[&key].1;
                let result = json!({"state":state,"revision":revision,"connection":connection.as_ref().map(|c|json!({"connectionId":c.id,"generation":c.generation}))});
                if p["afterRevision"].as_u64() != Some(revision) || Instant::now() >= deadline {
                    return Ok(result);
                }
                drop(registry);
                drop(remote);
                drop(config_guard);
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let op_key = (
                caller.instance_id.clone(),
                field(p, "operationId")?.to_owned(),
            );
            if remote.expired.contains_key(&op_key) {
                return Err(CoreError::new(
                    "RESULT_EXPIRED",
                    "操作回执已过期，请使用新的操作 ID",
                ));
            }
            if let Some(receipt) = remote.receipts.get(&op_key) {
                if receipt.method != method || receipt.params != *p {
                    return Err(CoreError::new("REQUEST_CONFLICT", "操作标识已用于其他参数"));
                }
                return receipt
                    .result
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| Err(CoreError::new("RESULT_UNKNOWN", "操作正在处理")));
            }
            if remote
                .receipts
                .keys()
                .filter(|(owner, _)| owner == &caller.instance_id)
                .count()
                >= 128
            {
                return Err(CoreError::new("LIMIT_EXCEEDED", "操作回执已满"));
            }
            let canceled = self.remote_token(caller, field(p, "operationId")?)?;
            if canceled.load(Ordering::SeqCst) {
                return Err(CoreError::new("CANCELED", "仅复用连接操作已取消"));
            }
            let connection = connection
                .ok_or_else(|| CoreError::new("CONNECTION_NOT_READY", "当前没有可复用连接"))?;
            if registry.leases.len()>=1024{return Err(CoreError::new("LIMIT_EXCEEDED","连接使用关系已达到上限"));}
            let lease_id = id();
            registry.leases.insert(
                lease_id.clone(),
                Lease {
                    connection: connection.clone(),
                    instance_id: caller.instance_id.clone(),
                },
            );
            remote.leases.insert(
                lease_id.clone(),
                (
                    scope.clone(),
                    Arc::new(AtomicBool::new(true)),
                    caller.instance_id.clone(),
                ),
            );
            let result = json!({"leaseId":lease_id,"connectionId":connection.id,"generation":connection.generation});
            remote.receipts.insert(
                op_key,
                Receipt {
                    method: method.into(),
                    params: p.clone(),
                    scope,
                    canceled,
                    result: Arc::new(Mutex::new(Some(Ok(result.clone())))),
                    completed_at: None,
                    resource: None,
                    lease: Some(lease_id),
                    claimed: false,
                },
            );
            return Ok(result);
        }
    }
}
fn validate_params(method: &str, p: &Value) -> Result<()> {
    if p["operationId"]
        .as_str()
        .is_none_or(|s| s.is_empty() || s.len() > 256)
    {
        return Err(CoreError::new("INVALID_PARAMS", "操作 ID 无效"));
    }
    if let Some(path) = p.get("path") {
        let bytes = STANDARD
            .decode(path["bytesBase64"].as_str().unwrap_or(""))
            .map_err(|_| CoreError::new("INVALID_PARAMS", "路径编码无效"))?;
        if bytes.is_empty() || bytes.len() > 16384 || bytes.contains(&0) || bytes[0] != b'/' {
            return Err(CoreError::new("INVALID_PARAMS", "需要不含 NUL 的绝对路径"));
        }
    }
    for (name, max) in [
        ("maxEntries", 256),
        ("maxBytes", 65536),
        ("timeoutMs", 30000),
        ("maxOutputBytes", 4 * 1024 * 1024),
    ] {
        if let Some(v) = p.get(name) {
            if v.as_u64().is_none_or(|n| n == 0 || n > max) {
                return Err(CoreError::new("INVALID_PARAMS", format!("{name} 超出范围")));
            }
        }
    }
    if method == "exec.start" {
        if p["command"]
            .as_str()
            .is_none_or(|s| s.is_empty() || s.len() > 65536 || s.contains('\0'))
        {
            return Err(CoreError::new("INVALID_PARAMS", "执行命令无效"));
        }
        if let Some(s) = p.get("stdinBase64") {
            if STANDARD
                .decode(s.as_str().unwrap_or("!"))
                .map_or(true, |b| b.len() > 65536)
            {
                return Err(CoreError::new("INVALID_PARAMS", "执行输入无效"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_contract_examples_match_the_sdk() {
        let examples: Value = serde_json::from_str(include_str!(
            "../../../contracts/remote-read-v1.examples.json"
        ))
        .unwrap();
        for example in examples.as_array().unwrap() {
            assert_eq!(
                contract(
                    example["method"].as_str().unwrap(),
                    example["kind"].as_str().unwrap(),
                    &example["value"]
                )
                .is_ok(),
                example["valid"].as_bool().unwrap(),
                "{}",
                example["name"]
            );
        }
    }

    #[test]
    fn receipt_expiry_releases_unclaimed_lease_but_keeps_acknowledged_use() {
        let mut remote = RemoteRegistry::default();
        let mut registry = Registry::default();
        let scope = WorkspaceScope {
            workspace_id: "w".into(),
            workspace_generation: "g".into(),
        };
        let orphan = Arc::new(AtomicBool::new(true));
        let owned = Arc::new(AtomicBool::new(true));
        for (name, alive) in [("orphan", orphan.clone()), ("owned", owned.clone())] {
            remote
                .leases
                .insert(name.into(), (scope.clone(), alive, "owner".into()));
            remote.receipts.insert(
                ("owner".into(), name.into()),
                Receipt {
                    method: "connections.acquireExisting".into(),
                    params: json!({}),
                    scope: scope.clone(),
                    canceled: Arc::new(AtomicBool::new(false)),
                    result: Arc::new(Mutex::new(Some(Ok(json!({"leaseId":name}))))),
                    completed_at: Some(Instant::now() - Duration::from_secs(61)),
                    resource: None,
                    lease: Some(name.into()),
                    claimed: false,
                },
            );
        }
        remote.claim(&("owner".into(), "owned".into()));
        remote.sweep(&mut registry);
        assert!(!orphan.load(Ordering::SeqCst));
        assert!(owned.load(Ordering::SeqCst));
        assert_eq!(remote.leases.len(), 1);
        assert!(remote.receipts.is_empty());
        assert_eq!(remote.expired.len(), 2);
    }

    #[test]
    fn strict_contract_rejects_wrong_scope_extra_fields_and_invalid_paths() {
        let p = json!({"scope":{"workspaceId":"w","workspaceGeneration":"g"},"leaseId":"l","connectionGeneration":1,"operationId":"o","path":{"bytesBase64":"Lw=="}});
        assert!(contract("files.stat", "x-methods", &p).is_ok());
        let mut bad = p.clone();
        bad["scope"]["workspaceGeneration"] = json!(1);
        assert!(contract("files.stat", "x-methods", &bad).is_err());
        bad = p.clone();
        bad["pluginId"] = json!("spoof");
        assert!(contract("files.stat", "x-methods", &bad).is_err());
        for bytes in [b"relative".as_slice(), b"/nul\0"] {
            bad = p.clone();
            bad["path"]["bytesBase64"] = json!(STANDARD.encode(bytes));
            assert!(validate_params("files.stat", &bad).is_err());
        }
    }
}
