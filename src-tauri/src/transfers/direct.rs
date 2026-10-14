use super::model::{Source, WriteHandle};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path, ssh,
    ssh_transport::{self, SshConnection},
};
use serde_json::{json, Value};
use ssh2::{Channel, OpenFlags, OpenType};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct Controller {
    _connection: SshConnection,
    channel: Channel,
    run_id: String,
    input: Vec<u8>,
    errors: Vec<u8>,
    writer: Arc<Mutex<Channel>>,
    heartbeat_stop: mpsc::Sender<()>,
    heartbeat: Option<JoinHandle<()>>,
    heartbeat_error: Arc<Mutex<Option<AppError>>>,
    stopped: bool,
    copying: bool,
}
impl Controller {
    pub fn start(state: &crate::State, item: &WriteHandle) -> AppResult<Self> {
        let Source::Remote { context, .. } = &item.source else {
            unreachable!()
        };
        let endpoint = item.endpoint.as_ref().expect("direct endpoint");
        item.check()?;
        let connection = connection::connect_verified(context, &item.cancel)?;
        let sftp = connection.session().sftp()?;
        let home = sftp
            .realpath(Path::new("."))?
            .to_str()
            .ok_or_else(|| AppError::new("path", "源服务器主目录不是 UTF-8"))?
            .to_owned();
        let root = remote_path::join(
            &home,
            &format!(".ssh-workspace-task-{}", uuid::Uuid::new_v4()),
        )?;
        sftp.mkdir(Path::new(&root), 0o700)?;
        item.receipt.lock().unwrap().remote_root = root.clone();
        item.receipt.lock().unwrap().remote_stopped = false;
        state.transfers.persist(item)?;
        let run_id = item.task.lock().unwrap().run_id.clone();
        let config = json!({"runId": run_id, "host": endpoint.host, "port": endpoint.port, "user": endpoint.username, "publicKey": endpoint.public_key});
        for (name, contents) in [
            ("runner.py", include_bytes!("remote_runner.py").to_vec()),
            ("config.json", serde_json::to_vec(&config).unwrap()),
            (
                "known_hosts",
                format!("ssh-workspace-target {}\n", endpoint.public_key).into_bytes(),
            ),
        ] {
            let mut file = sftp.open_mode(
                Path::new(&format!("{root}/{name}")),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                0o600,
                OpenType::File,
            )?;
            file.write_all(&contents)?;
            file.close()?;
        }
        drop(sftp);
        connection.session().set_blocking(false);
        let mut channel = ssh_transport::retry(&item.cancel, Duration::from_secs(12), || {
            connection.session().channel_session()
        })?;
        let command = format!(
            "exec python3 {} {}",
            ssh::quote(&format!("{root}/runner.py")),
            ssh::quote(&root)
        );
        ssh_transport::retry(&item.cancel, Duration::from_secs(12), || {
            channel.exec(&command)
        })?;
        let writer = Arc::new(Mutex::new(channel.clone()));
        let (heartbeat_stop, receiver) = mpsc::channel();
        let heartbeat_writer = writer.clone();
        let heartbeat_error = Arc::new(Mutex::new(None));
        let thread_error = heartbeat_error.clone();
        let heartbeat_run = run_id.clone();
        let heartbeat = std::thread::spawn(move || {
            while receiver.recv_timeout(Duration::from_secs(2))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                if let Err(error) =
                    send_message(&heartbeat_writer, &heartbeat_run, json!({"type":"ping"}))
                {
                    *thread_error.lock().unwrap() = Some(error);
                    break;
                }
            }
        });
        let mut controller = Self {
            _connection: connection,
            channel,
            run_id,
            input: Vec::new(),
            errors: Vec::new(),
            writer,
            heartbeat_stop,
            heartbeat: Some(heartbeat),
            heartbeat_error,
            stopped: false,
            copying: false,
        };
        controller.wait(item, "ready", None)?;
        Ok(controller)
    }
    fn send(&self, message: Value) -> AppResult<()> {
        send_message(&self.writer, &self.run_id, message)
    }
    fn event(&mut self) -> AppResult<Option<Value>> {
        let mut bytes = [0u8; 8192];
        for stream in [0, 1] {
            match self.channel.stream(stream).read(&mut bytes) {
                Ok(n) => {
                    let output = if stream == 0 {
                        &mut self.input
                    } else {
                        &mut self.errors
                    };
                    output.extend_from_slice(&bytes[..n]);
                    if output.len() > 262144 {
                        return Err(AppError::new("unknown", "远端任务输出异常，需核对远端状态"));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(AppError::new("unknown", e)),
            }
        }
        if let Some(end) = self.input.iter().position(|c| *c == b'\n') {
            let line: Vec<_> = self.input.drain(..=end).collect();
            let event: Value = serde_json::from_slice(&line)
                .map_err(|_| AppError::new("unknown", "远端任务返回无效状态"))?;
            if event["runId"].as_str() != Some(&self.run_id) && event["type"] != "failed" {
                return Err(AppError::new("unknown", "远端任务代次不匹配"));
            }
            return Ok(Some(event));
        }
        if self.channel.eof() {
            let message = String::from_utf8_lossy(&self.errors);
            return Err(AppError::new(
                if self.copying { "unknown" } else { "command" },
                format!(
                    "源服务器任务已断开：{}",
                    if message.trim().is_empty() {
                        "未收到完成确认"
                    } else {
                        message.trim()
                    }
                ),
            ));
        }
        Ok(None)
    }
    fn wait(&mut self, item: &WriteHandle, expected: &str, id: Option<usize>) -> AppResult<()> {
        let started = Instant::now();
        let mut cancel_sent = (expected == "cancelled").then(Instant::now);
        loop {
            if item.stop.load(std::sync::atomic::Ordering::SeqCst) && cancel_sent.is_none() {
                self.send(json!({"type":"cancel"}))?;
                cancel_sent = Some(Instant::now());
                item.task.lock().unwrap().status = "cancelling".into();
            }
            if cancel_sent.is_some_and(|t| t.elapsed() > Duration::from_secs(10)) {
                return Err(AppError::new(
                    "unknown",
                    "未确认源服务器进程已停止，请稍后核对状态",
                ));
            }
            if expected == "finished" && started.elapsed() > Duration::from_secs(10) {
                return Err(AppError::new(
                    "unknown",
                    "未收到远端任务结束确认，请核对状态",
                ));
            }
            if expected == "ready" && started.elapsed() > Duration::from_secs(20) {
                return Err(AppError::new("command", "直传运行环境检查超时"));
            }
            if let Some(event) = self.event()? {
                let kind = event["type"].as_str().unwrap_or("");
                match kind {
                    "cancelled" => {
                        self.stopped = true;
                        self.copying = false;
                        return Err(AppError::new("cancelled", "传输已取消，远端进程已停止"));
                    }
                    "failed" => {
                        self.stopped = true;
                        self.copying = false;
                        return Err(AppError::new(
                            "command",
                            event["error"].as_str().unwrap_or("远端传输失败"),
                        ));
                    }
                    "itemFailed"
                        if id.is_some() && event["itemId"].as_u64() == id.map(|i| i as u64) =>
                    {
                        self.copying = false;
                        return Err(AppError::new(
                            "directTransfer",
                            event["error"].as_str().unwrap_or("服务器之间传输失败"),
                        ));
                    }
                    _ => {}
                }
                if kind == expected
                    && id.is_none_or(|id| event["itemId"].as_u64() == Some(id as u64))
                {
                    if expected == "itemDone" {
                        self.copying = false;
                    }
                    if expected == "finished" {
                        self.stopped = true;
                    }
                    return Ok(());
                }
            }
            if let Some(error) = self.heartbeat_error.lock().unwrap().clone() {
                return Err(error);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    pub fn copy(
        &mut self,
        item: &WriteHandle,
        id: usize,
        entry: &super::planner::Entry,
        destination: &str,
    ) -> AppResult<()> {
        // Once a copy request may have reached A, a lost acknowledgement means
        // unknown, never a safe-to-retry network failure.
        self.copying = true;
        self.send(json!({"type":"copy", "itemId":id,"source":entry.source,"destination":destination,"size":entry.size,"mtime":entry.modified}))?;
        self.wait(item, "itemDone", Some(id))
    }
    pub fn finish(&mut self, item: &WriteHandle) -> AppResult<()> {
        if self.stopped {
            return Ok(());
        }
        self.send(json!({"type":"finish"}))?;
        self.wait(item, "finished", None)
    }
    pub fn confirmed_stopped(&self) -> bool {
        self.stopped
    }
    pub fn stop(&mut self, item: &WriteHandle) -> AppResult<()> {
        if self.stopped {
            return Ok(());
        }
        self.send(json!({"type":"cancel"}))?;
        let result = self.wait(item, "cancelled", None);
        if self.stopped {
            Ok(())
        } else {
            result
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.heartbeat_stop.send(());
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        // Closing the input also activates the runner's EOF/lease handling.
        let _ = self.channel.send_eof();
    }
}

fn send_message(writer: &Mutex<Channel>, run_id: &str, mut message: Value) -> AppResult<()> {
    message["runId"] = run_id.into();
    let mut bytes = serde_json::to_vec(&message).unwrap();
    bytes.push(b'\n');
    let mut channel = writer.lock().unwrap();
    let mut offset = 0;
    let deadline = Instant::now() + Duration::from_secs(8);
    while offset < bytes.len() {
        match channel.write(&bytes[offset..]) {
            Ok(0) => {
                return Err(AppError::new(
                    "unknown",
                    "源服务器控制通道已关闭，需核对远端任务状态",
                ))
            }
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(AppError::new("unknown", e)),
        }
    }
    Ok(())
}
