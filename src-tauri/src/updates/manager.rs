use super::*;
pub(super) enum Action {
    Check,
    Download(String),
    Cancel(String),
    Preferences(u64, PreferencePatch),
    Skip(String, bool),
    Review(String),
    Install(String),
    AckNotice(String),
    AckResult(String),
    ManualAsset(String),
}
pub(super) enum Reply {
    Snapshot(Snapshot),
    Review(InstallReview),
    Submission(InstallSubmission),
    Url(String),
}
type Responder = oneshot::Sender<AppResult<Reply>>;
pub(super) enum Request {
    Action(Action, Responder),
    Done(String, AppResult<Outcome>),
    Verifying(String),
    Stop,
}
pub(super) enum Outcome {
    Checked(Option<Candidate>),
    Downloaded(Arc<Vec<u8>>),
    Prepared,
    Installed,
}
struct Job {
    id: String,
    kind: &'static str,
    source: &'static str,
    cancel: watch::Sender<bool>,
    cancelled: bool,
    handle: tauri::async_runtime::JoinHandle<()>,
}
fn supervise(
    tx: mpsc::Sender<Request>,
    job_id: String,
    future: impl std::future::Future<Output = AppResult<Outcome>> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn(future)
            .await
            .unwrap_or_else(|_| Err(error("更新工作任务意外结束，请重试。")));
        let _ = tx.send(Request::Done(job_id, result)).await;
    })
}
struct Core {
    app: tauri::AppHandle,
    path: PathBuf,
    tx: mpsc::Sender<Request>,
    view: Snapshot,
    shared: Arc<Mutex<Snapshot>>,
    phase: Arc<AtomicU8>,
    saved: Saved,
    healthy: bool,
    writable: bool,
    candidate: Option<Candidate>,
    bytes: Option<Arc<Vec<u8>>>,
    job: Option<Job>,
    review: Option<ReviewSecret>,
    install_reply: Option<Responder>,
    deadline: Instant,
    progress_tx: watch::Sender<Progress>,
    progress_rx: watch::Receiver<Progress>,
}
impl Core {
    fn publish(&mut self) -> Snapshot {
        if self.shared.lock().unwrap().phase != self.view.phase {
            crate::diagnostics::event::Context::new().event("INFO", "updates", "update.state", serde_json::json!({
                "phase": self.view.phase.label(), "currentVersion": self.view.current_version,
                "targetVersion": self.view.release.as_ref().map(|r| r.version.as_str()), "updateTarget": self.view.install.target,
            }));
        }
        self.view.revision += 1;
        self.view.preferences = Preferences {
            revision: self.saved.preferences_revision,
            auto_check: self.saved.auto_check,
            auto_download: self.saved.auto_download,
        };
        self.view.skipped_version = self.saved.skipped_version.clone();
        self.view.last_successful_check_at = self.saved.last_successful_check_at;
        self.view.suppression = if self
            .view
            .release
            .as_ref()
            .is_some_and(|r| self.saved.skipped_version.as_ref() == Some(&r.version))
        {
            Some("skipped".into())
        } else {
            self.saved.suppression.clone().filter(|_| {
                self.saved.suppressed_version
                    == self.view.release.as_ref().map(|r| r.version.clone())
            })
        };
        self.view.result = self.saved.unread_result.clone();
        *self.shared.lock().unwrap() = self.view.clone();
        let _ = self.app.emit(EVENT, &self.view);
        self.view.clone()
    }
    async fn save(&mut self, saved: Saved) -> AppResult<()> {
        if !self.writable {
            let failure = error("更新设置损坏或不可读，自动任务已暂停，原文件已保留。");
            self.view.error = Some(UpdateError {
                stage: "store".into(),
                code: "storeFailed".into(),
                retry: "none".into(),
            });
            self.view.message = Some(failure.message.clone());
            return Err(failure);
        }
        match store::save(self.path.clone(), saved.clone()).await {
            Ok(()) => {
                self.saved = saved;
                self.healthy = true;
                Ok(())
            }
            Err(e) => {
                self.healthy = false;
                self.view.error = Some(UpdateError {
                    stage: "store".into(),
                    code: "storeFailed".into(),
                    retry: "none".into(),
                });
                self.view.message = Some(e.message.clone());
                Err(e)
            }
        }
    }
    fn candidate(&self, id: &str) -> AppResult<&Candidate> {
        self.candidate
            .as_ref()
            .filter(|c| c.release.id == id)
            .ok_or_else(|| error("更新版本已变化，请重新检查。"))
    }
    fn spawn(
        &mut self,
        kind: &'static str,
        source: &'static str,
        future: impl std::future::Future<Output = AppResult<Outcome>> + Send + 'static,
        cancel: watch::Sender<bool>,
        id: String,
    ) {
        let tx = self.tx.clone();
        let job_id = id.clone();
        let handle = supervise(tx, job_id, future);
        self.view.operation = Some(Operation {
            id: id.clone(),
            source: source.into(),
        });
        self.job = Some(Job {
            id,
            kind,
            source,
            cancel,
            cancelled: false,
            handle,
        });
    }
    fn check(&mut self, source: &'static str) {
        if self.job.is_some() || self.bytes.is_some() {
            return;
        }
        let (cancel, rx) = watch::channel(false);
        let app = self.app.clone();
        let id = uuid::Uuid::new_v4().to_string();
        self.view.phase = Phase::Checking;
        self.view.error = None;
        self.view.message = None;
        self.spawn(
            "check",
            source,
            async move { transport::check(app, rx).await.map(Outcome::Checked) },
            cancel,
            id,
        );
    }
    async fn download(&mut self, id: &str, source: &'static str) -> AppResult<()> {
        let candidate = self.candidate(id)?.clone();
        if self.job.is_some() || self.bytes.is_some() {
            return Ok(());
        }
        self.app.state::<Arc<State>>().ensure_running()?;
        let update = candidate
            .update
            .ok_or_else(|| error("当前安装方式请手动更新。"))?;
        if source == "automatic"
            && !policy::auto_download(&self.saved, self.candidate.as_ref().unwrap(), self.healthy)
        {
            return Ok(());
        }
        if source == "manual" {
            let mut saved = self.saved.clone();
            saved.skipped_version = None;
            saved.suppression = None;
            saved.suppressed_version = None;
            self.save(saved).await?;
        }
        let (cancel, rx) = watch::channel(false);
        let tx = self.tx.clone();
        let op = uuid::Uuid::new_v4().to_string();
        let job_id = op.clone();
        let progress = self.progress_tx.clone();
        self.view.phase = Phase::Downloading;
        self.view.error = None;
        self.view.message = None;
        self.review = None;
        self.view.progress = Some(Progress {
            downloaded: 0,
            total: None,
        });
        self.progress_tx.send_replace(Progress {
            downloaded: 0,
            total: None,
        });
        self.progress_rx.borrow_and_update();
        self.spawn(
            "download",
            source,
            async move {
                transport::download(update, rx, progress, tx, job_id)
                    .await
                    .map(Outcome::Downloaded)
            },
            cancel,
            op,
        );
        Ok(())
    }
    async fn cancel_download(&mut self) {
        let Some(job) = self.job.as_mut().filter(|j| j.kind == "download") else {
            return;
        };
        job.cancelled = true;
        job.cancel.send_replace(true);
        self.bytes = None;
        self.view.phase = Phase::Available;
        self.view.progress = None;
        self.view.error = None;
        self.view.message = Some("正在结束下载，完成后可重新下载。".into());
        let mut saved = self.saved.clone();
        saved.suppressed_version = self.view.release.as_ref().map(|r| r.version.clone());
        saved.suppression = Some("cancelled".into());
        // Cancel wins immediately even if its persistence fails. Disable automatic work on failure.
        self.saved = saved.clone();
        let _ = self.save(saved).await;
    }
    fn review(&mut self, id: &str) -> AppResult<InstallReview> {
        if self.bytes.is_none() || self.job.is_some() {
            return Err(error("请先完成更新下载。"));
        }
        let fingerprint = self.candidate(id)?.fingerprint.clone();
        let state = self.app.state::<Arc<State>>();
        let _gate = state.configuration.lock().unwrap();
        state.ensure_running()?;
        let owner = state.owner("main");
        let (mut review, impact) = install::preflight(&state, owner.generation)?;
        review.release_id = id.into();
        self.review = None;
        if review.blockers.is_empty() {
            let token = uuid::Uuid::new_v4().to_string();
            review.review_token = Some(token.clone());
            self.review = Some(ReviewSecret {
                token,
                release_id: id.into(),
                fingerprint,
                main_generation: owner.generation,
                impact,
                expires: Instant::now() + Duration::from_secs(60),
            });
        }
        Ok(review)
    }
    fn prepare(&mut self, token: &str) -> AppResult<()> {
        if self.job.is_some() || self.view.phase != Phase::Ready {
            return Err(error("请先完成更新下载。"));
        }
        let secret = self
            .review
            .as_ref()
            .filter(|r| r.token == token)
            .ok_or_else(|| error("安装确认已失效，请重新检查。"))?;
        // Expiry is re-evaluated after preparation and returns a fresh review instead of consuming work.
        let candidate = self.candidate(&secret.release_id)?.clone();
        let bytes = self.bytes.clone().ok_or_else(|| error("更新包不可用。"))?;
        let update = candidate
            .update
            .clone()
            .ok_or_else(|| error("此安装方式请手动更新。"))?;
        self.phase.store(1, Ordering::SeqCst);
        self.view.phase = Phase::Preparing;
        self.view.error = None;
        self.view.message = None;
        let (cancel, rx) = watch::channel(false);
        let id = uuid::Uuid::new_v4().to_string();
        let app = self.app.clone();
        self.spawn(
            "prepare",
            "manual",
            async move {
                crate::blocking(move || {
                    platform::preflight_path()?;
                    update
                        .verify(bytes.as_ref())
                        .map_err(|_| error("更新包再次验签失败。"))
                })
                .await?;
                let raw = transport::manifest(rx).await?;
                let latest = transport::candidate(&app, raw)?;
                if latest.fingerprint != candidate.fingerprint {
                    return Err(AppError::new(
                        "candidateChanged",
                        "稳定版本或更新包已变化，请重新检查。",
                    ));
                }
                Ok(Outcome::Prepared)
            },
            cancel,
            id,
        );
        Ok(())
    }
    async fn action(&mut self, action: Action) -> AppResult<Reply> {
        if self.phase.load(Ordering::SeqCst) == 2 {
            return Err(error("正在关闭连接并安装，请等待。"));
        }
        match action {
            Action::Check => {
                self.app.state::<Arc<State>>().ensure_running()?;
                self.check("manual");
            }
            Action::Download(id) => self.download(&id, "manual").await?,
            Action::Cancel(id) => {
                if self
                    .job
                    .as_ref()
                    .is_some_and(|j| j.id == id && j.kind == "download")
                {
                    self.cancel_download().await;
                }
            }
            Action::Preferences(rev, patch) => {
                if self.saved.preferences_revision != rev {
                    return Err(error("更新偏好已变化，请刷新后重试。"));
                }
                let mut saved = self.saved.clone();
                let previous = saved.auto_download;
                if let Some(v) = patch.auto_check {
                    saved.auto_check = v;
                }
                if let Some(v) = patch.auto_download {
                    saved.auto_download = v;
                }
                if !previous && saved.auto_download {
                    saved.suppressed_version = None;
                    saved.suppression = None;
                }
                saved.preferences_revision += 1;
                self.save(saved).await?;
                if previous
                    && !self.saved.auto_download
                    && self
                        .job
                        .as_ref()
                        .is_some_and(|j| j.kind == "download" && j.source == "automatic")
                {
                    self.cancel_download().await;
                }
            }
            Action::Skip(id, skip) => {
                let version = self.candidate(&id)?.release.version.clone();
                let mut saved = self.saved.clone();
                saved.skipped_version = if skip { Some(version) } else { None };
                self.save(saved).await?;
                if skip
                    && self
                        .job
                        .as_ref()
                        .is_some_and(|j| j.kind == "download" && j.source == "automatic")
                {
                    self.cancel_download().await;
                }
            }
            Action::Review(id) => return self.review(&id).map(Reply::Review),
            Action::Install(_) => return Err(error("内部操作无效。")),
            Action::AckNotice(id) => {
                let version = self.candidate(&id)?.release.version.clone();
                if self.saved.last_presented_version.as_ref() != Some(&version) {
                    let mut saved = self.saved.clone();
                    saved.last_presented_version = Some(version);
                    self.save(saved).await?;
                }
            }
            Action::AckResult(id) => {
                if self
                    .saved
                    .unread_result
                    .as_ref()
                    .is_some_and(|r| r.id == id)
                {
                    let mut saved = self.saved.clone();
                    saved.unread_result = None;
                    self.save(saved).await?;
                }
            }
            Action::ManualAsset(id) => {
                return self
                    .candidate(&id)?
                    .manual_url
                    .clone()
                    .map(Reply::Url)
                    .ok_or_else(|| error("当前安装方式没有对应安装包，请查看发布页。"))
            }
        }
        Ok(Reply::Snapshot(self.publish()))
    }
    async fn commit(&mut self) -> AppResult<InstallSubmission> {
        let secret = self
            .review
            .take()
            .ok_or_else(|| error("安装确认已失效。"))?;
        let candidate = self.candidate(&secret.release_id)?.clone();
        let bytes = self.bytes.clone().ok_or_else(|| error("更新包已失效。"))?;
        let update = candidate
            .update
            .clone()
            .ok_or_else(|| error("更新包不支持安装。"))?;
        let state = self.app.state::<Arc<State>>().inner().clone();
        let unchanged = {
            let _gate = state.configuration.lock().unwrap();
            state.ensure_running()?;
            let owner = state.owner("main");
            let (review, impact) = install::preflight(&state, owner.generation)?;
            let valid = self.phase.load(Ordering::SeqCst) == 1
                && !state.closed_windows.lock().unwrap().contains("main")
                && review.blockers.is_empty()
                && secret.expires > Instant::now()
                && owner.generation == secret.main_generation
                && secret.impact == impact
                && secret.fingerprint == candidate.fingerprint;
            if valid {
                state.stopping.store(true, Ordering::SeqCst);
                self.phase.store(2, Ordering::SeqCst);
            }
            valid
        };
        if !unchanged {
            self.phase.store(0, Ordering::SeqCst);
            self.view.phase = Phase::Ready;
            return self
                .review(&secret.release_id)
                .map(|review| InstallSubmission::ReviewChanged { review });
        }
        let mut saved = self.saved.clone();
        saved.pending_install = Some(PendingInstall {
            id: uuid::Uuid::new_v4().to_string(),
            source_version: self.view.current_version.clone(),
            target_version: candidate.release.version.clone(),
            handed_off: false,
            started_at: now(),
        });
        if let Err(e) = self.save(saved).await {
            state.stopping.store(false, Ordering::SeqCst);
            self.phase.store(0, Ordering::SeqCst);
            return Err(e);
        }
        self.view.phase = Phase::Installing;
        self.publish();
        // The actor is the sole store writer. The install worker receives a frozen pending record;
        // mutations are rejected until its handoff or recovery completes.
        let path = self.path.clone();
        let mut handed_off = self.saved.clone();
        if let Some(pending) = handed_off.pending_install.as_mut() {
            pending.handed_off = true;
        }
        let (cancel, _) = watch::channel(false);
        let id = uuid::Uuid::new_v4().to_string();
        self.spawn(
            "install",
            "manual",
            async move {
                // Any failure after cleanup begins requires an explicit restart; the half-closed process never resumes new work.
                crate::shutdown_work(state.clone()).await.inspect_err(|_| {
                    state.stopping.store(true, Ordering::SeqCst);
                })?;
                store::save(path, handed_off).await?;
                if let Some(service) = crate::diagnostics::global() {
                    let _ = service.flush();
                }
                crate::blocking(move || {
                    update
                        .install(bytes.as_ref())
                        .map_err(|_| error("系统安装器未能完成交接。"))
                })
                .await?;
                // NSIS exits during handoff; platforms that return restart via Done.
                Ok(Outcome::Installed)
            },
            cancel,
            id,
        );
        Ok(InstallSubmission::Accepted)
    }
    async fn done(&mut self, id: &str, result: AppResult<Outcome>) {
        if !self.job.as_ref().is_some_and(|j| j.id == id) {
            return;
        }
        let job = self.job.take().unwrap();
        self.view.operation = None;
        self.view.progress = None;
        if job.cancelled {
            self.view.phase = Phase::Available;
            if self.healthy {
                self.view.message = None;
            }
            self.publish();
            return;
        }
        match result {
            Ok(Outcome::Checked(candidate)) => {
                self.saved.failures = 0;
                self.saved.last_successful_check_at = Some(now());
                self.saved.next_check_at = now() + DAY;
                self.deadline = Instant::now() + Duration::from_secs(DAY);
                let _ = self.save(self.saved.clone()).await;
                self.view.release = candidate.as_ref().map(|c| c.release.clone());
                self.view.install = candidate
                    .as_ref()
                    .map(|c| c.capability.clone())
                    .unwrap_or_else(platform::capability);
                self.view.phase = if candidate.is_some() {
                    Phase::Available
                } else if self.view.install.mode == "unavailable" {
                    self.view.message = self.view.install.reason.clone();
                    Phase::Idle
                } else {
                    Phase::UpToDate
                };
                self.candidate = candidate;
                self.review = None;
            }
            Ok(Outcome::Downloaded(bytes)) => {
                self.bytes = Some(bytes);
                self.view.phase = Phase::Ready;
                self.saved.suppressed_version = None;
                self.saved.suppression = None;
                let _ = self.save(self.saved.clone()).await;
            }
            Ok(Outcome::Prepared) => {
                let result = self.commit().await;
                if let Err(e) = &result {
                    self.phase.store(0, Ordering::SeqCst);
                    self.view.phase = Phase::Ready;
                    self.view.message = Some(e.message.clone());
                }
                if let Some(reply) = self.install_reply.take() {
                    let _ = reply.send(result.map(Reply::Submission));
                }
            }
            Ok(Outcome::Installed) => {
                self.phase.store(3, Ordering::SeqCst);
                self.app.restart();
            }
            Err(e) => {
                let failure_stage = if self.view.phase == Phase::Verifying {
                    "verify"
                } else {
                    job.kind
                };
                self.view.message = Some(e.message.clone());
                self.view.phase = if self.bytes.is_some() {
                    Phase::Ready
                } else {
                    Phase::Failed
                };
                self.view.error = Some(UpdateError {
                    stage: failure_stage.into(),
                    code: "operationFailed".into(),
                    retry: match job.kind {
                        "check" => "check",
                        "download" => "download",
                        "prepare" => "install",
                        _ => "none",
                    }
                    .into(),
                });
                match job.kind {
                    "check" => {
                        self.saved.failures = self.saved.failures.saturating_add(1);
                        let delay = policy::backoff(self.saved.failures);
                        self.saved.next_check_at = now() + delay;
                        self.deadline = Instant::now() + Duration::from_secs(delay);
                        let _ = self.save(self.saved.clone()).await;
                    }
                    "download" => {
                        self.saved.suppressed_version =
                            self.view.release.as_ref().map(|r| r.version.clone());
                        self.saved.suppression = Some("failed".into());
                        let _ = self.save(self.saved.clone()).await;
                    }
                    "prepare" => {
                        self.phase.store(0, Ordering::SeqCst);
                        if e.kind == "candidateChanged" {
                            self.bytes = None;
                            self.candidate = None;
                            self.view.release = None;
                            self.view.phase = Phase::Failed;
                            if let Some(err) = self.view.error.as_mut() {
                                err.retry = "check".into();
                            }
                        }
                        if let Some(reply) = self.install_reply.take() {
                            let _ = reply.send(Err(e));
                        }
                    }
                    "install" => {
                        self.app
                            .state::<Arc<State>>()
                            .stopping
                            .store(true, Ordering::SeqCst);
                        self.phase.store(0, Ordering::SeqCst);
                        self.view.phase = Phase::Failed;
                        self.view.error = Some(UpdateError {
                            stage: "install".into(),
                            code: "restartRequired".into(),
                            retry: "none".into(),
                        });
                        self.view.message =
                            Some("安装未完成，部分连接已关闭。请重启 SoDeck 后继续工作。".into());
                    }
                    _ => {}
                }
            }
        }
        self.publish();
    }
    async fn automatic(&mut self) {
        if self.job.is_some()
            || self.bytes.is_some()
            || !self.healthy
            || cfg!(debug_assertions)
            || self.app.state::<Arc<State>>().ensure_running().is_err()
        {
            return;
        }
        if let Some(candidate) = self
            .candidate
            .as_ref()
            .filter(|c| policy::auto_download(&self.saved, c, self.healthy))
        {
            let id = candidate.release.id.clone();
            let _ = self.download(&id, "automatic").await;
            self.publish();
        } else if self.saved.auto_check && Instant::now() >= self.deadline {
            self.check("automatic");
            self.publish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn worker_panic_is_delivered_as_a_terminal_result() {
        let (tx, mut rx) = mpsc::channel(1);
        let handle = supervise(tx, "panic-job".into(), async {
            panic!("injected worker failure")
        });
        let result = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Request::Done(id, Err(_)) if id == "panic-job"));
        handle.await.unwrap();
    }
    #[tokio::test]
    async fn verification_retains_ownership_until_blocking_worker_finishes_after_cancel() {
        let (tx, mut rx) = mpsc::channel(1);
        let (cancel, cancellation) = watch::channel(false);
        let (finish, wait) = oneshot::channel::<()>();
        let handle = supervise(tx, "verify-job".into(), async move {
            crate::blocking(move || {
                wait.blocking_recv().unwrap();
                Ok(Outcome::Downloaded(Arc::new(vec![1])))
            })
            .await
        });
        cancel.send_replace(true);
        assert!(*cancellation.borrow());
        assert!(rx.try_recv().is_err());
        finish.send(()).unwrap();
        assert!(
            matches!(rx.recv().await, Some(Request::Done(id, Ok(Outcome::Downloaded(_)))) if id == "verify-job")
        );
        handle.await.unwrap();
    }
}
pub(super) fn initialize(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join("update-state.json");
    let directory_ok = path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok());
    let (mut saved, loaded) = store::load(&path);
    let writable = directory_ok && loaded;
    let mut healthy = writable;
    let current = app.package_info().version.to_string();
    if writable && policy::startup_result(&mut saved, &current) {
        healthy = store::persist(&path, &saved).is_ok();
    }
    let deadline = Instant::now()
        + Duration::from_secs(saved.next_check_at.saturating_sub(now()).clamp(30, DAY));
    let view = Snapshot {
        revision: 0,
        phase: Phase::Idle,
        current_version: current,
        release: None,
        install: platform::capability(),
        operation: None,
        progress: None,
        preferences: Preferences {
            revision: saved.preferences_revision,
            auto_check: saved.auto_check,
            auto_download: saved.auto_download,
        },
        last_successful_check_at: saved.last_successful_check_at,
        suppression: None,
        error: if healthy {
            None
        } else {
            Some(UpdateError {
                stage: "store".into(),
                code: "storeFailed".into(),
                retry: "none".into(),
            })
        },
        result: saved.unread_result.clone(),
        message: if healthy {
            None
        } else {
            Some("更新设置无法读取或保存，自动任务已暂停；原文件已保留。".into())
        },
        skipped_version: saved.skipped_version.clone(),
    };
    let shared = Arc::new(Mutex::new(view.clone()));
    let phase = Arc::new(AtomicU8::new(0));
    let (tx, mut rx) = mpsc::channel(64);
    let (progress_tx, progress_rx) = watch::channel(Progress {
        downloaded: 0,
        total: None,
    });
    app.manage(UpdateManager {
        tx: tx.clone(),
        view: shared.clone(),
        phase: phase.clone(),
    });
    let mut core = Core {
        app: app.clone(),
        path,
        tx,
        view,
        shared,
        phase,
        saved,
        healthy,
        writable,
        candidate: None,
        bytes: None,
        job: None,
        review: None,
        install_reply: None,
        deadline,
        progress_tx,
        progress_rx,
    };
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(150));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                request = rx.recv() => match request {
                    Some(Request::Action(Action::Install(token), reply)) => { match core.prepare(&token) { Ok(()) => { core.install_reply = Some(reply); core.publish(); }, Err(e) => { let _ = reply.send(Err(e)); } } },
                    Some(Request::Action(action, reply)) => { let result = core.action(action).await; if result.is_err() { core.publish(); } let _ = reply.send(result); },
                    Some(Request::Done(id, result)) => core.done(&id, result).await,
                    Some(Request::Verifying(id)) => { if core.job.as_ref().is_some_and(|j| j.id == id && !j.cancelled) { core.view.phase = Phase::Verifying; core.publish(); } },
                    Some(Request::Stop) | None => { if let Some(job) = core.job.take() { job.cancel.send_replace(true); if job.kind != "install" { job.handle.abort(); } } break; },
                },
                _ = tick.tick() => {
                    if core.progress_rx.has_changed().unwrap_or(false) {
                        let progress = core.progress_rx.borrow_and_update().clone();
                        if core.view.phase == Phase::Downloading { core.view.progress = Some(progress); core.publish(); }
                    }
                },
            }
            core.automatic().await;
        }
    });
    Ok(())
}
