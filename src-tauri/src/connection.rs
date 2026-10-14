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
}
struct VerifiedHop {
    server: ServerConfig,
    credentials: Credentials,
    fingerprint: String,
    public_key: String,
}
pub struct ConnectionContext {
    hops: Vec<VerifiedHop>,
    identity: String,
}
impl ConnectionContext {
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
    Frozen {
        token: String,
    },
    Closed {
        message: String,
        reason: String,
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
    fn answer(&self, id: &str, answer: ConnectionAnswer) -> AppResult<()> {
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
        stream: &Mutex<TerminalStream>,
        session: &ssh2::Session,
        cancel: &Cancellation,
    ) -> AppResult<ConnectionAnswer> {
        let (sender, receiver) = mpsc::sync_channel(1);
        *self.pending.lock().unwrap() = Some(PendingChallenge {
            id: id.clone(),
            kind,
            sender,
        });
        stream.lock().unwrap().send(event);
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
    stream: Arc<Mutex<TerminalStream>>,
    interaction: Arc<Interaction>,
}
impl AuthProvider for InteractiveProvider<'_> {
    fn interactive(&self) -> bool {
        true
    }
    fn progress(&mut self, hop: &HopView, stage: &str) {
        self.stream.lock().unwrap().send(ConnectionEvent::Progress {
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
        let previous = self.state.database.lock().unwrap().fingerprint(
            scope,
            &hop.server.host,
            hop.server.port,
        )?;
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
            &self.stream,
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
            &self.stream,
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

fn connect(
    plan: &ConnectionPlan,
    provider: &mut impl AuthProvider,
    cancel: &Cancellation,
) -> AppResult<(SshConnection, SharedContext)> {
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
        let mut current = match connection.take() {
            None => ssh_transport::tcp(&server.host, server.port, cancel)
                .and_then(|s| SshConnection::new(s, None)),
            Some(parent) => ssh_transport::forward(parent, &server.host, server.port, cancel),
        }
        .map_err(|e| e.at(server, index, stage))?;
        provider.progress(&hop, "SSH 握手");
        current.handshake(cancel).map_err(|e| {
            current
                .transport_error()
                .unwrap_or(e)
                .at(server, index, "SSH 握手")
        })?;
        let fingerprint =
            ssh::fingerprint(current.session()).map_err(|e| e.at(server, index, "读取指纹"))?;
        provider
            .verify(
                &hop,
                &route_identity(&verified),
                &fingerprint,
                current.session(),
                cancel,
            )
            .map_err(|e| e.at(server, index, "核对指纹"))?;
        let mut failure = None;
        let mut attempt = 0;
        let credentials = loop {
            let credentials = provider
                .credentials(&hop, server, failure.as_ref(), current.session(), cancel)
                .map_err(|e| e.at(server, index, "认证"))?;
            provider.progress(&hop, "认证");
            match ssh::authenticate(current.session(), server, &credentials, cancel) {
                Ok(()) => break credentials,
                Err(e)
                    if provider.interactive()
                        && e.kind == "authentication"
                        && server.auth_type != "agent"
                        && attempt < 2 =>
                {
                    attempt += 1;
                    failure = Some(e);
                }
                Err(e) => return Err(e.at(server, index, "认证")),
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
    Ok((
        connection,
        Arc::new(ConnectionContext {
            hops: verified,
            identity,
        }),
    ))
}

pub fn connect_verified(
    context: &ConnectionContext,
    cancel: &Cancellation,
) -> AppResult<SshConnection> {
    let plan = ConnectionPlan::new(context.hops.iter().map(|h| h.server.clone()).collect())?;
    let connection = connect(&plan, &mut VerifiedProvider(context), cancel)?.0;
    connection.session().set_timeout(12_000);
    Ok(connection)
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
fn finish(state: &State, id: &str, cancel: &Cancellation, result: AppResult<()>) {
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
            error,
        });
        let frozen = stream.is_frozen();
        drop(stream);
        if !frozen {
            connections.remove(id);
        }
    }
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
    events: Channel<ConnectionEvent>,
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
    drop(gate);
    let worker_id = id.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let (connection, context) = connect(
                &plan,
                &mut InteractiveProvider {
                    state: &state,
                    stream,
                    interaction,
                },
                &cancel,
            )?;
            terminal::run(
                connection, context, &worker_id, None, &state, receiver, &cancel,
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
) -> AppResult<String> {
    let gate = state.configuration.lock().unwrap();
    let plan = Arc::new(ConnectionPlan::new(
        context.hops.iter().map(|h| h.server.clone()).collect(),
    )?);
    let (id, receiver, cancel, _, _) = register(&state, owner, plan, output)?;
    drop(gate);
    let worker_id = id.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let connection = connect_verified(&context, &cancel)?;
            let command = crate::docker::terminal_command_on(connection.session(), &target)?;
            terminal::run(
                connection,
                context,
                &worker_id,
                Some(command),
                &state,
                receiver,
                &cancel,
            )
        })();
        finish(&state, &worker_id, &cancel, result);
    });
    Ok(id)
}

#[cfg(test)]
#[path = "connection_tests.rs"]
pub(crate) mod tests;
