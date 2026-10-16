use super::*;
use crate::model::ServerView;
use serde::Serialize;
use std::sync::atomic::AtomicU64;
use tauri::ipc::Channel;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub subscription_id: String,
    pub revision: u64,
    pub rules: Vec<RuleView>,
    pub servers: Vec<ServerView>,
    pub selected_server: Option<String>,
    pub error: Option<AppError>,
}
pub struct Observer {
    pub owner: WindowOwner,
    revision: AtomicU64,
    acknowledged: AtomicU64,
    stopped: AtomicBool,
}
impl Observer {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}
impl PortForwardManager {
    pub fn subscribe(
        &self,
        state: Arc<State>,
        owner: WindowOwner,
        output: Channel<Frame>,
    ) -> AppResult<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let observer = Arc::new(Observer {
            owner,
            revision: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        });
        let mut observers = self.observers.lock().unwrap();
        for old in observers.values() {
            old.stop();
        }
        observers.clear();
        observers.insert(id.clone(), observer.clone());
        drop(observers);
        let subscription_id = id.clone();
        let weak = Arc::downgrade(&state);
        std::thread::spawn(move || {
            let mut sent_at = Instant::now();
            loop {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                if observer.stopped.load(Ordering::SeqCst) || state.ensure_running().is_err() {
                    break;
                }
                let current = state.owner(&observer.owner.label);
                if current.generation != observer.owner.generation
                    || state
                        .closed_windows
                        .lock()
                        .unwrap()
                        .contains(&observer.owner.label)
                {
                    break;
                }
                let revision = observer.revision.load(Ordering::SeqCst);
                if observer.acknowledged.load(Ordering::SeqCst) == revision {
                    // At most one unacknowledged frame. Current auth challenges stay
                    // in Runtime until answered/expired, so coalescing cannot lose them.
                    let _gate = state.configuration.lock().unwrap();
                    let result = state.port_forwarding.snapshot(&state);
                    let servers = state
                        .database
                        .lock()
                        .unwrap()
                        .list()
                        .map(|v| v.into_iter().map(|s| s.view()).collect());
                    let error = result
                        .as_ref()
                        .err()
                        .cloned()
                        .or_else(|| servers.as_ref().err().cloned());
                    let frame = Frame {
                        subscription_id: subscription_id.clone(),
                        revision: revision + 1,
                        rules: result
                            .unwrap_or_else(|_| state.port_forwarding.emergency_snapshot()),
                        servers: servers.unwrap_or_default(),
                        selected_server: state
                            .port_forwarding
                            .selected_server
                            .lock()
                            .unwrap()
                            .take(),
                        error,
                    };
                    observer.revision.store(revision + 1, Ordering::SeqCst);
                    if output.send(frame).is_err() {
                        break;
                    }
                    sent_at = Instant::now();
                } else if sent_at.elapsed() > Duration::from_secs(15) {
                    break;
                }
                drop(state);
                std::thread::sleep(Duration::from_millis(250));
            }
            if let Some(state) = weak.upgrade() {
                state
                    .port_forwarding
                    .observers
                    .lock()
                    .unwrap()
                    .remove(&subscription_id);
            }
        });
        Ok(id)
    }
    pub fn ack(&self, id: &str, revision: u64, owner: &WindowOwner) -> AppResult<()> {
        let observers = self.observers.lock().unwrap();
        let observer = observers
            .get(id)
            .ok_or_else(|| AppError::new("subscription", "订阅已结束，请重新连接"))?;
        if observer.owner.label != owner.label
            || observer.owner.generation != owner.generation
            || revision != observer.revision.load(Ordering::SeqCst)
        {
            return Err(AppError::new("conflict", "状态帧已过期"));
        }
        observer.acknowledged.store(revision, Ordering::SeqCst);
        Ok(())
    }
}
