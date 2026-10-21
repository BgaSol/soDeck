use crate::{
    connection::{ConnectionContext, ConnectionEvent as TerminalEvent},
    model::{AppError, AppResult},
    shell_integration,
    ssh_transport::{Cancellation, SshConnection},
    terminal_cwd::{CwdParser, ParsedOutput},
    State,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    io::{Read, Write},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

pub enum Input {
    Data(Vec<u8>),
    Resize(u32, u32),
    ClearDisplay,
    Close,
}
pub struct InputSender {
    pub channel: mpsc::Sender<Input>,
    pub cancel: Cancellation,
}
impl InputSender {
    pub fn send(&self, input: Input) -> Result<(), mpsc::SendError<Input>> {
        if matches!(input, Input::Close) {
            self.cancel.cancel();
        }
        self.channel.send(input)
    }
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
    last_cwd: Option<TerminalEvent>,
    last_ready: Option<TerminalEvent>,
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
            last_cwd: None,
            last_ready: None,
            closed: false,
        }
    }
    pub fn send(&mut self, event: TerminalEvent) {
        if matches!(event, TerminalEvent::Ready { .. }) {
            self.last_ready = Some(event.clone());
        }
        if matches!(event, TerminalEvent::Cwd { .. }) {
            self.last_cwd = Some(event.clone());
        }
        if matches!(event, TerminalEvent::Closed { .. }) {
            self.closed = true;
        }
        if self.is_frozen() {
            self.replay_bytes += event_size(&event);
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
        self.pending_bytes += event_size(&event);
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
        if !self.closed {
            if let Some(ready) = self.last_ready.clone() {
                if self.output.as_ref().unwrap().send(ready).is_err() {
                    self.output = None;
                }
            }
        }
        // A destination may attach after the last prompt, with no later Cwd in
        // replay. Re-send the latest state; sequence numbers let it ignore older
        // replayed reports and any duplicate from the frontend snapshot.
        if let Some(cwd) = self.last_cwd.clone() {
            if self.output.as_ref().is_some_and(|o| o.send(cwd).is_err()) {
                self.output = None;
            }
        }
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
        self.pending_bytes += replay.iter().map(event_size).sum::<usize>();
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
                    self.pending_bytes -= event_size(&event);
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
fn event_size(event: &TerminalEvent) -> usize {
    match event {
        TerminalEvent::Data { data } => data.len(),
        TerminalEvent::Cwd { path, home, .. } => {
            path.len() + home.as_ref().map_or(0, String::len) + 32
        }
        TerminalEvent::Closed { message, .. } => message.len() + 16,
        TerminalEvent::Frozen { token } => token.len() + 16,
        _ => 512,
    }
}
pub fn run(
    connection: SshConnection,
    context: Arc<ConnectionContext>,
    id: &str,
    command: Option<String>,
    state: &State,
    receiver: mpsc::Receiver<Input>,
    cancel: &Cancellation,
    history_origin: Option<(String, crate::terminal_history::model::Source)>,
) -> AppResult<()> {
    cancel.check()?;
    let diagnostic = cancel.diagnostic();
    let mut setup_stage = diagnostic.stage(
        "terminal",
        "terminal.setup",
        serde_json::json!({"connectionRef":crate::diagnostics::alias(id)}),
    );
    let session = connection.session();
    session.set_blocking(true);
    let integration = if command.is_none() {
        let token = uuid::Uuid::new_v4().to_string();
        let blocking = session.is_blocking();
        // Probe through a separate exec channel on this same SSH connection.
        // Banners are tolerated, but unknown shells and failed/rejected probes
        // use the server's genuine login-shell request without guessed flags.
        let probe = crate::ssh::exec(session, &shell_integration::probe_command(&token));
        session.set_blocking(blocking);
        probe
            .ok()
            .filter(|result| result.code == 0)
            .and_then(|result| {
                shell_integration::supported_shell(&result.stdout, &token)
                    .map(|shell| (token, shell))
            })
    } else {
        None
    };
    let mut channel = session.channel_session()?;
    diagnostic.event(
        "DEBUG",
        "terminal",
        "channel.created",
        serde_json::json!({}),
    );
    channel.handle_extended_data(ssh2::ExtendedData::Merge)?;
    channel.request_pty("xterm-256color", None, Some((100, 30, 0, 0)))?;
    diagnostic.event("DEBUG", "terminal", "pty.created", serde_json::json!({}));
    let cwd_token = if let Some(command) = command {
        channel.exec(&command)?;
        None
    } else if let Some((token, shell)) = integration {
        match channel.exec(&shell_integration::launch_command(&token, &shell)) {
            Ok(()) => Some(token),
            Err(_) => {
                // Restricted SSH endpoints can allow shell but reject exec.
                // Preserve their ordinary terminal even without integration.
                let _ = channel.close();
                channel = session.channel_session()?;
                channel.handle_extended_data(ssh2::ExtendedData::Merge)?;
                channel.request_pty("xterm-256color", None, Some((100, 30, 0, 0)))?;
                channel.shell()?;
                None
            }
        }
    } else {
        channel.shell()?;
        None
    };
    session.set_blocking(false);
    setup_stage.finish(&Ok::<_, AppError>(()));
    diagnostic.event(
        "INFO",
        "terminal",
        "terminal.ready",
        serde_json::json!({"connectionRef":crate::diagnostics::alias(id)}),
    );

    use crate::terminal_history::model::EventKind;
    let binding = history_origin
        .as_ref()
        .map(|(workspace, source)| TerminalEvent::History {
            session_id: id.into(),
            source_id: source.id.clone(),
            workspace_id: workspace.clone(),
        });
    let mut recorder =
        history_origin
            .filter(|_| state.history.root.is_some())
            .map(|(workspace, source)| {
                let mut recorder = state.history.start(id, &workspace, source);
                recorder.push(EventKind::Start {
                    cols: 100,
                    rows: 30,
                });
                recorder
            });

    let stream = {
        let mut connections = state.terminals.lock().unwrap();
        cancel.check()?;
        let handle = connections
            .get_mut(id)
            .ok_or_else(|| AppError::new("cancelled", "连接已关闭"))?;
        handle.spec = Some(context.clone());
        let stream = handle.stream.clone();
        stream.lock().unwrap().send(TerminalEvent::Ready {
            connection: context.view(),
        });
        if let Some(binding) = binding {
            stream.lock().unwrap().send(binding);
        }
        stream
    };
    let mut cwd_parser = cwd_token.as_deref().map(CwdParser::new);
    let mut buffer = [0u8; 16384];
    let mut queued = std::collections::VecDeque::<u8>::new();
    let mut last_keepalive = Instant::now();
    let mut pending_size = None;
    let mut pending_write = Vec::<u8>::new();
    let mut write_offset = 0;
    let mut history_checkpoint = Instant::now();
    let mut eof_started: Option<Instant> = None;
    let result: Result<(), String> = (|| loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        for input in receiver.try_iter() {
            match input {
                Input::Close => return Ok(()),
                Input::Data(data) => {
                    if queued.len() + pending_write.len() - write_offset + data.len()
                        > 4 * 1024 * 1024
                    {
                        return Err("输入队列过大，连接已结束".into());
                    }
                    queued.extend(data);
                }
                Input::Resize(cols, rows) => pending_size = Some((cols, rows)),
                Input::ClearDisplay => {
                    if let Some(recorder) = &mut recorder {
                        recorder.push(EventKind::Clear);
                    }
                    stream.lock().unwrap().send(TerminalEvent::DisplayCleared);
                }
            }
        }
        if let Some((cols, rows)) = pending_size {
            match channel.request_pty_size(cols, rows, None, None) {
                Ok(()) => {
                    pending_size = None;
                    if let Some(recorder) = &mut recorder {
                        recorder.push(EventKind::Resize {
                            cols: cols as u16,
                            rows: rows as u16,
                        });
                    }
                }
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        if write_offset == pending_write.len() && !queued.is_empty() {
            pending_write = queued.drain(..).collect();
            write_offset = 0;
        }
        if write_offset < pending_write.len() {
            match channel.write(&pending_write[write_offset..]) {
                Ok(0) => return Err("终端输入通道已关闭".into()),
                Ok(count) => {
                    write_offset += count;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        if last_keepalive.elapsed() > Duration::from_secs(20) {
            if let Err(e) = session.keepalive_send() {
                if e.code() != ssh2::ErrorCode::Session(-37) {
                    return Err(e.to_string());
                }
            }
            last_keepalive = Instant::now();
        }
        if stream.lock().unwrap().backpressured() {
            std::thread::sleep(Duration::from_millis(8));
            continue;
        }
        if history_checkpoint.elapsed() >= Duration::from_millis(250) {
            if let Some(recorder) = &mut recorder {
                recorder.checkpoint();
            }
            history_checkpoint = Instant::now();
        }
        match channel.read(&mut buffer) {
            Ok(count) if count > 0 => {
                let events = if let Some(parser) = &mut cwd_parser {
                    parser.push(&buffer[..count])
                } else {
                    vec![ParsedOutput::Data(buffer[..count].to_vec())]
                };
                let mut stream = stream.lock().unwrap();
                for event in events {
                    match event {
                        ParsedOutput::Data(data) => {
                            if let Some(recorder) = &mut recorder {
                                recorder.push(EventKind::Output {
                                    bytes: data.clone(),
                                });
                            }
                            stream.send(TerminalEvent::Data {
                                data: STANDARD.encode(data),
                            });
                        }
                        ParsedOutput::Cwd(cwd) => {
                            if let Some(recorder) = &mut recorder {
                                recorder.push(EventKind::Cwd {
                                    path: cwd.path.clone(),
                                });
                            }
                            stream.send(TerminalEvent::Cwd {
                                path: cwd.path,
                                home: cwd.home,
                                sequence: cwd.sequence,
                            });
                        }
                    }
                }
            }
            Ok(_) if channel.eof() => {
                if eof_started.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(12)
                {
                    return Err("等待远端关闭终端超时".into());
                }
                // Consume the channel-close packet before inspecting the exit
                // signal. A transport failure here is not a normal shell exit.
                match channel.wait_close() {
                    Ok(()) => {
                        if let Some(error) = connection.transport_error() {
                            return Err(error.message);
                        }
                        let signal = channel.exit_signal().map_err(|e| e.to_string())?;
                        return if signal.exit_signal.is_some() {
                            Err("远端终端被信号中断".into())
                        } else {
                            Ok(())
                        };
                    }
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        std::thread::sleep(Duration::from_millis(8));
    })();
    let _ = channel.close();
    let _ = session.disconnect(None, "Terminal closed", None);
    if let Some(data) = cwd_parser.as_mut().and_then(CwdParser::finish) {
        if let Some(recorder) = &mut recorder {
            recorder.push(EventKind::Output {
                bytes: data.clone(),
            });
        }
        stream.lock().unwrap().send(TerminalEvent::Data {
            data: STANDARD.encode(data),
        });
    }
    if let Some(recorder) = &mut recorder {
        recorder.finish(if cancel.is_cancelled() {
            "主动关闭"
        } else {
            result
                .as_ref()
                .err()
                .map(String::as_str)
                .unwrap_or("连接已结束")
        });
    }

    if cancel.is_cancelled() {
        return Ok(());
    }
    result.map_err(|e| {
        connection
            .transport_error()
            .unwrap_or_else(|| AppError::new("network", e))
    })
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

    #[test]
    fn cwd_survives_transfer_without_new_prompt_and_replays_changes() {
        let (source, source_messages) = collector();
        let (target, target_messages) = collector();
        let mut stream = TerminalStream::new(source);
        stream.send(TerminalEvent::Cwd {
            path: "/home/ops".into(),
            home: Some("/home/ops".into()),
            sequence: 1,
        });
        stream.freeze("move-cwd".into()).unwrap();
        stream.send(TerminalEvent::Cwd {
            path: "/var/log".into(),
            home: Some("/home/ops".into()),
            sequence: 2,
        });
        stream.attach(target).unwrap();
        stream.rollback();
        assert_eq!(source_messages.lock().unwrap()[2]["path"], "/var/log");
        assert_eq!(target_messages.lock().unwrap()[0]["path"], "/var/log");
        assert_eq!(target_messages.lock().unwrap()[1]["sequence"], 2);

        let (next, next_messages) = collector();
        stream.freeze("move-cwd-2".into()).unwrap();
        stream.attach(next).unwrap();
        stream.commit().unwrap();
        assert_eq!(next_messages.lock().unwrap()[0]["path"], "/var/log");
    }

    #[test]
    fn cwd_events_contribute_to_transfer_backpressure() {
        let (source, _) = collector();
        let mut stream = TerminalStream::new(source);
        stream.freeze("move-cwd".into()).unwrap();
        for sequence in 0..512 {
            stream.send(TerminalEvent::Cwd {
                path: format!("/{}", "x".repeat(8192)),
                home: None,
                sequence,
            });
        }
        assert!(stream.backpressured());
        stream.rollback();
        assert_eq!(stream.pending_bytes, 0);
        assert_eq!(stream.replay_bytes, 0);
    }
}
