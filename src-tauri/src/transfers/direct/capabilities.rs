use crate::{
    model::{AppError, AppResult},
    ssh,
    ssh_transport::{self, Cancellation, SshConnection},
};
use std::{
    io::Read,
    time::{Duration, Instant},
};

/// Used only for bounded capability/control commands, never for a file stream.
pub fn exec(
    connection: &SshConnection,
    cancel: &Cancellation,
    command: &str,
) -> AppResult<(i32, String, String)> {
    exec_checked(connection, cancel, command, None)
}
fn exec_checked(
    connection: &SshConnection,
    cancel: &Cancellation,
    command: &str,
    item: Option<&super::super::model::WriteHandle>,
) -> AppResult<(i32, String, String)> {
    let session = connection.session();
    session.set_blocking(false);
    let result = (|| {
        let mut channel = ssh_transport::retry(cancel, Duration::from_secs(12), || {
            session.channel_session()
        })?;
        ssh_transport::retry(cancel, Duration::from_secs(12), || channel.exec(command))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut output = [Vec::new(), Vec::new()];
        let mut buffer = [0u8; 8192];
        loop {
            cancel.check()?;
            if let Some(item) = item {
                item.check()?;
            }
            for (stream, bytes) in output.iter_mut().enumerate() {
                match channel.stream(stream as i32).read(&mut buffer) {
                    Ok(n) => bytes.extend_from_slice(
                        &buffer[..n.min(65536usize.saturating_sub(bytes.len()))],
                    ),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            }
            if channel.eof() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(AppError::new("timeout", "直传环境检查或控制命令超时"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        ssh_transport::retry(cancel, Duration::from_secs(3), || channel.wait_close())?;
        Ok((
            channel.exit_status()?,
            String::from_utf8_lossy(&output[0]).into_owned(),
            String::from_utf8_lossy(&output[1]).into_owned(),
        ))
    })();
    session.set_blocking(true);
    result
}

pub fn check(
    connection: &SshConnection,
    item: &super::super::model::WriteHandle,
    source: bool,
) -> AppResult<Vec<String>> {
    let tools = if source {
        "sh cat sleep mkdir rmdir mv ps ssh"
    } else {
        "sh cat sleep mkdir rmdir mv ps"
    };
    let script = format!("set -f; for tool in {tools}; do command -v \"$tool\" >/dev/null 2>&1 || {{ printf '缺少工具：%s\\n' \"$tool\" >&2; exit 69; }}; done; group=$(ps -o pgid= -p $$) || exit 69; set -- $group; test \"$#\" -eq 1 && test \"$1\" = \"$$\" || {{ printf '%s\\n' 'SSH 命令不具备独立进程组' >&2; exit 69; }}");
    item.check()?;
    let (code, _, error) = exec_checked(
        connection,
        &item.cancel,
        &format!("exec sh -c {}", ssh::quote(&script)),
        Some(item),
    )?;
    if code != 0 {
        return Err(AppError::new(
            "environment",
            format!(
                "{}环境检查失败：{}",
                if source {
                    "源服务器 A"
                } else {
                    "目标服务器 B"
                },
                error.trim()
            ),
        ));
    }
    if !source {
        return Ok(Vec::new());
    }
    // -V parses options without connecting. Optional directives are sent only
    // when the client actually supports them, rather than guessing by version.
    let mut optional = Vec::new();
    for value in [
        "ProxyJump=none",
        "RemoteCommand=none",
        "KnownHostsCommand=none",
        "UpdateHostKeys=no",
        "ForkAfterAuthentication=no",
        "StdinNull=no",
        "SessionType=default",
    ] {
        item.check()?;
        let (code, _, _) = exec_checked(
            connection,
            &item.cancel,
            &format!("exec ssh -F /dev/null -o {} -V", ssh::quote(value)),
            Some(item),
        )?;
        if code == 0 {
            optional.push(value.into());
        }
    }
    Ok(optional)
}
