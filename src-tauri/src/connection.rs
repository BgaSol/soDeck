//! Connection paths, interactive authentication and connection ownership.
use crate::{
    model::{AppError, AppResult, ServerConfig, ServerView},
    ssh::{self, Credentials},
    ssh_transport::{self, Cancellation, SshConnection},
    terminal::{self, Input, InputSender, TerminalStream},
    State,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State as TauriState, WebviewWindow};

pub type SharedContext = Arc<ConnectionContext>;

#[derive(Clone)]
pub struct ConnectionPlan {
    hops: Vec<ServerConfig>,
}
impl ConnectionPlan {
    pub fn new(hops: Vec<ServerConfig>) -> AppResult<Self> {
        if !(1..=2).contains(&hops.len()) || hops.iter().any(|s| s.username.is_empty()) {
            return Err(AppError::new(
                "validation",
                "连接路径必须包含一至两台已配置用户名的服务器",
            ));
        }
        if hops[0].jump_server_id.is_some()
            || (hops.len() == 2
                && (hops[0].id == hops[1].id
                    || hops[1].jump_server_id.as_deref() != Some(hops[0].id.as_str())))
        {
            return Err(AppError::new("validation", "无效的单层跳板路径"));
        }
        Ok(Self { hops })
    }
    pub fn uses_server(&self, id: &str) -> bool {
        self.hops.iter().any(|h| h.id == id)
    }
    pub fn server_ids(&self) -> Vec<String> {
        self.hops.iter().map(|h| h.id.clone()).collect()
    }
}
#[derive(Clone)]
struct VerifiedHop {
    server: ServerConfig,
    credentials: Credentials,
    fingerprint: String,
    public_key: String,
}
pub struct ConnectionContext {
    hops: Vec<VerifiedHop>,
    identity: String,
    pub(crate) traffic: Arc<crate::traffic::TrafficManager>,
}
impl ConnectionContext {
    pub fn plan(&self) -> ConnectionPlan {
        ConnectionPlan {
            hops: self.hops.iter().map(|h| h.server.clone()).collect(),
        }
    }
    pub fn matches_plan(&self, plan: &ConnectionPlan) -> bool {
        self.hops.len() == plan.hops.len()
            && self.hops.iter().zip(&plan.hops).all(|(h, s)| {
                let a = &h.server;
                a.id == s.id
                    && a.host == s.host
                    && a.port == s.port
                    && a.username == s.username
                    && a.auth_type == s.auth_type
                    && a.private_key == s.private_key
                    && a.password == s.password
                    && a.jump_server_id == s.jump_server_id
            })
    }
    pub fn target(&self) -> &ServerConfig {
        &self.hops.last().expect("validated nonempty route").server
    }
    pub fn target_public_key(&self) -> &str {
        &self.hops.last().expect("nonempty route").public_key
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn uses_server(&self, id: &str) -> bool {
        self.hops.iter().any(|h| h.server.id == id)
    }
    pub fn server_ids(&self) -> impl Iterator<Item = &str> {
        self.hops.iter().map(|h| h.server.id.as_str())
    }
    pub fn view(&self) -> ConnectionView {
        ConnectionView {
            target: self.target().view(),
            identity: self.identity.clone(),
            hops: self
                .hops
                .iter()
                .enumerate()
                .map(|(i, h)| HopView::new(&h.server, i, self.hops.len()))
                .collect(),
        }
    }
}
// Stable identity excludes names, tags and credentials. Each preceding host key
// binds the network namespace reached through that hop. IDs distinguish explicit
// server profiles even when different networks use cloned keys and addresses.
fn route_identity(hops: &[VerifiedHop]) -> String {
    if hops.is_empty() {
        return "direct".into();
    }
    let values: Vec<_> = hops
        .iter()
        .map(|h| {
            (
                &h.server.id,
                h.server.host.to_ascii_lowercase(),
                h.server.port,
                &h.server.username,
                &h.fingerprint,
            )
        })
        .collect();
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&values).expect("route serialization"))
    )
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HopView {
    pub index: usize,
    pub role: String,
    pub server: ServerView,
}
impl HopView {
    fn new(server: &ServerConfig, index: usize, length: usize) -> Self {
        Self {
            index,
            role: if index + 1 == length {
                "target"
            } else {
                "jump"
            }
            .into(),
            server: server.view(),
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub target: ServerView,
    pub hops: Vec<HopView>,
    pub identity: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthChallenge {
    pub id: String,
    pub hop: HopView,
    pub message: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyChallenge {
    pub id: String,
    pub hop: HopView,
    pub fingerprint: String,
    pub previous: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConnectionEvent {
    Progress {
        hop: HopView,
        stage: String,
    },
    AuthRequired {
        challenge: AuthChallenge,
    },
    HostKeyRequired {
        challenge: HostKeyChallenge,
    },
    Ready {
        connection: ConnectionView,
    },
    Data {
        data: String,
    },
    Cwd {
        path: String,
        home: Option<String>,
        sequence: u64,
    },
    History {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "workspaceId")]
        workspace_id: String,
    },
    DisplayCleared,
    Frozen {
        token: String,
    },
    Closed {
        message: String,
        reason: String,
        recovery: RecoveryAction,
        error: Option<AppError>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConnectionAnswer {
    Trust {
        accept: bool,
    },
    Credentials {
        #[serde(default)]
        password: String,
        #[serde(default)]
        passphrase: String,
    },
}
/// Connection/authentication events need no terminal or PTY.
pub trait ConnectEventSink: Send + Sync {
    fn send(&self, event: ConnectionEvent) -> AppResult<()>;
}
struct TerminalConnectSink(Arc<Mutex<TerminalStream>>);
impl ConnectEventSink for TerminalConnectSink {
    fn send(&self, event: ConnectionEvent) -> AppResult<()> {
        self.0.lock().unwrap().send(event);
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq)]
enum ChallengeKind {
    Trust,
    Credentials,
}
struct PendingChallenge {
    id: String,
    kind: ChallengeKind,
    sender: mpsc::SyncSender<ConnectionAnswer>,
}
#[derive(Default)]
pub struct Interaction {
    pending: Mutex<Option<PendingChallenge>>,
}
impl Interaction {
    pub fn answer(&self, id: &str, answer: ConnectionAnswer) -> AppResult<()> {
        let mut pending = self.pending.lock().unwrap();
        let challenge = pending
            .as_ref()
            .ok_or_else(|| AppError::new("challenge", "认证提示已结束"))?;
        let kind = match answer {
            ConnectionAnswer::Trust { .. } => ChallengeKind::Trust,
            _ => ChallengeKind::Credentials,
        };
        if challenge.id != id || challenge.kind != kind {
            return Err(AppError::new("challenge", "认证提示已过期或回复类型错误"));
        }
        let challenge = pending.take().unwrap();
        challenge
            .sender
            .send(answer)
            .map_err(|_| AppError::new("cancelled", "连接已结束"))
    }
    fn wait(
        &self,
        id: String,
        kind: ChallengeKind,
        event: ConnectionEvent,
        events: &dyn ConnectEventSink,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<ConnectionAnswer> {
        let (sender, receiver) = mpsc::sync_channel(1);
        *self.pending.lock().unwrap() = Some(PendingChallenge {
            id: id.clone(),
            kind,
            sender,
        });
        if let Err(error) = events.send(event) {
            self.pending.lock().unwrap().take();
            return Err(error);
        }
        let started = Instant::now();
        let mut keepalive = Instant::now();
        let result = (|| loop {
            cancel.check()?;
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(answer) => {
                    cancel.check()?;
                    return Ok(answer);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(AppError::new("cancelled", "认证提示已结束"))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if started.elapsed() > Duration::from_secs(180) {
                return Err(AppError::new(
                    "interactionTimeout",
                    "等待认证或指纹确认超时，请重新连接",
                ));
            }
            if session.authenticated() && keepalive.elapsed() > Duration::from_secs(20) {
                match session.keepalive_send() {
                    Ok(_) => keepalive = Instant::now(),
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                    Err(e) => return Err(e.into()),
                }
            }
        })();
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|c| c.id == id) {
            pending.take();
        }
        result
    }
}

pub struct ConnectionHandle {
    pub window: String,
    pub sender: InputSender,
    pub spec: Option<SharedContext>,
    pub plan: Arc<ConnectionPlan>,
    pub stream: Arc<Mutex<TerminalStream>>,
    pub interaction: Arc<Interaction>,
}
impl ConnectionHandle {
    pub fn uses_server(&self, id: &str) -> bool {
        self.plan.uses_server(id)
    }
}
#[derive(Clone)]
pub struct WindowOwner {
    pub label: String,
    pub generation: u64,
}

trait AuthProvider {
    fn progress(&mut self, hop: &HopView, stage: &str);
    fn verify(
        &mut self,
        hop: &HopView,
        scope: &str,
        fingerprint: &str,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<()>;
    fn credentials(
        &mut self,
        hop: &HopView,
        server: &ServerConfig,
        failure: Option<&AppError>,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<Credentials>;
    fn authenticated(&mut self, server: &ServerConfig, credentials: &Credentials) -> AppResult<()>;
    fn interactive(&self) -> bool {
        false
    }
}
struct InteractiveProvider<'a> {
    state: &'a State,
    events: Arc<dyn ConnectEventSink>,
    interaction: Arc<Interaction>,
    pinned: Option<SharedContext>,
}
impl AuthProvider for InteractiveProvider<'_> {
    fn interactive(&self) -> bool {
        true
    }
    fn progress(&mut self, hop: &HopView, stage: &str) {
        let _ = self.events.send(ConnectionEvent::Progress {
            hop: hop.clone(),
            stage: stage.into(),
        });
    }
    fn verify(
        &mut self,
        hop: &HopView,
        scope: &str,
        fingerprint: &str,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<()> {
        let previous = if let Some(context) = &self.pinned {
            Some(context.hops[hop.index].fingerprint.clone())
        } else {
            self.state.database.lock().unwrap().fingerprint(
                scope,
                &hop.server.host,
                hop.server.port,
            )?
        };
        if previous.as_deref() == Some(fingerprint) {
            return Ok(());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let event = ConnectionEvent::HostKeyRequired {
            challenge: HostKeyChallenge {
                id: id.clone(),
                hop: hop.clone(),
                fingerprint: fingerprint.into(),
                previous,
            },
        };
        match self.interaction.wait(
            id,
            ChallengeKind::Trust,
            event,
            &*self.events,
            session,
            cancel,
        )? {
            ConnectionAnswer::Trust { accept: true } => {
                cancel.check()?;
                self.state.database.lock().unwrap().trust(
                    scope,
                    &hop.server.host,
                    hop.server.port,
                    fingerprint,
                )
            }
            _ => Err(AppError::new("cancelled", "已取消信任服务器指纹")),
        }
    }
    fn credentials(
        &mut self,
        hop: &HopView,
        server: &ServerConfig,
        failure: Option<&AppError>,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<Credentials> {
        if failure.is_none() {
            if server.auth_type == "agent" {
                return Ok(Credentials::default());
            }
            if server.auth_type == "password" && !server.password.is_empty() {
                return Ok(Credentials {
                    password: server.password.clone(),
                    ..Credentials::default()
                });
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let event = ConnectionEvent::AuthRequired {
            challenge: AuthChallenge {
                id: id.clone(),
                hop: hop.clone(),
                message: failure.map(|e| e.message.clone()),
            },
        };
        match self.interaction.wait(
            id,
            ChallengeKind::Credentials,
            event,
            &*self.events,
            session,
            cancel,
        )? {
            ConnectionAnswer::Credentials {
                password,
                passphrase,
            } => Ok(Credentials {
                password,
                passphrase,
            }),
            _ => Err(AppError::new("challenge", "无效的认证回复")),
        }
    }
    fn authenticated(&mut self, server: &ServerConfig, credentials: &Credentials) -> AppResult<()> {
        if server.auth_type == "password" && credentials.password != server.password {
            // Do not replace a password on a profile that was edited while this
            // connection was waiting for input. The live route remains a snapshot.
            let db = self.state.database.lock().unwrap();
            let current = db.get(&server.id)?;
            if current.host == server.host
                && current.port == server.port
                && current.username == server.username
                && current.auth_type == server.auth_type
                && current.password == server.password
                && current.jump_server_id == server.jump_server_id
            {
                db.save_password(&server.id, &credentials.password)?;
            }
        }
        Ok(())
    }
}
struct VerifiedProvider<'a>(&'a ConnectionContext);
impl AuthProvider for VerifiedProvider<'_> {
    fn progress(&mut self, _: &HopView, _: &str) {}
    fn verify(
        &mut self,
        hop: &HopView,
        _: &str,
        fingerprint: &str,
        _: &ssh2::Session,
        _: &Cancellation,
    ) -> AppResult<()> {
        let expected = &self.0.hops[hop.index].fingerprint;
        if expected != fingerprint {
            return Err(AppError::host_key(
                fingerprint.into(),
                Some(expected.clone()),
            ));
        }
        Ok(())
    }
    fn credentials(
        &mut self,
        hop: &HopView,
        _: &ServerConfig,
        _: Option<&AppError>,
        _: &ssh2::Session,
        _: &Cancellation,
    ) -> AppResult<Credentials> {
        Ok(self.0.hops[hop.index].credentials.clone())
    }
    fn authenticated(&mut self, _: &ServerConfig, _: &Credentials) -> AppResult<()> {
        Ok(())
    }
}

fn connect_metered(
    plan: &ConnectionPlan,
    provider: &mut impl AuthProvider,
    cancel: &Cancellation,
    traffic: &Arc<crate::traffic::TrafficManager>,
    purpose: crate::traffic::TrafficPurpose,
) -> AppResult<(SshConnection, SharedContext)> {
    let diagnostic = cancel.diagnostic();
    diagnostic.event(
        "INFO",
        "connection",
        "connection.start",
        serde_json::json!({"hops":plan.hops.len()}),
    );
    let mut connection = None;
    let mut verified = Vec::new();
    for (index, server) in plan.hops.iter().enumerate() {
        let hop = HopView::new(server, index, plan.hops.len());
        let stage = if index == 0 {
            "连接服务器"
        } else {
            "建立跳板转发"
        };
        provider.progress(&hop, stage);
        let mut transport_stage = diagnostic.stage(
            "ssh",
            "transport.open",
            serde_json::json!({"hop":index+1,"timeoutMs":12000}),
        );
        let current = match connection.take() {
            None => ssh_transport::tcp(&server.host, server.port, cancel)
                .and_then(|s| SshConnection::new(s, None)),
            Some(parent) => ssh_transport::forward(parent, &server.host, server.port, cancel),
        }
        .map_err(|e| e.at(server, index, stage).traced(&diagnostic));
        transport_stage.finish(&current);
        let mut current = current?;
        provider.progress(&hop, "SSH 握手");
        let mut handshake = diagnostic.stage(
            "ssh",
            "handshake",
            serde_json::json!({"hop":index+1,"timeoutMs":12000}),
        );
        let result = current.handshake(cancel).map_err(|e| {
            current
                .transport_error()
                .unwrap_or(e)
                .at(server, index, "SSH 握手")
                .traced(&diagnostic)
        });
        handshake.finish(&result);
        result?;
        let fingerprint =
            ssh::fingerprint(current.session()).map_err(|e| e.at(server, index, "读取指纹"))?;
        let mut verification =
            diagnostic.stage("ssh", "host_key.verify", serde_json::json!({"hop":index+1}));
        let verification_result = provider
            .verify(
                &hop,
                &route_identity(&verified),
                &fingerprint,
                current.session(),
                cancel,
            )
            .map_err(|e| e.at(server, index, "核对指纹").traced(&diagnostic));
        verification.finish(&verification_result);
        verification_result?;
        if index + 1 == plan.hops.len() {
            let mut identity_hops = verified.clone();
            identity_hops.push(VerifiedHop {
                server: server.clone(),
                credentials: Credentials::default(),
                fingerprint: fingerprint.clone(),
                public_key: String::new(),
            });
            traffic
                .account(&route_identity(&identity_hops))
                .attach(current.traffic(), purpose);
        }
        let mut failure = None;
        let mut attempt = 0;
        let credentials = loop {
            let credentials = provider
                .credentials(&hop, server, failure.as_ref(), current.session(), cancel)
                .map_err(|e| e.at(server, index, "认证"))?;
            provider.progress(&hop, "认证");
            let mut auth_stage = diagnostic.stage("ssh", "authentication", serde_json::json!({"hop":index+1,"attempt":attempt+1,"method":match server.auth_type.as_str(){"key"=>"key","password"=>"password","agent"=>"agent",_=>"unknown"}}));
            let auth_result = ssh::authenticate(current.session(), server, &credentials, cancel);
            auth_stage.finish(&auth_result);
            match auth_result {
                Ok(()) => break credentials,
                Err(e)
                    if provider.interactive()
                        && e.kind == "authentication"
                        && server.auth_type != "agent"
                        && attempt < 2 =>
                {
                    attempt += 1;
                    diagnostic.event(
                        "INFO",
                        "ssh",
                        "authentication.retry",
                        serde_json::json!({"attempt":attempt+1,"reason":"credentials_required"}),
                    );
                    failure = Some(e);
                }
                Err(e) => return Err(e.at(server, index, "认证").traced(&diagnostic)),
            }
        };
        cancel.check()?;
        provider.authenticated(server, &credentials)?;
        let mut saved = server.clone();
        if saved.auth_type == "password" {
            saved.password = credentials.password.clone();
        }
        verified.push(VerifiedHop {
            server: saved,
            credentials,
            fingerprint,
            public_key: ssh::public_key(current.session())?,
        });
        connection = Some(current);
    }
    let connection = connection.expect("validated nonempty route");
    connection.session().set_blocking(true);
    let identity = route_identity(&verified);
    diagnostic.event(
        "INFO",
        "connection",
        "connection.verified",
        serde_json::json!({"outcome":"ok"}),
    );
    Ok((
        connection,
        Arc::new(ConnectionContext {
            hops: verified,
            identity,
            traffic: traffic.clone(),
        }),
    ))
}

pub fn connect_verified(
    context: &ConnectionContext,
    cancel: &Cancellation,
    purpose: crate::traffic::TrafficPurpose,
) -> AppResult<SshConnection> {
    let plan = ConnectionPlan::new(context.hops.iter().map(|h| h.server.clone()).collect())?;
    let connection = connect_metered(
        &plan,
        &mut VerifiedProvider(context),
        cancel,
        &context.traffic,
        purpose,
    )?
    .0;
    connection.session().set_timeout(12_000);
    Ok(connection)
}

pub fn connect_interactive(
    state: &State,
    plan: &ConnectionPlan,
    events: Arc<dyn ConnectEventSink>,
    interaction: Arc<Interaction>,
    pinned: Option<SharedContext>,
    cancel: &Cancellation,
    purpose: crate::traffic::TrafficPurpose,
) -> AppResult<(SshConnection, SharedContext)> {
    connect_metered(
        plan,
        &mut InteractiveProvider {
            state,
            events,
            interaction,
            pinned,
        },
        cancel,
        &state.traffic,
        purpose,
    )
}

type RegisteredConnection = (
    String,
    mpsc::Receiver<Input>,
    Cancellation,
    Arc<Mutex<TerminalStream>>,
    Arc<Interaction>,
);

fn register(
    state: &State,
    owner: WindowOwner,
    plan: Arc<ConnectionPlan>,
    output: Channel<ConnectionEvent>,
) -> AppResult<RegisteredConnection> {
    state.ensure_running()?;
    let generations = state.window_generations.lock().unwrap();
    let closed = state.closed_windows.lock().unwrap();
    if closed.contains(&owner.label)
        || generations.get(&owner.label).copied().unwrap_or(0) != owner.generation
    {
        return Err(AppError::new("cancelled", "窗口已经关闭"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = Cancellation::default();
    let stream = Arc::new(Mutex::new(TerminalStream::new(output)));
    let interaction = Arc::new(Interaction::default());
    let (sender, receiver) = mpsc::channel();
    state.terminals.lock().unwrap().insert(
        id.clone(),
        ConnectionHandle {
            window: owner.label,
            sender: InputSender {
                channel: sender,
                cancel: cancel.clone(),
            },
            spec: None,
            plan,
            stream: stream.clone(),
            interaction: interaction.clone(),
        },
    );
    Ok((id, receiver, cancel, stream, interaction))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryAction {
    Retry,
    UserAction,
    None,
}
fn recovery_action(error: Option<&AppError>) -> RecoveryAction {
    match error.map(|error| error.kind.as_str()) {
        Some("network" | "timeout" | "io" | "remoteInterrupted") => RecoveryAction::Retry,
        Some("authentication" | "hostKey") => RecoveryAction::UserAction,
        _ => RecoveryAction::None,
    }
}

fn finish(state: &State, id: &str, cancel: &Cancellation, result: AppResult<()>) {
    let diagnostic = cancel.diagnostic();
    let result = result.map_err(|e| e.traced(&diagnostic));
    let mut details = result
        .as_ref()
        .err()
        .map(crate::diagnostics::event::error_details)
        .unwrap_or_else(|| serde_json::json!({"outcome":"ok"}));
    details["connectionRef"] = serde_json::json!(crate::diagnostics::alias(id));
    details["cancelled"] = serde_json::json!(cancel.is_cancelled());
    diagnostic.event(
        if result.is_err() && !cancel.is_cancelled() {
            "WARN"
        } else {
            "INFO"
        },
        "connection",
        "connection.closed",
        details,
    );
    let mut connections = state.terminals.lock().unwrap();
    if let Some(handle) = connections.get(id) {
        let error = if cancel.is_cancelled() {
            None
        } else {
            result.err().filter(|e| e.kind != "cancelled")
        };
        let reason = if error.is_some() {
            "failed"
        } else if cancel.is_cancelled() {
            "cancelled"
        } else {
            "ended"
        };
        let mut stream = handle.stream.lock().unwrap();
        stream.send(ConnectionEvent::Closed {
            message: error
                .as_ref()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "会话已结束".into()),
            reason: reason.into(),
            recovery: recovery_action(error.as_ref()),
            error,
        });
        let frozen = stream.is_frozen();
        drop(stream);
        if !frozen {
            connections.remove(id);
        }
    }
    drop(connections);
    state.monitor.close_session(id);
    state.netbird.close_session(id);
}

pub fn wait_closed(state: &State) -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if state
            .terminals
            .lock()
            .unwrap()
            .values()
            .all(|handle| handle.stream.lock().unwrap().closed)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(AppError::new("busy", "SSH 连接尚未释放，请稍后再次退出"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tauri::command]
pub fn open_connection(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    server_id: String,
    workspace_id: Option<String>,
    events: Channel<ConnectionEvent>,
    diagnostic_trace_id: Option<String>,
) -> AppResult<String> {
    crate::moves::workspace_only(&window)?;
    let state = state.inner().clone();
    let gate = state.configuration.lock().unwrap();
    let plan = Arc::new(ConnectionPlan::new(
        state
            .database
            .lock()
            .unwrap()
            .resolve_connection(&server_id)?,
    )?);
    let (id, receiver, cancel, stream, interaction) =
        register(&state, state.owner(window.label()), plan.clone(), events)?;
    cancel.set_diagnostic(crate::diagnostics::event::Context::from_trace(
        diagnostic_trace_id.as_deref(),
    ));
    cancel.diagnostic().event("INFO","connection","connection.registered",serde_json::json!({"connectionRef":crate::diagnostics::alias(&id),"windowRef":crate::diagnostics::alias(window.label())}));
    drop(gate);
    let worker_id = id.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let (connection, context) = connect_metered(
                &plan,
                &mut InteractiveProvider {
                    state: &state,
                    events: Arc::new(TerminalConnectSink(stream)),
                    interaction,
                    pinned: None,
                },
                &cancel,
                &state.traffic,
                crate::traffic::TrafficPurpose::Terminal,
            )?;
            let source = crate::terminal_history::model::Source::new(&context, None);
            terminal::run(
                connection,
                context,
                &worker_id,
                None,
                &state,
                receiver,
                &cancel,
                Some((workspace_id.unwrap_or_else(|| worker_id.clone()), source)),
            )
        })();
        finish(&state, &worker_id, &cancel, result);
    });
    Ok(id)
}
#[tauri::command]
pub fn answer_connection(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    connection_id: String,
    challenge_id: String,
    response: ConnectionAnswer,
) -> AppResult<()> {
    let connections = state.terminals.lock().unwrap();
    let handle = connections
        .get(&connection_id)
        .ok_or_else(|| AppError::new("cancelled", "连接已结束"))?;
    if handle.window != window.label() {
        return Err(AppError::new("permission", "此窗口无权回复该连接"));
    }
    handle.sender.cancel.check()?;
    handle.interaction.answer(&challenge_id, response)
}
#[tauri::command]
pub fn close_connection(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    connection_id: String,
) -> AppResult<()> {
    let connections = state.terminals.lock().unwrap();
    if let Some(handle) = connections.get(&connection_id) {
        if handle.window != window.label() {
            return Err(AppError::new("permission", "此窗口无权关闭该连接"));
        }
        let _ = handle.sender.send(Input::Close);
    }
    Ok(())
}

pub fn open_container(
    state: Arc<State>,
    owner: WindowOwner,
    context: SharedContext,
    target: crate::docker::ContainerTarget,
    output: Channel<ConnectionEvent>,
    diagnostic_trace_id: Option<String>,
) -> AppResult<String> {
    let workspace_id = owner.label.clone();
    let gate = state.configuration.lock().unwrap();
    let plan = Arc::new(ConnectionPlan::new(
        context.hops.iter().map(|h| h.server.clone()).collect(),
    )?);
    let (id, receiver, cancel, _, _) = register(&state, owner, plan, output)?;
    cancel.set_diagnostic(crate::diagnostics::event::Context::from_trace(
        diagnostic_trace_id.as_deref(),
    ));
    cancel.diagnostic().event(
        "INFO",
        "connection",
        "container.registered",
        serde_json::json!({"connectionRef":crate::diagnostics::alias(&id)}),
    );
    drop(gate);
    let worker_id = id.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let connection = connect_verified(
                &context,
                &cancel,
                crate::traffic::TrafficPurpose::ContainerTerminal,
            )?;
            let command = crate::docker::terminal_command_on(connection.session(), &target)?;
            let source = crate::terminal_history::model::Source::new(&context, Some(&target));
            terminal::run(
                connection,
                context,
                &worker_id,
                Some(command),
                &state,
                receiver,
                &cancel,
                Some((workspace_id, source)),
            )
        })();
        finish(&state, &worker_id, &cancel, result);
    });
    Ok(id)
}

#[cfg(test)]
#[path = "connection_tests.rs"]
pub(crate) mod tests;
