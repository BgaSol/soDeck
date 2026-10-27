use super::*;

impl Registry {
    pub(super) fn cleanup(&self, group: &str) -> usize {
        self.pools.values().filter(|p| p.groups.contains(group)).map(|p|
            p.members.iter().map(|m| if m.retiring && m.connection.transport_live.load(Ordering::SeqCst) {1} else {m.connection.network_metrics.lock().unwrap().cleanup_pending}).sum::<usize>() + p.building).sum()
    }
    pub(super) fn summary(&self, id: &str) -> Result<Value> {
        let g = self.groups.get(id).ok_or_else(stale)?;
        let endpoints: Vec<_> = self.endpoints.values().filter(|e| e.group == id).collect();
        let (read, written) = (g.stat_read, g.stat_written);
        let available=self.routes.values().filter(|r|r.group==id).filter(|r|self.pools.get(&r.pool).is_some_and(|p|p.members.iter().any(|m|!m.retiring&&*m.connection.status.lock().unwrap()=="active"))).count();
        Ok(json!({"group":g.handle,"viewId":g.view,"state":if g.closed.is_some() {if self.cleanup(id)>0 {"closing"}else{"closed"}}else{"active"},
            "availableRoutes":available,"connections":endpoints.iter().filter(|e|e.accepted&&!e.state.lock().unwrap().closed).count(),
            "listeners":self.listeners.values().filter(|l|l.group==id).count(),"routes":self.routes.values().filter(|r|r.group==id).count(),
            "streams":endpoints.len(),"pendingAccept":self.listeners.values().filter(|l|l.group==id).map(|l|l.pending.len()).sum::<usize>(),
            "pendingOpen":self.pending.iter().filter(|o|o.group==id).count(),"bytesRead":read,"bytesWritten":written,"cleanupPending":self.cleanup(id),"revision":g.revision}))
    }
    pub(super) fn resources(&self, p: &Value) -> Result<Value> {
        let scope: Option<WorkspaceScope> = p.get("scope").map(|s| self.check_scope(s)).transpose()?;
        let group = p.get("group").map(Handle::parse).transpose()?;
        if let Some(h) = &group { if self.groups.get(&h.id).is_none_or(|g| g.handle != *h) { return Err(stale()); } }
        let cursor = p.get("cursor").and_then(Value::as_str).unwrap_or("");
        let mut items = Vec::new();
        let mut add = |h: &Handle, kind: &str, state: &str, gid: &str, read: u64, written: u64, cleanup: usize, extra: Value| {
            let Some(g) = self.groups.get(gid) else { return };
            if scope.as_ref().is_some_and(|s| s != &g.scope) || group.as_ref().is_some_and(|h| h.id != gid) || h.id.as_str() <= cursor { return; }
            let mut value = json!({"handle":h,"kind":kind,"state":state,"scope":g.scope,"group":g.handle,"instanceId":g.owner,"viewId":g.view,"bytesRead":read,"bytesWritten":written,"cleanupPending":cleanup});
            if let Some(extra) = extra.as_object() { value.as_object_mut().unwrap().extend(extra.clone()); }
            items.push(value);
        };
        for g in self.groups.values() { if g.closed.is_none() || self.cleanup(&g.handle.id)>0 { add(&g.handle,"group",if g.closed.is_some(){"closing"}else{"active"},&g.handle.id,g.bytes_read,g.bytes_written,self.cleanup(&g.handle.id),Value::Null); } }
        for l in self.listeners.values() { if let Ok(address) = l.socket.local_addr() { add(&l.handle,"listener",if l.accepting {"accepting"} else {"paused"},&l.group,0,0,0,json!({"address":{"host":address.ip().to_string(),"port":address.port()}})); } }
        for r in self.routes.values() {
            if let Some(pool) = self.pools.get(&r.pool) {
                let ready = pool.members.iter().any(|m| *m.connection.status.lock().unwrap()=="active"&&!m.connection.network_metrics.lock().unwrap().draining);
                add(&r.handle,"route",if ready {"ready"} else {"unavailable"},&r.group,0,0,pool.building,json!({"serverId":pool.server,"routeRevision":pool.revision}));
            }
        }
        for (key,pool) in &self.pools {
            if let Some(group) = self.routes.values().find(|r| r.pool == *key).map(|r| &r.group).or_else(|| pool.groups.iter().next()) {
                for m in &pool.members {
                    let metrics=m.connection.network_metrics.lock().unwrap();
                    add(&Handle{id:m.connection.id.clone(),generation:m.connection.generation},"transport",if m.retiring {"closing"} else if metrics.draining {"draining"} else {"ready"},group,0,0,metrics.cleanup_pending.max(usize::from(m.retiring)),json!({"serverId":pool.server,"routeRevision":pool.revision}));
                }
            }
        }
        for e in self.endpoints.values() {
            let s=e.state.lock().unwrap();
            add(&e.handle,"stream",if s.closed {"closed"}else if s.spliced {"spliced"}else{"active"},&e.group,s.received,s.written,0,Value::Null);
        }
        for s in self.splices.values() { add(&s.handle,"splice",if s.active {"active"}else{"transferring"},&s.group,0,0,0,Value::Null); }
        items.sort_by(|a,b| a["handle"]["id"].as_str().cmp(&b["handle"]["id"].as_str()));
        let more=items.len()>64;items.truncate(64);
        let cursor=if more {items.last().unwrap()["handle"]["id"].clone()}else{Value::Null};
        Ok(json!({"items":items,"nextCursor":cursor}))
    }
    pub(super) fn stop(&mut self, value: &Value) -> Result<Value> {
        let h=Handle::parse(value)?;
        let mut affected=std::collections::HashSet::new();
        if self.groups.contains_key(&h.id){affected.insert(h.id.clone());}
        for l in self.listeners.values().filter(|l|l.handle==h){affected.insert(l.group.clone());}
        for r in self.routes.values().filter(|r|r.handle==h){affected.insert(r.group.clone());}
        for e in self.endpoints.values().filter(|e|e.handle==h||e.connection.as_ref().is_some_and(|c|c.id==h.id&&c.generation==h.generation)){affected.insert(e.group.clone());}
        for s in self.splices.values().filter(|s|s.handle==h){affected.insert(s.group.clone());}
        for (key,_) in self.pools.iter().filter(|(_,p)|p.members.iter().any(|m|m.connection.id==h.id&&m.connection.generation==h.generation)){affected.extend(self.routes.values().filter(|r|r.pool==*key).map(|r|r.group.clone()));}
        let found=self.groups.get(&h.id).map(|g|&g.handle).or_else(||self.listeners.get(&h.id).map(|l|&l.handle))
            .or_else(||self.routes.get(&h.id).map(|r|&r.handle)).or_else(||self.endpoints.get(&h.id).map(|e|&e.handle))
            .or_else(||self.splices.get(&h.id).map(|s|&s.handle));
        if let Some(found)=found { if *found!=h {return Err(stale());}self.close_resource(&h.id); }
        else {
            for p in self.pools.values() { for m in &p.members {
                if m.connection.id==h.id {if m.connection.generation!=h.generation{return Err(stale());}m.connection.stop();}
            }}
        }
        let groups:Vec<_>=affected.iter().filter_map(|id|self.groups.get(id).map(|g|g.handle.clone())).collect();
        let cleanup:usize=self.pools.values().filter(|p|p.groups.iter().any(|g|affected.contains(g))).map(|p|p.building+p.members.iter().map(|m|if m.retiring&&m.connection.transport_live.load(Ordering::SeqCst){1}else{m.connection.network_metrics.lock().unwrap().cleanup_pending}).sum::<usize>()).sum();
        Ok(json!({"closed":true,"cleanupPending":cleanup,"affectedGroups":groups}))
    }
}

impl Core {
    /// Domain provider for the host's audited state feed API.
    pub fn network_group_state(&self,caller:&Caller,target:&Value)->Result<Value> {
        let mut n=self.network.lock().unwrap();let id=n.group(caller,target,true)?;n.summary(&id)
    }
    pub fn network_feed_snapshots(&self)->Vec<(WorkspaceScope,Value)> {
        let n=self.network.lock().unwrap();n.groups.values().filter(|g|g.closed.is_none_or(|t|t.elapsed()<Duration::from_secs(60)))
            .filter_map(|g|n.summary(&g.handle.id).ok().map(|s|(g.scope.clone(),s))).collect()
    }
}
