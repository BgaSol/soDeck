use crate::{
    model::{AppError, AppResult},
    ssh,
};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// `attempted` is set before the exec request; a lost acknowledgement is uncertain.
/// Callers must never replay a command merely because this function returns Err.
pub fn execute_progress(
    session: &ssh2::Session,
    command: &str,
    deadline: Duration,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
    progress: &impl Fn(&str),
) -> AppResult<ssh::ExecResult> {
    session.set_timeout(0);
    session.set_blocking(false);
    let result = (|| {
        let until = Instant::now() + deadline;
        let guard = || {
            check()?;
            if Instant::now() >= until {
                Err(AppError::new(
                    "timeout",
                    "等待 Docker 执行结果超时，请核对当前状态",
                ))
            } else {
                Ok(())
            }
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
                Ok(c) => break c,
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => return Err(e.into()),
            }
        };
        guard()?;
        attempted.store(true, Ordering::SeqCst);
        // libssh2 returns zero when a server closes a channel without sending an
        // exit-status. Require our own unpredictable completion receipt as well.
        let marker = format!("__SODECK_DOCKER_RESULT_{}__", uuid::Uuid::new_v4().simple());
        let wrapped = format!("{command}; sodeck_docker_status=$?; printf '\\n{marker}%s\\n' \"$sodeck_docker_status\"; exit \"$sodeck_docker_status\"");
        retry(&mut || channel.exec(&wrapped))?;
        let mut output = [Vec::new(), Vec::new()];
        let mut buffer = [0; 8192];
        let mut last_progress = Instant::now();
        loop {
            guard()?;
            let mut received = false;
            for (index, bytes) in output.iter_mut().enumerate() {
                match channel.stream(index as i32).read(&mut buffer) {
                    Ok(n) => {
                        received |= n > 0;
                        bytes.extend_from_slice(&buffer[..n]);
                        if bytes.len() > 32768 {
                            bytes.drain(..bytes.len() - 32768);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
            }
            if received && last_progress.elapsed() >= Duration::from_millis(600) {
                let text = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output[0]),
                    String::from_utf8_lossy(&output[1])
                );
                let lines: Vec<_> = text
                    .lines()
                    .filter(|line| !line.contains(&marker))
                    .rev()
                    .take(8)
                    .collect();
                progress(&super::inspect::clean(
                    &lines.into_iter().rev().collect::<Vec<_>>().join("\n"),
                    2000,
                ));
                last_progress = Instant::now();
            }
            if channel.eof() && !received {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        retry(&mut || channel.wait_close())?;
        let stdout = String::from_utf8_lossy(&output[0]).into_owned();
        let code = completion_code(&stdout, &marker)?;
        Ok(ssh::ExecResult {
            code,
            stdout: stdout
                .lines()
                .filter(|line| !line.starts_with(&marker))
                .collect::<Vec<_>>()
                .join("\n"),
            stderr: String::from_utf8_lossy(&output[1]).into_owned(),
        })
    })();
    session.set_blocking(true);
    session.set_timeout(12_000);
    result
}

fn completion_code(output: &str, marker: &str) -> AppResult<i32> {
    output
        .lines()
        .last()
        .and_then(|line| line.strip_prefix(marker))
        .and_then(|code| code.parse::<i32>().ok())
        .filter(|code| (0..=255).contains(code))
        .ok_or_else(|| AppError::new("unknown", "未收到远端命令完成回执"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_eof_without_receipt_is_never_success() {
        assert!(completion_code("container-id\n", "receipt").is_err());
        assert!(completion_code("receipt0\nmore output", "receipt").is_err());
        assert_eq!(completion_code("output\nreceipt0\n", "receipt").unwrap(), 0);
        assert_eq!(completion_code("receipt127\n", "receipt").unwrap(), 127);
    }
}
