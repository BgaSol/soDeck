use crate::{
    model::{AppError, AppResult, ServerConfig},
    ssh_transport::{retry, Cancellation},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Credentials {
    pub password: String,
    pub passphrase: String,
}

pub fn fingerprint(session: &Session) -> AppResult<String> {
    let key = session
        .host_key()
        .ok_or_else(|| AppError::new("ssh", "服务器没有提供主机密钥"))?
        .0;
    Ok(format!(
        "SHA256:{}",
        STANDARD.encode(Sha256::digest(key)).trim_end_matches('=')
    ))
}
pub fn authenticate(
    session: &Session,
    server: &ServerConfig,
    credentials: &Credentials,
    cancel: &Cancellation,
) -> AppResult<()> {
    session.set_blocking(false);
    let auth = match server.auth_type.as_str() {
        "password" => retry(cancel, Duration::from_secs(12), || {
            session.userauth_password(&server.username, &credentials.password)
        }),
        "key" => retry(cancel, Duration::from_secs(12), || {
            session.userauth_pubkey_file(
                &server.username,
                None,
                Path::new(&server.private_key),
                (!credentials.passphrase.is_empty()).then_some(credentials.passphrase.as_str()),
            )
        }),
        "agent" => authenticate_agent(session, &server.username, cancel),
        _ => Err(AppError::new("authentication", "不支持的认证方式")),
    };
    auth.map_err(|e| {
        if e.kind == "ssh" {
            // Authentication errors must never enter a network retry loop.
            AppError::new(
                "authentication",
                format!("SSH 认证失败，请检查用户名及认证配置：{}", e.message),
            )
        } else {
            e
        }
    })?;
    if !session.authenticated() {
        return Err(AppError::new("authentication", "SSH 认证失败"));
    }
    session.set_keepalive(true, 30);
    Ok(())
}

fn authenticate_agent(session: &Session, username: &str, cancel: &Cancellation) -> AppResult<()> {
    // userauth_agent recreates and frees its Agent on each call. libssh2 keeps
    // the signing callback's Agent pointer across EAGAIN, so hold one Agent and
    // its identities until the complete authentication attempt has finished.
    let mut agent = session.agent()?;
    retry(cancel, Duration::from_secs(12), || agent.connect())?;
    retry(cancel, Duration::from_secs(12), || agent.list_identities())?;
    let identities = agent.identities()?;
    let mut last_error = AppError::new("authentication", "SSH Agent 中没有可用密钥");
    for identity in &identities {
        match retry(cancel, Duration::from_secs(12), || {
            agent.userauth(username, identity)
        }) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind == "authentication" => last_error = error,
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

pub struct ExecResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub fn exec(session: &Session, command: &str) -> AppResult<ExecResult> {
    session.set_blocking(false);
    let start = Instant::now();
    let mut channel = loop {
        match session.channel_session() {
            Ok(channel) => break channel,
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                if start.elapsed() > Duration::from_secs(12) {
                    return Err(AppError::new("timeout", "Docker 查询超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    };
    loop {
        match channel.exec(command) {
            Ok(()) => break,
            Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                if start.elapsed() > Duration::from_secs(12) {
                    return Err(AppError::new("timeout", "Docker 查询超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let mut received = false;
        for (stream, result) in [(0, &mut stdout), (1, &mut stderr)] {
            match channel.stream(stream).read(&mut buf) {
                Ok(n) => {
                    received |= n > 0;
                    result.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
        if stdout.len() + stderr.len() > 8 * 1024 * 1024 {
            return Err(AppError::new("limit", "查询输出超过 8 MB"));
        }
        if channel.eof() && !received {
            break;
        }
        if start.elapsed() > Duration::from_secs(12) {
            let _ = channel.close();
            return Err(AppError::new("timeout", "查询超时，未确认服务状态"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    session.set_blocking(true);
    let _ = channel.wait_close();
    Ok(ExecResult {
        code: channel.exit_status()?,
        stdout: String::from_utf8_lossy(&stdout).into(),
        stderr: String::from_utf8_lossy(&stderr).into(),
    })
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
