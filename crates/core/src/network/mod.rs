//! Core-owned TCP resources; HTTP and SOCKS belong to the plugin.
pub(crate) mod stream;
mod operation;
mod listener;
mod route_pool;
mod services;
mod resources;
#[path="contract.generated.rs"]
mod contract;

use crate::{model::id, Caller, Core, CoreError, Result, WorkspaceScope};
use crate::ssh::Connection;
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, VecDeque}, net::{TcpListener, TcpStream, SocketAddr},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use stream::Stream;
type Key = (String, String);
pub(crate) type Job = Box<dyn FnOnce(&Core) + Send>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Handle { id: String, generation: u64 }
impl Handle {
    fn new(generation: u64) -> Self { Self { id: id(), generation } }
    fn parse(value: &Value) -> Result<Self> { Ok(serde_json::from_value(value.clone())?) }
}
struct Group {
    handle: Handle, scope: WorkspaceScope, owner: String, view: String,
    closed: Option<Instant>, revision: u64, bytes_read: u64, bytes_written: u64,
    stats_at: Instant, stat_read: u64, stat_written: u64,
}
struct Listener {
    handle: Handle, group: String, socket: TcpListener, accepting: bool,
    pending: VecDeque<Accepted>, probes: HashMap<SocketAddr, String>,
}
struct Accepted { stream: Handle, peer: SocketAddr, probe: Option<String> }
struct Endpoint {
    handle: Handle, group: String, state: Arc<Mutex<Stream>>, socket: Option<TcpStream>,
    connection: Option<Arc<Connection>>, delivered: bool, accepted: bool, created: Instant,
    exchange: Option<(Caller, String, u64)>, closed_at: Option<Instant>, route:Option<String>,
}
struct Route { handle: Handle, group: String, pool: String }
struct Member { connection: Arc<Connection>, lease: String, idle_since: Instant, retiring: bool }
struct Pool {
    scope: WorkspaceScope, server: String, revision: String, members: Vec<Member>,
    building: usize, failure: Option<CoreError>, groups: std::collections::HashSet<String>,
}
struct PendingTcp {
    key: Key, group: String, pool: String, endpoint: String, host: String, port: u16,
    canceled: Arc<AtomicBool>, queued: Instant,
    opening: Option<Arc<Mutex<Option<Result<()>>>>>,
}
struct Splice { handle: Handle, group: String, left: String, right: String, key: Key, active: bool, created: Instant }
struct Receipt {
    caller: Caller,
    method: String, params: Value, scope: Option<WorkspaceScope>, group: Option<String>,
    canceled: Arc<AtomicBool>, outcome: Option<Result<Value>>, completed: Option<Instant>,
    claimed: bool, resources: Vec<String>,
}
#[derive(Default)]
pub(crate) struct Registry {
    scopes: HashMap<String, String>, views: HashMap<String, (String, WorkspaceScope)>,
    groups: HashMap<String, Group>, listeners: HashMap<String, Listener>,
    endpoints: HashMap<String, Endpoint>, routes: HashMap<String, Route>, pools: HashMap<String, Pool>,
    pending: Vec<PendingTcp>, splices: HashMap<String, Splice>,
    receipts: HashMap<Key, Receipt>, tombstones: HashMap<Key, Instant>,
    next_generation: u64, next_slot: u32, next_epoch: u32,
    attachments: HashMap<(String, u32), (u32, String)>,
    output_cursor:HashMap<String,u32>,
    closed_resources:HashMap<String,(Handle,String,&'static str,Instant)>,
}
fn stale() -> CoreError { CoreError::new("STALE_HANDLE", "网络资源不存在或已失效") }
fn capacity() -> CoreError { CoreError::new("CAPACITY", "网络资源达到上限，请释放其他规则或连接") }

#[cfg(test)]
mod budget_tests {
    use super::*;
    use sodeck_communication::binary::{Frame,Kind,WINDOW};
    #[test]
    fn pending_target_admission_is_bounded_globally_and_per_pool() {
        let mut n=Registry::default();
        for p in 0..4 {let pool=p.to_string();for _ in 0..16 {n.target_capacity(&pool).unwrap();n.pending.push(PendingTcp{key:("owner".into(),id()),group:"g".into(),pool:pool.clone(),endpoint:id(),host:"test.invalid".into(),port:80,canceled:Arc::new(AtomicBool::new(false)),queued:Instant::now(),opening:None});}assert_eq!(n.target_capacity(&pool).unwrap_err().code,"CAPACITY");}
        assert_eq!(n.target_capacity("extra").unwrap_err().code,"CAPACITY");assert_eq!(n.pending.len(),64);
    }
    #[test]
    fn endpoint_limits_bound_both_direction_buffers_to_64_mib() {
        let mut n=Registry::default();let bytes=vec![7;WINDOW];
        for group in 0..4 {
            for _ in 0..128 {n.reserve_endpoint(&group.to_string(),None,true).unwrap();}
            assert_eq!(n.reserve_endpoint(&group.to_string(),None,true).unwrap_err().code,"CAPACITY");
        }
        assert_eq!(n.reserve_endpoint("extra",None,true).unwrap_err().code,"CAPACITY");
        for e in n.endpoints.values(){let mut s=e.state.lock().unwrap();s.attach(1,1).unwrap();s.read_from(&mut bytes.as_slice());s.input(&Frame{kind:Kind::Data,slot:1,epoch:1,offset:0,payload:bytes.clone()}).unwrap();assert_eq!(s.buffered(),2*WINDOW);s.read_from(&mut bytes.as_slice());assert_eq!(s.buffered(),2*WINDOW);}
        assert_eq!(n.endpoints.values().map(|e|e.state.lock().unwrap().buffered()).sum::<usize>(),64*1024*1024);
        for e in n.endpoints.values(){e.state.lock().unwrap().close();}
        assert_eq!(n.endpoints.values().map(|e|e.state.lock().unwrap().buffered()).sum::<usize>(),0);
    }
}
impl Registry {
    fn target_capacity(&self,pool:&str)->Result<()> {if self.pending.len()>=64||self.pending.iter().filter(|o|o.pool==pool).count()>=16 {Err(capacity())}else{Ok(())}}
    fn handle(&mut self) -> Handle { self.next_generation += 1; Handle::new(self.next_generation) }
    fn check_scope(&self, value: &Value) -> Result<WorkspaceScope> {
        let scope: WorkspaceScope = serde_json::from_value(value.clone())?;
        if self.scopes.get(&scope.workspace_id) != Some(&scope.workspace_generation) {
            return Err(CoreError::new("STALE_SCOPE", "工作区代次已经失效"));
        }
        Ok(scope)
    }
    fn group(&mut self, caller: &Caller, p: &Value, allow_closed: bool) -> Result<String> {
        let scope = self.check_scope(&p["scope"])?;
        let handle = Handle::parse(&p["group"])?;
        let g = self.groups.get(&handle.id).ok_or_else(stale)?;
        if g.handle != handle || g.owner != caller.instance_id || g.scope != scope
            || (!allow_closed && g.closed.is_some()) { return Err(stale()); }
        self.claim_resource(&handle.id);
        Ok(handle.id)
    }
    fn endpoint(&mut self, group: &str, value: &Value) -> Result<String> {
        let h = Handle::parse(value)?;
        let e = self.endpoints.get(&h.id).ok_or_else(stale)?;
        if e.handle != h || e.group != group { return Err(stale()); }
        self.claim_resource(&h.id);
        if let Some(e) = self.endpoints.get_mut(&h.id) { e.delivered = true; }
        Ok(h.id)
    }
    fn reserve_endpoint(&mut self, group: &str, socket: Option<TcpStream>, delivered: bool) -> Result<Handle> {
        if self.endpoints.len() >= 512 || self.endpoints.values().filter(|e| e.group == group).count() >= 128 { return Err(capacity()); }
        if let Some(s) = &socket { s.set_nonblocking(true)?; s.set_nodelay(true)?; }
        let handle = self.handle();
        self.endpoints.insert(handle.id.clone(), Endpoint { handle: handle.clone(), group: group.into(),
            state: Arc::new(Mutex::new(Stream::default())), socket, connection: None,
            delivered, accepted:false, created: Instant::now(), exchange: None, closed_at:None, route:None });
        Ok(handle)
    }
    fn close_resource(&mut self, id: &str) {
        if self.groups.contains_key(id) {
            let g = self.groups.get_mut(id).unwrap();
            g.closed.get_or_insert_with(Instant::now); g.revision += 1;
            for r in self.receipts.values_mut().filter(|r| r.group.as_deref() == Some(id) && r.outcome.is_none() && !matches!(r.method.as_str(), "net.groups.close"|"net.resources.stop")) { r.canceled.store(true, Ordering::SeqCst); }
            let children: Vec<_> = self.listeners.values().filter(|l| l.group == id).map(|l| l.handle.id.clone())
                .chain(self.endpoints.values().filter(|e| e.group == id).map(|e| e.handle.id.clone()))
                .chain(self.routes.values().filter(|r| r.group == id).map(|r| r.handle.id.clone())).collect();
            for child in children { self.close_resource(&child); }
            return;
        }
        if let Some(l) = self.listeners.remove(id) {
            self.closed_resources.insert(id.into(),(l.handle,l.group,"listener",Instant::now()));
            for pending in l.pending { self.close_resource(&pending.stream.id); }
        }
        if let Some(e) = self.endpoints.get_mut(id) {
            e.state.lock().unwrap().close();
            if let Some(socket) = e.socket.take() { let _ = socket.shutdown(std::net::Shutdown::Both); }
        }
        if let Some(r)=self.routes.remove(id) {
            self.closed_resources.insert(id.into(),(r.handle,r.group,"route",Instant::now()));
            let endpoints:Vec<_>=self.endpoints.values().filter(|e|e.route.as_deref()==Some(id)).map(|e|e.handle.id.clone()).collect();
            for endpoint in endpoints {self.close_resource(&endpoint);}
        }
        if let Some(s) = self.splices.remove(id) { self.close_resource(&s.left); self.close_resource(&s.right); }
    }
    fn was_closed(&self,group:&str,value:&Value,kind:&str)->Result<bool> {
        let h=Handle::parse(value)?;
        Ok(self.closed_resources.get(&h.id).is_some_and(|(old,g,k,_)|*old==h&&g==group&&*k==kind))
    }
}

impl Core {
    pub(crate) fn start_network(&self) {
        let (send, receive) = std::sync::mpsc::sync_channel::<Job>(64);
        *self.network_jobs.lock().unwrap() = Some(send);
        let receive = Arc::new(Mutex::new(receive));
        for _ in 0..4 {
            let weak = Arc::downgrade(&self.shared); let receive = receive.clone();
            std::thread::Builder::new().name("sodeck-network-connect".into()).spawn(move || loop {
                let job = receive.lock().unwrap().recv_timeout(Duration::from_millis(50));
                let Some(shared) = weak.upgrade() else { break };
                let core = Core { shared, owns_lifetime: false };
                if core.network_stopped.load(Ordering::SeqCst) { break; }
                if let Ok(job) = job { job(&core); }
            }).expect("network connect executor");
        }
        let weak = Arc::downgrade(&self.shared);
        std::thread::Builder::new().name("sodeck-network-io".into()).spawn(move || loop {
            let Some(shared) = weak.upgrade() else { break };
            let core = Core { shared, owns_lifetime: false };
            if core.network_stopped.load(Ordering::SeqCst) { break; }
            let progress=core.network_tick(); drop(core);
            if progress {std::thread::yield_now();} else {std::thread::park_timeout(Duration::from_millis(1));}
        }).expect("network I/O executor");
    }
    fn queue_network(&self, job: Job) -> Result<()> {
        self.network_jobs.lock().unwrap().as_ref().ok_or_else(stale)?.try_send(job).map_err(|_| capacity())
    }
    pub fn register_network_view(&self, instance: &str, view: &str, scope: WorkspaceScope) -> Result<()> {
        let mut n = self.network.lock().unwrap();
        n.check_scope(&serde_json::to_value(&scope)?)?;
        if n.views.get(view).is_some_and(|v| v != &(instance.to_owned(), scope.clone())) { return Err(stale()); }
        n.views.insert(view.into(), (instance.into(), scope)); Ok(())
    }
    pub fn close_network_view(&self, instance: &str, view: &str) {
        let mut n = self.network.lock().unwrap();
        if n.views.get(view).is_some_and(|(owner, _)| owner == instance) { n.views.remove(view); }
        let groups: Vec<_> = n.groups.values().filter(|g| g.owner == instance && g.view == view).map(|g| g.handle.id.clone()).collect();
        for g in groups { n.close_resource(&g); }
    }
    pub(crate) fn network_stop_instance(&self, instance: &str) {
        let mut n = self.network.lock().unwrap();
        n.views.retain(|_, (owner, _)| owner != instance);
        let ids: Vec<_> = n.groups.values().filter(|g| g.owner == instance).map(|g| g.handle.id.clone()).collect();
        for id in ids { n.close_resource(&id); }
        for ((owner, _), r) in &n.receipts { if owner == instance { r.canceled.store(true, Ordering::SeqCst); } }
    }
    pub(crate) fn network_stop(&self) {
        self.network_stopped.store(true, Ordering::SeqCst);
        self.network_jobs.lock().unwrap().take();
        let mut n = self.network.lock().unwrap();
        let ids: Vec<_> = n.groups.keys().cloned().collect();
        n.scopes.clear(); n.views.clear();
        for id in ids { n.close_resource(&id); }
        for r in n.receipts.values() { r.canceled.store(true, Ordering::SeqCst); }
        for p in n.pools.values() { for m in &p.members { m.connection.stop(); } }
        drop(n);self.network_tick();
    }
    pub(crate) fn network_scope(&self, id: &str, generation: &str, register: bool) {
        let mut n = self.network.lock().unwrap();
        if register { n.scopes.insert(id.into(), generation.into()); }
        else {
            if n.scopes.get(id).is_some_and(|g| g == generation) { n.scopes.remove(id); }
            let ids: Vec<_> = n.groups.values().filter(|g| g.scope.workspace_id == id && g.scope.workspace_generation == generation).map(|g| g.handle.id.clone()).collect();
            for id in ids { n.close_resource(&id); }
            n.views.retain(|_, (_, s)| s.workspace_id != id || s.workspace_generation != generation);
        }
    }
}
