//! Core-owned connection attempts. Registry locks never span network or user waits.
mod admission;
pub mod desktop;
mod host_keys;
pub(crate) mod io;
mod route;
pub(crate) use route::Session;
mod socks5;
mod ssh;
mod validation;

use crate::{
    servers::connection_profile::{Auth, Node, Profile},
    workspaces::Context,
    AppCore,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: String,
    pub stage: String,
    pub message: String,
}
impl Error {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            stage: String::new(),
            message: message.into(),
        }
    }
    fn cancelled() -> Self {
        Self::new("cancelled", "连接已取消")
    }
    fn io(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::ConnectionRefused => {
                Self::new("connectionRefused", "连接被拒绝，请检查地址、端口和服务")
            }
            std::io::ErrorKind::TimedOut => Self::new("timeout", "网络连接超时"),
            _ => Self::new("network", "网络连接已中断或地址无法访问"),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<russh::Error> for Error {
    fn from(_: russh::Error) -> Self {
        Self::new("sshProtocol", "SSH 握手、算法协商或传输失败")
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Question {
    #[serde(rename_all = "camelCase")]
    TargetChange {
        id: String,
        previous_endpoint: String,
        endpoint: String,
    },
    #[serde(rename_all = "camelCase")]
    Credentials {
        id: String,
        node_index: usize,
        name: String,
        endpoint: String,
        authentication: String,
        username: String,
        private_key_path: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    HostKey {
        id: String,
        node_index: usize,
        name: String,
        endpoint: String,
        algorithm: String,
        fingerprint: String,
        previous_fingerprint: Option<String>,
    },
}
impl Question {
    fn id(&self) -> &str {
        match self {
            Self::Credentials { id, .. }
            | Self::HostKey { id, .. }
            | Self::TargetChange { id, .. } => id,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Answer {
    #[serde(rename_all = "camelCase")]
    Credentials {
        secret: String,
        private_key_path: Option<String>,
    },
    HostKey {
        accept: bool,
    },
    TargetChange {
        accept: bool,
    },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub attempt_id: String,
    pub server_id: String,
    pub window_id: String,
    pub state: String,
    pub stage: String,
    pub node_index: usize,
    pub node_count: usize,
    pub endpoint: String,
    pub question: Option<Question>,
    pub context: Option<Context>,
    pub error: Option<Error>,
    /// Physical socket bytes including handshakes and encapsulation. Decimal u64 strings.
    pub sent_bytes: String,
    pub received_bytes: String,
}
struct State {
    view: View,
    reply: Option<oneshot::Sender<Answer>>,
    question_clock: Option<Instant>,
    paused: Duration,
    question_sequence: u64,
}
pub(crate) struct Attempt {
    state: Mutex<State>,
    identity: String,
    source_window: String,
    expected_context: Option<Context>,
    previous_endpoint: Option<String>,
    cancel: CancellationToken,
    done: CancellationToken,
    counters: Arc<io::Counters>,
    core: Weak<AppCore>,
    session: Mutex<Option<Arc<route::Session>>>,
}
impl Attempt {
    fn view(&self) -> View {
        let mut v = self.state.lock().unwrap().view.clone();
        (v.sent_bytes, v.received_bytes) = self.counters.totals();
        v
    }
    fn changed(&self) {
        if let Some(core) = self.core.upgrade() {
            core.event("connections-changed", serde_json::Value::Null);
        }
    }
    fn stage(&self, stage: &str, index: usize) {
        let view = {
            let mut state = self.state.lock().unwrap();
            state.view.stage = stage.into();
            state.view.node_index = index;
            state.view.clone()
        };
        if let Some(core) = self.core.upgrade() {
            core.log(None, "debug", "连接阶段已更新", Some(serde_json::json!({"attemptId":view.attempt_id,"serverId":view.server_id,"stage":stage,"nodeIndex":index})));
        }
        self.changed();
    }
    fn paused(&self) -> Duration {
        let s = self.state.lock().unwrap();
        s.paused + s.question_clock.map(|t| t.elapsed()).unwrap_or_default()
    }
    async fn wait<T>(
        &self,
        stage: &str,
        index: usize,
        future: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        self.stage(stage, index);
        let start = Instant::now();
        let paused = self.paused();
        tokio::pin!(future);
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! { biased;
                _=self.cancel.cancelled()=>return Err(Error::cancelled()),
                result=&mut future=>{
                    // Cancellation may arrive while the IO future is being polled on
                    // another runtime worker. Socket abort must retain cancelled status.
                    if self.cancel.is_cancelled(){return Err(Error::cancelled());}
                    return result.map_err(|mut e|{if e.stage.is_empty(){e.stage=stage.into();}e});
                },
                _=timer.tick()=>if start.elapsed().saturating_sub(self.paused().saturating_sub(paused))>=Duration::from_secs(30){return Err(Error {stage:stage.into(),..Error::new("timeout","当前连接阶段超过 30 秒，请检查路径或服务")});}
            }
        }
    }
    async fn ask(&self, make: impl FnOnce(String) -> Question) -> Result<Answer, Error> {
        let (tx, rx) = oneshot::channel();
        {
            let mut s = self.state.lock().unwrap();
            if self.cancel.is_cancelled() {
                return Err(Error::cancelled());
            }
            s.question_sequence += 1;
            let id = format!("{}-q{}", s.view.attempt_id, s.question_sequence);
            s.view.question = Some(make(id));
            s.reply = Some(tx);
            s.question_clock = Some(Instant::now());
        }
        self.changed();
        let result = tokio::select! {biased;_=self.cancel.cancelled()=>Err(Error::cancelled()),v=rx=>v.map_err(|_|Error::cancelled())};
        {
            let mut s = self.state.lock().unwrap();
            s.view.question = None;
            s.reply = None;
            if let Some(t) = s.question_clock.take() {
                s.paused += t.elapsed();
            }
        }
        self.changed();
        result
    }
    async fn credentials(&self, node: &mut Node, index: usize) -> Result<(), Error> {
        let auth = match &node.auth {
            Auth::Password(None) => Some("password"),
            Auth::Key { path: None, .. } => Some("privateKey"),
            Auth::Socks {
                username: Some(_),
                password: None,
            } => Some("usernamePassword"),
            _ => None,
        };
        if let Some(auth) = auth {
            self.fill_credentials(node, index, auth).await?;
        }
        Ok(())
    }
    async fn fill_credentials(
        &self,
        node: &mut Node,
        index: usize,
        authentication: &str,
    ) -> Result<(), Error> {
        self.stage("credentials", index);
        let answer = self
            .ask(|id| Question::Credentials {
                id,
                node_index: index,
                name: node.name.clone(),
                endpoint: node.endpoint(),
                authentication: authentication.into(),
                username: match &node.auth {
                    Auth::Socks { username, .. } => username.clone().unwrap_or_default(),
                    _ => node.username.clone(),
                },
                private_key_path: match &node.auth {
                    Auth::Key { path, .. } => path.clone(),
                    _ => None,
                },
            })
            .await?;
        match answer {
            Answer::Credentials {
                secret,
                private_key_path,
            } => match &mut node.auth {
                Auth::Password(p) => *p = Some(secret),
                Auth::Socks { password, .. } => *password = Some(secret),
                Auth::Key { path, passphrase } => {
                    if let Some(p) = private_key_path.filter(|s| !s.trim().is_empty()) {
                        *path = Some(p);
                    }
                    *passphrase = Some(secret);
                }
            },
            _ => return Err(Error::new("invalidAnswer", "连接问题的回复类型不匹配")),
        }
        Ok(())
    }
}

pub(crate) struct Connections {
    entries: Mutex<HashMap<String, Arc<Attempt>>>,
    host_keys: Arc<host_keys::HostKeys>,
}
impl Connections {
    pub fn open(root: &std::path::Path) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            host_keys: Arc::new(host_keys::HostKeys::open(root)),
        }
    }
    pub fn views(&self) -> Vec<View> {
        let mut v = self
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|a| a.view())
            .collect::<Vec<_>>();
        v.sort_by(|a, b| a.server_id.cmp(&b.server_id));
        v
    }
    pub fn cancel_stale(&self, store: &crate::servers::ServerStore) {
        for (id, a) in self.entries.lock().unwrap().iter() {
            if a.view().state == "connecting"
                && !store
                    .connection_snapshot(id)
                    .is_ok_and(|p| p.identity == a.identity)
            {
                a.cancel.cancel();
            }
        }
    }
    pub fn cancel_window(&self, window: &str) {
        for a in self.entries.lock().unwrap().values() {
            if a.source_window == window && a.view().state == "connecting" {
                a.cancel.cancel();
            }
        }
    }
    pub fn cancel_workspace(&self, id: &str) {
        for a in self.entries.lock().unwrap().values() {
            if a.view().context.is_some_and(|c| c.workspace_id == id)
                || a.expected_context
                    .as_ref()
                    .is_some_and(|c| c.workspace_id == id)
            {
                a.cancel.cancel();
            }
        }
    }
    pub fn stop_all(&self) -> Vec<CancellationToken> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .map(|a| {
                a.cancel.cancel();
                a.done.clone()
            })
            .collect()
    }
}
impl Drop for Connections {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl AppCore {
    pub(crate) fn connection_session(
        &self,
        context: &Context,
    ) -> Result<Arc<route::Session>, String> {
        let entries = self.connections.entries.lock().unwrap();
        self.workspaces.lock().unwrap().validate_context(context)?;
        entries
            .get(&context.server_id)
            .filter(|a| a.view().context.as_ref() == Some(context))
            .and_then(|a| a.session.lock().unwrap().clone())
            .filter(|s| !s.is_closed())
            .ok_or("服务器连接已断开".into())
    }
    #[cfg(feature = "test-fixtures")]
    pub async fn connection_probe(
        &self,
        context: &Context,
        command: &str,
    ) -> Result<(Vec<u8>, u32), String> {
        self.connection_validation_exec(context, command).await
    }
    pub fn cancel_workspace_connection(&self, id: &str) {
        self.connections.cancel_workspace(id);
    }
    pub fn cancel_window_connections(&self, window: &str) {
        self.connections.cancel_window(window);
    }
    pub fn connection_views(&self) -> Vec<View> {
        self.connections.views()
    }
    pub fn start_connection(self: &Arc<Self>, window: &str, server: &str) -> Result<View, String> {
        if self.is_closing() {
            return Err("应用正在退出".into());
        }
        // Same order as catalog edits and admission: servers -> connections -> workspaces -> UI.
        let servers = self.servers.lock().unwrap();
        let profile = servers.connection_snapshot(server)?;
        let mut entries = self.connections.entries.lock().unwrap();
        if self.is_closing() {
            return Err("应用正在退出".into());
        }
        if let Some(a) = entries.get(server) {
            if !a.done.is_cancelled() {
                return Ok(a.view());
            }
        }
        if entries.values().filter(|a| !a.done.is_cancelled()).count() >= 32 {
            return Err("同时连接数量已达上限（32）".into());
        }
        // Keep at most 64 last outcomes; sessions/attempts in progress are never evicted.
        if entries.len() >= 64 {
            if let Some(id) = entries
                .iter()
                .find(|(_, a)| a.done.is_cancelled())
                .map(|(id, _)| id.clone())
            {
                entries.remove(&id);
            }
        }
        let w = self.workspaces.lock().unwrap();
        w.projection(window)?;
        if let Some(existing) = w.by_server(server) {
            if existing.connected {
                return Err("此服务器已有连接，请前往工作台".into());
            }
            if self
                .component_ui
                .lock()
                .unwrap()
                .frozen(&existing.context.workspace_id)
            {
                return Err("工作区正在确认关闭".into());
            }
        }
        let expected_context = w.by_server(server).map(|w| w.context.clone());
        let previous_endpoint = w
            .by_server(server)
            .filter(|w| {
                w.target_identity
                    .as_ref()
                    .is_some_and(|i| i != &profile.identity)
            })
            .map(|w| w.endpoint.clone());
        drop(w);
        let id = self.next_id();
        let a = Arc::new(Attempt {
            state: Mutex::new(State {
                view: View {
                    attempt_id: id,
                    server_id: server.into(),
                    window_id: window.into(),
                    state: "connecting".into(),
                    stage: "preparing".into(),
                    node_index: 0,
                    node_count: profile.nodes.len(),
                    endpoint: profile.nodes.last().unwrap().endpoint(),
                    question: None,
                    context: None,
                    error: None,
                    sent_bytes: "0".into(),
                    received_bytes: "0".into(),
                },
                reply: None,
                paused: Duration::ZERO,
                question_clock: None,
                question_sequence: 0,
            }),
            identity: profile.identity.clone(),
            source_window: window.into(),
            expected_context,
            previous_endpoint,
            cancel: CancellationToken::new(),
            done: CancellationToken::new(),
            counters: Arc::new(io::Counters::default()),
            core: Arc::downgrade(self),
            session: Mutex::new(None),
        });
        entries.insert(server.into(), a.clone());
        self.network_metrics
            .register_transport(server, a.counters.clone());
        drop(entries);
        drop(servers);
        let view = a.view();
        let keys = self.connections.host_keys.clone();
        self.log(None, "info", "开始连接远程服务器", Some(serde_json::json!({"attemptId":view.attempt_id,"serverId":view.server_id,"nodeCount":view.node_count})));
        tauri::async_runtime::spawn(run(a, profile, keys));
        self.event("connections-changed", serde_json::Value::Null);
        Ok(view)
    }
    pub fn answer_connection(
        &self,
        window: &str,
        attempt: &str,
        question: &str,
        answer: Answer,
    ) -> Result<(), String> {
        let entries = self.connections.entries.lock().unwrap();
        let a = entries
            .values()
            .find(|a| a.view().attempt_id == attempt)
            .ok_or("连接尝试已失效")?;
        let mut s = a.state.lock().unwrap();
        if a.cancel.is_cancelled()
            || s.view.state != "connecting"
            || a.source_window != window
            || s.view.question.as_ref().is_none_or(|q| q.id() != question)
        {
            return Err("连接问题已失效".into());
        }
        let matches = matches!(
            (&s.view.question, &answer),
            (
                Some(Question::Credentials { .. }),
                Answer::Credentials { .. }
            ) | (Some(Question::HostKey { .. }), Answer::HostKey { .. })
                | (
                    Some(Question::TargetChange { .. }),
                    Answer::TargetChange { .. }
                )
        );
        if !matches {
            return Err("回复类型不匹配".into());
        }
        if let Answer::Credentials {
            secret,
            private_key_path,
        } = &answer
        {
            if secret.len() > 4096 || private_key_path.as_ref().is_some_and(|p| p.len() > 4096) {
                return Err("认证输入过长".into());
            }
        }
        s.reply
            .take()
            .ok_or("该问题已回复")?
            .send(answer)
            .map_err(|_| "连接问题已失效".into())
    }
    pub fn cancel_connection(&self, window: &str, attempt: &str) -> Result<(), String> {
        let entries = self.connections.entries.lock().unwrap();
        let a = entries
            .values()
            .find(|a| a.view().attempt_id == attempt)
            .ok_or("连接尝试已失效")?;
        if a.source_window != window || a.view().state != "connecting" {
            return Err("连接尝试已失效".into());
        }
        a.cancel.cancel();
        Ok(())
    }
    pub fn disconnect_connection(&self, window: &str, context: &Context) -> Result<(), String> {
        let entries = self.connections.entries.lock().unwrap();
        self.workspaces
            .lock()
            .unwrap()
            .validate_owned(window, context)?;
        let a = entries
            .get(&context.server_id)
            .filter(|a| a.view().context.as_ref() == Some(context))
            .ok_or("连接已失效")?;
        a.cancel.cancel();
        Ok(())
    }
}

async fn run(a: Arc<Attempt>, profile: Profile, keys: Arc<host_keys::HostKeys>) {
    let result = async {
        if let Some(previous) = &a.previous_endpoint {
            let node = profile.nodes.last().unwrap();
            let answer = a
                .ask(|id| Question::TargetChange {
                    id,
                    previous_endpoint: previous.clone(),
                    endpoint: format!("{}@{}", node.username, node.endpoint()),
                })
                .await?;
            if !matches!(answer, Answer::TargetChange { accept: true }) {
                return Err(Error::cancelled());
            }
        }
        route::connect(profile.clone(), a.clone(), keys).await
    }
    .await;
    let mut connected = false;
    let outcome = match result {
        Ok(session) => {
            let session = Arc::new(session);
            let accepted = a
                .core
                .upgrade()
                .ok_or_else(Error::cancelled)
                .and_then(|core| core.adopt_connection(&a, &profile, session.clone()));
            drop(profile);
            match accepted {
                Ok(context) => {
                    {
                        let mut s = a.state.lock().unwrap();
                        s.view.context = Some(context);
                        s.view.state = "connected".into();
                        s.view.stage = "connected".into();
                    }
                    connected = true;
                    a.changed();
                    loop {
                        tokio::select! {_=a.cancel.cancelled()=>break, _=tokio::time::sleep(Duration::from_millis(200))=>if session.is_closed(){break;}}
                    }
                    if a.cancel.is_cancelled() {
                        Ok(())
                    } else {
                        Err(Error::new("connectionLost", "远程连接已断开，可手动重连"))
                    }
                }
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    };
    // Cancel the physical socket even if a handshake spawned a task before returning a handle.
    a.cancel.cancel();
    let session = a.session.lock().unwrap().take();
    if let Some(session) = session {
        session.close().await;
    }
    // Publish the workspace disconnection before exposing a retryable terminal outcome.
    // Otherwise a fast retry can observe "cancelled" while start still returns the old attempt.
    if let Some(core) = a.core.upgrade() {
        if let Some(context) = a.view().context {
            let _ = core.workspaces.lock().unwrap().disconnected(&context);
        }
    }
    let view = {
        let mut s = a.state.lock().unwrap();
        s.reply = None;
        s.view.question = None;
        s.view.state = if connected {
            "disconnected"
        } else if outcome.as_ref().is_err_and(|e| e.code == "cancelled") {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        s.view.error = outcome.err().map(|mut e| {
            if e.stage.is_empty() {
                e.stage = s.view.stage.clone();
            }
            e
        });
        a.done.cancel();
        s.view.clone()
    };
    if let Some(core) = a.core.upgrade() {
        if view.context.is_some() {
            core.workspace_changed();
        }
        core.log(None,if view.state=="failed"{"warn"}else{"info"},if connected{"远程会话已结束"}else{"连接尝试已结束"},Some(serde_json::json!({"serverId":view.server_id,"attemptId":view.attempt_id,"code":view.error.as_ref().map(|e|&e.code),"stage":view.stage})));
    }
    a.changed();
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_socket_error_in_same_poll_keeps_cancelled_outcome() {
        let a = Attempt {
            state: Mutex::new(State {
                view: View {
                    attempt_id: "test".into(),
                    server_id: "server".into(),
                    window_id: "main".into(),
                    state: "connecting".into(),
                    stage: "".into(),
                    node_index: 0,
                    node_count: 1,
                    endpoint: "test".into(),
                    question: None,
                    context: None,
                    error: None,
                    sent_bytes: "0".into(),
                    received_bytes: "0".into(),
                },
                reply: None,
                question_clock: None,
                paused: Duration::ZERO,
                question_sequence: 0,
            }),
            identity: "test".into(),
            source_window: "main".into(),
            expected_context: None,
            previous_endpoint: None,
            cancel: CancellationToken::new(),
            done: CancellationToken::new(),
            counters: Arc::new(io::Counters::default()),
            core: Weak::new(),
            session: Mutex::new(None),
        };
        let result = a
            .wait("sshHandshake", 0, async {
                // Models the socket waking with ConnectionAborted while its enclosing
                // select has already polled the cancellation branch for this turn.
                a.cancel.cancel();
                Err::<(), _>(Error::io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "cancelled socket",
                )))
            })
            .await;
        assert_eq!(result.unwrap_err().code, "cancelled");
    }
}
