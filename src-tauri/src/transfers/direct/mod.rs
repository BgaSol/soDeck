pub(super) mod capabilities;
mod command;
use super::{
    model::{Attempt, DirectAuth, Source, WriteHandle},
    planner::Plan,
};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path, remote_write, ssh,
    ssh_transport::{Cancellation, SshConnection},
};
use ssh2::{OpenFlags, OpenType, Sftp};
use std::{
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

pub fn write_new(sftp: &Sftp, path: &str, bytes: &[u8]) -> AppResult<()> {
    let mut file = sftp.open_mode(
        Path::new(path),
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
        0o600,
        OpenType::File,
    )?;
    file.write_all(bytes)?;
    file.close()?;
    Ok(())
}
// One-shot exit receipts, written after the tool exits. No remote supervisor,
// lease, saved PID or final-file commit is involved.
fn recorded(command: &str, root: &str, name: &str) -> String {
    format!(
        "trap ':' TERM INT HUP; {command}; code=$?; printf '%s\\n' \"$code\" > {} && mv {} {}; exit \"$code\"",
        ssh::quote(&format!("{root}/{name}.next")),
        ssh::quote(&format!("{root}/{name}.next")),
        ssh::quote(&format!("{root}/{name}"))
    )
}
pub fn exit_code(sftp: &Sftp, root: &str, name: &str) -> AppResult<Option<i32>> {
    let path = format!("{root}/{name}");
    if remote_write::metadata(sftp, &path)?.is_none() {
        return Ok(None);
    }
    let mut bytes = vec![];
    sftp.open(Path::new(&path))?
        .take(16)
        .read_to_end(&mut bytes)?;
    let code = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .filter(|c| (0..=255).contains(c))
        .ok_or_else(|| AppError::new("interrupted", "远端退出记录无效"))?;
    Ok(Some(code))
}
pub struct Executor {
    source: SshConnection,
    _target: SshConnection,
    source_sftp: Sftp,
    target_sftp: Sftp,
    root: String,
    ssh: String,
    auth: DirectAuth,
}
impl Executor {
    pub fn prepare(state: &crate::State, item: &WriteHandle, index: usize) -> AppResult<Self> {
        let Source::Remote { context, .. } = &item.source else {
            unreachable!()
        };
        let source = connection::connect_verified(
            context,
            &item.cancel,
            crate::traffic::TrafficPurpose::Transfer,
        )?;
        let target = connection::connect_verified(
            &item.target,
            &item.cancel,
            crate::traffic::TrafficPurpose::Transfer,
        )?;
        let auth = item
            .auth
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::new("authentication", "请选择直传认证方式"))?;
        capabilities::check(
            &source,
            &item.cancel,
            true,
            matches!(auth, DirectAuth::Password(_)),
        )?;
        capabilities::check(&target, &item.cancel, false, false)?;
        let source_sftp = source.session().sftp()?;
        let target_sftp = target.session().sftp()?;
        let home = source_sftp
            .realpath(Path::new("."))?
            .to_str()
            .ok_or_else(|| AppError::new("path", "源主目录不是 UTF-8"))?
            .to_owned();
        let root = remote_path::join(
            &home,
            &format!(".ssh-workspace-transfer-{}", uuid::Uuid::new_v4()),
        )?;
        item.receipt.lock().unwrap().attempts[index].source_root = Some(root.clone());
        state.transfers.persist(item)?;
        source_sftp.mkdir(Path::new(&root), 0o700)?;
        let endpoint = item
            .endpoint
            .as_ref()
            .ok_or_else(|| AppError::new("validation", "缺少真实地址"))?;
        write_new(
            &source_sftp,
            &format!("{root}/known_hosts"),
            format!("ssh-workspace-target {}\n", endpoint.public_key).as_bytes(),
        )?;
        let ssh = command::ssh_command(endpoint, &format!("{root}/known_hosts"), &auth)?;
        Ok(Self {
            source,
            _target: target,
            source_sftp,
            target_sftp,
            root,
            ssh,
            auth,
        })
    }
    pub fn transfer(
        &mut self,
        state: &crate::State,
        item: &WriteHandle,
        plan: &mut Plan,
        index: usize,
        entries: &[usize],
    ) -> AppResult<()> {
        let target_root = item.receipt.lock().unwrap().attempts[index]
            .target_root
            .clone();
        let endpoint = item.endpoint.as_ref().unwrap();
        let nonce = uuid::Uuid::new_v4().to_string();
        write_new(
            &self.target_sftp,
            &format!("{target_root}/probe"),
            nonce.as_bytes(),
        )?;
        let probe = format!(
            "{} {} {}",
            self.ssh,
            ssh::quote(&endpoint.host),
            ssh::quote(&format!(
                "cat {}",
                ssh::quote(&format!("{target_root}/probe"))
            ))
        );
        let (code, out, err) = self.execute(
            item,
            &self.auth_command(&probe),
            Some(Duration::from_secs(20)),
            false,
        )?;
        if code != 0 || out != nonce {
            return Err(AppError::new(
                "environment",
                format!("A → B 认证、真实地址或目录检查失败：{}", err.trim()),
            ));
        }
        let mut manifest = vec![];
        for &i in entries {
            let entry = &mut plan.entries[i];
            manifest.extend_from_slice(entry.source.trim_start_matches('/').as_bytes());
            manifest.push(0);
            entry.staged = Some(format!(
                "{target_root}/data/{}",
                entry.source.trim_start_matches('/')
            ));
        }
        write_new(
            &self.source_sftp,
            &format!("{}/manifest", self.root),
            &manifest,
        )?;
        let receiver = recorded("rsync \"$@\"", &target_root, "receiver.exit");
        let receiver = command::shell_call("sh", &["-c", &receiver, "rsync-receiver"]);
        let host = if endpoint.host.contains(':') {
            format!("[{}]", endpoint.host)
        } else {
            endpoint.host.clone()
        };
        let destination = format!("{host}:{target_root}/data/");
        let command = command::shell_call(
            "rsync",
            &[
                "--relative",
                "--dirs",
                "--no-recursive",
                "--no-links",
                "--no-devices",
                "--no-specials",
                "--protect-args",
                "--from0",
                "--partial-dir=.rsync-partial",
                "--checksum",
                "--info=progress2",
                "--outbuf=L",
                "--timeout=60",
                &format!("--files-from={}/manifest", self.root),
                &format!("--rsync-path={receiver}"),
                "-e",
                &self.ssh,
                "--",
                "/",
                &destination,
            ],
        );
        state.transfers.save_plan(item, plan)?;
        item.receipt.lock().unwrap().attempts[index].dispatched = true;
        state.transfers.persist(item)?;
        let wrapped = recorded(&self.auth_command(&command), &self.root, "source.exit");
        let result = self.execute(item, &wrapped, None, true);
        let a = exit_code(&self.source_sftp, &self.root, "source.exit")
            .ok()
            .flatten();
        let b = exit_code(&self.target_sftp, &target_root, "receiver.exit")
            .ok()
            .flatten();
        {
            let mut r = item.receipt.lock().unwrap();
            let attempt = &mut r.attempts[index];
            attempt.source_finished = a.is_some();
            attempt.receiver_finished = b.is_some();
            attempt.successful =
                a == Some(0) && b == Some(0) && result.as_ref().is_ok_and(|r| r.0 == 0);
        }
        state.transfers.persist(item)?;
        if item.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::new(
                if a.is_some() && b.is_some() {
                    "cancelled"
                } else {
                    "interrupted"
                },
                if a.is_some() && b.is_some() {
                    "传输已停止，暂存内容未提交"
                } else {
                    "已请求取消；远端退出尚未确认，暂存内容未提交"
                },
            ));
        }
        let (code, _, err) = result?;
        if code != 0 {
            return Err(AppError::new(
                "directTransfer",
                format!("rsync 直传失败（退出码 {code}）：{}", err.trim()),
            ));
        }
        if a != Some(0) || b != Some(0) {
            return Err(AppError::new(
                "interrupted",
                "rsync 退出记录不完整，临时内容尚未提交",
            ));
        }
        Ok(())
    }
    fn auth_command(&self, command: &str) -> String {
        if matches!(self.auth, DirectAuth::Password(_)) {
            format!("sshpass -d 3 {command} < /dev/null")
        } else {
            format!("{command} < /dev/null")
        }
    }
    fn execute(
        &self,
        item: &WriteHandle,
        command: &str,
        timeout: Option<Duration>,
        progress: bool,
    ) -> AppResult<(i32, String, String)> {
        execute_remote(
            &self.source,
            Some(&self.auth),
            item,
            command,
            timeout,
            progress,
        )
    }
}
pub(super) fn execute_remote(
    connection: &crate::ssh_transport::SshConnection,
    auth: Option<&DirectAuth>,
    item: &WriteHandle,
    command: &str,
    timeout: Option<Duration>,
    progress: bool,
) -> AppResult<(i32, String, String)> {
    let session = connection.session();
    session.set_blocking(false);
    let result = (|| {
        let mut channel =
            crate::ssh_transport::retry(&item.cancel, Duration::from_secs(15), || {
                session.channel_session()
            })?;
        crate::ssh_transport::retry(&item.cancel, Duration::from_secs(15), || {
            channel.exec(&format!("exec 3<&0; printf 'SSH_WORKSPACE_PROCESS %s %s\\n' \"$$\" \"$(awk '{{print $22}}' /proc/$$/stat)\" >&2; {command}"))
        })?;
        if let Some(DirectAuth::Password(password)) = auth {
            let secret = format!("{password}\n");
            let mut offset = 0;
            let deadline = Instant::now() + Duration::from_secs(15);
            while offset < secret.len() {
                item.check()?;
                match channel.write(&secret.as_bytes()[offset..]) {
                    Ok(0) => return Err(AppError::new("authentication", "密码通道已关闭")),
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() > deadline {
                            return Err(AppError::new("timeout", "密码通道超时"));
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        crate::ssh_transport::retry(&item.cancel, Duration::from_secs(3), || channel.send_eof())?;
        let started = Instant::now();
        let base_bytes = item.task.lock().unwrap().bytes_done;
        let mut stopping = None;
        let mut output = [Vec::new(), Vec::new()];
        let mut process = None;
        let mut progress_bytes = vec![];
        loop {
            if stopping.is_none()
                && (item.stop.load(std::sync::atomic::Ordering::SeqCst)
                    || item.cancel.is_cancelled())
            {
                stopping = Some(Instant::now());
                if let Some((pid, birth)) = process {
                    // Only the live channel's process may be signalled. No
                    // persisted PID is ever used after reconnect/restart.
                    let stop = format!("test \"$(awk '{{print $22}}' /proc/{pid}/stat 2>/dev/null)\" = {birth} && test \"$(ps -o pgid= -p {pid} | tr -d ' ')\" = {pid} && kill -TERM -{pid}");
                    let _ = capabilities::exec_checked(
                        connection,
                        &item.cancel,
                        &stop,
                        Duration::from_secs(2),
                        || item.cancel.check(),
                    );
                    session.set_blocking(false);
                }
            }
            if stopping.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                let _ = channel.close();
                return Err(AppError::new(
                    "interrupted",
                    "已请求取消；远端退出尚未确认。旧数据仅在临时目录，可使用新目录重试",
                ));
            }
            if timeout.is_some_and(|t| started.elapsed() > t) {
                let _ = channel.close();
                return Err(AppError::new("timeout", "A → B 连接或认证检查超时"));
            }
            let mut buffer = [0u8; 8192];
            for stream in 0..2 {
                match channel.stream(stream as i32).read(&mut buffer) {
                    Ok(n) => {
                        output[stream].extend_from_slice(
                            &buffer[..n.min(32768usize.saturating_sub(output[stream].len()))],
                        );
                        if progress && stream == 0 {
                            progress_bytes.extend_from_slice(&buffer[..n]);
                            update_progress(item, &mut progress_bytes, base_bytes);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(AppError::new("interrupted", e)),
                }
            }
            if process.is_none() {
                process = String::from_utf8_lossy(&output[1])
                    .split_inclusive('\n')
                    .filter(|s| s.ends_with('\n'))
                    .find_map(|line| {
                        let fields = line
                            .strip_prefix("SSH_WORKSPACE_PROCESS ")?
                            .split_whitespace()
                            .collect::<Vec<_>>();
                        if fields.len() != 2 {
                            return None;
                        }
                        Some((
                            fields[0].parse::<u32>().ok()?,
                            fields[1].parse::<u64>().ok()?,
                        ))
                    });
            }
            if channel.eof() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        crate::ssh_transport::retry(&item.cancel, Duration::from_secs(3), || {
            channel.wait_close()
        })?;
        let text = |bytes: &[u8]| {
            let s = String::from_utf8_lossy(bytes)
                .lines()
                .filter(|s| !s.starts_with("SSH_WORKSPACE_PROCESS "))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(DirectAuth::Password(p)) = auth {
                s.replace(p, "[已隐藏]")
            } else {
                s
            }
        };
        Ok((channel.exit_status()?, text(&output[0]), text(&output[1])))
    })();
    session.set_blocking(true);
    result
}
fn update_progress(item: &WriteHandle, buffer: &mut Vec<u8>, base: u64) {
    while let Some(end) = buffer.iter().position(|b| *b == b'\r' || *b == b'\n') {
        let line = String::from_utf8_lossy(&buffer[..end]);
        let mut fields = line.split_whitespace();
        if let (Some(bytes), Some(percent)) = (fields.next(), fields.next()) {
            if percent.ends_with('%') {
                if let Ok(bytes) = bytes.replace(',', "").parse::<u64>() {
                    let mut t = item.task.lock().unwrap();
                    t.bytes_done = (base + bytes).min(t.bytes_total);
                    t.current_bytes = bytes;
                }
            }
        }
        buffer.drain(..=end);
    }
    if buffer.len() > 16384 {
        buffer.clear();
    }
}
pub fn refresh_attempt(item: &WriteHandle, attempt: &mut Attempt) -> AppResult<()> {
    if item.same_server() {
        return super::same_server::refresh(item, attempt);
    }
    if attempt.source_finished && attempt.receiver_finished {
        return Ok(());
    }
    if !attempt.dispatched {
        attempt.source_finished = true;
        attempt.receiver_finished = true;
        return Ok(());
    }
    let Source::Remote { context, .. } = &item.source else {
        attempt.source_finished = true;
        attempt.receiver_finished = true;
        return Ok(());
    };
    let source = connection::connect_verified(
        context,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Transfer,
    )?;
    let target = connection::connect_verified(
        &item.target,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::Transfer,
    )?;
    let a = match &attempt.source_root {
        Some(root) => exit_code(&source.session().sftp()?, root, "source.exit")?,
        None => None,
    };
    let b = exit_code(
        &target.session().sftp()?,
        &attempt.target_root,
        "receiver.exit",
    )?;
    attempt.source_finished |= a.is_some();
    attempt.receiver_finished |= b.is_some();
    attempt.successful |= a == Some(0) && b == Some(0);
    Ok(())
}
