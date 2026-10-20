use super::protocol::{classify, ContainerObservation};
use super::*;
pub(super) fn command(
    target: &crate::docker::ContainerTarget,
    following: bool,
    since: Option<&str>,
) -> AppResult<String> {
    validate_id(&target.id)?;
    Ok(format!("env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} logs --tail all --timestamps {} {} {}",ssh::quote(&target.endpoint),if following{"--follow"}else{""},
  since.map(|s|format!("--since {}",ssh::quote(s))).unwrap_or_default(),ssh::quote(&target.id)))
}
fn wait_cancel(run: &Run, duration: Duration) {
    let until = Instant::now() + duration;
    let mut wake = run.wake.0.lock().unwrap();
    while !run.cancel.is_cancelled() && Instant::now() < until && !*wake {
        wake = run
            .wake
            .1
            .wait_timeout(wake, Duration::from_millis(50))
            .unwrap()
            .0;
    }
    *wake = false;
}
pub(super) fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs((1u64 << attempt.min(5)).min(30))
}
fn event(
    root: &std::path::Path,
    source: &Source,
    output: &Channel<LogEvent>,
    kind: &str,
    detail: &str,
) {
    // A connection may fail before Store::open creates source.json. Do not
    // create a half-initialized archive that would break the archive library.
    if !log_store::source_dir(root, &source.source_id)
        .is_ok_and(|dir| dir.join("source.json").exists())
    {
        return;
    }
    if let Err(e) = lifecycle::record(root, &source.source_id, kind, detail, None) {
        let _ = output.send(LogEvent::Notice {
            error: format!("运行事件保存失败：{}", e.message),
        });
    }
    let _ = output.send(LogEvent::Lifecycle {});
}
pub(super) fn collect_logs(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    source: &Source,
    archive_root: &std::path::Path,
    run: &Run,
    output: &Channel<LogEvent>,
) -> AppResult<()> {
    let mut attempt = 0;

    while !run.cancel.is_cancelled() {
        run.emit(output, "connecting", None, None, None, None);
        let history = !crate::log_ingest::history_complete(archive_root, &source.source_id)?;
        match read_logs(spec, target, source, archive_root, history, run, output) {
            Ok(following) => {
                if run.cancel.is_cancelled() {
                    break;
                }
                attempt = 0;
                if history {
                    continue;
                }
                run.emit(output, "waitingContainer", None, None, None, None);
                if following {
                    event(
                        archive_root,
                        source,
                        output,
                        "disconnected",
                        "日志流已结束，正在检查容器",
                    );
                }
                wait_cancel(run, Duration::from_secs(5));
            }
            Err(e) => {
                let failure = classify(&e);
                if !failure.retry(attempt) {
                    run.emit(
                        output,
                        "failed",
                        Some(failure.reason()),
                        None,
                        Some(e.message.clone()),
                        None,
                    );
                    event(archive_root, source, output, "failed", &e.message);
                    return Err(e);
                }
                {
                    event(archive_root, source, output, "disconnected", "日志连接中断");
                }
                let delay = retry_delay(attempt);
                attempt = attempt.saturating_add(1);
                run.emit(
                    output,
                    "reconnecting",
                    Some(failure.reason()),
                    Some(log_store::now() + delay.as_millis() as i64),
                    Some(e.message),
                    None,
                );
                wait_cancel(run, delay);
            }
        }
    }
    Ok(())
}
pub(super) fn read_logs(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    source: &Source,
    archive_root: &std::path::Path,
    history: bool,
    run: &Run,
    output: &Channel<LogEvent>,
) -> AppResult<bool> {
    let connection =
        connection::connect_verified(spec, &run.cancel, crate::traffic::TrafficPurpose::Logs)?;
    let session = connection.session();
    if run.cancel.is_cancelled() {
        return Ok(false);
    }
    let inspect=ssh::exec(session,&format!("env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {} inspect --type container --format '{{{{json .}}}}' {}",ssh::quote(&target.endpoint),ssh::quote(&target.id)))?;
    if inspect.code != 0 {
        return Err(AppError::new("docker", inspect.stderr.trim()));
    }
    let metadata: serde_json::Value =
        serde_json::from_str(inspect.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
    let following = metadata["State"]["Running"]
        .as_bool()
        .ok_or_else(|| AppError::new("docker", "无法确认容器状态"))?;
    let tty = metadata["Config"]["Tty"].as_bool().unwrap_or(false);
    let observation = ContainerObservation::parse(&metadata);
    *run.observation.lock().unwrap() = Some(observation.clone());
    if run.cancel.is_cancelled() {
        return Ok(false);
    }
    let mut saved_source = source.clone();
    saved_source.tty = tty;
    let mut store = log_store::Store::open(
        archive_root,
        &saved_source,
        &uuid::Uuid::new_v4().to_string(),
        history,
    )
    .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
    if let Err(e) = lifecycle::observe(
        archive_root,
        &source.source_id,
        &observation,
        metadata["Config"]["Labels"]
            .as_object()
            .map(|_| &metadata["Config"]["Labels"]),
    ) {
        let _ = output.send(LogEvent::Notice {
            error: format!("运行事件保存失败：{}", e.message),
        });
    }
    let _ = output.send(LogEvent::Lifecycle {});
    let _probe = if following && !history {
        Some(probe(spec, target, source, archive_root, run, output))
    } else {
        None
    };
    let mut opened_channel = None;
    let mut chunks = Vec::new();
    let mut count = 0;
    let mut stderr_tail = Vec::new();
    let mut diagnostics = super::diagnostics::Diagnostics::default();
    let mut buffer = [0u8; 16384];
    let mut flush = Instant::now();
    let mut keepalive = Instant::now();
    let mut write_failed = false;
    let result = (|| {
        // Once the archive run exists, setup failures and early cancellation
        // must take the same flush/finish path as a running log stream.
        run.cancel.check()?;
        let mut channel = session.channel_session()?;
        channel.exec(&command(
            target,
            following && !history,
            store.since.as_deref(),
        )?)?;
        session.set_blocking(false);
        opened_channel = Some(channel);
        let channel = opened_channel.as_mut().unwrap();
        run.emit(
            output,
            if history {
                "syncingHistory"
            } else if following {
                "streaming"
            } else {
                "waitingContainer"
            },
            None,
            None,
            None,
            Some(tty),
        );
        if following && !history {
            event(archive_root, source, output, "connected", "日志连接已建立");
        }
        loop {
            if run.cancel.is_cancelled() {
                break;
            }
            let mut received = false;
            let mut read_error = None;
            for stream in 0..2 {
                match channel.stream(stream as i32).read(&mut buffer) {
                    Ok(n) if n > 0 => {
                        received = true;
                        let bytes = if stream == 1 {
                            diagnostics.push(&buffer[..n])
                        } else {
                            buffer[..n].to_vec()
                        };
                        count += bytes.len();
                        if !bytes.is_empty() {
                            chunks.push((stream, bytes));
                        }
                        if stream == 1 {
                            stderr_tail.extend_from_slice(&buffer[..n]);
                            if stderr_tail.len() > 8192 {
                                stderr_tail.drain(..stderr_tail.len() - 8192);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => {
                        read_error = Some(e);
                        break;
                    }
                }
            }
            let eof = channel.eof() && !received;
            if eof || read_error.is_some() {
                let tail = diagnostics.finish();
                if !tail.is_empty() {
                    chunks.push((1, tail));
                }
            }
            if eof
                || read_error.is_some()
                || flush.elapsed() >= Duration::from_millis(100)
                || count >= 256 * 1024
            {
                if !chunks.is_empty() {
                    let last_seq = match store.append(&chunks) {
                        Ok(seq) => seq,
                        Err(e) => {
                            write_failed = true;
                            return Err(AppError::new(
                                "logs",
                                format!("日志保存失败：{}", e.message),
                            ));
                        }
                    };
                    chunks.clear();
                    count = 0;
                    let _ = output.send(LogEvent::Saved {
                        last_seq,
                        changed_from: store.changed_from,
                    });
                }
                flush = Instant::now();
            }
            if let Some(e) = read_error {
                return Err(AppError::new("network", e));
            }
            if eof {
                let started = Instant::now();
                loop {
                    match channel.wait_close() {
                        Ok(()) => break,
                        Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                            if run.cancel.is_cancelled() {
                                break;
                            }
                            if started.elapsed() > Duration::from_secs(5) {
                                return Err(AppError::new(
                                    "network",
                                    "日志连接已结束，未收到退出状态",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                if !run.cancel.is_cancelled() && channel.exit_status()? != 0 {
                    return Err(AppError::new(
                        "docker",
                        String::from_utf8_lossy(&stderr_tail).trim(),
                    ));
                }
                break;
            }
            if keepalive.elapsed() > Duration::from_secs(20) {
                if let Err(e) = session.keepalive_send() {
                    if e.code() != ssh2::ErrorCode::Session(-37) {
                        return Err(e.into());
                    }
                }
                keepalive = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    // Flush even after cancellation, EOF, transport failure or a closed frontend.
    if !write_failed {
        if result.is_ok() {
            let original = diagnostics.success();
            if !original.is_empty() {
                chunks.push((1, original));
            }
        }
        let tail = diagnostics.finish();
        if !tail.is_empty() {
            chunks.push((1, tail));
        }
        if !chunks.is_empty() {
            let seq = store
                .append(&chunks)
                .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
            let _ = output.send(LogEvent::Saved {
                last_seq: seq,
                changed_from: store.changed_from,
            });
        }
        store
            .finish(if run.cancel.is_cancelled() {
                "用户停止采集"
            } else if result.is_err() {
                "连接中断；下次补采的覆盖无法确认"
            } else {
                "日志读取已结束"
            })
            .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
        if history && result.is_ok() && !run.cancel.is_cancelled() {
            store
                .mark_history_complete()
                .map_err(|e| AppError::new("logs", format!("日志保存失败：{}", e.message)))?;
        }
        let _ = output.send(LogEvent::Saved {
            last_seq: log_store::stats(archive_root, &source.source_id)?.last_seq,
            changed_from: store.changed_from,
        });
    }
    if !diagnostics.details.is_empty() {
        event(
            archive_root,
            source,
            output,
            "diagnostic",
            &String::from_utf8_lossy(&diagnostics.details),
        );
    }
    if let Some(channel) = &mut opened_channel {
        let _ = channel.close();
    }
    let _ = session.disconnect(None, "Log collection ended", None);
    if run.cancel.is_cancelled() {
        Ok(following)
    } else {
        result.map(|()| following)
    }
}

struct Probe {
    cancel: Cancellation,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn probe(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    source: &Source,
    root: &std::path::Path,
    run: &Run,
    output: &Channel<LogEvent>,
) -> Probe {
    let cancel = Cancellation::default();
    let stop = cancel.clone();
    let spec = spec.clone();
    let target = target.clone();
    let source = source.clone();
    let root = root.to_path_buf();
    let run = run.clone();
    let output = output.clone();
    let worker = std::thread::spawn(move || {
        let mut failures = 0;
        while !stop.is_cancelled() && !run.cancel.is_cancelled() {
            let until = Instant::now()
                + Duration::from_secs(if failures == 0 {
                    5
                } else {
                    (5u64 << failures.min(3)).min(30)
                });
            while Instant::now() < until && !stop.is_cancelled() && !run.cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(50));
            }
            if stop.is_cancelled() || run.cancel.is_cancelled() {
                break;
            }
            let result = (|| -> AppResult<()> {
                let connection = connection::connect_verified(
                    &spec,
                    &stop,
                    crate::traffic::TrafficPurpose::Logs,
                )?;
                let session = connection.session();
                let prefix = format!(
                    "env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {}",
                    ssh::quote(&target.endpoint)
                );
                if lifecycle::metadata(&root, &source.source_id, "daemonId")?.is_none() {
                    let r = ssh::exec(
                        session,
                        &format!("{prefix} info --format '{{{{json .ID}}}}'"),
                    )?;
                    if r.code == 0 {
                        if let Ok(id) = serde_json::from_str::<String>(r.stdout.trim()) {
                            lifecycle::set_metadata(&root, &source.source_id, "daemonId", &id)?;
                        }
                    }
                }
                let r = ssh::exec(
                    session,
                    &format!(
                        "{prefix} inspect --type container --format '{{{{json .}}}}' {}",
                        ssh::quote(&target.id)
                    ),
                )?;
                if r.code != 0 {
                    return Err(AppError::new("docker", r.stderr));
                }
                let v: serde_json::Value = serde_json::from_str(r.stdout.trim())
                    .map_err(|e| AppError::new("docker", e))?;
                let o = ContainerObservation::parse(&v);
                let changed = lifecycle::observe(&root, &source.source_id, &o, None)?;
                let phase = if matches!(o.state.as_str(), "running" | "paused") {
                    "streaming"
                } else {
                    "waitingContainer"
                };
                *run.observation.lock().unwrap() = Some(o);
                run.emit(&output, phase, None, None, None, None);
                if changed {
                    let _ = output.send(LogEvent::Lifecycle {});
                }
                Ok(())
            })();
            if result.is_err() {
                failures += 1;
                if let Some(o) = run.observation.lock().unwrap().as_mut() {
                    o.state = "unknown".into();
                }
                run.emit(
                    &output,
                    "streaming",
                    Some("containerUnknown"),
                    None,
                    None,
                    None,
                );
            } else {
                failures = 0;
            }
        }
    });
    Probe {
        cancel,
        worker: Some(worker),
    }
}
