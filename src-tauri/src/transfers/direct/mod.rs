mod capabilities;
mod command;
mod protocol;
pub(super) mod remote_job;

use super::model::{Source, WriteHandle};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_write,
    ssh_transport::{self, SshConnection},
};
use remote_job::{write_new, Lease};
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

pub struct Controller<'a> {
    state: &'a crate::State,
    source_meta: SshConnection,
    target_meta: SshConnection,
    source_exec: SshConnection,
    source_root: String,
    target_root: String,
    arguments: Vec<String>,
    leases: Vec<Lease>,
    stopped: bool,
}
impl<'a> Controller<'a> {
    pub fn start(state: &'a crate::State, item: &WriteHandle) -> AppResult<Self> {
        let Source::Remote { context, .. } = &item.source else {
            unreachable!()
        };
        item.check()?;
        let source_meta = connection::connect_verified(context, &item.cancel)?;
        let target_meta = connection::connect_verified(&item.target, &item.cancel)?;
        let source_exec = connection::connect_verified(context, &item.cancel)?;
        for connection in [&source_meta, &target_meta, &source_exec] {
            connection.session().set_timeout(3000);
        }
        let mut result = Self {
            state,
            source_meta,
            target_meta,
            source_exec,
            source_root: String::new(),
            target_root: String::new(),
            arguments: vec![],
            leases: vec![],
            stopped: false,
        };
        let prepared = (|| {
            let optional = capabilities::check(&result.source_meta, item, true)?;
            capabilities::check(&result.target_meta, item, false)?;
            result.source_root =
                remote_job::deploy(state, item, &result.source_meta.session().sftp()?, true)?;
            result.target_root =
                remote_job::deploy(state, item, &result.target_meta.session().sftp()?, false)?;
            let endpoint = item
                .endpoint
                .as_ref()
                .ok_or_else(|| AppError::new("validation", "缺少直连目标快照"))?;
            let known_hosts = format!("{}/known_hosts", result.source_root);
            write_new(
                &result.source_meta.session().sftp()?,
                &known_hosts,
                format!("ssh-workspace-target {}\n", endpoint.public_key).as_bytes(),
            )?;
            result.arguments = command::ssh_arguments(endpoint, &known_hosts, &optional)?;
            for (source, root, context) in [
                (true, &result.source_root, context),
                (false, &result.target_root, &item.target),
            ] {
                {
                    let mut receipt = item.receipt.lock().unwrap();
                    let direct = receipt.direct.as_mut().unwrap();
                    if source {
                        direct.source.launched = true;
                    } else {
                        direct.target.launched = true;
                    }
                }
                state.transfers.persist(item)?;
                result.leases.push(Lease::start(
                    context,
                    &item.cancel,
                    root.clone(),
                    item.task.lock().unwrap().run_id.clone(),
                )?);
            }
            let run = item.task.lock().unwrap().run_id.clone();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                item.check()?;
                result.check_leases()?;
                let a = remote_job::state(
                    &result.source_meta.session().sftp()?,
                    &result.source_root,
                    &run,
                    None,
                )?;
                let b = remote_job::state(
                    &result.target_meta.session().sftp()?,
                    &result.target_root,
                    &run,
                    None,
                )?;
                if a.is_some_and(|s| s.phase == "ready") && b.is_some_and(|s| s.phase == "ready") {
                    break;
                }
                if Instant::now() > deadline {
                    return Err(AppError::new("command", "两端任务监督启动超时"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        })();
        if let Err(error) = prepared {
            result.stop(item)?;
            super::recovery::cleanup(item)?;
            return Err(error);
        }
        Ok(result)
    }
    pub fn probe_source(&self) -> String {
        format!("{}/probe", self.source_root)
    }
    fn check_leases(&self) -> AppResult<()> {
        for (index, lease) in self.leases.iter().enumerate() {
            if let Some(error) = lease.error.lock().unwrap().as_ref() {
                return Err(AppError::new(
                    "unknown",
                    format!(
                        "{} 的任务心跳中断：{}",
                        if index == 0 { "A" } else { "B" },
                        error.message
                    ),
                ));
            }
        }
        Ok(())
    }
    pub fn copy(
        &mut self,
        item: &WriteHandle,
        id: usize,
        entry: &super::planner::Entry,
        destination: &str,
    ) -> AppResult<()> {
        let result = self.copy_inner(item, id, entry, destination);
        if result.is_err() && !self.stopped {
            self.stop(item)?;
        }
        result
    }
    fn copy_inner(
        &mut self,
        item: &WriteHandle,
        id: usize,
        entry: &super::planner::Entry,
        destination: &str,
    ) -> AppResult<()> {
        item.check()?;
        self.check_leases()?;
        let source_sftp = self.source_meta.session().sftp()?;
        let target_sftp = self.target_meta.session().sftp()?;
        let before = source_sftp.lstat(Path::new(&entry.source))?;
        if !before.is_file()
            || before.size != Some(entry.size)
            || (entry.modified.is_some() && before.mtime != entry.modified)
        {
            return Err(AppError::new(
                "changed",
                "源文件的类型、大小或修改时间已变化，请重新创建任务",
            ));
        }
        let attempt = uuid::Uuid::new_v4().to_string();
        let run = item.task.lock().unwrap().run_id.clone();
        {
            let mut receipt = item.receipt.lock().unwrap();
            let direct = receipt.direct.as_mut().unwrap();
            direct.attempt = Some(attempt.clone());
            direct.attempt_finished = false;
        }
        self.state.transfers.persist(item)?;
        let source_job = format!("{}/job-{attempt}", self.source_root);
        let target_job = format!("{}/job-{attempt}", self.target_root);
        source_sftp.mkdir(Path::new(&source_job), 0o700)?;
        target_sftp.mkdir(Path::new(&target_job), 0o700)?;
        let receiver = format!(
            "exec {}\n",
            command::shell_call(
                "sh",
                &[&format!("{}/receiver.sh", self.target_root), destination]
            )
        );
        write_new(
            &target_sftp,
            &format!("{target_job}/command.sh"),
            receiver.as_bytes(),
        )?;
        let remote_command = format!(
            "exec {}",
            command::shell_call(
                "sh",
                &[
                    &format!("{}/supervisor.sh", self.target_root),
                    "run",
                    &self.target_root,
                    &run,
                    &attempt
                ]
            )
        );
        let mut args = vec![
            format!("{}/sender.sh", self.source_root),
            entry.source.clone(),
            "ssh".into(),
        ];
        args.extend(self.arguments.iter().cloned());
        args.push(remote_command);
        let sender = format!(
            "exec {}\n",
            command::shell_call("sh", &args.iter().map(String::as_str).collect::<Vec<_>>())
        );
        write_new(
            &source_sftp,
            &format!("{source_job}/command.sh"),
            sender.as_bytes(),
        )?;
        let session = self.source_exec.session();
        session.set_blocking(false);
        let mut channel = ssh_transport::retry(&item.cancel, Duration::from_secs(12), || {
            session.channel_session()
        })?;
        let command = format!(
            "exec {}",
            command::shell_call(
                "sh",
                &[
                    &format!("{}/supervisor.sh", self.source_root),
                    "run",
                    &self.source_root,
                    &run,
                    &attempt
                ]
            )
        );
        // A dispatched request is uncertain until its task-specific receipts
        // say otherwise. Never infer safety solely from channel.exit_status().
        let dispatched = ssh_transport::retry(&item.cancel, Duration::from_secs(12), || {
            channel.exec(&command)
        });
        if let Err(e) = dispatched {
            return Err(AppError::new("unknown", e.message));
        }
        let mut diagnostic = Vec::new();
        let mut buffer = [0u8; 8192];
        let mut next_poll = Instant::now();
        let received = loop {
            if item.stop.load(std::sync::atomic::Ordering::SeqCst) {
                self.stop(item)?;
                break Err(AppError::new("cancelled", "传输已取消，两端进程已停止"));
            }
            if item.cancel.is_cancelled() {
                break Err(AppError::new(
                    "unknown",
                    "控制连接中断，需要核对两端任务状态",
                ));
            }
            self.check_leases()?;
            for stream in [0, 1] {
                match channel.stream(stream).read(&mut buffer) {
                    Ok(n) if stream == 1 => diagnostic.extend_from_slice(
                        &buffer[..n.min(16384usize.saturating_sub(diagnostic.len()))],
                    ),
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(AppError::new("unknown", e)),
                }
            }
            if Instant::now() >= next_poll {
                next_poll = Instant::now() + Duration::from_millis(400);
                let a = remote_job::state(&source_sftp, &self.source_root, &run, Some(&attempt))?;
                let b = remote_job::state(&target_sftp, &self.target_root, &run, Some(&attempt))?;
                if let Some(stat) =
                    remote_write::metadata(&target_sftp, destination)?.filter(|_| id != usize::MAX)
                {
                    let mut task = item.task.lock().unwrap();
                    task.current_file = Some(entry.source.clone());
                    task.current_bytes = stat.size.unwrap_or(0).min(entry.size);
                    task.bytes_done = task.committed_bytes + task.current_bytes;
                }
                if a.as_ref().is_some_and(|s| s.stopped()) {
                    if a.as_ref().is_some_and(|s| s.phase == "done" && s.code == 0)
                        && b.as_ref().is_some_and(|s| s.phase == "done" && s.code == 0)
                    {
                        break Ok(());
                    }
                    // Seal both admissions before accepting a failed/absent B
                    // receipt, including an SSH handshake that failed on A.
                    self.stop(item)?;
                    let message = String::from_utf8_lossy(&diagnostic);
                    break Err(AppError::new(
                        "directTransfer",
                        format!(
                            "A → B 直传失败：{}",
                            if message.trim().is_empty() {
                                "远端发送或接收进程未成功完成"
                            } else {
                                message.trim()
                            }
                        ),
                    ));
                }
            }
            if channel.eof()
                && !remote_job::state(&source_sftp, &self.source_root, &run, Some(&attempt))?
                    .is_some_and(|s| s.stopped())
            {
                break Err(AppError::new(
                    "unknown",
                    "源端连接结束但没有本次任务的停止记录",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let _ = channel.close();
        received?;
        let after = source_sftp.lstat(Path::new(&entry.source))?;
        if !after.is_file() || after.size != before.size || after.mtime != before.mtime {
            return Err(AppError::new(
                "changed",
                "传输期间源文件发生变化，未提交目标文件",
            ));
        }
        // The one-use admission directories and terminal receipts prove these
        // two writers cannot start again, even though the task remains open.
        // Persist that proof before deleting either terminal record: a lost
        // SFTP reply halfway through deletion must remain recoverable.
        item.receipt
            .lock()
            .unwrap()
            .direct
            .as_mut()
            .unwrap()
            .attempt_finished = true;
        self.state.transfers.persist(item)?;
        remote_job::remove_job(&source_sftp, &self.source_root, &attempt)?;
        remote_job::remove_job(&target_sftp, &self.target_root, &attempt)?;
        item.receipt
            .lock()
            .unwrap()
            .direct
            .as_mut()
            .unwrap()
            .attempt = None;
        self.state.transfers.persist(item)?;
        Ok(())
    }
    pub fn finish(&mut self, item: &WriteHandle) -> AppResult<()> {
        self.stop(item)
    }
    pub fn confirmed_stopped(&self) -> bool {
        self.stopped
    }
    pub fn stop(&mut self, item: &WriteHandle) -> AppResult<()> {
        if self.stopped {
            return Ok(());
        }
        let _proof = remote_job::stop_and_verify(item)?;
        self.state.transfers.persist(item).map_err(|e| {
            AppError::new(
                "unknown",
                format!(
                    "已停止远端进程，但停止记录未能保存，暂不清理：{}",
                    e.message
                ),
            )
        })?;
        self.stopped = true;
        self.leases.clear();
        Ok(())
    }
}
