//! Bounded, cancellable reads on a connection owned exclusively by the caller.
use crate::{
    model::{AppError, AppResult},
    ssh,
};
use ssh2::Session;
use std::{
    io::Read,
    time::{Duration, Instant},
};

pub(crate) fn exec(
    session: &Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    exec_with_timeout(session, command, Duration::from_secs(6), check)
}

pub(crate) const DOCKER_QUERY_TIMEOUT: Duration = Duration::from_secs(12);

pub(crate) fn docker_query(
    session: &Session,
    command: &str,
    stage: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    observed_read(session, command, stage, DOCKER_QUERY_TIMEOUT, check)
}

pub(crate) fn observed_read(
    session: &Session,
    command: &str,
    stage: &str,
    timeout: Duration,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    let started = Instant::now();
    let result = exec_with_timeout(session, command, timeout, check);
    if result.is_err() || started.elapsed() >= Duration::from_secs(2) {
        let ctx = crate::diagnostics::event::Context::new();
        let mut details = result
            .as_ref()
            .err()
            .map(crate::diagnostics::event::error_details)
            .unwrap_or_else(|| serde_json::json!({}));
        details["phase"] = serde_json::json!(stage);
        details["durationMs"] = serde_json::json!(started.elapsed().as_millis() as u64);
        details["timeoutMs"] = serde_json::json!(timeout.as_millis() as u64);
        details["exitCode"] = serde_json::json!(result.as_ref().ok().map(|r| r.code));
        ctx.event(
            if result.is_err() { "WARN" } else { "INFO" },
            "docker",
            "query.end",
            details,
        );
    }
    result
}

pub(crate) fn exec_with_timeout(
    session: &Session,
    command: &str,
    timeout: Duration,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    session.set_blocking(false);
    let result = (|| {
        let deadline = Instant::now() + timeout;
        let guard = || {
            check()?;
            if Instant::now() >= deadline {
                return Err(AppError::new("timeout", "远端资源查询超时"));
            }
            Ok(())
        };
        let retry = |call: &mut dyn FnMut() -> Result<(), ssh2::Error>| -> AppResult<()> {
            loop {
                guard()?;
                match call() {
                    Ok(()) => return Ok(()),
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
        let mut channel = loop {
            guard()?;
            match session.channel_session() {
                Ok(channel) => break channel,
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e.into()),
            }
        };
        retry(&mut || channel.exec(command))?;
        let mut output = [Vec::new(), Vec::new()];
        let mut buffer = [0; 8192];
        loop {
            guard()?;
            let mut received = false;
            for (i, bytes) in output.iter_mut().enumerate() {
                match channel.stream(i as i32).read(&mut buffer) {
                    Ok(n) => {
                        received |= n > 0;
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
                if bytes.len() > 2 * 1024 * 1024 {
                    return Err(AppError::new("limit", "远端资源查询输出过大"));
                }
            }
            if channel.eof() && !received {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        retry(&mut || channel.wait_close())?;
        Ok(ssh::ExecResult {
            code: channel.exit_status()?,
            stdout: String::from_utf8(std::mem::take(&mut output[0]))
                .map_err(|_| AppError::new("format", "远端资源查询编码无效"))?,
            stderr: String::from_utf8_lossy(&output[1]).into_owned(),
        })
    })();
    session.set_blocking(true);
    result
}
