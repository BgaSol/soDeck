use super::*;
use sodeck_communication::binary::Frame;

impl Core {
    pub(crate) fn network_dispatch(&self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        contract::validate("x-methods", method, p)?;
        let mut n = self.network.lock().unwrap();
        if method.starts_with("net.operations.") { return n.operation(caller, method, p); }
        if method == "net.resources.list" { return n.resources(p); }
        if method == "net.groups.get" { let group = n.group(caller, p, true)?; return n.summary(&group); }
        if let Some(scope) = p.get("scope") { n.check_scope(scope)?; }
        let key = (caller.instance_id.clone(), p["operationId"].as_str().unwrap().to_owned());
        if n.receipts.contains_key(&key) {
            n.begin(caller, method, p, None)?;
            return n.receipt(&key);
        }
        let group = if matches!(method, "net.groups.create" | "net.resources.stop") { None }
            else { Some(n.group(caller, p, method.ends_with(".close")||method.ends_with(".release"))?) };
        let (key, _) = n.begin(caller, method, p, group.clone())?;
        if method == "net.routes.acquire" {
            drop(n);
            let result = self.network_acquire_route(caller.clone(), key.clone(), group.unwrap(), p.clone());
            let mut n = self.network.lock().unwrap();
            if let Err(error) = result { let ids = n.receipts[&key].resources.clone(); n.complete(&key, Err(error), ids); }
            return n.receipt(&key);
        }
        if method == "net.routes.refresh" {
            let route = n.route_id(group.as_deref().unwrap(), &p["route"]);
            drop(n);
            let result = route.and_then(|route| self.network_refresh_route(caller.clone(), key.clone(), route));
            let mut n = self.network.lock().unwrap();
            if let Err(error) = result { n.complete(&key, Err(error), Vec::new()); }
            return n.receipt(&key);
        }
        let result = (|| -> Result<Option<(Value, Vec<String>)>> {
            let group = group.as_deref().unwrap_or("");
            let cleanup=match method {"net.listeners.close"=>Some(("listener","listener")),"net.routes.release"=>Some(("route","route")),"net.streams.close"=>Some(("stream","stream")),_=>None};
            if let Some((field,kind))=cleanup {if n.was_closed(group,&p[field],kind)? {return Ok(Some((json!({"closed":true,"cleanupPending":n.cleanup(group)}),Vec::new())));}}
            match method {
                "net.groups.create" => {
                    let scope = n.check_scope(&p["scope"])?; let view = p["viewId"].as_str().unwrap();
                    if n.views.get(view) != Some(&(caller.instance_id.clone(), scope.clone())) { return Err(CoreError::new("STALE_HANDLE", "视图不属于当前工作区和插件实例")); }
                    if n.groups.values().filter(|g| g.closed.is_none()).count() >= 32
                        || n.groups.values().filter(|g| g.closed.is_none() && g.owner == caller.instance_id).count() >= 16 { return Err(capacity()); }
                    let h = n.handle();
                    n.groups.insert(h.id.clone(), Group { handle:h.clone(), scope, owner:caller.instance_id.clone(), view:view.into(), closed:None, revision:1, bytes_read:0, bytes_written:0, stats_at:Instant::now(), stat_read:0, stat_written:0 });
                    n.receipts.get_mut(&key).unwrap().group = Some(h.id.clone());
                    Ok(Some((json!({"group":h}), vec![h.id])))
                }
                "net.groups.close" => { n.close_resource(group); Ok(Some((json!({"closed":true,"cleanupPending":n.cleanup(group)}), Vec::new()))) }
                "net.listeners.open" => n.open_listener(group, p["port"].as_u64().unwrap() as u16).map(Some),
                "net.listeners.setAccepting" | "net.listeners.accept" | "net.listeners.close" | "net.listeners.connectSelf" => {
                    let id = n.listener_id(group, &p["listener"])?;
                    match method {
                        "net.listeners.setAccepting" => { n.listeners.get_mut(&id).unwrap().accepting = p["enabled"] == true; Ok(Some((json!({"accepted":true}), Vec::new()))) }
                        "net.listeners.accept" => Ok(Some(n.accept(&id, p["maxItems"].as_u64().unwrap() as usize))),
                        "net.listeners.connectSelf" => n.connect_self(&id).map(Some),
                        _ => { n.close_resource(&id); Ok(Some((json!({"closed":true,"cleanupPending":0}), Vec::new()))) },
                    }
                }
                "net.routes.release" => { let id = n.route_id(group, &p["route"])?; n.close_resource(&id); Ok(Some((json!({"closed":true,"cleanupPending":n.cleanup(group)}), Vec::new()))) }
                "net.tcp.open" => {
                    let route = n.route_id(group, &p["route"])?; let pool = n.routes[&route].pool.clone();
                    let host = p["host"].as_str().unwrap();
                    if host.as_bytes().len() > 255 || host.chars().any(|c| c.is_control() || c.is_whitespace()) || host.contains(['/', '\\', '@', '[', ']']) { return Err(CoreError::new("INVALID_PARAMS", "目标主机格式无效")); }
                    n.target_capacity(&pool)?;
                    let endpoint = n.reserve_endpoint(group, None, false)?;
                    n.endpoints.get_mut(&endpoint.id).unwrap().route=Some(route);
                    let canceled = n.receipts[&key].canceled.clone();
                    n.receipts.get_mut(&key).unwrap().resources.push(endpoint.id.clone());
                    n.pending.push(PendingTcp { key:key.clone(), group:group.into(), pool, endpoint:endpoint.id, host:host.into(), port:p["port"].as_u64().unwrap() as u16, canceled, queued:Instant::now(), opening:None });
                    Ok(None)
                }
                "net.streams.close" | "net.streams.halfClose" | "net.streams.exchange" => {
                    let id = n.endpoint(group, &p["stream"])?;
                    match method {
                        "net.streams.close" => { n.close_resource(&id); Ok(Some((json!({"closed":true,"cleanupPending":0}), Vec::new()))) }
                        "net.streams.halfClose" => { n.endpoints[&id].state.lock().unwrap().half_close(); Ok(Some((json!({"accepted":true}), Vec::new()))) }
                        _ => {
                            if n.next_slot == u32::MAX || n.next_epoch == u32::MAX { return Err(capacity()); }
                            n.next_slot += 1; n.next_epoch += 1; let (slot, epoch) = (n.next_slot, n.next_epoch);
                            let audit = self.observability.call_start(caller, "net.streams.exchange.lifecycle", p)?;
                            let e = n.endpoints.get_mut(&id).unwrap();
                            let result = e.state.lock().unwrap().attach(slot, epoch);
                            match result {
                                Ok(attachment) => {
                                    e.exchange = Some((caller.clone(), audit, crate::model::now_ms()));
                                    n.attachments.insert((caller.instance_id.clone(), slot), (epoch, id));
                                    Ok(Some((json!({"attachment":attachment}), Vec::new())))
                                }
                                Err(error) => { let _ = self.observability.call_finish(&audit, &Err(error.clone())); Err(error) }
                            }
                        }
                    }
                }
                "net.streams.splice" => {
                    let left = n.endpoint(group, &p["left"]["stream"])?; let right = n.endpoint(group, &p["right"]["stream"])?;
                    if left == right { return Err(CoreError::new("INVALID_PARAMS", "不能把字节流连接到自身")); }
                    let frozen = (|| {
                        for (id, cursor) in [(&left, &p["left"]), (&right, &p["right"])] {
                            n.endpoints[id].state.lock().unwrap().freeze(cursor["epoch"].as_u64().unwrap() as u32, cursor["consumed"].as_u64().unwrap(), cursor["written"].as_u64().unwrap())?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = frozen { n.close_resource(&left); n.close_resource(&right); return Err(error); }
                    let handle = n.handle();
                    n.splices.insert(handle.id.clone(), Splice { handle, group:group.into(), left, right, key:key.clone(), active:false, created:Instant::now() });
                    Ok(None)
                }
                "net.resources.stop" => n.stop(&p["resource"]).map(|v| Some((v, Vec::new()))),
                _ => Err(CoreError::new("UNSUPPORTED_API", "未知网络接口")),
            }
        })();
        match result { Ok(Some((v, ids))) => n.complete(&key, Ok(v), ids), Err(error) => n.complete(&key, Err(error), Vec::new()), Ok(None) => {} }
        let result = n.receipt(&key)?;
        contract::validate("x-results", method, &result)?;
        Ok(result)
    }
    pub(super) fn network_tick(&self) -> bool {
        let mut n = self.network.lock().unwrap();
        n.sweep_receipts(); n.tick_listeners(); let mut progress=n.tick_sockets();
        self.tick_network_targets(&mut n);
        let splices: Vec<_> = n.splices.keys().cloned().collect();
        for id in splices {
            let s = &n.splices[&id];
            let (left, right, key, handle, active, timeout) = (s.left.clone(), s.right.clone(), s.key.clone(), s.handle.clone(), s.active, s.created.elapsed() >= Duration::from_secs(5));
            let states = n.endpoints.get(&left).zip(n.endpoints.get(&right)).map(|(a,b)| (a.state.clone(), b.state.clone()));
            let Some((a,b)) = states else { n.close_resource(&id); n.complete(&key, Err(stale()), Vec::new()); continue };
            let (mut a, mut b) = (a.lock().unwrap(), b.lock().unwrap());
            if !active {
                let canceled = n.receipts.get(&key).is_none_or(|r| r.canceled.load(Ordering::SeqCst));
                if canceled || timeout || a.closed || b.closed {
                    a.close(); b.close(); drop(a); drop(b); n.splices.remove(&id);n.observed.remove(&id);
                    n.complete(&key, Err(CoreError::new(if canceled {"CANCELED"} else {"TARGET_CONNECT_TIMEOUT"}, "流移交未能完成，连接已关闭")), Vec::new()); continue;
                }
                if !a.ready_to_splice() || !b.ready_to_splice() { continue; }
                a.transfer(); b.transfer(); n.splices.get_mut(&id).unwrap().active = true;
                n.complete(&key, Ok(json!({"splice":handle})), vec![id.clone()]);
            }
            let before=(a.consumed,b.consumed);a.splice_into(&mut b); b.splice_into(&mut a);progress|=before!=(a.consumed,b.consumed);
            if a.finished() && b.finished() { a.close(); b.close(); n.splices.remove(&id); n.observed.remove(&id); }
        }
        self.reap_network_pools(&mut n);
        let update:Vec<_>=n.groups.values().filter(|g|g.stats_at.elapsed()>=Duration::from_secs(1)).map(|g|g.handle.id.clone()).collect();
        for id in update {
            let (mut read,mut written)=(n.groups[&id].bytes_read,n.groups[&id].bytes_written);
            for e in n.endpoints.values().filter(|e|e.group==id&&e.accepted){let s=e.state.lock().unwrap();read+=s.received;written+=s.written;}
            let g=n.groups.get_mut(&id).unwrap();g.stat_read=read;g.stat_written=written;g.stats_at=Instant::now();g.revision+=1;
        }
        for e in n.endpoints.values_mut() { if e.state.lock().unwrap().closed { e.closed_at.get_or_insert_with(Instant::now); } }
        let expired: Vec<_> = n.endpoints.values().filter(|e| {
            let state = e.state.lock().unwrap();
            (state.closed && (state.reset_delivered() || e.closed_at.is_some_and(|t|t.elapsed()>=Duration::from_secs(1))))
                || (state.finished() && state.attachment.is_none()) || (!e.delivered && e.created.elapsed() >= Duration::from_secs(30))
        }).map(|e| e.handle.id.clone()).collect();
        let mut audits = Vec::new();
        for id in expired {
            n.observed.remove(&id);
            if let Some(e) = n.endpoints.remove(&id) {
                n.closed_resources.insert(id.clone(),(e.handle.clone(),e.group.clone(),"stream",Instant::now()));
                let mut s = e.state.lock().unwrap(); s.close();
                if let Some(socket) = e.socket { let _ = socket.shutdown(std::net::Shutdown::Both); }
                if let Some(g) = n.groups.get_mut(&e.group) { if e.accepted {g.bytes_read += s.received; g.bytes_written += s.written;} g.revision += 1; }
                if let Some((_, audit, started)) = e.exchange { audits.push((audit, json!({"bytesRead":s.received,"bytesWritten":s.written,"durationMs":crate::model::now_ms().saturating_sub(started),"error":s.error.as_ref().map(|e| &e.code)}))); }
            }
            n.attachments.retain(|_, (_, endpoint)| endpoint != &id);
            for l in n.listeners.values_mut() { l.pending.retain(|a| a.stream.id != id); }
        }
        drop(n);
        for (audit, result) in audits { if self.observability.network_stream_finish(&audit,result["bytesRead"].as_u64().unwrap(),result["bytesWritten"].as_u64().unwrap(),result["durationMs"].as_u64().unwrap(),result["error"].as_str()).is_err() { self.fault("net.streams.exchange.lifecycle", "result-not-persisted"); } }
        progress
    }
    /// Frames inherit the host's binding; attachment IDs never establish identity.
    pub fn network_input(&self, caller: &Caller, frame: Frame) -> Result<()> {
        let n = self.network.lock().unwrap();
        let (epoch, id) = n.attachments.get(&(caller.instance_id.clone(), frame.slot)).ok_or_else(stale)?;
        if *epoch != frame.epoch { return Err(stale()); }
        let e = n.endpoints.get(id).ok_or_else(stale)?;
        let mut state = e.state.lock().unwrap();
        let result = state.input(&frame);
        if let Err(error) = &result { if error.code == "PROTOCOL_ERROR" { state.fail(error.clone()); self.fault("net.streams.exchange", "invalid-frame"); } }
        result
    }
    pub fn network_output(&self, caller: &Caller, mut deliver: impl FnMut(Frame) -> bool) {
        let mut n = self.network.lock().unwrap();
        let cursor=n.output_cursor.get(&caller.instance_id).copied().unwrap_or(0);
        let mut slots:Vec<_>=n.attachments.iter().filter(|((owner,_),_)|owner==&caller.instance_id).map(|((_,slot),(_,id))|(*slot,id.clone())).collect();
        slots.sort_by_key(|(slot,_)|(*slot<=cursor,*slot));
        for (_,id) in &slots {if let Some(e)=n.endpoints.get(id){e.state.lock().unwrap().output_controls(&mut deliver);}}
        let mut total = 0;let mut last=cursor;
        for (slot,id) in slots {
            if let Some(e) = n.endpoints.get(&id) {
                e.state.lock().unwrap().output(|frame| {
                    if total >= 256 * 1024 && !frame.is_control() { return false; }
                    let size = frame.size(); let sent = deliver(frame); if sent { total += size; } sent
                });
            }
            last=slot;if total>=256*1024 {break;}
        }
        n.output_cursor.insert(caller.instance_id.clone(),last);
    }
}
