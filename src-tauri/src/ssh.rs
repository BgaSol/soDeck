use crate::{
    model::{AppError, AppResult, Server},
    State,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub passphrase: String,
}
#[derive(Clone)]
pub struct ConnectionSpec {
    pub server: Server,
    pub credentials: Credentials,
    pub fingerprint: String,
}
#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalEvent {
    Data { data: String },
    Closed { message: String },
    Frozen { token: String },
}
pub enum Input {
    Data(Vec<u8>),
    Resize(u32, u32),
    Close,
}
pub struct TerminalHandle {
    pub window: String,
    pub sender: mpsc::Sender<Input>,
    pub spec: ConnectionSpec,
    pub stream: std::sync::Arc<std::sync::Mutex<TerminalStream>>,
}

// The SSH worker outlives its frontend subscriber during a window transfer.
// A bounded queue applies backpressure to the remote PTY while no view is attached.
pub struct TerminalStream {
    output: Option<Channel<TerminalEvent>>,
    fallback: Option<Channel<TerminalEvent>>,
    pending: std::collections::VecDeque<TerminalEvent>,
    pending_bytes: usize,
    replay: Vec<TerminalEvent>,
    replay_bytes: usize,
    replay_cursor: usize,
    frozen_token: Option<String>,
    pub closed: bool,
}
impl TerminalStream {
    pub fn new(output: Channel<TerminalEvent>) -> Self {
        Self {
            output: Some(output),
            fallback: None,
            pending: Default::default(),
            pending_bytes: 0,
            replay: Vec::new(),
            replay_bytes: 0,
            replay_cursor: 0,
            frozen_token: None,
            closed: false,
        }
    }
    pub fn send(&mut self, event: TerminalEvent) {
        if matches!(event, TerminalEvent::Closed { .. }) {
            self.closed = true;
        }
        if self.is_frozen() {
            if let TerminalEvent::Data { data } = &event {
                self.replay_bytes += data.len();
            }
            self.replay.push(event);
            self.flush_replay();
            return;
        }
        if let Some(output) = &self.output {
            if output.send(event.clone()).is_ok() {
                return;
            }
            self.output = None;
        }
        if let TerminalEvent::Data { data } = &event {
            self.pending_bytes += data.len();
        }
        self.pending.push_back(event);
    }
    pub fn freeze(&mut self, token: String) -> AppResult<()> {
        if self.is_frozen() {
            return Err(AppError::new("busy", "会话正在移动，请稍后重试"));
        }
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| AppError::new("window", "终端窗口未连接"))?;
        output
            .send(TerminalEvent::Frozen {
                token: token.clone(),
            })
            .map_err(|e| AppError::new("window", e))?;
        self.frozen_token = Some(token);
        self.fallback = self.output.take();
        Ok(())
    }
    pub fn attach(&mut self, output: Channel<TerminalEvent>) -> AppResult<()> {
        if !self.is_frozen() {
            return Err(AppError::new("transfer", "会话未准备好接收"));
        }
        if self.output.is_some() {
            return Err(AppError::new("transfer", "此会话已由另一个终端接收"));
        }
        self.output = Some(output);
        self.replay_cursor = 0;
        self.flush_replay();
        self.ready_to_commit()
    }
    pub fn rollback(&mut self) {
        if !self.is_frozen() {
            return;
        }
        self.output = self.fallback.take();
        self.frozen_token = None;
        // The source received none of the output produced after the freeze.
        // Replay it all, including events that reached the failed target.
        let replay = std::mem::take(&mut self.replay);
        self.pending_bytes += replay
            .iter()
            .map(|event| match event {
                TerminalEvent::Data { data } => data.len(),
                _ => 0,
            })
            .sum::<usize>();
        self.pending.extend(replay);
        self.replay.clear();
        self.replay_bytes = 0;
        self.replay_cursor = 0;
        self.flush();
    }
    pub fn ready_to_commit(&self) -> AppResult<()> {
        if !self.is_frozen() || self.output.is_none() || self.replay_cursor != self.replay.len() {
            return Err(AppError::new("transfer", "目标终端尚未完整接收会话"));
        }
        Ok(())
    }
    pub fn commit(&mut self) -> AppResult<()> {
        self.ready_to_commit()?;
        self.fallback = None;
        self.frozen_token = None;
        self.replay.clear();
        self.replay_bytes = 0;
        self.replay_cursor = 0;
        Ok(())
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen_token.is_some()
    }
    pub fn frozen_with(&self, token: &str) -> bool {
        self.frozen_token.as_deref() == Some(token)
    }
    pub fn abandon(&mut self) {
        self.output = None;
        self.fallback = None;
        self.frozen_token = None;
        self.pending.clear();
        self.replay.clear();
        self.pending_bytes = 0;
        self.replay_bytes = 0;
        self.replay_cursor = 0;
    }
    fn flush_replay(&mut self) {
        while self.replay_cursor < self.replay.len() {
            if let Some(output) = &self.output {
                if output.send(self.replay[self.replay_cursor].clone()).is_ok() {
                    self.replay_cursor += 1;
                    continue;
                }
            }
            self.output = None;
            break;
        }
    }
    fn flush(&mut self) {
        while let Some(event) = self.pending.pop_front() {
            if let Some(output) = &self.output {
                if output.send(event.clone()).is_ok() {
                    if let TerminalEvent::Data { data } = &event {
                        self.pending_bytes -= data.len();
                    }
                    continue;
                }
            }
            self.output = None;
            self.pending.push_front(event);
            break;
        }
    }
    fn backpressured(&self) -> bool {
        self.replay_bytes >= 4 * 1024 * 1024
            || (self.output.is_none() && self.pending_bytes >= 4 * 1024 * 1024)
    }
}
pub struct WindowOwner {
    pub label: String,
    pub generation: u64,
}

pub fn establish(
    server: &Server,
    credentials: &Credentials,
    expected: Option<&str>,
) -> AppResult<(Session, String)> {
    let addresses = (server.host.as_str(), server.port).to_socket_addrs()?;
    let mut stream = None;
    for address in addresses.take(4) {
        if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_secs(8)) {
            stream = Some(socket);
            break;
        }
    }
    let stream =
        stream.ok_or_else(|| AppError::new("network", "连接失败或超时，请检查地址、端口及网络"))?;
    stream.set_read_timeout(Some(Duration::from_secs(12)))?;
    stream.set_write_timeout(Some(Duration::from_secs(12)))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(stream);
    session.set_timeout(12000);
    session.handshake()?;
    let key = session
        .host_key()
        .ok_or_else(|| AppError::new("ssh", "服务器没有提供主机密钥"))?
        .0;
    let fingerprint = format!(
        "SHA256:{}",
        STANDARD.encode(Sha256::digest(key)).trim_end_matches('=')
    );
    if expected != Some(fingerprint.as_str()) {
        return Err(AppError::host_key(
            fingerprint,
            expected.map(str::to_string),
        ));
    }
    if server.username.trim().is_empty() {
        return Err(AppError::new(
            "authentication",
            "请在服务器配置中填写用户名",
        ));
    }
    match server.auth_type.as_str() {
        "password" => session
            .userauth_password(&server.username, &credentials.password)
            .map_err(|error| {
                if error.code() == ssh2::ErrorCode::Session(-18) {
                    AppError::new("authentication", "SSH 密码认证失败，请检查用户名和密码。")
                } else {
                    AppError::from(error)
                }
            })?,
        "key" => {
            if server.private_key.is_empty() {
                return Err(AppError::new("authentication", "请填写本地私钥路径"));
            }
            session.userauth_pubkey_file(
                &server.username,
                None,
                Path::new(&server.private_key),
                (!credentials.passphrase.is_empty()).then_some(credentials.passphrase.as_str()),
            )?;
        }
        _ => session.userauth_agent(&server.username)?,
    }
    if !session.authenticated() {
        return Err(AppError::new("authentication", "SSH 认证失败"));
    }
    session.set_keepalive(true, 30);
    Ok((session, fingerprint))
}
pub fn establish_spec(spec: &ConnectionSpec) -> AppResult<Session> {
    Ok(establish(&spec.server, &spec.credentials, Some(&spec.fingerprint))?.0)
}
pub fn start_terminal(
    session: Session,
    spec: ConnectionSpec,
    owner: WindowOwner,
    id: String,
    output: Channel<TerminalEvent>,
    command: Option<String>,
    state: std::sync::Arc<State>,
) -> AppResult<()> {
    let mut channel = session.channel_session()?;
    channel.handle_extended_data(ssh2::ExtendedData::Merge)?;
    channel.request_pty("xterm-256color", None, Some((100, 30, 0, 0)))?;
    if let Some(command) = command {
        channel.exec(&command)?;
    } else {
        channel.shell()?;
    }
    session.set_blocking(false);
    let (sender, receiver) = mpsc::channel();
    let generations = state.window_generations.lock().unwrap();
    let closed_windows = state.closed_windows.lock().unwrap();
    if closed_windows.contains(&owner.label)
        || generations.get(&owner.label).copied().unwrap_or(0) != owner.generation
    {
        return Err(AppError::new("cancelled", "窗口已经关闭"));
    }
    let stream = std::sync::Arc::new(std::sync::Mutex::new(TerminalStream::new(output)));
    state.terminals.lock().unwrap().insert(
        id.clone(),
        TerminalHandle {
            window: owner.label,
            sender,
            spec,
            stream: stream.clone(),
        },
    );
    drop(closed_windows);
    drop(generations);
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        let mut queued = std::collections::VecDeque::<u8>::new();
        let mut last_keepalive = Instant::now();
        let mut pending_size = None;
        let result: Result<(), String> = (|| loop {
            for input in receiver.try_iter() {
                match input {
                    Input::Close => return Ok(()),
                    Input::Data(data) => {
                        if queued.len() + data.len() > 4 * 1024 * 1024 {
                            return Err("输入队列过大，连接已结束".into());
                        }
                        queued.extend(data);
                    }
                    Input::Resize(cols, rows) => pending_size = Some((cols, rows)),
                }
            }
            if let Some((cols, rows)) = pending_size {
                match channel.request_pty_size(cols, rows, None, None) {
                    Ok(()) => pending_size = None,
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            if !queued.is_empty() {
                let bytes = queued.make_contiguous();
                match channel.write(bytes) {
                    Ok(count) => {
                        queued.drain(..count);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            if stream.lock().unwrap().backpressured() {
                std::thread::sleep(Duration::from_millis(8));
                continue;
            }
            match channel.read(&mut buffer) {
                Ok(count) if count > 0 => {
                    stream.lock().unwrap().send(TerminalEvent::Data {
                        data: STANDARD.encode(&buffer[..count]),
                    });
                }
                Ok(_) if channel.eof() => return Ok(()),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
            if last_keepalive.elapsed() > Duration::from_secs(20) {
                if let Err(e) = session.keepalive_send() {
                    if e.code() != ssh2::ErrorCode::Session(-37) {
                        return Err(e.to_string());
                    }
                }
                last_keepalive = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(8));
        })();
        let _ = channel.close();
        let _ = session.disconnect(None, "Terminal closed", None);
        stream.lock().unwrap().send(TerminalEvent::Closed {
            message: result.err().unwrap_or_else(|| "会话已结束".into()),
        });
        // Retain a closed stream only while its view is being transferred, so the
        // destination receives the final output and exit event as well.
        let mut terminals = state.terminals.lock().unwrap();
        if !stream.lock().unwrap().is_frozen() {
            terminals.remove(&id);
        }
    });
    Ok(())
}
pub struct ExecResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub fn exec(session: &Session, command: &str) -> AppResult<ExecResult> {
    session.set_blocking(false);
    let start = Instant::now();
    let mut channel = loop {
        match session.channel_session() {
            Ok(channel) => break channel,
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                if start.elapsed() > Duration::from_secs(12) {
                    return Err(AppError::new("timeout", "Docker 查询超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    };
    loop {
        match channel.exec(command) {
            Ok(()) => break,
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                if start.elapsed() > Duration::from_secs(12) {
                    return Err(AppError::new("timeout", "Docker 查询超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let mut received = false;
        for (stream, result) in [(0, &mut stdout), (1, &mut stderr)] {
            match channel.stream(stream).read(&mut buf) {
                Ok(n) => {
                    received |= n > 0;
                    result.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
        if stdout.len() + stderr.len() > 8 * 1024 * 1024 {
            return Err(AppError::new("limit", "查询输出超过 8 MB"));
        }
        if channel.eof() && !received {
            break;
        }
        if start.elapsed() > Duration::from_secs(12) {
            let _ = channel.close();
            return Err(AppError::new("timeout", "查询超时，未确认服务状态"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    session.set_blocking(true);
    let _ = channel.wait_close();
    Ok(ExecResult {
        code: channel.exit_status()?,
        stdout: String::from_utf8_lossy(&stdout).into(),
        stderr: String::from_utf8_lossy(&stderr).into(),
    })
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tauri::ipc::InvokeResponseBody;

    fn collector() -> (Channel<TerminalEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let received = messages.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                received.lock().unwrap().push(serde_json::from_str(&json)?);
            }
            Ok(())
        });
        (channel, messages)
    }

    #[test]
    fn rollback_replays_output_already_sent_to_failed_target() {
        let (source, source_messages) = collector();
        let (target, target_messages) = collector();
        let mut stream = TerminalStream::new(source);
        stream.freeze("move-1".into()).unwrap();
        stream.send(TerminalEvent::Data {
            data: "first".into(),
        });
        stream.attach(target).unwrap();
        stream.send(TerminalEvent::Data {
            data: "second".into(),
        });
        stream.rollback();

        let source = source_messages.lock().unwrap();
        assert_eq!(source.len(), 3);
        assert_eq!(source[0]["type"], "frozen");
        assert_eq!(source[1]["data"], "first");
        assert_eq!(source[2]["data"], "second");
        let target = target_messages.lock().unwrap();
        assert_eq!(target.len(), 2);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    #[ignore = "requires python tests/ssh_fixture.py on 127.0.0.1:22222"]
    fn loopback_ssh_sftp_and_docker() {
        let server = Server {
            id: "fixture".into(),
            name: "Fixture".into(),
            host: "127.0.0.1".into(),
            port: 22222,
            username: "ops".into(),
            tags: vec![],
            auth_type: "password".into(),
            private_key: String::new(),
            password: String::new(),
        };
        let credentials = Credentials {
            password: "fixture-password".into(),
            passphrase: String::new(),
        };
        let unknown = match establish(&server, &credentials, None) {
            Err(error) => error,
            Ok(_) => panic!("untrusted host accepted"),
        };
        assert_eq!(unknown.kind, "hostKey", "{}", unknown.message);
        let fingerprint = unknown.fingerprint.unwrap();
        assert!(fingerprint.starts_with("SHA256:"));
        assert!(
            matches!(establish(&server, &credentials, Some("SHA256:wrong")), Err(error) if error.kind == "hostKey")
        );
        let bad = Credentials {
            password: "wrong".into(),
            passphrase: String::new(),
        };
        assert!(establish(&server, &bad, Some(&fingerprint)).is_err());
        let (session, _) = establish(&server, &credentials, Some(&fingerprint)).unwrap();
        let sftp = session.sftp().unwrap();
        let root = sftp.realpath(Path::new(".")).unwrap();
        let files = sftp.readdir(&root).unwrap();
        assert!(files
            .iter()
            .any(|(path, stat)| path.file_name().unwrap() == "projects" && stat.is_dir()));
        let spec = ConnectionSpec {
            server,
            credentials,
            fingerprint,
        };
        let docker = crate::docker::detect(&spec).unwrap();
        assert_eq!(docker.state, "running");
        assert_eq!(docker.endpoint, "unix:///var/run/docker.sock");
        let images = crate::docker::images(&spec, &docker.endpoint).unwrap();
        assert_eq!(images[0].name, "nginx:alpine");
        let containers = crate::docker::containers(&spec, &docker.endpoint).unwrap();
        assert_eq!(containers.len(), 3);
        assert_eq!(containers[0].state, "running");
        let target = crate::docker::ContainerTarget {
            id: containers[0].id.clone(),
            name: containers[0].name.clone(),
            endpoint: docker.endpoint,
        };
        let command = crate::docker::terminal_command(&spec, &target).unwrap();
        assert!(
            command.ends_with("/bin/sh"),
            "missing bash must fall back to sh"
        );
        assert!(command.contains(&target.id));
    }
}
