use crate::{
    model::{AppError, AppResult},
    ssh::ExecResult,
};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// Exclusive SSH session only. A lost exec acknowledgement may already have run.
pub fn run(
    session: &ssh2::Session,
    command: &str,
    seconds: u64,
    limit: usize,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ExecResult> {
    session.set_timeout(0);
    session.set_blocking(false);
    let result = (|| {
        let until = Instant::now() + Duration::from_secs(seconds);
        let guard = || {
            check()?;
            if Instant::now() >= until {
                Err(AppError::new("timeout", "NetBird 远端命令超时"))
            } else {
                Ok(())
            }
        };
        let retry = |f: &mut dyn FnMut() -> Result<(), ssh2::Error>| -> AppResult<()> {
            loop {
                guard()?;
                match f() {
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
        let marker = format!("__SODECK_NB_{}__", uuid::Uuid::new_v4().simple());
        let wrapped = format!("{command}; sodeck_nb_code=$?; printf '\\n{marker}%s\\n' \"$sodeck_nb_code\"; exit \"$sodeck_nb_code\"");
        let result = (|| {
            attempted.store(true, Ordering::SeqCst);
            retry(&mut || channel.exec(&wrapped))?;
            let mut data = [Vec::new(), Vec::new()];
            let mut buf = [0u8; 8192];
            loop {
                guard()?;
                let mut received = false;
                for (i, bytes) in data.iter_mut().enumerate() {
                    match channel.stream(i as i32).read(&mut buf) {
                        Ok(n) => {
                            received |= n > 0;
                            bytes.extend_from_slice(&buf[..n]);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                        Err(e) => return Err(e.into()),
                    }
                    if bytes.len() > if i == 0 { limit } else { 256 * 1024 } {
                        return Err(AppError::new(
                            "limit",
                            "NetBird 输出超出上限，未读取完整结果",
                        ));
                    }
                }
                if channel.eof() && !received {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            retry(&mut || channel.wait_close())?;
            let output = String::from_utf8_lossy(&data[0]);
            let (stdout, code) = receipt(&output, &marker)?;
            Ok(ExecResult {
                code,
                stdout,
                stderr: String::from_utf8_lossy(&data[1]).into_owned(),
            })
        })();
        if result.is_err() {
            let _ = channel.close();
        }
        result
    })();
    session.set_blocking(true);
    session.set_timeout(12_000);
    result
}
pub fn receipt(output: &str, marker: &str) -> AppResult<(String, i32)> {
    let last = output.lines().last().unwrap_or("");
    let code = last
        .strip_prefix(marker)
        .filter(|v| !v.is_empty() && v.len() <= 3 && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|n| (0..=255).contains(n))
        .ok_or_else(|| AppError::new("uncertain", "未收到远端完成回执"))?;
    Ok((
        output[..output.rfind(last).unwrap_or(0)]
            .trim_end()
            .to_string(),
        code,
    ))
}
pub fn read(
    session: &ssh2::Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ExecResult> {
    run(
        session,
        command,
        8,
        8 * 1024 * 1024,
        &AtomicBool::new(false),
        check,
    )
}
