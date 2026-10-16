pub mod commands;
pub mod model;
mod observe;
mod store;
#[cfg(test)]
mod tests;
mod worker;

use crate::{
    connection::{self, ConnectionPlan, SharedContext, WindowOwner},
    model::{AppError, AppResult},
    ssh_transport::Cancellation,
    State,
};
use model::*;
use polling::Poller;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use store::Store;

pub enum Control {
    Resume(WindowOwner),
    CancelAuth(String),
    Probe(String),
    CancelProbe(String),
}
pub struct Run {
    pub id: String,
    pub rule: Rule,
    pub server_ids: Vec<String>,
    pub stopped: AtomicBool,
    pub done: AtomicBool,
    pub ever_ready: AtomicBool,
    pub owner: Mutex<WindowOwner>,
    pub cancel: Mutex<Option<Cancellation>>,
    pub interaction: Mutex<Option<(String, Arc<connection::Interaction>)>>,
    pub context: Mutex<Option<SharedContext>>,
    pub view: Mutex<Runtime>,
    pub commands: SyncSender<Control>,
    pub wake: Arc<Poller>,
}
impl Run {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        {
            // Completion publishes its final phase and done under the same
            // lock, so a late stop cannot overwrite "stopped" with "stopping".
            let mut view = self.view.lock().unwrap();
            if !self.done.load(Ordering::SeqCst) {
                view.phase("stopping");
            }
        }
        if let Some(cancel) = self.cancel.lock().unwrap().as_ref() {
            cancel.cancel();
        }
        let _ = self.wake.notify();
    }
    pub fn control(&self, command: Control) -> AppResult<()> {
        if self.done.load(Ordering::SeqCst) || self.stopped.load(Ordering::SeqCst) {
            return Err(AppError::new("disconnected", "此转发正在停止或已结束"));
        }
        self.commands
            .try_send(command)
            .map_err(|_| AppError::new("busy", "操作队列已满，请稍后重试"))?;
        let _ = self.wake.notify();
        Ok(())
    }
    pub fn wait(&self) -> AppResult<()> {
        let start = Instant::now();
        while !self.done.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_secs(15) {
                return Err(AppError::new("busy", "转发资源尚未释放，请稍后重试"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
}
pub struct PortForwardManager {
    store: Mutex<Result<Store, AppError>>,
    pub runs: Mutex<HashMap<String, Arc<Run>>>,
    pub total_connections: AtomicUsize,
    pub intents: Mutex<HashMap<String, String>>,
    pub open_request: Mutex<Option<OpenRequest>>,
    observers: Mutex<HashMap<String, Arc<observe::Observer>>>,
    pub window_pending: AtomicBool,
}
impl Default for PortForwardManager {
    fn default() -> Self {
        Self::open(Path::new(":memory:"))
    }
}
impl PortForwardManager {
    pub fn open(path: &Path) -> Self {
        Self {
            store: Mutex::new(Store::open(path)),
            runs: Mutex::new(HashMap::new()),
            total_connections: AtomicUsize::new(0),
            intents: Mutex::new(HashMap::new()),
            open_request: Mutex::new(None),
            observers: Mutex::new(HashMap::new()),
            window_pending: AtomicBool::new(false),
        }
    }
    pub fn with_store<T>(&self, f: impl FnOnce(&mut Store) -> AppResult<T>) -> AppResult<T> {
        match &mut *self.store.lock().unwrap() {
            Ok(s) => f(s),
            Err(e) => Err(e.clone()),
        }
    }
    pub fn run(&self, id: &str, expected: &str) -> AppResult<Arc<Run>> {
        self.runs
            .lock()
            .unwrap()
            .get(id)
            .filter(|r| r.id == expected)
            .cloned()
            .ok_or_else(|| AppError::new("conflict", "运行已结束或已被新运行替换，请刷新"))
    }
    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for r in self.runs.lock().unwrap().values() {
            let view = r.view.lock().unwrap();
            if !r.done.load(Ordering::SeqCst) {
                summary.active += 1;
            }
            if view.phase == "ready" {
                summary.ready += 1;
            }
            summary.connections += view.active_connections;
            if matches!(view.phase.as_str(), "attention" | "failed") {
                summary.attention += 1;
            }
        }
        summary
    }
    pub fn has_server(&self, id: &str) -> AppResult<bool> {
        if self
            .with_store(|s| s.list())?
            .iter()
            .any(|r| r.settings.server_id == id)
        {
            return Ok(true);
        }
        Ok(self
            .runs
            .lock()
            .unwrap()
            .values()
            .any(|r| !r.done.load(Ordering::SeqCst) && r.server_ids.iter().any(|s| s == id)))
    }
    pub fn stop_all(&self) {
        self.intents.lock().unwrap().clear();
        for observer in self.observers.lock().unwrap().values() {
            observer.stop();
        }
        for run in self.runs.lock().unwrap().values() {
            run.stop();
        }
    }
    pub fn shutdown(&self) -> AppResult<()> {
        self.stop_all();
        let runs: Vec<_> = self.runs.lock().unwrap().values().cloned().collect();
        for run in runs {
            run.wait()?;
        }
        Ok(())
    }
    pub fn window_closed(&self, owner: &WindowOwner) {
        if owner.label != "port-forwarding" {
            return;
        }
        self.observers.lock().unwrap().retain(|_, observer| {
            if observer.owner.generation == owner.generation {
                observer.stop();
                false
            } else {
                true
            }
        });
        self.intents.lock().unwrap().clear();
        let runs: Vec<_> = self.runs.lock().unwrap().values().cloned().collect();
        for run in runs {
            let current = run.owner.lock().unwrap();
            if current.label != owner.label || current.generation != owner.generation {
                continue;
            }
            drop(current);
            if !run.ever_ready.load(Ordering::SeqCst) {
                run.stop();
            } else if run.interaction.lock().unwrap().is_some() {
                if let Some(cancel) = run.cancel.lock().unwrap().as_ref() {
                    cancel.cancel();
                }
            }
        }
    }
    pub fn snapshot(&self, state: &State) -> AppResult<Vec<RuleView>> {
        let rules = self.with_store(|s| s.list())?;
        rules
            .into_iter()
            .map(|rule| {
                let plan = state
                    .database
                    .lock()
                    .unwrap()
                    .resolve_connection(&rule.settings.server_id)
                    .and_then(ConnectionPlan::new);
                let run = self.runs.lock().unwrap().get(&rule.id).cloned();
                let runtime = run.as_ref().map(|r| r.view.lock().unwrap().clone());
                let route_changed = if let (Some(r), Ok(p)) = (&run, &plan) {
                    r.context
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|c| !c.matches_plan(p))
                } else {
                    false
                };
                let pending_changes = runtime.as_ref().is_some_and(|v| {
                    run.as_ref().is_some_and(|r| !r.done.load(Ordering::SeqCst))
                        && (v.settings != rule.settings || route_changed)
                });
                Ok(RuleView {
                    rule,
                    runtime,
                    pending_changes,
                    server_error: plan.err().map(|e| e.message),
                })
            })
            .collect()
    }
    pub fn emergency_snapshot(&self) -> Vec<RuleView> {
        self.runs
            .lock()
            .unwrap()
            .values()
            .map(|run| RuleView {
                rule: run.rule.clone(),
                runtime: Some(run.view.lock().unwrap().clone()),
                pending_changes: false,
                server_error: Some("配置存储不可用；仍可停止当前运行".into()),
            })
            .collect()
    }
    /// Caller holds State.configuration. The worker never holds it during I/O.
    pub fn start(
        &self,
        state: Arc<State>,
        mut rule: Rule,
        plan: Arc<ConnectionPlan>,
        owner: WindowOwner,
    ) -> AppResult<String> {
        state.ensure_running()?;
        rule.validate()?;
        let generation = state.window_generations.lock().unwrap();
        if generation.get(&owner.label).copied().unwrap_or(0) != owner.generation
            || state.closed_windows.lock().unwrap().contains(&owner.label)
        {
            return Err(AppError::new("cancelled", "管理窗口已关闭"));
        }
        let mut runs = self.runs.lock().unwrap();
        if let Some(run) = runs
            .get(&rule.id)
            .filter(|r| !r.done.load(Ordering::SeqCst))
        {
            if run.stopped.load(Ordering::SeqCst) {
                return Err(AppError::new("busy", "旧运行尚未结束"));
            }
            return Ok(run.id.clone());
        }
        if runs
            .values()
            .filter(|r| !r.done.load(Ordering::SeqCst))
            .count()
            >= 32
        {
            return Err(AppError::new("limit", "最多同时运行 32 条转发规则"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (send, receive) = mpsc::sync_channel(16);
        let run = Arc::new(Run {
            id: id.clone(),
            rule: rule.clone(),
            server_ids: plan.server_ids(),
            stopped: AtomicBool::new(false),
            done: AtomicBool::new(false),
            ever_ready: AtomicBool::new(false),
            owner: Mutex::new(owner),
            cancel: Mutex::new(None),
            interaction: Mutex::new(None),
            context: Mutex::new(None),
            view: Mutex::new(Runtime {
                run_id: id.clone(),
                phase: "starting".into(),
                actual_address: None,
                settings: rule.settings.clone(),
                connection: None,
                started_at: crate::traffic::now(),
                active_connections: 0,
                sent_bytes: "0".into(),
                received_bytes: "0".into(),
                sent_per_second: 0.0,
                received_per_second: 0.0,
                attempts: 0,
                retry_at: None,
                error: None,
                prompt: None,
                target_result: None,
                probe_id: None,
                notes: Vec::new(),
            }),
            commands: send,
            wake: Arc::new(Poller::new()?),
        });
        runs.insert(rule.id.clone(), run.clone());
        drop(runs);
        drop(generation);
        let rule_id = rule.id.clone();
        std::thread::Builder::new()
            .name("port-forward".into())
            .spawn(move || worker::run(state, run, rule, plan, receive))
            .map_err(|e| {
                if let Some(run) = self.runs.lock().unwrap().get(&rule_id) {
                    run.done.store(true, Ordering::SeqCst);
                    run.view.lock().unwrap().phase("failed");
                }
                AppError::new("internal", e)
            })?;
        Ok(id)
    }
}
