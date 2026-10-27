use super::*;

impl Registry {
    pub(super) fn listener_id(&mut self, group: &str, value: &Value) -> Result<String> {
        let h = Handle::parse(value)?;
        let l = self.listeners.get(&h.id).ok_or_else(stale)?;
        if l.handle != h || l.group != group { return Err(stale()); }
        self.claim_resource(&h.id); Ok(h.id)
    }
    pub(super) fn open_listener(&mut self, group: &str, port: u16) -> Result<(Value, Vec<String>)> {
        if self.listeners.values().filter(|l| l.group == group).count() >= 128 { return Err(capacity()); }
        let socket = TcpListener::bind(("127.0.0.1", port)).map_err(|e| CoreError::new(
            if e.kind() == std::io::ErrorKind::AddrInUse { "PORT_IN_USE" } else { "IO_ERROR" }, "无法绑定本机监听端口"))?;
        socket.set_nonblocking(true)?;
        let address = socket.local_addr()?;
        let handle = self.handle();
        self.listeners.insert(handle.id.clone(), Listener { handle: handle.clone(), group: group.into(), socket,
            accepting: false, pending: VecDeque::new(), probes: HashMap::new() });
        Ok((json!({"listener":handle,"address":{"host":address.ip().to_string(),"port":address.port()},"accepting":false}), vec![handle.id]))
    }
    pub(super) fn accept(&mut self, id: &str, count: usize) -> (Value, Vec<String>) {
        let l = self.listeners.get_mut(id).unwrap();
        let accepted: Vec<_> = l.pending.drain(..count.min(l.pending.len())).collect();
        let ids = accepted.iter().map(|a| a.stream.id.clone()).collect();
        let items: Vec<_> = accepted.into_iter().map(|a| {
            let mut value = json!({"stream":a.stream,"peer":{"host":a.peer.ip().to_string(),"port":a.peer.port()}});
            if let Some(probe) = a.probe { value["probeId"] = json!(probe); }
            value
        }).collect();
        (json!({"items":items}), ids)
    }
    pub(super) fn connect_self(&mut self, id: &str) -> Result<(Value, Vec<String>)> {
        let l = self.listeners.get(id).unwrap();
        let group = l.group.clone();
        if !l.accepting || l.probes.len() >= 16 { return Err(CoreError::new("CAPACITY", "监听入口暂停或检查过多")); }
        // Only the address held by this group's live listener is dialable.
        let socket = TcpStream::connect_timeout(&l.socket.local_addr()?, Duration::from_millis(100))?;
        let local = socket.local_addr()?; let probe = id::new();
        let h = self.reserve_endpoint(&group, Some(socket), false)?;
        self.listeners.get_mut(id).unwrap().probes.insert(local, probe.clone());
        Ok((json!({"stream":h,"probeId":probe}), vec![h.id]))
    }
    pub(super) fn tick_listeners(&mut self) {
        let ids: Vec<_> = self.listeners.keys().cloned().collect();
        let mut pending: usize = self.listeners.values().map(|l| l.pending.len()).sum();
        for id in ids {
            for _ in 0..16 {
                let l = self.listeners.get(&id).unwrap();
                if !l.accepting || l.pending.len() >= 32 || pending >= 128 || self.endpoints.len() >= 512
                    || self.endpoints.values().filter(|e| e.group == l.group).count() >= 128 { break; }
                let group = l.group.clone();
                match l.socket.accept() {
                    Ok((socket, peer)) => {
                        if let Ok(stream) = self.reserve_endpoint(&group, Some(socket), false) {
                            self.endpoints.get_mut(&stream.id).unwrap().accepted=true;
                            let l = self.listeners.get_mut(&id).unwrap(); let probe = l.probes.remove(&peer);
                            l.pending.push_back(Accepted { stream, peer, probe }); pending += 1;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    pub(super) fn tick_sockets(&mut self) -> bool {
        let mut progress=false;
        for e in self.endpoints.values_mut() {
            if let Some(socket) = &mut e.socket {
                let mut state = e.state.lock().unwrap();
                if state.closed { let _ = socket.shutdown(std::net::Shutdown::Both); continue; }
                let before=(state.received,state.written);state.write_to(socket); state.read_from(socket);progress|=before!=(state.received,state.written);
                if state.wants_eof() {
                    match socket.shutdown(std::net::Shutdown::Write) {
                        Ok(()) => state.write_eof = true,
                        Err(_) => state.fail(CoreError::new("TRANSPORT_LOST", "本机连接结束写入失败")),
                    }
                }
            }
        }
        progress
    }
}
// Avoid shadowing the common id constructor with listener IDs.
mod id { pub(super) fn new() -> String { crate::model::id() } }
