use super::*;

impl Core {
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
        self.receipts.insert(key.clone(), Receipt { caller:caller.clone(), method: method.into(), params: p.clone(),
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
                receipt.outcome = Some(Err(CoreError::new("CANCELED", "网络操作已取消")));
                receipt.completed = Some(Instant::now());
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
