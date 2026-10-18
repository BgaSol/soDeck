use super::{executor, inspect, model::*, query};
use crate::{
    connection::{self, SharedContext, WindowOwner},
    model::{AppError, AppResult},
    ssh_transport::Cancellation,
    traffic::{now, TrafficPurpose},
    State,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

#[derive(Clone)]
struct Subscriber {
    owner: WindowOwner,
    session: String,
    identity: String,
    output: Channel<Frame>,
    target: Option<String>,
    last: String,
    revision: u64,
    awaiting: bool,
}
struct CachedDetail {
    state: DetailState,
    next: Instant,
    running: bool,
    version: u64,
}
struct Record {
    view: Operation,
    identity: String,
    servers: Vec<String>,
    request: OperationRequest,
    cancel: Cancellation,
}
struct Draft {
    preview: Preview,
    identity: String,
    owner: WindowOwner,
    fingerprint: String,
    create: Option<super::create::Spec>,
    transfer: Option<super::transfer::Spec>,
}
#[derive(Default)]
struct Core {
    subscribers: HashMap<String, Subscriber>,
    details: HashMap<String, CachedDetail>,
    records: Vec<Record>,
    drafts: HashMap<String, Draft>,
}
pub struct DockerManager {
    store: Mutex<Result<super::store::Store, AppError>>,
    core: Mutex<Core>,
    pumping: AtomicBool,
    catalog_revision: AtomicU64,
    jobs: AtomicUsize,
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn detail_key(identity: &str, engine: &EngineBinding, id: &str) -> String {
    format!("{identity}\0{}\0{id}", engine.token)
}
fn check_binding(state: &State, spec: &SharedContext, engine: &EngineBinding) -> AppResult<()> {
    if state.monitor.docker_binding(spec.identity())? != *engine {
        return Err(AppError::new("conflict", "Docker 引擎已变化，请刷新后重试"));
    }
    Ok(())
}
pub(super) fn verify_engine(
    session: &ssh2::Session,
    engine: &EngineBinding,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<()> {
    let current = query::detect_on(session, check)?;
    if current.state != "running" {
        return Err(AppError::new("docker", current.message));
    }
    if current.endpoint != engine.endpoint || current.daemon_id != engine.daemon_id {
        return Err(AppError::new("conflict", "Docker 目标已变化，操作未发送"));
    }
    Ok(())
}
fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    request_id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, Option<String>)> {
    if let Action::Transfer { kind, .. } = action {
        super::transfer::observe(session, endpoint, id, kind, check).map(|v| (v, None))
    } else if matches!(action, Action::Create { .. }) {
        super::create::observe(session, endpoint, request_id, check)
    } else if action.is_container() {
        inspect::container(session, endpoint, id, check).map(|d| (d.state, d.health))
    } else {
        super::operations::observe(session, endpoint, id, action, check).map(|state| (state, None))
    }
}
impl Default for DockerManager {
    fn default() -> Self {
        Self::open(std::path::Path::new(":memory:"))
    }
}
impl DockerManager {
    pub fn open(path: &std::path::Path) -> Self {
        Self {
            store: Mutex::new(super::store::Store::open(path)),
            core: Mutex::default(),
            pumping: AtomicBool::new(false),
            catalog_revision: AtomicU64::new(0),
            jobs: AtomicUsize::new(0),
        }
    }
    pub(super) fn with_store<T>(
        &self,
        f: impl FnOnce(&mut super::store::Store) -> AppResult<T>,
    ) -> AppResult<T> {
        match &mut *self.store.lock().unwrap() {
            Ok(store) => f(store),
            Err(e) => Err(e.clone()),
        }
    }
    pub(super) fn catalog_changed(&self) {
        self.catalog_revision.fetch_add(1, Ordering::SeqCst);
    }
    pub fn has_associations(&self, server: &str) -> AppResult<bool> {
        self.with_store(|s| Ok(!s.list(server)?.is_empty()))
    }

    pub fn subscribe(
        &self,
        state: &Arc<State>,
        session: String,
        owner: WindowOwner,
        output: Channel<Frame>,
    ) -> AppResult<String> {
        let spec = state.spec(&session, &owner.label)?;
        let id = uuid();
        self.core.lock().unwrap().subscribers.insert(
            id.clone(),
            Subscriber {
                owner,
                session,
                identity: spec.identity().into(),
                output,
                target: None,
                last: String::new(),
                revision: 0,
                awaiting: false,
            },
        );
        if !self.pumping.swap(true, Ordering::SeqCst) {
            let weak = Arc::downgrade(state);
            std::thread::spawn(move || loop {
                let Some(state) = weak.upgrade() else { break };
                if !state.stopping.load(Ordering::SeqCst) {
                    state.docker.tick(&state);
                }
                drop(state);
                std::thread::sleep(Duration::from_millis(200));
            });
        }
        Ok(id)
    }
    fn subscriber(&self, state: &State, id: &str, owner: &WindowOwner) -> AppResult<Subscriber> {
        let sub = self
            .core
            .lock()
            .unwrap()
            .subscribers
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::new("permission", "Docker 订阅已结束"))?;
        if sub.owner.label != owner.label || sub.owner.generation != owner.generation {
            return Err(AppError::new("permission", "Docker 订阅不属于当前窗口"));
        }
        state.spec(&sub.session, &owner.label)?;
        Ok(sub)
    }
    pub fn interest(
        &self,
        state: &State,
        id: &str,
        owner: &WindowOwner,
        target: Option<String>,
    ) -> AppResult<()> {
        self.subscriber(state, id, owner)?;
        if let Some(id) = &target {
            validate_id(id)?;
        }
        if let Some(sub) = self.core.lock().unwrap().subscribers.get_mut(id) {
            sub.target = target;
        }
        Ok(())
    }
    pub fn ack(
        &self,
        state: &State,
        id: &str,
        owner: &WindowOwner,
        revision: u64,
    ) -> AppResult<()> {
        self.subscriber(state, id, owner)?;
        if let Some(sub) = self.core.lock().unwrap().subscribers.get_mut(id) {
            if sub.revision == revision {
                sub.awaiting = false;
            }
        }
        Ok(())
    }
    pub fn unsubscribe(&self, id: &str, owner: &WindowOwner) {
        self.core.lock().unwrap().subscribers.retain(|key, sub| {
            key != id || sub.owner.label != owner.label || sub.owner.generation != owner.generation
        });
    }
    pub fn window_closed(&self, label: &str) {
        let mut core = self.core.lock().unwrap();
        core.subscribers.retain(|_, s| s.owner.label != label);
        core.drafts.retain(|_, d| d.owner.label != label);
    }
    pub fn summary(&self) -> Summary {
        let core = self.core.lock().unwrap();
        Summary {
            active: core.records.iter().filter(|r| r.view.active()).count(),
            attention: core
                .records
                .iter()
                .filter(|r| r.view.phase == "unknown")
                .count(),
        }
    }
    pub fn has_server(&self, id: &str) -> bool {
        self.core
            .lock()
            .unwrap()
            .records
            .iter()
            .any(|r| r.view.active() && r.servers.iter().any(|s| s == id))
    }
    pub fn shutdown(&self) -> AppResult<()> {
        for r in &self.core.lock().unwrap().records {
            r.cancel.cancel();
        }
        let until = Instant::now() + Duration::from_secs(15);
        while self.jobs.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= until {
                return Err(AppError::new("busy", "Docker 连接仍在收尾，请稍后再退出"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
    fn tick(&self, state: &Arc<State>) {
        let subscribers: Vec<_> = self
            .core
            .lock()
            .unwrap()
            .subscribers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (id, sub) in subscribers {
            if state.owner(&sub.owner.label).generation != sub.owner.generation {
                self.unsubscribe(&id, &sub.owner);
                continue;
            }
            let spec = match state.spec(&sub.session, &sub.owner.label) {
                Ok(s) => Some(s),
                Err(e) if e.kind == "permission" => continue,
                Err(_) => None,
            };
            let binding = spec
                .as_ref()
                .map(|s| state.monitor.docker_binding(s.identity()))
                .transpose();
            let (engine, error) = match binding {
                Ok(Some(e)) => (Some(e), None),
                Ok(None) => (None, Some("SSH 会话已结束；已接受的任务仍独立运行".into())),
                Err(e) => (None, Some(e.message)),
            };
            if let (Some(spec), Some(engine), Some(target)) = (&spec, &engine, &sub.target) {
                self.ensure_detail(state, spec.clone(), engine.clone(), target.clone());
            }
            let mut core = self.core.lock().unwrap();
            let recent: Vec<_> = core
                .records
                .iter()
                .rev()
                .filter(|r| r.identity == sub.identity)
                .take(200)
                .map(|r| r.view.clone())
                .collect();
            let mut operations: Vec<_> = core
                .records
                .iter()
                .rev()
                .filter(|r| {
                    r.identity == sub.identity
                        && r.view.blocks()
                        && !recent.iter().any(|v| v.id == r.view.id)
                })
                .map(|r| r.view.clone())
                .collect();
            operations.extend(recent);
            let detail = engine.as_ref().and_then(|e| {
                sub.target.as_ref().and_then(|t| {
                    core.details
                        .get(&detail_key(&sub.identity, e, t))
                        .map(|c| c.state.clone())
                })
            });
            let mut frame = Frame {
                subscription_id: id.clone(),
                revision: 0,
                catalog_revision: self.catalog_revision.load(Ordering::SeqCst),
                engine,
                operations,
                detail,
                error,
            };
            let signature = serde_json::to_string(&frame).unwrap_or_default();
            let Some(live) = core.subscribers.get_mut(&id) else {
                continue;
            };
            if live.awaiting || live.last == signature {
                continue;
            }
            live.revision += 1;
            live.last = signature;
            live.awaiting = true;
            frame.revision = live.revision;
            drop(core);
            if sub.output.send(frame).is_err() {
                self.unsubscribe(&id, &sub.owner);
            }
        }
        let mut core = self.core.lock().unwrap();
        core.drafts.retain(|_, d| d.preview.expires_at > now());
        // Details contain no credentials, and only currently subscribed objects stay cached.
        let targets: Vec<_> = core
            .subscribers
            .values()
            .filter_map(|s| s.target.as_ref().map(|id| (s.identity.clone(), id.clone())))
            .collect();
        core.details.retain(|key, d| {
            d.running
                || targets.iter().any(|(identity, id)| {
                    key.starts_with(&format!("{identity}\0")) && key.ends_with(&format!("\0{id}"))
                })
        });
    }
    fn ensure_detail(
        &self,
        state: &Arc<State>,
        spec: SharedContext,
        engine: EngineBinding,
        id: String,
    ) {
        let key = detail_key(spec.identity(), &engine, &id);
        let mut core = self.core.lock().unwrap();
        let cache = core
            .details
            .entry(key.clone())
            .or_insert_with(|| CachedDetail {
                state: DetailState {
                    container_id: id.clone(),
                    value: None,
                    loading: false,
                    stale: false,
                    missing: false,
                    error: None,
                },
                next: Instant::now(),
                running: false,
                version: 0,
            });
        if cache.running || Instant::now() < cache.next || self.jobs.load(Ordering::SeqCst) >= 8 {
            return;
        }
        cache.running = true;
        cache.state.loading = true;
        let version = cache.version;
        self.jobs.fetch_add(1, Ordering::SeqCst);
        drop(core);
        let state = state.clone();
        std::thread::spawn(move || {
            let result: AppResult<ContainerDetail> = (|| {
                let check = || {
                    state.ensure_running()?;
                    check_binding(&state, &spec, &engine)
                };
                check()?;
                let connection = connection::connect_verified(
                    &spec,
                    &Cancellation::default(),
                    TrafficPurpose::DockerMonitor,
                )?;
                verify_engine(connection.session(), &engine, &check)?;
                let mut detail =
                    inspect::container(connection.session(), &engine.endpoint, &id, &check)?;
                if detail.state == "running" {
                    match super::stats::sample(connection.session(), &engine.endpoint, &id, &check)
                    {
                        Ok(stats) => detail.stats = Some(stats),
                        Err(e) => detail.stats_error = Some(e.message),
                    }
                }
                Ok(detail)
            })();
            let mut core = state.docker.core.lock().unwrap();
            if let Some(cache) = core.details.get_mut(&key) {
                cache.running = false;
                cache.state.loading = false;
                if cache.version == version {
                    cache.next = Instant::now() + Duration::from_secs(5);
                    match result {
                        Ok(mut detail) => {
                            if detail.stats_error.is_some() {
                                detail.stats = cache
                                    .state
                                    .value
                                    .as_ref()
                                    .and_then(|previous| previous.stats.clone());
                            }
                            cache.state.value = Some(detail);
                            cache.state.error = None;
                            cache.state.stale = false;
                            cache.state.missing = false;
                        }
                        Err(e) => {
                            cache.state.stale = true;
                            cache.state.missing = e.kind == "notFound";
                            cache.state.error = Some(e.message);
                        }
                    }
                }
            }
            state.docker.jobs.fetch_sub(1, Ordering::SeqCst);
        });
    }
    pub fn refresh(&self, state: &State, session: &str, owner: &WindowOwner) -> AppResult<()> {
        let spec = state.spec(session, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        self.invalidate(state, spec.identity(), &engine);
        Ok(())
    }
    fn invalidate(&self, state: &State, identity: &str, engine: &EngineBinding) {
        state.monitor.invalidate_docker(identity, engine);
        let prefix = format!("{identity}\0{}\0", engine.token);
        for (key, cache) in &mut self.core.lock().unwrap().details {
            if key.starts_with(&prefix) {
                cache.version += 1;
                cache.next = Instant::now();
                cache.state.stale = true;
            }
        }
    }
    pub fn prepare(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        token: &str,
        id: &str,
        action: Action,
    ) -> AppResult<Preview> {
        action.validate_target(id)?;
        action.validate()?;
        let spec = state.spec(session, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        if engine.token != token {
            return Err(AppError::new("conflict", "Docker 引擎已变化"));
        }
        let check = || {
            state.spec(session, &owner.label)?;
            check_binding(state, &spec, &engine)
        };
        let connection = connection::connect_verified(
            &spec,
            &Cancellation::default(),
            TrafficPurpose::DockerMonitor,
        )?;
        verify_engine(connection.session(), &engine, &check)?;
        let (target_name, fingerprint, consequences) = if action.is_container() {
            let detail = inspect::container(connection.session(), &engine.endpoint, id, &check)?;
            action.check(&detail)?;
            let mut consequences = Vec::new();
            if action == Action::Kill {
                consequences
                    .push("立即发送 SIGKILL，容器进程无法正常收尾，未写入的数据可能丢失。".into());
            }
            if action == Action::Remove {
                consequences.push(
                    "删除容器及其可写层，无法撤销。保留挂载的卷、宿主文件和镜像，不使用强制删除。"
                        .into(),
                );
            }
            if detail.auto_remove && matches!(action, Action::Stop | Action::Restart | Action::Kill)
            {
                consequences.push(
                    "此容器启用了自动删除。退出后容器及匿名卷可能被移除，无法保证重新启动。".into(),
                );
            }
            (
                detail.name.clone(),
                inspect::fingerprint(&detail),
                consequences,
            )
        } else {
            let plan = super::operations::prepare(
                state,
                &spec,
                connection.session(),
                &engine,
                id,
                &action,
                &check,
            )?;
            (plan.name, plan.fingerprint, plan.consequences)
        };
        let preview = Preview {
            id: uuid(),
            engine: engine.clone(),
            target_id: id.into(),
            target_name,
            action,
            consequences,
            expires_at: now() + 60_000,
        };
        let _gate = state.configuration.lock().unwrap();
        check()?;
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new("permission", "原窗口已结束"));
        }
        let mut core = self.core.lock().unwrap();
        if core.drafts.len() >= 100 {
            return Err(AppError::new("busy", "确认请求过多，请稍后重试"));
        }
        core.drafts.insert(
            preview.id.clone(),
            Draft {
                preview: preview.clone(),
                identity: spec.identity().into(),
                owner: owner.clone(),
                fingerprint,
                create: None,
                transfer: None,
            },
        );
        Ok(preview)
    }
    pub fn prepare_create(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        token: &str,
        input: super::create::Spec,
    ) -> AppResult<Preview> {
        input.validate()?;
        let spec = state.spec(session, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        if engine.token != token {
            return Err(AppError::new("conflict", "Docker 引擎已变化"));
        }
        let check = || {
            state.spec(session, &owner.label)?;
            check_binding(state, &spec, &engine)
        };
        let connection = connection::connect_verified(
            &spec,
            &Cancellation::default(),
            TrafficPurpose::DockerMonitor,
        )?;
        verify_engine(connection.session(), &engine, &check)?;
        let plan = super::create::prepare(connection.session(), &engine, &input, &check)?;
        let id = uuid();
        let preview = Preview {
            id: id.clone(),
            engine: engine.clone(),
            target_id: input.name.clone(),
            target_name: input.name.clone(),
            action: Action::Create {
                draft_id: id.clone(),
            },
            consequences: plan.consequences,
            expires_at: now() + 60_000,
        };
        let _gate = state.configuration.lock().unwrap();
        check()?;
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new("permission", "原窗口已结束"));
        }
        let mut core = self.core.lock().unwrap();
        if core.drafts.len() >= 100 {
            return Err(AppError::new("busy", "确认请求过多，请稍后重试"));
        }
        core.drafts.insert(
            id,
            Draft {
                preview: preview.clone(),
                identity: spec.identity().into(),
                owner: owner.clone(),
                fingerprint: plan.fingerprint,
                create: Some(input),
                transfer: None,
            },
        );
        Ok(preview)
    }
    pub fn prepare_transfer(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        token: &str,
        input: super::transfer::Spec,
    ) -> AppResult<Preview> {
        let spec = state.spec(session, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        if engine.token != token {
            return Err(AppError::new("conflict", "Docker 引擎已变化"));
        }
        let check = || {
            state.spec(session, &owner.label)?;
            check_binding(state, &spec, &engine)
        };
        let connection = connection::connect_verified(
            &spec,
            &Cancellation::default(),
            TrafficPurpose::DockerMonitor,
        )?;
        verify_engine(connection.session(), &engine, &check)?;
        let (name, fingerprint, consequences) =
            super::transfer::prepare(connection.session(), &engine, &input, &check)?;
        let id = uuid();
        let preview = Preview {
            id: id.clone(),
            engine: engine.clone(),
            target_id: input.target.clone(),
            target_name: name,
            action: Action::Transfer {
                draft_id: id.clone(),
                kind: input.kind.clone(),
            },
            consequences,
            expires_at: now() + 60_000,
        };
        let _gate = state.configuration.lock().unwrap();
        check()?;
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new("permission", "原窗口已结束"));
        }
        let mut core = self.core.lock().unwrap();
        if core.drafts.len() >= 100 {
            return Err(AppError::new("busy", "确认请求过多，请稍后重试"));
        }
        core.drafts.insert(
            id,
            Draft {
                preview: preview.clone(),
                identity: spec.identity().into(),
                owner: owner.clone(),
                fingerprint,
                create: None,
                transfer: Some(input),
            },
        );
        Ok(preview)
    }
    pub fn cancel_transfer(&self, id: &str) -> bool {
        let core = self.core.lock().unwrap();
        if let Some(r) = core
            .records
            .iter()
            .find(|r| r.view.id == id && r.view.transfer.is_some())
        {
            if r.view.active() {
                r.cancel.cancel();
            }
            true
        } else {
            false
        }
    }
    pub fn reconcile_transfer(
        &self,
        state: &Arc<State>,
        owner: &WindowOwner,
        id: &str,
    ) -> AppResult<bool> {
        let identity = {
            let core = self.core.lock().unwrap();
            let Some(r) = core
                .records
                .iter()
                .find(|r| r.view.id == id && r.view.transfer.is_some())
            else {
                return Ok(false);
            };
            r.identity.clone()
        };
        let sessions: Vec<_> = state.terminals.lock().unwrap().keys().cloned().collect();
        let session = sessions
            .into_iter()
            .find(|session| {
                state
                    .spec(session, &owner.label)
                    .is_ok_and(|s| s.identity() == identity)
            })
            .ok_or_else(|| {
                AppError::new(
                    "disconnected",
                    "请在当前窗口重新连接原服务器，再核对 Docker 传输",
                )
            })?;
        self.reconcile(state, &session, owner, id)?;
        Ok(true)
    }
    pub fn history(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        before: Option<&str>,
    ) -> AppResult<HistoryPage> {
        let spec = state.spec(session, &owner.label)?;
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new("permission", "原窗口已结束"));
        }
        let core = self.core.lock().unwrap();
        let end = match before {
            Some(id) => core
                .records
                .iter()
                .position(|r| r.identity == spec.identity() && r.view.id == id)
                .ok_or_else(|| AppError::new("notFound", "操作记录游标不存在或不属于当前连接"))?,
            None => core.records.len(),
        };
        let mut operations: Vec<_> = core.records[..end]
            .iter()
            .rev()
            .filter(|r| r.identity == spec.identity())
            .take(101)
            .map(|r| r.view.clone())
            .collect();
        let more = operations.len() > 100;
        operations.truncate(100);
        Ok(HistoryPage {
            next_cursor: more.then(|| operations.last().unwrap().id.clone()),
            operations,
        })
    }
    pub fn transfer_snapshot(&self) -> Vec<crate::transfers::TaskView> {
        use crate::transfers::model::{TaskPresentation, TransferStatus};
        let core = self.core.lock().unwrap();
        core.records.iter().filter_map(|record| {
            let view = &record.view;
            let transfer = view.transfer.as_ref()?;
            let completed = view.phase == "completed";
            let status: TransferStatus = match view.phase.as_str() {
                "accepted" => "queued",
                "checking" => "checking",
                "running" => "running",
                "verifying" => "verifying",
                "completed" => "completed",
                "unknown" => "unknown",
                _ => "failed",
            }.into();
            Some(crate::transfers::TaskView {
                id: view.id.clone(),
                run_id: view.id.clone(),
                presentation: TaskPresentation {
                    created_at: Some(view.created_at),
                    last_started_at: Some(view.created_at),
                    finished_at: view.finished_at,
                    sort_order: transfer.sort_order,
                    source_paths: vec![transfer.source_path.clone()],
                    last_attempt: None,
                },
                kind: "docker".into(),
                source_name: transfer.source_name.clone(),
                source_path: transfer.source_path.clone(),
                target_name: transfer.target_name.clone(),
                target_path: transfer.target_path.clone(),
                conflict: crate::transfers::ConflictPolicy::Rename,
                status,
                bytes_done: transfer.bytes_done,
                bytes_total: transfer.bytes_total,
                current_file: None,
                current_bytes: transfer.bytes_done,
                committed_bytes: if completed { transfer.bytes_done } else { 0 },
                waiting_reason: None,
                files_done: u64::from(completed),
                files_total: 1,
                skipped: 0,
                error: ["failed", "unknown", "acknowledged"].contains(&view.phase.as_str())
                    .then(|| view.message.clone()),
                warnings: vec!["Docker 传输记录仅在本次 App 运行期间保留。结果待确认时，请在 Docker 操作记录中核对和解除锁定。".into()],
                cleanup_pending: false,
                items: vec![],
                direct_address: None,
                temporary_paths: vec![],
                can_resume: false,
            })
        }).collect()
    }
    pub fn submit(
        &self,
        state: &Arc<State>,
        session: &str,
        owner: WindowOwner,
        request: OperationRequest,
    ) -> AppResult<String> {
        request.validate()?;
        let _gate = state.configuration.lock().unwrap();
        let spec = state.spec(session, &owner.label)?;
        if state.owner(&owner.label).generation != owner.generation {
            return Err(AppError::new("permission", "原窗口已结束"));
        }
        let engine = state.monitor.docker_binding(spec.identity())?;
        if engine.token != request.engine_token {
            return Err(AppError::new("conflict", "Docker 引擎已变化，请刷新后重试"));
        }
        let mut core = self.core.lock().unwrap();
        if let Some(previous) = core
            .records
            .iter()
            .find(|r| r.identity == spec.identity() && r.request.request_id == request.request_id)
        {
            return if previous.request == request {
                Ok(previous.view.id.clone())
            } else {
                Err(AppError::new("conflict", "同一请求标识不能用于不同操作"))
            };
        }
        if core.records.len() >= 2000 {
            return Err(AppError::new(
                "limit",
                "本次会话操作记录已达上限，请完成现有任务后重新启动 App",
            ));
        }
        if core.records.iter().filter(|r| r.view.active()).count() >= 4 {
            return Err(AppError::new(
                "busy",
                "已有四个 Docker 操作正在执行，请稍后重试",
            ));
        }
        if core.records.iter().any(|r| {
            r.identity == spec.identity()
                && r.view.engine.endpoint == engine.endpoint
                && (r.view.target_id == request.target_id
                    || !r.view.action.is_container()
                    || !request.action.is_container())
                && r.view.blocks()
        }) {
            return Err(AppError::new(
                "busy",
                "此对象或引擎已有冲突操作，请先核对进行中或待确认的结果",
            ));
        }
        let fingerprint = if let Some(id) = &request.preview_id {
            let draft = core
                .drafts
                .get(id)
                .ok_or_else(|| AppError::new("conflict", "确认已失效，请重新预览"))?;
            if draft.identity != spec.identity()
                || draft.owner.label != owner.label
                || draft.owner.generation != owner.generation
                || draft.preview.expires_at < now()
                || draft.preview.engine != engine
                || draft.preview.target_id != request.target_id
                || draft.preview.action != request.action
            {
                return Err(AppError::new("conflict", "确认范围已变化，请重新预览"));
            }
            Some(draft.fingerprint.clone())
        } else {
            None
        };
        if request.action.requires_preview() && fingerprint.is_none() {
            return Err(AppError::new(
                "confirmationRequired",
                "删除前请确认影响范围",
            ));
        }
        let id = uuid();
        let cancel = Cancellation::default();
        let transfer_view = request
            .preview_id
            .as_ref()
            .and_then(|id| core.drafts.get(id))
            .and_then(|d| d.transfer.as_ref())
            .map(|input| {
                super::transfer::presentation(
                    input,
                    &spec.target().name,
                    state.transfers.next_order(),
                )
            });
        core.records.push(Record {
            view: Operation {
                id: id.clone(),
                request_id: request.request_id.clone(),
                engine: engine.clone(),
                target_id: request.target_id.clone(),
                target_name: request.target_id.chars().take(48).collect(),
                action: request.action.clone(),
                phase: "accepted".into(),
                command_result: "notSubmitted".into(),
                message: "等待校验目标状态".into(),
                observed_state: None,
                observed_health: None,
                created_at: now(),
                finished_at: None,
                reconciling: false,
                created_container_id: None,
                items: vec![],
                transfer: transfer_view,
            },
            identity: spec.identity().into(),
            servers: spec.server_ids().map(str::to_owned).collect(),
            request: request.clone(),
            cancel: cancel.clone(),
        });
        let transfer_spec = request
            .preview_id
            .as_ref()
            .and_then(|id| core.drafts.get(id))
            .and_then(|draft| draft.transfer.clone());
        let create_spec = request
            .preview_id
            .as_ref()
            .and_then(|id| core.drafts.get(id))
            .and_then(|draft| draft.create.clone());
        if let Some(id) = &request.preview_id {
            core.drafts.remove(id);
        }
        self.jobs.fetch_add(1, Ordering::SeqCst);
        drop(core);
        let state = state.clone();
        let task = id.clone();
        std::thread::spawn(move || {
            state.docker.run(
                &state,
                &task,
                spec,
                engine,
                request,
                fingerprint,
                create_spec,
                transfer_spec,
                cancel,
            );
            state.docker.jobs.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(id)
    }
    fn change(&self, id: &str, change: impl FnOnce(&mut Operation)) {
        if let Some(record) = self
            .core
            .lock()
            .unwrap()
            .records
            .iter_mut()
            .find(|r| r.view.id == id)
        {
            change(&mut record.view);
        }
    }
    fn run(
        &self,
        state: &State,
        id: &str,
        spec: SharedContext,
        engine: EngineBinding,
        request: OperationRequest,
        fingerprint: Option<String>,
        create_spec: Option<super::create::Spec>,
        transfer_spec: Option<super::transfer::Spec>,
        cancel: Cancellation,
    ) {
        self.change(id, |v| {
            v.phase = "checking".into();
        });
        let attempted = AtomicBool::new(false);
        let result = (|| {
            let check = || {
                cancel.check()?;
                state.check_context(&spec)
            };
            check()?;
            let connection = connection::connect_verified(
                &spec,
                &cancel,
                if transfer_spec.is_some() {
                    TrafficPurpose::Transfer
                } else {
                    TrafficPurpose::DockerAction
                },
            )?;
            let session = connection.session();
            verify_engine(session, &engine, &check)?;
            let mut resource_plan = None;
            let timeout = if request.action.is_container() {
                let detail =
                    inspect::container(session, &engine.endpoint, &request.target_id, &check)?;
                self.change(id, |v| {
                    v.target_name = detail.name.clone();
                    v.observed_state = Some(detail.state.clone());
                    v.observed_health = detail.health.clone();
                });
                if let Some(fingerprint) = &fingerprint {
                    if *fingerprint != inspect::fingerprint(&detail) {
                        return Err(AppError::new(
                            "conflict",
                            "容器状态或删除范围已变化，请重新确认",
                        ));
                    }
                }
                if request.action.needs_confirmation(&detail) && fingerprint.is_none() {
                    return Err(AppError::new(
                        "confirmationRequired",
                        "此操作会触发自动删除，请查看详情并确认后重新操作",
                    ));
                }
                if !request.action.check(&detail)? {
                    self.change(id, |v| {
                        v.phase = "completed".into();
                        v.command_result = "notNeeded".into();
                        v.message = "当前已是目标状态，无需重复执行".into();
                    });
                    return Ok(());
                }
                check()?;
                detail
                    .stop_timeout
                    .filter(|s| *s >= 0)
                    .map(|s| (s as u64).saturating_add(60))
                    .unwrap_or(300)
                    .clamp(120, 86400)
            } else {
                let plan = if let Some(input) = &transfer_spec {
                    let (name, fingerprint, consequences) =
                        super::transfer::prepare(session, &engine, input, &check)?;
                    super::operations::Prepared {
                        name,
                        fingerprint,
                        consequences,
                        command: String::new(),
                        private_config: None,
                        batch: None,
                        timeout: 7200,
                    }
                } else if let Some(input) = &create_spec {
                    super::create::prepare(session, &engine, input, &check)?
                } else {
                    super::operations::prepare(
                        state,
                        &spec,
                        session,
                        &engine,
                        &request.target_id,
                        &request.action,
                        &check,
                    )?
                };
                self.change(id, |v| v.target_name = plan.name.clone());
                if fingerprint
                    .as_ref()
                    .is_some_and(|previous| *previous != plan.fingerprint)
                {
                    return Err(AppError::new(
                        "conflict",
                        "配置、目标或使用关系已变化，请重新预览",
                    ));
                }
                let timeout = plan.timeout;
                resource_plan = Some(plan);
                timeout
            };
            check()?;
            self.change(id, |v| {
                v.phase = "running".into();
                v.message = format!("正在{}…", request.action.label());
            });
            let progress = |output: &str| {
                if !request.action.is_container() && !output.trim().is_empty() {
                    self.change(id, |v| {
                        v.message = format!("正在{}…\n{}", request.action.label(), output)
                    });
                }
            };
            let result = if let Some(input) = &transfer_spec {
                super::transfer::execute(
                    session,
                    &engine.endpoint,
                    input,
                    &attempted,
                    &check,
                    &|done, total| {
                        self.change(id, |v| {
                            if let Some(transfer) = &mut v.transfer {
                                transfer.bytes_done = done;
                                transfer.bytes_total = total;
                            }
                            v.message = format!(
                                "已传输 {} 字节{}",
                                done,
                                if total > 0 {
                                    format!(" / {total}")
                                } else {
                                    String::new()
                                }
                            );
                        });
                        state.file_tasks.emit(id);
                    },
                )?
            } else if let Some(input) = &create_spec {
                super::create::execute(
                    session,
                    &engine.endpoint,
                    input,
                    &request.request_id,
                    &attempted,
                    &check,
                    &|created| {
                        self.change(id, |v| {
                            v.created_container_id = Some(created.into());
                            v.message = "容器已创建，正在核对并启动".into();
                        })
                    },
                )?
            } else if let Some(plan) = resource_plan {
                super::operations::execute(
                    session,
                    plan,
                    &attempted,
                    &check,
                    &progress,
                    &|index, item| {
                        self.change(id, |v| {
                            if index < v.items.len() {
                                v.items[index] = item;
                            } else {
                                v.items.push(item);
                            }
                        })
                    },
                )?
            } else {
                let command = request
                    .action
                    .command(&engine.endpoint, &request.target_id)?;
                executor::execute_progress(
                    session,
                    &command,
                    Duration::from_secs(timeout),
                    &attempted,
                    &check,
                    &progress,
                )?
            };
            self.change(id, |v| {
                v.command_result = if result.code == 0 {
                    "succeeded"
                } else {
                    "failed"
                }
                .into();
                v.phase = "verifying".into();
                v.message = "正在核对当前状态".into();
            });
            let observed = observe(
                session,
                &engine.endpoint,
                &request.target_id,
                &request.action,
                &request.request_id,
                &check,
            );
            self.change(id, |v| {
                v.phase = if result.code == 0 {
                    "completed"
                } else {
                    "failed"
                }
                .into();
                v.message = if result.code == 0 {
                    format!("{}命令已完成", request.action.label())
                } else {
                    format!(
                        "{}失败：{}",
                        request.action.label(),
                        inspect::clean(
                            if result.stderr.trim().is_empty() {
                                &result.stdout
                            } else {
                                &result.stderr
                            },
                            4096
                        )
                    )
                };
                if result.code == 0 && result.stderr.contains("临时配置清理未完成") {
                    v.message.push_str(&format!("；{}", result.stderr));
                }
                if (request.action == Action::CachePrune
                    || matches!(request.action, Action::Batch { .. }))
                    && result.code == 0
                {
                    v.message
                        .push_str(&format!("；{}", inspect::clean(&result.stdout, 2000)));
                }
                match observed {
                    Ok((runtime, health)) => {
                        v.observed_state = Some(runtime);
                        v.observed_health = health;
                    }
                    Err(e) if e.kind == "notFound" => {
                        v.observed_state = Some("removed".into());
                        v.observed_health = None;
                    }
                    Err(e) => {
                        v.message
                            .push_str(&format!("；当前状态待刷新：{}", e.message));
                    }
                }
            });
            Ok(())
        })();
        if let Err(e) = result {
            self.change(id, |v| {
                let unknown = attempted.load(Ordering::SeqCst)
                    && transfer_spec
                        .as_ref()
                        .is_none_or(|input| input.kind.upload());
                v.phase = if unknown { "unknown" } else { "failed" }.into();
                v.command_result = if unknown {
                    "unknown"
                } else if v.created_container_id.is_some() || !v.items.is_empty() {
                    "partiallySucceeded"
                } else {
                    "notSubmitted"
                }
                .into();
                if unknown {
                    v.observed_state = None;
                    v.observed_health = None;
                }
                v.message = if unknown {
                    format!("执行结果待确认：{}。不会自动重试。", e.message)
                } else {
                    e.message
                };
            });
        }
        self.change(id, |v| v.finished_at = Some(now()));
        self.invalidate(state, spec.identity(), &engine);
        if transfer_spec.is_some() {
            state.file_tasks.emit(id);
        }
    }
    pub fn reconcile(
        &self,
        state: &Arc<State>,
        session: &str,
        owner: &WindowOwner,
        id: &str,
    ) -> AppResult<()> {
        let spec = state.spec(session, &owner.label)?;
        let mut core = self.core.lock().unwrap();
        let record = core
            .records
            .iter_mut()
            .find(|r| r.view.id == id && r.identity == spec.identity())
            .ok_or_else(|| AppError::new("permission", "无权核对此任务"))?;
        if record.view.active() {
            return Err(AppError::new("busy", "任务仍在执行或核对"));
        }
        let engine = record.view.engine.clone();
        let target = record.view.target_id.clone();
        let action = record.view.action.clone();
        let request_id = record.view.request_id.clone();
        let id = id.to_owned();
        record.view.reconciling = true;
        self.jobs.fetch_add(1, Ordering::SeqCst);
        drop(core);
        let state = state.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let check = || state.check_context(&spec);
                let connection = connection::connect_verified(
                    &spec,
                    &Cancellation::default(),
                    TrafficPurpose::DockerMonitor,
                )?;
                verify_engine(connection.session(), &engine, &check)?;
                observe(
                    connection.session(),
                    &engine.endpoint,
                    &target,
                    &action,
                    &request_id,
                    &check,
                )
            })();
            state.docker.change(&id, |v| {
                v.reconciling = false;
                match result {
                    Ok((runtime, health)) => {
                        v.observed_state = Some(runtime);
                        v.observed_health = health;
                        v.message =
                            "已读取当前状态；这不证明先前命令是否完成。确认后可解除操作锁。".into();
                    }
                    Err(e) if e.kind == "notFound" => {
                        v.observed_state = Some("removed".into());
                        v.observed_health = None;
                        v.message = "原目标已不存在；不会操作同名新对象。".into();
                    }
                    Err(e) => v.message = format!("核对未完成：{}", e.message),
                }
            });
            state.docker.invalidate(&state, spec.identity(), &engine);
            state.docker.jobs.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(())
    }
    pub fn acknowledge(
        &self,
        state: &State,
        session: &str,
        owner: &WindowOwner,
        id: &str,
    ) -> AppResult<()> {
        let spec = state.spec(session, &owner.label)?;
        let mut core = self.core.lock().unwrap();
        let record = core
            .records
            .iter_mut()
            .find(|r| r.view.id == id && r.identity == spec.identity())
            .ok_or_else(|| AppError::new("permission", "无权访问此任务"))?;
        if record.view.active() || record.view.phase != "unknown" {
            return Err(AppError::new("conflict", "任务当前无需解除锁定"));
        }
        if record.view.observed_state.is_none() {
            return Err(AppError::new(
                "conflict",
                "请先成功核对当前状态，再解除操作锁",
            ));
        }
        record.view.phase = "acknowledged".into();
        record
            .view
            .message
            .push_str(" 已手动解除操作锁；原操作仍可能在远端完成。");
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
pub(crate) mod tests;
