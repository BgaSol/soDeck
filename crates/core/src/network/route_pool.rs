use super::*;

impl Registry {
    pub(super) fn route_id(&mut self, group: &str, value: &Value) -> Result<String> {
        let h = Handle::parse(value)?;
        let r = self.routes.get(&h.id).ok_or_else(stale)?;
        if r.handle != h || r.group != group { return Err(stale()); }
        self.claim_resource(&h.id); Ok(h.id)
    }
}
impl Core {
    pub(super) fn network_acquire_route(&self, caller: Caller, key: Key, group: String, p: Value) -> Result<()> {
        let mut n = self.network.lock().unwrap();
        let scope = n.groups.get(&group).ok_or_else(stale)?.scope.clone();
        let server = p["serverId"].as_str().unwrap().to_owned();
        let revision = p["expectedRouteRevision"].as_str().unwrap().to_owned();
        let pool = serde_json::to_string(&json!([scope, server, revision, "network-data"]))?;
        n.pools.entry(pool.clone()).or_insert_with(|| Pool { scope, server, revision, members: Vec::new(), building: 0, failure: None, groups:Default::default() }).groups.insert(group.clone());
        if n.routes.values().filter(|r| r.group == group).count() >= 128 { return Err(capacity()); }
        let handle = n.handle();
        n.routes.insert(handle.id.clone(), Route { handle: handle.clone(), group, pool: pool.clone() });
        n.receipts.get_mut(&key).unwrap().resources.push(handle.id.clone());
        let canceled = n.receipts[&key].canceled.clone();
        drop(n);
        self.queue_network(Box::new(move |core| {
            let result = core.ensure_network_member(&caller, &pool, &key.1, &canceled, false);
            let mut n = core.network.lock().unwrap();
            let result = result.and_then(|_| {
                let r = n.routes.get(&handle.id).ok_or_else(stale)?; let pool = n.pools.get(&r.pool).ok_or_else(stale)?;
                Ok(json!({"route":handle,"serverId":pool.server,"routeRevision":pool.revision,"state":"ready"}))
            });
            n.complete(&key, result, vec![handle.id]);
        }))
    }
    fn ensure_network_member(&self, caller: &Caller, pool_id: &str, operation: &str, canceled: &AtomicBool, extra: bool) -> Result<()> {
        let (scope, server, revision) = {
            let mut n = self.network.lock().unwrap();
            let p = n.pools.get_mut(pool_id).ok_or_else(stale)?;
            let available = p.members.iter().any(|m| *m.connection.status.lock().unwrap() == "active" && !m.connection.network_metrics.lock().unwrap().draining);
            if available && !extra { return Ok(()); }
            if p.members.len() + p.building >= 4 { return Err(capacity()); }
            p.building += 1; (p.scope.clone(), p.server.clone(), p.revision.clone())
        };
        let params = json!({"serverId":server,"workspaceId":scope.workspace_id,"expectedRouteRevision":revision,"operationId":operation,"networkOperation":true});
        let purpose = format!("network-data:{}:{}", scope.workspace_generation, id());
        let result = self.acquire_for(caller, &params, canceled, &purpose, &format!("network:{pool_id}"));
        let mut n = self.network.lock().unwrap();
        if let Some(p) = n.pools.get_mut(pool_id) { p.building = p.building.saturating_sub(1); }
        match result {
            Ok(receipt) => {
                let lease_id = receipt["leaseId"].as_str().unwrap().to_owned();
                let mut registry = self.registry.lock().unwrap();
                let connection = registry.leases.get(&lease_id).ok_or_else(stale)?.connection.clone();
                if canceled.load(Ordering::SeqCst) || self.network_stopped.load(Ordering::SeqCst) || !n.pools.contains_key(pool_id)
                    || !n.routes.values().any(|r| r.pool == pool_id) {
                    registry.leases.remove(&lease_id); Self::reap(&mut registry);
                    return Err(CoreError::new("CANCELED", "网络路由使用者已结束"));
                }
                let p = n.pools.get_mut(pool_id).unwrap(); p.failure = None;
                p.members.push(Member { connection, lease: lease_id, idle_since: Instant::now(), retiring:false });
                Ok(())
            }
            Err(error) => { if let Some(p) = n.pools.get_mut(pool_id) { p.failure = Some(error.clone()); } Err(error) }
        }
    }
    pub(super) fn network_refresh_route(&self, caller: Caller, key: Key, route: String) -> Result<()> {
        let n = self.network.lock().unwrap();
        let pool = n.routes.get(&route).ok_or_else(stale)?.pool.clone();
        let canceled = n.receipts[&key].canceled.clone(); drop(n);
        self.queue_network(Box::new(move |core| {
            // Refresh must check the current route even if an old member is still live.
            let expected = { let n = core.network.lock().unwrap(); n.pools.get(&pool).map(|p| (p.server.clone(), p.revision.clone())) };
            let result = expected.ok_or_else(stale).and_then(|(server, revision)| {
                let _config = core.configuration.lock().unwrap();
                if core.route_plan(&server)?.revision != revision { return Err(CoreError::new("ROUTE_CHANGED", "路径已修改，请应用并重启")); }
                Ok(())
            }).and_then(|_| core.ensure_network_member(&caller, &pool, &key.1, &canceled, false));
            core.network.lock().unwrap().complete(&key, result.map(|_| json!({"accepted":true})), Vec::new());
        }))
    }
    pub(super) fn tick_network_targets(&self, n: &mut Registry) {
        let mut pending = std::mem::take(&mut n.pending);
        for mut open in pending.drain(..) {
            if open.canceled.load(Ordering::SeqCst)
                || n.groups.get(&open.group).is_none_or(|g| g.closed.is_some()) {
                n.complete(&open.key, Err(CoreError::new("CANCELED", "目标连接已取消")), vec![open.endpoint]); continue;
            }
            if let Some(result) = &open.opening {
                let complete = result.lock().unwrap().clone();
                if let Some(result) = complete {
                    let result=result.and_then(|_|n.endpoints.get(&open.endpoint).map(|e|json!({"stream":e.handle})).ok_or_else(||CoreError::new("CANCELED","目标连接已关闭")));
                    n.complete(&open.key, result, vec![open.endpoint]);
                } else { n.pending.push(open); }
                continue;
            }
            if !n.endpoints.contains_key(&open.endpoint){n.complete(&open.key,Err(CoreError::new("CANCELED","目标连接已关闭")),vec![open.endpoint]);continue;}
            if open.queued.elapsed() >= Duration::from_secs(5) && n.pools.get(&open.pool).is_none_or(|p| p.building == 0) {
                n.complete(&open.key, Err(CoreError::new("TARGET_CONNECT_TIMEOUT", "目标连接等待超过 5 秒")), vec![open.endpoint]); continue;
            }
            let Some(pool) = n.pools.get_mut(&open.pool) else { n.complete(&open.key, Err(stale()), vec![open.endpoint]); continue };
            let member = pool.members.iter_mut().filter(|m| *m.connection.status.lock().unwrap() == "active")
                .find(|m| { let metrics = m.connection.network_metrics.lock().unwrap(); !metrics.draining && metrics.pending == 0 });
            if let Some(member) = member {
                let endpoint = n.endpoints.get_mut(&open.endpoint).unwrap();
                endpoint.connection = Some(member.connection.clone()); member.idle_since = Instant::now();
                let mut job = crate::ssh::network::Open::new(open.host.clone(), open.port, endpoint.state.clone(), open.canceled.clone());
                job.traffic=member.connection.traffic_resource(&n.receipts[&open.key].caller,
                    &endpoint.handle.id,&endpoint.handle.generation.to_string(),"tcp",Some(&pool.scope.workspace_generation));
                let result = job.result.clone();
                match member.connection.open_tcp(job) {
                    Ok(()) => { member.connection.network_metrics.lock().unwrap().pending += 1; open.opening = Some(result); n.pending.push(open); }
                    Err(error) => n.complete(&open.key, Err(error), vec![open.endpoint]),
                }
            } else if pool.building == 0 && !pool.members.iter().any(|m|!m.retiring&&*m.connection.status.lock().unwrap()=="active") {
                n.complete(&open.key, Err(CoreError::new("TRANSPORT_LOST", "SSH 路径已中断，请重新连接")), vec![open.endpoint]);
            } else if pool.building == 0 && pool.members.len() < 4 && pool.failure.is_none() {
                // Reserve before enqueueing, then convert to a real reservation in the worker.
                pool.building += 1;
                let pool_id = open.pool.clone(); let operation = open.key.1.clone(); let canceled = open.canceled.clone();
                let caller = n.receipts[&open.key].caller.clone();
                let queued = self.queue_network(Box::new(move |core| {
                    { let mut n = core.network.lock().unwrap(); if let Some(p) = n.pools.get_mut(&pool_id) { p.building = p.building.saturating_sub(1); } }
                    let _ = core.ensure_network_member(&caller, &pool_id, &operation, &canceled, true);
                }));
                if let Err(error) = queued { let p = n.pools.get_mut(&open.pool).unwrap(); p.building -= 1; p.failure = Some(error); }
                n.pending.push(open);
            } else if pool.building == 0 && pool.failure.is_some() {
                let error = pool.failure.clone().unwrap(); n.complete(&open.key, Err(error), vec![open.endpoint]);
            } else if pool.building == 0 && pool.members.len() >= 4 && pool.members.iter().all(|m| m.connection.network_metrics.lock().unwrap().draining) {
                n.complete(&open.key, Err(capacity()), vec![open.endpoint]);
            } else { n.pending.push(open); }
        }
    }
    pub(super) fn reap_network_pools(&self, n: &mut Registry) {
        let used: std::collections::HashSet<_> = n.routes.values().map(|r| r.pool.clone()).collect();
        let mut leases = Vec::new();
        n.pools.retain(|id, pool| {
            let mut retained_ready = false;
            pool.members.retain_mut(|m| {
                let metrics = m.connection.network_metrics.lock().unwrap().clone();
                if metrics.active > 0 || metrics.pending > 0 { m.idle_since = Instant::now(); }
                let closed = *m.connection.status.lock().unwrap() != "active";
                let idle = metrics.active == 0 && metrics.pending == 0;
                let retire = !used.contains(id) || closed || (metrics.draining && idle)
                    || (retained_ready && idle && m.idle_since.elapsed() >= Duration::from_secs(30));
                if retire { if !m.retiring {m.connection.stop(); m.retiring=true;} }
                else if !metrics.draining { retained_ready = true; }
                if m.retiring && !m.connection.transport_live.load(Ordering::SeqCst) { leases.push(m.lease.clone()); false } else { true }
            });
            used.contains(id) || pool.building > 0 || !pool.members.is_empty()
        });
        if !leases.is_empty() {
            let mut registry = self.registry.lock().unwrap();
            for lease in leases { registry.leases.remove(&lease); }
            Self::reap(&mut registry);
        }
    }
}
