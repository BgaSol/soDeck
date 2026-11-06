use super::rpc::RpcError;
use crate::{core::AppCore, model::PublishMessage};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex, Weak,
    },
    thread,
    time::Instant,
};

pub(super) fn plugin_request(
    core: &Arc<AppCore>,
    id: &str,
    instance: &str,
    frame: &Value,
) -> Result<Value, RpcError> {
    match frame["method"].as_str().unwrap_or_default() {
        "settings.get" => core
            .settings_plugin_get(
                id,
                instance,
                frame["params"]["pageId"]
                    .as_str()
                    .ok_or_else(|| RpcError::invalid("缺少 pageId"))?,
            )
            .map_err(RpcError::execution),
        method if method.starts_with("net.") => core
            .network_request(id, instance, method, &frame["params"])
            .map_err(RpcError::execution),
        "metrics.networkSnapshot" => Ok(core.network_snapshot()),
        "files.request" => core
            .files_control(id, instance, &frame["params"])
            .map_err(RpcError::execution),
        "terminal.open" => {
            let p = &frame["params"];
            let view = p["viewId"]
                .as_str()
                .ok_or_else(|| RpcError::invalid("缺少 viewId"))?;
            let cols = p["cols"].as_u64().unwrap_or(80);
            let rows = p["rows"].as_u64().unwrap_or(24);
            if cols > 1000 || rows > 500 {
                return Err(RpcError::invalid("终端尺寸超出范围"));
            }
            core.terminal_open(id, instance, view, cols as u32, rows as u32)
                .map_err(RpcError::execution)
        }
        "terminal.close" => core
            .terminal_close(
                id,
                instance,
                frame["params"]["id"]
                    .as_str()
                    .ok_or_else(|| RpcError::invalid("缺少终端 ID"))?,
            )
            .map_err(RpcError::execution),
        "logging.write" => {
            let params = &frame["params"];
            let level = params["level"]
                .as_str()
                .ok_or_else(|| RpcError::invalid("缺少日志级别"))?;
            let message = params["message"]
                .as_str()
                .ok_or_else(|| RpcError::invalid("缺少日志正文"))?;
            if !["debug", "info", "warn", "error"].contains(&level) {
                return Err(RpcError::invalid("无效日志级别"));
            }
            let accepted = core
                .plugin_log(id, instance, level, message, params.get("fields").cloned())
                .map_err(RpcError::execution)?;
            Ok(json!({"accepted":accepted}))
        }
        "messages.publish" => {
            let input: PublishMessage = serde_json::from_value(frame["params"].clone())
                .map_err(|error| RpcError::invalid(error.to_string()))?;
            if input.title.trim().is_empty()
                || input.title.len() > 1024
                || input.body.len() > 16 * 1024
                || !["info", "success", "warning", "error"].contains(&input.level.as_str())
            {
                return Err(RpcError::invalid("消息标题、正文或级别无效"));
            }
            let message = core
                .publish(Some((id, instance)), input)
                .map_err(RpcError::execution)?;
            Ok(json!({"id":message.id}))
        }
        "ui.updateView" => core
            .ui_process_update(id, instance, &frame["params"])
            .map_err(RpcError::execution),
        _ => Err(RpcError::method_not_found()),
    }
}

const WORKERS: usize = 4;
const MAX_PENDING: usize = 32;
struct Job {
    key: u64,
    frame: Value,
    valid: Arc<AtomicBool>,
    deadline: Instant,
}
pub(super) struct Completion {
    pub key: u64,
    pub result: Result<Value, RpcError>,
}
struct Pending {
    id: Option<Value>,
    valid: Arc<AtomicBool>,
    deadline: Instant,
}

/// The reader owns request lifetimes; a fixed worker set executes handlers without
/// preventing it from receiving nested responses, shutdown, or process exit.
pub(super) struct Dispatcher {
    jobs: Option<SyncSender<Job>>,
    completed: Receiver<Completion>,
    pending: HashMap<u64, Pending>,
    sequence: u64,
    alive: Arc<AtomicBool>,
}
impl Dispatcher {
    pub fn new(core: Weak<AppCore>, id: String, instance: String) -> Self {
        let (jobs, receiver) = mpsc::sync_channel::<Job>(MAX_PENDING);
        let receiver = Arc::new(Mutex::new(receiver));
        let (completed, completion_rx) = mpsc::sync_channel(MAX_PENDING);
        let alive = Arc::new(AtomicBool::new(true));
        for _ in 0..WORKERS {
            let (receiver, completed, core, id, instance, alive) = (
                receiver.clone(),
                completed.clone(),
                core.clone(),
                id.clone(),
                instance.clone(),
                alive.clone(),
            );
            thread::spawn(move || loop {
                let job = { receiver.lock().unwrap().recv() };
                let Ok(job) = job else { break };
                if !alive.load(Ordering::Acquire) {
                    break;
                }
                if !job.valid.load(Ordering::Acquire) || Instant::now() >= job.deadline {
                    continue;
                }
                let Some(core) = core.upgrade() else { break };
                // A previous process must never act as a newly loaded instance.
                let current = core.instances.lock().unwrap().get(&id).map(|i| i.view());
                let valid = current.is_some_and(|v| {
                    v.instance_id.as_deref() == Some(&instance) && !v.cleanup_complete
                });
                let result = if valid {
                    plugin_request(&core, &id, &instance, &job.frame)
                } else {
                    Err(RpcError::execution("插件实例已失效"))
                };
                if alive.load(Ordering::Acquire) && job.valid.load(Ordering::Acquire) {
                    // Bound completions without losing successful results. The reader
                    // never joins workers; dropping its receiver releases these senders.
                    if completed
                        .send(Completion {
                            key: job.key,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Self {
            jobs: Some(jobs),
            completed: completion_rx,
            pending: HashMap::new(),
            sequence: 0,
            alive,
        }
    }
    pub fn submit(&mut self, frame: Value) -> Result<(), RpcError> {
        if self.pending.len() >= MAX_PENDING {
            return Err(RpcError::busy());
        }
        self.sequence = self.sequence.checked_add(1).ok_or_else(RpcError::busy)?;
        let key = self.sequence;
        let valid = Arc::new(AtomicBool::new(true));
        let deadline = Instant::now() + super::rpc::REQUEST_TIMEOUT;
        let id = frame.get("id").cloned();
        self.jobs
            .as_ref()
            .ok_or_else(RpcError::busy)?
            .try_send(Job {
                key,
                frame,
                valid: valid.clone(),
                deadline,
            })
            .map_err(|_| RpcError::busy())?;
        self.pending.insert(
            key,
            Pending {
                id,
                valid,
                deadline,
            },
        );
        Ok(())
    }
    pub fn drain(&mut self) -> Vec<Value> {
        let mut frames = Vec::new();
        // Deadlines win over a completion received after its request expired.
        self.pending.retain(|_, p| {
            if Instant::now() < p.deadline {
                return true;
            }
            p.valid.store(false, Ordering::Release);
            if let Some(id) = &p.id {
                frames.push(RpcError::timeout().response(id.clone()));
            }
            false
        });
        while let Ok(done) = self.completed.try_recv() {
            if let Some(p) = self.pending.remove(&done.key) {
                p.valid.store(false, Ordering::Release);
                if let Some(id) = p.id {
                    frames.push(super::rpc::response(id, done.result));
                }
            }
        }
        frames
    }
}
impl Drop for Dispatcher {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        for p in self.pending.values() {
            p.valid.store(false, Ordering::Release);
        }
        self.jobs.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_requests_expire_and_late_completions_cannot_reply_to_a_new_request() {
        // Keep handlers paused so the test controls deadlines and completion order.
        let (jobs, work) = mpsc::sync_channel(MAX_PENDING);
        let (completed, receiver) = mpsc::sync_channel(MAX_PENDING);
        let mut dispatcher = Dispatcher {
            jobs: Some(jobs),
            completed: receiver,
            pending: HashMap::new(),
            sequence: 0,
            alive: Arc::new(AtomicBool::new(true)),
        };
        for id in 0..MAX_PENDING {
            dispatcher
                .submit(json!({"id":id,"method":"logging.write"}))
                .unwrap();
        }
        assert_eq!(
            dispatcher
                .submit(json!({"id":"overflow"}))
                .unwrap_err()
                .code,
            -32001
        );
        for p in dispatcher.pending.values_mut() {
            p.deadline = Instant::now();
        }
        let replies = dispatcher.drain();
        assert_eq!(replies.len(), MAX_PENDING);
        assert!(replies.iter().all(|r| r["error"]["code"] == -32003));
        let stale = work.recv().unwrap();
        assert!(!stale.valid.load(Ordering::Acquire));
        while work.try_recv().is_ok() {}
        dispatcher
            .submit(json!({"id":"new","method":"logging.write"}))
            .unwrap();
        completed
            .send(Completion {
                key: stale.key,
                result: Ok(json!({"late":true})),
            })
            .unwrap();
        assert!(dispatcher.drain().is_empty());
        let current = work.recv().unwrap();
        completed
            .send(Completion {
                key: current.key,
                result: Ok(json!({"current":true})),
            })
            .unwrap();
        assert_eq!(
            dispatcher.drain(),
            vec![json!({"jsonrpc":"2.0","id":"new","result":{"current":true}})]
        );
    }
    #[test]
    fn dropping_dispatcher_invalidates_queued_work_for_the_old_instance() {
        let (jobs, work) = mpsc::sync_channel(MAX_PENDING);
        let (_, completed) = mpsc::sync_channel(MAX_PENDING);
        let mut dispatcher = Dispatcher {
            jobs: Some(jobs),
            completed,
            pending: HashMap::new(),
            sequence: 0,
            alive: Arc::new(AtomicBool::new(true)),
        };
        dispatcher
            .submit(json!({"id":1,"method":"messages.publish"}))
            .unwrap();
        let job = work.recv().unwrap();
        let alive = dispatcher.alive.clone();
        drop(dispatcher);
        assert!(!alive.load(Ordering::Acquire));
        assert!(!job.valid.load(Ordering::Acquire));
        assert!(work.recv().is_err());
    }
}
