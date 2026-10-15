use super::{
    capabilities,
    command::shell_call,
    protocol::{JobState, GUARD_ID},
};
use crate::transfers::model::{DirectReceipt, RemoteJobReceipt, WriteHandle};
use crate::{
    connection::{self, SharedContext},
    model::{AppError, AppResult},
    remote_path, remote_write,
    ssh_transport::Cancellation,
};
use ssh2::{OpenFlags, OpenType, Sftp};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub const SCRIPTS: &[(&str, &[u8])] = &[
    ("supervisor.sh", include_bytes!("scripts/supervisor.sh")),
    ("sender.sh", include_bytes!("scripts/sender.sh")),
    ("receiver.sh", include_bytes!("scripts/receiver.sh")),
];

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
pub fn read_small(sftp: &Sftp, path: &str) -> AppResult<Option<Vec<u8>>> {
    if remote_write::metadata(sftp, path)?.is_none() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    sftp.open(Path::new(path))?
        .take(257)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 256 {
        return Err(AppError::new("unknown", "远端任务记录过大"));
    }
    Ok(Some(bytes))
}
pub fn state(
    sftp: &Sftp,
    root: &str,
    run: &str,
    attempt: Option<&str>,
) -> AppResult<Option<JobState>> {
    let path = attempt
        .map(|a| format!("{root}/job-{a}/state"))
        .unwrap_or_else(|| format!("{root}/guard.state"));
    read_small(sftp, &path)?
        .map(|b| JobState::parse(&b, run, attempt.unwrap_or(GUARD_ID)))
        .transpose()
}
pub fn deploy(
    state: &crate::State,
    item: &WriteHandle,
    sftp: &Sftp,
    source: bool,
) -> AppResult<String> {
    let home = sftp
        .realpath(Path::new("."))?
        .to_str()
        .ok_or_else(|| AppError::new("path", "远端主目录不是 UTF-8"))?
        .to_owned();
    let root = remote_path::join(
        &home,
        &format!(".ssh-workspace-task-{}", uuid::Uuid::new_v4()),
    )?;
    {
        let mut receipt = item.receipt.lock().unwrap();
        receipt.format_version = 2;
        let direct = receipt.direct.get_or_insert_with(DirectReceipt::default);
        let job = RemoteJobReceipt {
            root: root.clone(),
            launched: false,
        };
        if source {
            direct.source = job;
        } else {
            direct.target = job;
        }
    }
    state.transfers.persist(item)?;
    sftp.mkdir(Path::new(&root), 0o700)?;
    for (name, bytes) in SCRIPTS {
        write_new(sftp, &format!("{root}/{name}"), bytes)?;
    }
    write_new(
        sftp,
        &format!("{root}/run"),
        format!("{}\n", item.task.lock().unwrap().run_id).as_bytes(),
    )?;
    write_new(sftp, &format!("{root}/lease"), b"0\n")?;
    if source {
        write_new(sftp, &format!("{root}/probe"), b"")?;
    }
    Ok(root)
}

pub struct Lease {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
    pub error: Arc<Mutex<Option<AppError>>>,
}
impl Lease {
    pub fn start(
        context: &SharedContext,
        cancel: &Cancellation,
        root: String,
        run: String,
    ) -> AppResult<Self> {
        let connection = connection::connect_verified(context, cancel)?;
        connection.session().set_timeout(3000);
        let sftp = connection.session().sftp()?;
        let mut channel = connection.session().channel_session()?;
        let command = format!(
            "exec {}",
            shell_call(
                "sh",
                &[
                    &format!("{root}/supervisor.sh"),
                    "guard",
                    &root,
                    &run,
                    GUARD_ID
                ]
            )
        );
        channel.exec(&command)?;
        let (stop, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let worker_error = error.clone();
        let worker = std::thread::spawn(move || {
            let _connection = connection;
            let _guard_channel = channel;
            let mut seq: u64 = 0;
            loop {
                seq += 1;
                let result = (|| {
                    // The lease file is fixed and owned by this task. Truncate
                    // writes are not used for state proofs, only heartbeat.
                    let mut f = sftp.open_mode(
                        Path::new(&format!("{root}/lease")),
                        OpenFlags::WRITE | OpenFlags::TRUNCATE,
                        0o600,
                        OpenType::File,
                    )?;
                    f.write_all(format!("{seq}\n").as_bytes())?;
                    f.close()?;
                    Ok::<_, AppError>(())
                })();
                if let Err(e) = result {
                    *worker_error.lock().unwrap() = Some(e);
                    break;
                }
                if rx.recv_timeout(Duration::from_secs(2)) != Err(mpsc::RecvTimeoutError::Timeout) {
                    break;
                }
            }
        });
        Ok(Self {
            stop,
            worker: Some(worker),
            error,
        })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Constructed only after sealing admission and verifying both endpoints.
pub struct StopProof {
    _private: (),
}

fn seal_and_verify(
    context: &SharedContext,
    job: &RemoteJobReceipt,
    run: &str,
    attempt: Option<&str>,
) -> AppResult<()> {
    if job.root.is_empty() || !job.launched {
        return Ok(());
    }
    super::super::recovery::valid_task_directory(&job.root, ".ssh-workspace-task-")?;
    let cancel = Cancellation::default();
    let connection = connection::connect_verified(context, &cancel)?;
    connection.session().set_timeout(3000);
    let sftp = connection.session().sftp()?;
    remote_write::safe_ancestors(&sftp, &job.root)?;
    let command = format!(
        "exec {}",
        shell_call(
            "sh",
            &[
                &format!("{}/supervisor.sh", job.root),
                "seal",
                &job.root,
                run,
                GUARD_ID
            ]
        )
    );
    let (code, _, error) = capabilities::exec(&connection, &cancel, &command)?;
    if code != 0 {
        return Err(AppError::new(
            "unknown",
            format!("无法封闭远端任务入口：{}", error.trim()),
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let guard_admitted =
            remote_write::metadata(&sftp, &format!("{}/guard-admitted", job.root))?.is_some();
        let guard_done = !guard_admitted
            || state(&sftp, &job.root, run, None)?.is_some_and(|s| s.phase == "closed");
        let file_done = if let Some(id) = attempt {
            let admitted =
                remote_write::metadata(&sftp, &format!("{}/job-{id}/admitted", job.root))?
                    .is_some();
            !admitted || state(&sftp, &job.root, run, Some(id))?.is_some_and(|s| s.stopped())
        } else {
            true
        };
        if guard_done && file_done {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(AppError::new(
                "unknown",
                "远端进程尚未确认停止，请稍后核对状态",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn stop_and_verify(item: &WriteHandle) -> AppResult<StopProof> {
    let direct = item.receipt.lock().unwrap().direct.clone();
    let Some(direct) = direct else {
        return Ok(StopProof { _private: () });
    };
    if direct.stopped {
        return Ok(StopProof { _private: () });
    }
    let crate::transfers::Source::Remote { context, .. } = &item.source else {
        return Err(AppError::new("unknown", "直传来源记录缺失"));
    };
    let run = item.task.lock().unwrap().run_id.clone();
    let unfinished = if direct.attempt_finished {
        None
    } else {
        direct.attempt.as_deref()
    };
    // Always attempt both sides, even when one is unreachable.
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| seal_and_verify(context, &direct.source, &run, unfinished));
        let b = scope.spawn(|| seal_and_verify(&item.target, &direct.target, &run, unfinished));
        [a.join().unwrap(), b.join().unwrap()]
    });
    let errors: Vec<_> = results
        .into_iter()
        .enumerate()
        .filter_map(|(i, r)| {
            r.err()
                .map(|e| format!("{}：{}", if i == 0 { "A" } else { "B" }, e.message))
        })
        .collect();
    if !errors.is_empty() {
        return Err(AppError::new("unknown", errors.join("；")));
    }
    if let Some(d) = item.receipt.lock().unwrap().direct.as_mut() {
        d.stopped = true;
    }
    Ok(StopProof { _private: () })
}

pub fn remove_job(sftp: &Sftp, root: &str, id: &str) -> AppResult<()> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::new("path", "任务尝试标识无效"))?;
    let directory = format!("{root}/job-{id}");
    remote_write::safe_ancestors(sftp, &directory)?;
    if remote_write::metadata(sftp, &directory)?.is_none() {
        return Ok(());
    }
    for name in ["command.sh", "state", "state.next", "done"] {
        let path = format!("{directory}/{name}");
        if remote_write::metadata(sftp, &path)?.is_some() {
            sftp.unlink(Path::new(&path))?;
        }
    }
    let admitted = format!("{directory}/admitted");
    if remote_write::metadata(sftp, &admitted)?.is_some() {
        sftp.rmdir(Path::new(&admitted))?;
    }
    sftp.rmdir(Path::new(&directory))?;
    Ok(())
}

pub fn remove_root(sftp: &Sftp, job: &RemoteJobReceipt, attempt: Option<&str>) -> AppResult<()> {
    if job.root.is_empty() {
        return Ok(());
    }
    super::super::recovery::valid_task_directory(&job.root, ".ssh-workspace-task-")?;
    remote_write::safe_ancestors(sftp, &job.root)?;
    if remote_write::metadata(sftp, &job.root)?.is_none() {
        return Ok(());
    }
    if let Some(id) = attempt {
        remove_job(sftp, &job.root, id)?;
    }
    for name in [
        "supervisor.sh",
        "sender.sh",
        "receiver.sh",
        "run",
        "lease",
        "known_hosts",
        "probe",
        "closed",
        "stop",
        "guard.state",
        "guard.state.next",
    ] {
        let path = format!("{}/{name}", job.root);
        if remote_write::metadata(sftp, &path)?.is_some() {
            sftp.unlink(Path::new(&path))?;
        }
    }
    let admitted = format!("{}/guard-admitted", job.root);
    if remote_write::metadata(sftp, &admitted)?.is_some() {
        sftp.rmdir(Path::new(&admitted))?;
    }
    sftp.rmdir(Path::new(&job.root))?;
    Ok(())
}
