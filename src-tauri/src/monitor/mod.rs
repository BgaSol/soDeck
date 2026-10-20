mod commands;
mod windows;
pub use windows::*;
mod docker;
mod history;
mod linux;
mod model;
mod mounts;
#[cfg(test)]
pub(crate) mod tests;
pub use commands::*;

use crate::{
    connection::{self, SharedContext},
    model::{AppError, AppResult},
    ssh_transport::{Cancellation, SshConnection},
    State,
};
use history::History;
use model::*;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::ipc::Channel;

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone)]
struct Subscriber {
    session: String,
    identity: Option<String>,
    window: String,
    window_generation: u64,
    output: Channel<MonitorEvent>,
    interest: Interest,
}
struct Core {
    members: HashMap<String, SharedContext>,
    preferred_session: Option<String>,
    subscribers: HashMap<String, Subscriber>,
    metrics: Metrics,
    metrics_revision: u64,
    mounts: Mounts,
    mounts_revision: u64,
    history: History,
    schedule: mounts::Schedule,
    refresh: bool,
    docker: DockerState,
    verified_docker: Option<crate::docker::DockerStatus>,
    docker_revision: u64,
    docker_refresh: u64,
    images_running: bool,
    images_next: Instant,
    images_paused: bool,
}
impl Default for Core {
    fn default() -> Self {
        Self {
            members: HashMap::new(),
            preferred_session: None,
            subscribers: HashMap::new(),
            metrics: Metrics::default(),
            metrics_revision: 0,
            mounts: Mounts::default(),
            mounts_revision: 0,
            history: History::default(),
            schedule: mounts::Schedule::default(),
            refresh: true,
            docker: DockerState::default(),
            verified_docker: None,
            docker_revision: 0,
            docker_refresh: 0,
            images_running: false,
            images_next: Instant::now(),
            images_paused: false,
        }
    }
}
struct Entry {
    generation: String,
    core: Mutex<Core>,
    cancel: Cancellation,
    jobs: Arc<AtomicUsize>,
    traffic: Arc<crate::traffic::TrafficAccount>,
}
impl Entry {
    fn new(jobs: Arc<AtomicUsize>, traffic: Arc<crate::traffic::TrafficAccount>) -> Self {
        Self {
            generation: uuid::Uuid::new_v4().to_string(),
            core: Mutex::new(Core::default()),
            cancel: Cancellation::default(),
            jobs,
            traffic,
        }
    }
    fn check(&self, state: &State) -> AppResult<()> {
        self.cancel.check()?;
        state.ensure_running()?;
        let sessions: Vec<_> = self.core.lock().unwrap().members.keys().cloned().collect();
        let terminals = state.terminals.lock().unwrap();
        if !sessions.iter().any(|id| {
            terminals.get(id).is_some_and(|t| {
                !t.sender.cancel.is_cancelled() && !t.stream.lock().unwrap().closed
            })
        }) {
            return Err(AppError::new("cancelled", "服务器会话已结束"));
        }
        Ok(())
    }
    fn spec(&self) -> AppResult<SharedContext> {
        let core = self.core.lock().unwrap();
        core.preferred_session
            .as_ref()
            .and_then(|id| core.members.get(id))
            .or_else(|| core.members.values().next())
            .cloned()
            .ok_or_else(|| AppError::new("cancelled", "服务器会话已结束"))
    }
    fn snapshot(&self, c: &Core) -> MonitorEvent {
        MonitorEvent::Snapshot {
            generation: self.generation.clone(),
            metrics_revision: c.metrics_revision,
            mounts_revision: c.mounts_revision,
            metrics: c.metrics.clone(),
            mounts: c.mounts.clone(),
            docker_revision: c.docker_revision,
            docker: c.docker.clone(),
            traffic: self.traffic.snapshot(),
        }
    }
    fn publish(&self, state: &State, event: MonitorEvent) {
        let subscribers = self.core.lock().unwrap().subscribers.clone();
        let mut remove = Vec::new();
        for (id, sub) in subscribers {
            if state.owner(&sub.window).generation != sub.window_generation {
                remove.push(id);
                continue;
            }
            let access = if sub.identity.is_some() {
                if subscriber_valid(state, &sub) {
                    Ok(())
                } else {
                    Err(AppError::new("disconnected", "监控窗口已结束"))
                }
            } else {
                state.spec(&sub.session, &sub.window).map(|_| ())
            };
            match access {
                // Keep the old subscriber dormant while a workspace is in transit.
                // A failed move can restore its ownership without remounting React.
                Err(error) if error.kind == "permission" => continue,
                Err(_) => {
                    remove.push(id);
                    continue;
                }
                Ok(_) => (),
            }
            if sub.output.send(event.clone()).is_err() {
                remove.push(id);
            }
        }
        if !remove.is_empty() {
            let mut c = self.core.lock().unwrap();
            for id in remove {
                c.subscribers.remove(&id);
            }
        }
    }
    fn publish_mounts(&self, state: &State) {
        let event = {
            let mut c = self.core.lock().unwrap();
            c.mounts_revision += 1;
            MonitorEvent::Mounts {
                generation: self.generation.clone(),
                revision: c.mounts_revision,
                mounts: c.mounts.clone(),
            }
        };
        self.publish(state, event);
    }
    fn publish_docker(&self, state: &State) {
        let event = {
            let mut c = self.core.lock().unwrap();
            c.docker_revision += 1;
            MonitorEvent::Docker {
                generation: self.generation.clone(),
                revision: c.docker_revision,
                docker: c.docker.clone(),
            }
        };
        self.publish(state, event);
    }
    fn interests(&self, state: &State) -> Vec<Interest> {
        let subscribers: Vec<_> = self
            .core
            .lock()
            .unwrap()
            .subscribers
            .values()
            .cloned()
            .collect();
        subscribers
            .into_iter()
            .filter(|s| subscriber_valid(state, s))
            .map(|s| s.interest)
            .collect()
    }
}
fn subscriber_valid(state: &State, sub: &Subscriber) -> bool {
    state.owner(&sub.window).generation == sub.window_generation
        && match &sub.identity {
            Some(identity) => state
                .monitor
                .windows
                .lock()
                .unwrap()
                .get(&sub.window)
                .is_some_and(|binding| &binding.identity == identity),
            None => state.spec(&sub.session, &sub.window).is_ok(),
        }
}

#[derive(Default)]
pub struct MonitorManager {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    jobs: Arc<AtomicUsize>,
    windows: Mutex<HashMap<String, windows::WindowBinding>>,
    observers: Mutex<HashMap<String, Subscriber>>,
}
struct Job(Arc<AtomicUsize>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn spawn(entry: &Arc<Entry>, task: impl FnOnce() + Send + 'static) {
    entry.jobs.fetch_add(1, Ordering::SeqCst);
    let guard = Job(entry.jobs.clone());
    std::thread::spawn(move || {
        let _guard = guard;
        task();
    });
}
impl MonitorManager {
    fn subscribe(
        &self,
        state: &Arc<State>,
        spec: SharedContext,
        session: String,
        window: String,
        output: Channel<MonitorEvent>,
        interest: Interest,
    ) -> AppResult<String> {
        let identity = spec.identity().to_owned();
        let owner = state.owner(&window);
        let mut entries = self.entries.lock().unwrap();
        // Revalidate under the registry lock so a concurrently closing session cannot leave an orphan worker.
        state.spec(&session, &window)?;
        let created = !entries.contains_key(spec.identity());
        let entry = entries
            .entry(spec.identity().into())
            .or_insert_with(|| {
                Arc::new(Entry::new(
                    self.jobs.clone(),
                    spec.traffic.account(spec.identity()),
                ))
            })
            .clone();
        let id = uuid::Uuid::new_v4().to_string();
        let event = {
            let mut c = entry.core.lock().unwrap();
            let new_context = c
                .preferred_session
                .as_ref()
                .and_then(|id| c.members.get(id))
                .is_none_or(|previous| !Arc::ptr_eq(previous, &spec));
            if new_context {
                if c.docker.engine.status != "ready" {
                    c.docker_refresh += 1;
                }
                if c.images_paused {
                    c.images_next = Instant::now();
                }
                c.images_paused = false;
            }
            c.members.insert(session.clone(), spec);
            c.preferred_session = Some(session.clone());
            c.refresh = true;
            c.subscribers.insert(
                id.clone(),
                Subscriber {
                    session,
                    identity: None,
                    window,
                    window_generation: owner.generation,
                    output: output.clone(),
                    interest,
                },
            );
            entry.snapshot(&c)
        };
        self.attach_observers(state, &identity, &entry);
        drop(entries);
        // Initial snapshots and incrementals carry independent revisions for each data family.
        let _ = output.send(event);
        if created {
            let e = entry.clone();
            let s = Arc::downgrade(state);
            spawn(&entry, move || run_metrics(s, e));
            let e = entry.clone();
            let s = Arc::downgrade(state);
            spawn(&entry, move || mounts::run(s, e));
            let e = entry.clone();
            let s = Arc::downgrade(state);
            spawn(&entry, move || docker::run(s, e));
            let e = entry.clone();
            let s = Arc::downgrade(state);
            spawn(&entry, move || run_traffic(s, e));
        }
        Ok(id)
    }
    fn owned(&self, state: &State, id: &str, window: &str) -> AppResult<Arc<Entry>> {
        let entries = self.entries.lock().unwrap();
        for entry in entries.values() {
            let sub = entry.core.lock().unwrap().subscribers.get(id).cloned();
            if let Some(sub) = sub {
                if sub.window != window || !subscriber_valid(state, &sub) {
                    return Err(AppError::new("permission", "此窗口的监控订阅已失效"));
                }
                return Ok(entry.clone());
            }
        }
        Err(AppError::new("disconnected", "监控订阅已结束"))
    }
    pub fn close_session(&self, session: &str) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, entry| {
            let mut c = entry.core.lock().unwrap();
            c.members.remove(session);
            c.subscribers.retain(|_, s| s.session != session);
            let retain = !c.members.is_empty();
            if !retain {
                for sub in c.subscribers.values().filter(|sub| sub.identity.is_some()) {
                    let _ = sub.output.send(MonitorEvent::Connection {
                        generation: entry.generation.clone(),
                        connected: false,
                    });
                }
                entry.cancel.cancel();
            }
            retain
        });
    }
    pub fn stop_window(&self, window: &str) {
        let entries = self.entries.lock().unwrap();
        self.observers
            .lock()
            .unwrap()
            .retain(|_, s| s.window != window);
        for entry in entries.values() {
            entry
                .core
                .lock()
                .unwrap()
                .subscribers
                .retain(|_, s| s.window != window);
        }
    }
    pub fn shutdown(&self) -> AppResult<()> {
        for (_, entry) in self.entries.lock().unwrap().drain() {
            entry.cancel.cancel();
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.jobs.load(Ordering::SeqCst) != 0 {
            if Instant::now() >= deadline {
                return Err(AppError::new("busy", "监控连接尚未释放"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
}

fn run_metrics(state: Weak<State>, entry: Arc<Entry>) {
    let command = format!("sh -c {}", crate::ssh::quote(linux::COMMAND));
    let mut connection: Option<SshConnection> = None;
    let mut previous = None;
    let mut last = Instant::now() - Duration::from_secs(60);
    let mut retry_at = Instant::now();
    let mut failures = 0u32;
    let mut paused = false;
    let mut was_visible = false;
    loop {
        let Some(state) = state.upgrade() else { break };
        if entry.check(&state).is_err() {
            break;
        }
        let interests = entry.interests(&state);
        if interests.is_empty() {
            connection = None;
            previous = None;
            drop(state);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let (visible, force) = {
            let mut c = entry.core.lock().unwrap();
            let visible = interests.iter().any(|s| s.visible);
            let force = std::mem::take(&mut c.refresh);
            (visible, force)
        };
        let resumed = visible && !was_visible;
        was_visible = visible;
        let interval = Duration::from_secs(if visible { 2 } else { 10 });
        if force {
            paused = false;
            retry_at = Instant::now();
        }
        let due = !paused
            && Instant::now() >= retry_at
            && (force || resumed || last.elapsed() >= interval);
        if !due {
            drop(state);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        last = Instant::now();
        let result = (|| {
            if connection.is_none() {
                let spec = entry.spec()?;
                connection = Some(connection::connect_verified(
                    &spec,
                    &entry.cancel,
                    crate::traffic::TrafficPurpose::ResourceMonitor,
                )?);
            }
            let output = crate::remote_read::exec(
                connection.as_ref().unwrap().session(),
                &command,
                &|| entry.check(&state),
            )?;
            if output.code == 42 {
                return Err(AppError::new(
                    "unsupported",
                    "资源监控目前支持 Linux 服务器",
                ));
            }
            if output.code != 0 {
                return Err(AppError::new("unavailable", "远端无法执行资源采样命令"));
            }
            linux::parse(&output.stdout)
        })();
        if entry.check(&state).is_err() {
            break;
        }
        let event = {
            let mut c = entry.core.lock().unwrap();
            c.metrics_revision += 1;
            let sequence = c.metrics_revision;
            let at = now();
            let point = match result {
                Ok(raw) => {
                    let p = linux::sample(&raw, previous.as_ref(), &mut c.metrics, at, sequence);
                    previous = Some(raw);
                    failures = 0;
                    retry_at = Instant::now();
                    p
                }
                Err(error) => {
                    previous = None;
                    connection = None;
                    failures = failures.saturating_add(1);
                    paused = matches!(
                        error.kind.as_str(),
                        "hostKey" | "authentication" | "permission"
                    );
                    retry_at = Instant::now()
                        + Duration::from_secs(if error.kind == "unsupported" {
                            300
                        } else {
                            2u64.pow(failures.min(5)).min(30)
                        });
                    c.metrics.fail(
                        if error.kind == "unsupported" {
                            "unsupported"
                        } else {
                            "stale"
                        },
                        &error.message,
                    );
                    HistoryPoint {
                        sequence,
                        at,
                        cpu: None,
                        memory: None,
                        network: Default::default(),
                        gap: true,
                    }
                }
            };
            c.history.push(point.clone());
            MonitorEvent::Metrics {
                generation: entry.generation.clone(),
                revision: sequence,
                metrics: c.metrics.clone(),
                point,
            }
        };
        entry.publish(&state, event);
    }
}

fn run_traffic(state: Weak<State>, entry: Arc<Entry>) {
    let mut last = Instant::now() - Duration::from_secs(60);
    let mut revision = u64::MAX;
    loop {
        let Some(state) = state.upgrade() else { break };
        if entry.check(&state).is_err() {
            break;
        }
        let interests = entry.interests(&state);
        let requested = interests.iter().any(|i| i.client_traffic);
        let visible = interests.iter().any(|i| i.visible && i.client_traffic);
        if requested && last.elapsed() >= Duration::from_secs(if visible { 2 } else { 10 }) {
            let (traffic, point) = entry.traffic.update();
            if traffic.revision != revision {
                revision = traffic.revision;
                entry.publish(
                    &state,
                    MonitorEvent::Traffic {
                        generation: entry.generation.clone(),
                        traffic,
                        point,
                    },
                );
            }
            last = Instant::now();
        }
        drop(state);
        std::thread::sleep(Duration::from_millis(100));
    }
}

impl MonitorManager {
    pub(crate) fn docker_binding(
        &self,
        identity: &str,
    ) -> AppResult<crate::docker::model::EngineBinding> {
        let entries = self.entries.lock().unwrap();
        let entry = entries
            .get(identity)
            .ok_or_else(|| AppError::new("unavailable", "正在确认 Docker 引擎"))?;
        let core = entry.core.lock().unwrap();
        let engine = core
            .docker
            .engine
            .value
            .as_ref()
            .filter(|_| core.docker.can_attempt() && !entry.cancel.is_cancelled())
            .ok_or_else(|| AppError::new("unavailable", "Docker 引擎当前不可用，请刷新状态"))?;
        Ok(crate::docker::model::EngineBinding {
            token: format!("{}/{}", entry.generation, core.docker.engine_generation),
            generation: entry.generation.clone(),
            engine_generation: core.docker.engine_generation,
            endpoint: engine.endpoint.clone(),
            daemon_id: engine.daemon_id.clone(),
        })
    }
    pub(crate) fn retry_docker(&self, identity: &str) {
        if let Some(entry) = self.entries.lock().unwrap().get(identity) {
            let mut core = entry.core.lock().unwrap();
            if !core.docker.refreshing {
                core.docker_refresh += 1;
            }
        }
    }
    pub(crate) fn invalidate_docker(
        &self,
        identity: &str,
        binding: &crate::docker::model::EngineBinding,
    ) {
        if let Some(entry) = self.entries.lock().unwrap().get(identity) {
            let mut core = entry.core.lock().unwrap();
            if entry.generation == binding.generation
                && core.docker.engine_generation == binding.engine_generation
            {
                core.docker_refresh += 1;
            }
        }
    }
}
