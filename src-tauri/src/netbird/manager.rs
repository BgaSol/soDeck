use super::{model::*, query, store::Store};
use crate::{
    connection::{self, SharedContext, WindowOwner},
    model::{AppError, AppResult},
    ssh_transport::Cancellation,
    traffic::TrafficPurpose,
    State,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

pub(super) struct Subscriber {
    pub owner: WindowOwner,
    pub session: String,
    pub interest: Interest,
    events: Channel<Frame>,
    sent: Option<u64>,
    last: u64,
}
pub(super) struct Core {
    pub snapshot: Snapshot,
    pub operations: Vec<Operation>,
    subscribers: HashMap<String, Subscriber>,
    revision: u64,
    pub refreshing: bool,
    pub generation: u64,
    pub next: Instant,
    worker: bool,
    pub busy: bool,
}
pub(super) struct Entry {
    pub identity: String,
    pub core: Mutex<Core>,
    pub poll: Mutex<Option<Cancellation>>,
}
pub(super) struct Draft {
    pub owner: WindowOwner,
    pub identity: String,
    pub preview: Preview,
    pub signature: String,
}
pub struct NetBirdManager {
    pub(super) entries: Mutex<HashMap<String, Arc<Entry>>>,
    pub(super) store: Mutex<AppResult<Store>>,
    pub(super) drafts: Mutex<HashMap<String, Draft>>,
    pub(super) locks: Mutex<HashMap<String, Vec<String>>>,
    jobs: Mutex<HashMap<String, Cancellation>>,
}
pub(super) struct Job<'a> {
    manager: &'a NetBirdManager,
    pub cancel: Cancellation,
    id: String,
}
impl Drop for Job<'_> {
    fn drop(&mut self) {
        self.manager.jobs.lock().unwrap().remove(&self.id);
    }
}
impl Default for NetBirdManager {
    fn default() -> Self {
        Self::open(Path::new(":memory:"))
    }
}
impl NetBirdManager {
    pub fn open(path: &Path) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            store: Mutex::new(Store::open(path)),
            drafts: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
        }
    }
    pub(super) fn with_store<T>(&self, f: impl FnOnce(&Store) -> AppResult<T>) -> AppResult<T> {
        let store = self.store.lock().unwrap();
        match &*store {
            Ok(s) => f(s),
            Err(e) => Err(e.clone()),
        }
    }
    pub(super) fn job(&self) -> AppResult<Job<'_>> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.len() >= 12 {
            return Err(AppError::new("busy", "NetBird 后台请求过多，请稍后重试"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancel = Cancellation::default();
        jobs.insert(id.clone(), cancel.clone());
        Ok(Job {
            manager: self,
            cancel,
            id,
        })
    }
    pub(super) fn entry(&self, identity: &str) -> AppResult<Arc<Entry>> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(e) = entries.get(identity) {
            return Ok(e.clone());
        }
        let operations = self
            .with_store(|s| s.list(identity))?
            .into_iter()
            .map(|r| r.operation)
            .collect();
        let entry = Arc::new(Entry {
            identity: identity.into(),
            poll: Mutex::new(None),
            core: Mutex::new(Core {
                snapshot: Snapshot::default(),
                operations,
                subscribers: HashMap::new(),
                revision: 1,
                refreshing: false,
                generation: 0,
                next: Instant::now(),
                worker: false,
                busy: false,
            }),
        });
        entries.insert(identity.into(), entry.clone());
        Ok(entry)
    }
    pub fn subscribe(
        &self,
        state: &Arc<State>,
        session: String,
        owner: WindowOwner,
        interest: Interest,
        events: Channel<Frame>,
    ) -> AppResult<String> {
        interest.validate()?;
        let spec = state.spec(&session, &owner.label)?;
        let entry = self.entry(spec.identity())?;
        let id = uuid::Uuid::new_v4().to_string();
        let start = {
            let mut c = entry.core.lock().unwrap();
            c.subscribers.insert(
                id.clone(),
                Subscriber {
                    owner,
                    session,
                    interest,
                    events,
                    sent: None,
                    last: 0,
                },
            );
            c.next = Instant::now();
            let start = !c.worker;
            c.worker = true;
            start
        };
        entry.publish();
        if start {
            let weak = Arc::downgrade(state);
            let entry = entry.clone();
            std::thread::spawn(move || loop {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                if state.ensure_running().is_err() {
                    entry.core.lock().unwrap().worker = false;
                    break;
                }
                let work = {
                    let mut c = entry.core.lock().unwrap();
                    if c.subscribers.is_empty() {
                        c.worker = false;
                        break;
                    }
                    let sub = c
                        .subscribers
                        .values()
                        .find(|s| s.interest.level == "visible")
                        .or_else(|| {
                            c.subscribers
                                .values()
                                .find(|s| s.interest.level == "background")
                        });
                    let target = sub.map(|s| {
                        (
                            s.session.clone(),
                            s.owner.clone(),
                            if s.interest.level == "visible" {
                                15
                            } else {
                                60
                            },
                        )
                    });
                    if !c.busy && !c.refreshing && c.next <= Instant::now() {
                        if let Some((session, owner, seconds)) = target {
                            c.refreshing = true;
                            c.next = Instant::now() + Duration::from_secs(seconds);
                            Some((session, owner, c.generation))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some((session, owner, generation)) = work {
                    entry.publish();
                    let result = (|| {
                        let job = state.netbird.job()?;
                        *entry.poll.lock().unwrap() = Some(job.cancel.clone());
                        let spec = state.spec(&session, &owner.label)?;
                        let check = || {
                            job.cancel.check()?;
                            state
                                .netbird
                                .check_owner(&state, &session, &owner, spec.identity())
                        };
                        check()?;
                        let transport = connection::connect_verified(
                            &spec,
                            &job.cancel,
                            TrafficPurpose::NetBirdMonitor,
                        )?;
                        let path = state.netbird.with_store(|s| s.path(&spec.target().id))?;
                        let old = entry.core.lock().unwrap().snapshot.clone();
                        query::detect(transport.session(), path.as_deref(), &old, &check)
                    })();
                    *entry.poll.lock().unwrap() = None;
                    {
                        let mut c = entry.core.lock().unwrap();
                        c.refreshing = false;
                        if !c.busy && c.generation == generation {
                            match result {
                                Ok(snapshot) => c.snapshot = snapshot,
                                Err(e) => {
                                    c.snapshot.collection_status =
                                        if c.snapshot.updated_at.is_some() {
                                            "stale"
                                        } else {
                                            "unavailable"
                                        }
                                        .into();
                                    c.snapshot.error = Some(e.message);
                                }
                            }
                        }
                    }
                    entry.publish();
                }
                drop(state);
                std::thread::sleep(Duration::from_millis(250));
            });
        }
        Ok(id)
    }
    pub(super) fn check_owner(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        identity: &str,
    ) -> AppResult<()> {
        if state.owner(&owner.label).generation != owner.generation
            || state.spec(session, &owner.label)?.identity() != identity
        {
            return Err(AppError::new("conflict", "窗口或 SSH 会话已变化"));
        }
        Ok(())
    }
    fn subscription(&self, id: &str, owner: &WindowOwner) -> AppResult<Arc<Entry>> {
        let entries = self.entries.lock().unwrap();
        for e in entries.values() {
            let c = e.core.lock().unwrap();
            if let Some(s) = c.subscribers.get(id) {
                if s.owner.label != owner.label || s.owner.generation != owner.generation {
                    return Err(AppError::new("permission", "订阅不属于此窗口"));
                }
                return Ok(e.clone());
            }
        }
        Err(AppError::new("disconnected", "NetBird 订阅已结束"))
    }
    pub fn interest(&self, id: &str, owner: &WindowOwner, interest: Interest) -> AppResult<()> {
        interest.validate()?;
        let e = self.subscription(id, owner)?;
        let pause = {
            let mut c = e.core.lock().unwrap();
            c.subscribers.get_mut(id).unwrap().interest = interest;
            c.next = Instant::now();
            c.subscribers.values().all(|s| s.interest.level == "none")
        };
        if pause {
            if let Some(cancel) = e.poll.lock().unwrap().as_ref() {
                cancel.cancel();
            }
        }
        Ok(())
    }
    pub fn ack(&self, id: &str, owner: &WindowOwner, revision: u64) -> AppResult<()> {
        let e = self.subscription(id, owner)?;
        {
            let mut c = e.core.lock().unwrap();
            let s = c.subscribers.get_mut(id).unwrap();
            if s.sent == Some(revision) {
                s.sent = None;
            }
        }
        e.send();
        Ok(())
    }
    pub fn unsubscribe(&self, id: &str, owner: &WindowOwner) -> AppResult<()> {
        let e = self.subscription(id, owner)?;
        e.core.lock().unwrap().subscribers.remove(id);
        if e.core.lock().unwrap().subscribers.is_empty() {
            if let Some(c) = e.poll.lock().unwrap().as_ref() {
                c.cancel();
            }
        }
        Ok(())
    }
    pub fn refresh(&self, id: &str, owner: &WindowOwner) -> AppResult<()> {
        let e = self.subscription(id, owner)?;
        e.core.lock().unwrap().next = Instant::now();
        Ok(())
    }
    pub fn close_session(&self, id: &str) {
        for e in self.entries.lock().unwrap().values() {
            let empty = {
                let mut c = e.core.lock().unwrap();
                c.subscribers.retain(|_, s| s.session != id);
                if c.subscribers.is_empty() {
                    c.snapshot.collection_status = "disconnected".into();
                    true
                } else {
                    false
                }
            };
            if empty {
                if let Some(c) = e.poll.lock().unwrap().as_ref() {
                    c.cancel();
                }
            }
        }
    }
    pub fn window_closed(&self, label: &str) {
        self.drafts
            .lock()
            .unwrap()
            .retain(|_, d| d.owner.label != label);
        for e in self.entries.lock().unwrap().values() {
            e.core
                .lock()
                .unwrap()
                .subscribers
                .retain(|_, s| s.owner.label != label);
            if e.core.lock().unwrap().subscribers.is_empty() {
                if let Some(c) = e.poll.lock().unwrap().as_ref() {
                    c.cancel();
                }
            }
        }
    }
    pub fn has_server(&self, id: &str) -> bool {
        self.locks
            .lock()
            .unwrap()
            .values()
            .any(|ids| ids.iter().any(|s| s == id))
    }
    pub fn blocks_update(&self) -> AppResult<bool> {
        Ok(self.active() || self.with_store(|s| s.needs_update_attention())?)
    }
    pub fn active(&self) -> bool {
        !self.locks.lock().unwrap().is_empty()
    }
    pub fn forget_server(&self, id: &str) -> AppResult<()> {
        self.with_store(|s| s.set_path(id, None))
    }
    pub fn shutdown(&self) -> AppResult<()> {
        for e in self.entries.lock().unwrap().values() {
            e.core.lock().unwrap().subscribers.clear();
        }
        for cancel in self.jobs.lock().unwrap().values() {
            cancel.cancel();
        }
        let until = Instant::now() + Duration::from_secs(5);
        while !self.jobs.lock().unwrap().is_empty() || self.active() {
            if Instant::now() >= until {
                return Err(AppError::new(
                    "busy",
                    "NetBird 后台任务尚未结束，请稍后退出",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
    pub(super) fn bound(
        &self,
        spec: &SharedContext,
        token: &str,
    ) -> AppResult<(Arc<Entry>, Snapshot)> {
        let e = self.entry(spec.identity())?;
        let snapshot = e.core.lock().unwrap().snapshot.clone();
        if snapshot.binding.as_ref().is_none_or(|b| b.token != token) {
            return Err(AppError::new("conflict", "客户端绑定已变化，请刷新后重试"));
        }
        Ok((e, snapshot))
    }
    pub(super) fn read<T>(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        token: &str,
        task: impl FnOnce(
            &ssh2::Session,
            &SharedContext,
            &Snapshot,
            &dyn Fn() -> AppResult<()>,
        ) -> AppResult<T>,
    ) -> AppResult<T> {
        let spec = state.spec(session, &owner.label)?;
        let (_, old) = self.bound(&spec, token)?;
        let job = self.job()?;
        let check = || {
            job.cancel.check()?;
            self.check_owner(state, session, owner, spec.identity())
        };
        check()?;
        let transport =
            connection::connect_verified(&spec, &job.cancel, TrafficPurpose::NetBirdMonitor)?;
        let path = self.with_store(|s| s.path(&spec.target().id))?;
        let fresh = query::detect(transport.session(), path.as_deref(), &old, &check)?;
        if fresh.binding.as_ref().is_none_or(|b| b.token != token) {
            return Err(AppError::new("conflict", "客户端已变化，请刷新后重试"));
        }
        let result = task(transport.session(), &spec, &fresh, &check)?;
        check()?;
        self.bound(&spec, token)?;
        Ok(result)
    }
}
impl Entry {
    pub fn publish(&self) {
        self.core.lock().unwrap().revision += 1;
        self.send();
    }
    fn send(&self) {
        let frames = {
            let mut c = self.core.lock().unwrap();
            let revision = c.revision;
            let snapshot = c.snapshot.clone();
            let operations = c.operations.clone();
            let refreshing = c.refreshing;
            c.subscribers
                .iter_mut()
                .filter_map(|(id, s)| {
                    if s.sent.is_some() || s.last >= revision {
                        return None;
                    }
                    s.sent = Some(revision);
                    s.last = revision;
                    Some((
                        id.clone(),
                        s.events.clone(),
                        Frame {
                            subscription_id: id.clone(),
                            revision,
                            connection_identity: self.identity.clone(),
                            snapshot: snapshot.clone(),
                            operations: operations.clone(),
                            refreshing,
                        },
                    ))
                })
                .collect::<Vec<_>>()
        };
        for (id, events, frame) in frames {
            if events.send(frame).is_err() {
                self.core.lock().unwrap().subscribers.remove(&id);
            }
        }
    }
}
