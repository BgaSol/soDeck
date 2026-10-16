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
    session.set_blocking(false);
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(6);
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
