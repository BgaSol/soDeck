use crate::{
    model::{AppError, AppResult},
    ssh_transport::{Cancellation, SshConnection},
};
use std::{
    io::Read,
    time::{Duration, Instant},
};
pub fn exec(
    connection: &SshConnection,
    cancel: &Cancellation,
    command: &str,
) -> AppResult<(i32, String, String)> {
    exec_checked(connection, cancel, command, Duration::from_secs(20), || {
        cancel.check()
    })
}
pub fn exec_checked(
    connection: &SshConnection,
    cancel: &Cancellation,
    command: &str,
    timeout: Duration,
    mut check: impl FnMut() -> AppResult<()>,
) -> AppResult<(i32, String, String)> {
    let session = connection.session();
    session.set_blocking(false);
    let result = (|| {
        let mut channel = crate::ssh_transport::retry(cancel, Duration::from_secs(15), || {
            session.channel_session()
        })?;
        crate::ssh_transport::retry(cancel, Duration::from_secs(15), || channel.exec(command))?;
        let deadline = Instant::now() + timeout;
        let mut output = [vec![], vec![]];
        let mut buffer = [0u8; 8192];
        loop {
            check()?;
            for (stream, bytes) in output.iter_mut().enumerate() {
                match channel.stream(stream as i32).read(&mut buffer) {
                    Ok(n) => bytes.extend_from_slice(
                        &buffer[..n.min(65536usize.saturating_sub(bytes.len()))],
                    ),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
            }
            if channel.eof() {
                break;
            }
            if Instant::now() > deadline {
                return Err(AppError::new("timeout", "检查远端工具超时"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        crate::ssh_transport::retry(cancel, Duration::from_secs(3), || channel.wait_close())?;
        Ok((
            channel.exit_status()?,
            String::from_utf8_lossy(&output[0]).into(),
            String::from_utf8_lossy(&output[1]).into(),
        ))
    })();
    session.set_blocking(true);
    result
}
pub fn check(
    connection: &SshConnection,
    cancel: &Cancellation,
    source: bool,
    password: bool,
) -> AppResult<()> {
    let tools = if source && password {
        "rsync ssh sshpass sh mv cat awk ps tr"
    } else if source {
        "rsync ssh sh mv cat awk ps tr"
    } else {
        "rsync sh mv cat"
    };
    let command=format!("test \"$(uname -s)\" = Linux || exit 69; for tool in {tools}; do command -v \"$tool\" >/dev/null || {{ printf '缺少工具：%s\\n' \"$tool\" >&2; exit 69; }}; done; rsync --version");
    let (code, _, err) = exec(connection, cancel, &command)?;
    if code != 0 {
        return Err(AppError::new(
            "environment",
            format!(
                "{}环境检查失败（首版要求 Linux）：{}",
                if source { "A" } else { "B" },
                err.trim()
            ),
        ));
    }
    Ok(())
}
