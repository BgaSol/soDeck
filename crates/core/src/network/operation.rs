use super::*;

impl Core {
    pub(crate) fn network_diagnostic_end(&self, instance: &str, operation: &str) -> Option<&'static str> {
        let n = self.network.lock().unwrap();
        let Some(r) = n.receipts.get(&(instance.into(), operation.into())) else { return Some("HOST_CHALLENGE_EXPIRED") };
        if r.canceled.load(Ordering::SeqCst)
            || r.group.as_ref().is_none_or(|id| n.groups.get(id).is_none_or(|g| g.closed.is_some()))
            || r.scope.as_ref().is_none_or(|s| n.scopes.get(&s.workspace_id) != Some(&s.workspace_generation)) {
            Some("CANCELED")
        } else { None }
    }
    pub(crate) fn network_challenge_live(&self, instance: &str, operation: &str) -> bool {
        let n = self.network.lock().unwrap();
        n.receipts.get(&(instance.into(),operation.into())).is_some_and(|r|
            !r.canceled.load(Ordering::SeqCst) && r.group.as_ref().is_some_and(|id| n.groups.get(id).is_some_and(|g|g.closed.is_none()))
            && r.scope.as_ref().is_some_and(|s|n.scopes.get(&s.workspace_id)==Some(&s.workspace_generation)))
    }
}

impl Receipt {
    fn value(&self, operation: &str) -> Value {
        let mut value = json!({"operationId":operation,"claimed":self.claimed,"state":"pending"});
        match &self.outcome {
            Some(Ok(result)) => { value["state"] = json!("succeeded"); value["result"] = result.clone(); }
            Some(Err(error)) => { value["state"] = json!(if error.code == "CANCELED" { "canceled" } else { "failed" }); value["error"] = json!(error); }
            None => {},
        }
        value
    }
}
impl Registry {
    pub(super) fn receipt(&self, key: &Key) -> Result<Value> {
        self.receipts.get(key).map(|r| r.value(&key.1)).ok_or_else(|| CoreError::new("OPERATION_EXPIRED", "操作回执不存在或已过期"))
    }
    pub(super) fn begin(&mut self, caller: &Caller, method: &str, p: &Value, group: Option<String>) -> Result<(Key, bool)> {
        let key = (caller.instance_id.clone(), p["operationId"].as_str().ok_or_else(stale)?.into());
        if let Some(r) = self.receipts.get(&key) {
            if r.method != method || r.params != *p { return Err(CoreError::new("OPERATION_CONFLICT", "同一操作标识不能用于不同参数")); }
            return Ok((key, false));
        }
        if self.tombstones.contains_key(&key) { return Err(CoreError::new("OPERATION_EXPIRED", "操作标识已过期，不能重复执行")); }
        let cleanup = method.ends_with(".close") || method.ends_with(".release") || method == "net.resources.stop";
        let limit = if cleanup { 4096 } else { 4000 };
        let per_instance = if cleanup { 512 } else { 480 };
        if self.receipts.len() + self.tombstones.len() >= limit || self.receipts.keys().chain(self.tombstones.keys()).filter(|k| k.0 == caller.instance_id).count() >= per_instance { return Err(capacity()); }
        self.receipts.insert(key.clone(), Receipt { diagnostic:crate::observability::DiagnosticScope::capture(),caller:caller.clone(), method: method.into(), params: p.clone(),
            scope: p.get("scope").map(|s| serde_json::from_value(s.clone())).transpose()?, group,
            canceled: Arc::new(AtomicBool::new(false)), outcome: None, completed: None, claimed: false, resources: Vec::new() });
        Ok((key, true))
    }
    pub(super) fn complete(&mut self, key: &Key, result: Result<Value>, resources: Vec<String>) {
        let Some(r) = self.receipts.get(key) else {
            for id in resources { self.close_resource(&id); } return;
        };
        if r.outcome.is_some() { for id in resources { self.close_resource(&id); } return; }
        let canceled = r.canceled.load(Ordering::SeqCst);
        let failed = canceled || result.is_err();
        let receipt = self.receipts.get_mut(key).unwrap();
        receipt.completed = Some(Instant::now());
        receipt.outcome = Some(if canceled { Err(CoreError::new("CANCELED", "网络操作已取消")) } else { result });
        if let Some(store)=self.observation.upgrade(){
            let mut targets=Vec::new();
            for name in ["group","route","listener","stream","resource"]{if let Ok(handle)=Handle::parse(&receipt.params[name]){targets.push((handle.id,handle.generation));}}
            if let Some(Ok(value))=&receipt.outcome{for name in ["group","route","listener","stream","splice"]{if let Ok(handle)=Handle::parse(&value[name]){targets.push((handle.id,handle.generation));}}}
            let (caller,method,diagnostic,outcome)=(receipt.caller.clone(),receipt.method.clone(),receipt.diagnostic.clone(),receipt.outcome.clone().unwrap());
            if let Some(group)=receipt.group.as_ref(){if let Some(route)=self.routes.values().find(|r|&r.group==group){if let Some(pool)=self.pools.get(&route.pool){for member in pool.members.iter().take(2){targets.push((member.connection.id.clone(),member.connection.generation));}}}}
            store.network_completed(&caller,&method,&key.1,&outcome,targets,diagnostic);
        }
        if !failed{if let Some(store)=self.observation.upgrade(){let caller=self.receipts[key].caller.clone();for id in &resources{if self.observed.contains_key(id){continue;}let handle=self.groups.get(id).map(|v|&v.handle).or_else(||self.listeners.get(id).map(|v|&v.handle)).or_else(||self.routes.get(id).map(|v|&v.handle)).or_else(||self.endpoints.get(id).map(|v|&v.handle)).or_else(||self.splices.get(id).map(|v|&v.handle));if let Some(handle)=handle{self.observed.insert(id.clone(),crate::observability::resource::ResourceObservation::new(&store,&caller,id,Some(handle.generation),None,"network.resource"));}}}}
        let receipt=self.receipts.get_mut(key).unwrap();
        if !failed { receipt.resources = resources.clone(); }
        if failed { for id in resources { self.close_resource(&id); } }
    }
    pub(super) fn claim_resource(&mut self, id: &str) {
        for r in self.receipts.values_mut() { if r.resources.iter().any(|v| v == id) { r.claimed = true; } }
    }
    pub(super) fn operation(&mut self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        let scope = p.get("scope").map(|s|self.check_scope(s)).transpose()?;
        let key = (caller.instance_id.clone(), p["operationId"].as_str().unwrap().to_owned());
        let receipt = self.receipts.get_mut(&key).ok_or_else(|| CoreError::new("OPERATION_EXPIRED", "操作回执不存在或已过期"))?;
        if receipt.scope != scope
            || p.get("group").is_some_and(|g| receipt.params.get("group") != Some(g)) { return Err(stale()); }
        if method == "net.operations.claim" {
            receipt.claimed = true;
            for id in &receipt.resources { if let Some(e) = self.endpoints.get_mut(id) { e.delivered = true; } }
        }
        if method == "net.operations.cancel" {
            receipt.canceled.store(true, Ordering::SeqCst);
            let resources = receipt.resources.clone();
            if receipt.outcome.is_none() {
                self.complete(&key,Err(CoreError::new("CANCELED", "网络操作已取消")),Vec::new());
            }
            for id in resources { self.close_resource(&id); }
        }
        self.receipt(&key)
    }
    pub(super) fn sweep_receipts(&mut self) {
        let unclaimed: Vec<_> = self.receipts.values_mut().filter(|r| !r.claimed && r.completed.is_some_and(|t| t.elapsed() >= Duration::from_secs(30)))
            .flat_map(|r| { r.claimed = true; std::mem::take(&mut r.resources) }).collect();
        for id in unclaimed { self.close_resource(&id); }
        let expired: Vec<_> = self.receipts.iter().filter(|(_, r)| r.completed.is_some_and(|t| t.elapsed() >= Duration::from_secs(60))).map(|(k, _)| k.clone()).collect();
        for key in expired { self.receipts.remove(&key); self.tombstones.insert(key, Instant::now()); }
        self.tombstones.retain(|_, t| t.elapsed() < Duration::from_secs(300));
        self.closed_resources.retain(|_,(_,_,_,t)|t.elapsed()<Duration::from_secs(60));
        // Tombstones are bounded without silently authorizing re-execution of an old ID.
        // Admission checks include this count, so normal traffic cannot evict one early.
        self.groups.retain(|_, g| g.closed.is_none_or(|t| t.elapsed() < Duration::from_secs(300)));
    }
}
