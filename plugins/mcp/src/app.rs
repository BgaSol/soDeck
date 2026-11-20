use crate::{targets::Target, task::now, tasks::Registry};
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc, Mutex},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
pub struct App {
    pub client: Client,
    pub root: PathBuf,
    pub run: String,
    pub token: String,
    pub port: u16,
    pub http_state: Mutex<String>,
    pub shutdown: CancellationToken,
    pub runtime: tokio::runtime::Handle,
    pub waiting: Arc<Semaphore>,
    pub io: Arc<Semaphore>,
    pub(crate) active: Arc<Semaphore>,
    pub(crate) views: Mutex<HashMap<String, Arc<Target>>>,
    pub(crate) registry: Mutex<Registry>,
    pub(crate) numbers: AtomicU64,
    pub(crate) target_reads: Arc<Semaphore>,
    pub(crate) opens: crate::connections::Opens,
    pub(crate) core_changes: tokio::sync::watch::Sender<u64>,
    pub(crate) target_changes: tokio::sync::watch::Sender<u64>,
}
impl App {
    pub fn new(
        client: Client,
        root: PathBuf,
        token: String,
        port: u16,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            client,
            root,
            token,
            port,
            runtime,
            run: format!("{}-{:016x}", now(), rand::random::<u64>()),
            http_state: Mutex::new("starting".into()),
            shutdown: CancellationToken::new(),
            waiting: Arc::new(Semaphore::new(16)),
            io: Arc::new(Semaphore::new(4)),
            active: Arc::new(Semaphore::new(24)),
            views: Mutex::new(HashMap::new()),
            registry: Mutex::new(Registry::default()),
            numbers: AtomicU64::new(1),
            target_reads: Arc::new(Semaphore::new(4)),
            opens: crate::connections::Opens::default(),
            core_changes: tokio::sync::watch::channel(0).0,
            target_changes: tokio::sync::watch::channel(0).0,
        }
    }
    pub fn rpc(&self, method: &str, p: Value) -> Result<Value, Value> {
        self.client
            .call_detailed(method, p)
            .map_err(|e: RpcError| json!({"code":e.code,"message":e.message,"details":e.data}))
    }
}
