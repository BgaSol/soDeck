use super::super::model::{DirectAuth, DirectEndpoint};
use crate::{
    model::{AppError, AppResult},
    ssh,
};
pub fn shell_call(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .map(ssh::quote)
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn ssh_command(
    endpoint: &DirectEndpoint,
    known_hosts: &str,
    auth: &DirectAuth,
) -> AppResult<String> {
    crate::remote_path::validate_host(&endpoint.host)?;
    if endpoint.username.is_empty() || endpoint.username.contains(['\0', '\r', '\n']) {
        return Err(AppError::new("validation", "直连用户名无效"));
    }
    let known_hosts = known_hosts
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut args = vec![
        "-F".into(),
        "/dev/null".into(),
        "-T".into(),
        "-p".into(),
        endpoint.port.to_string(),
        "-l".into(),
        endpoint.username.clone(),
    ];
    let mut options = vec![
        "StrictHostKeyChecking=yes".into(),
        format!("UserKnownHostsFile=\"{known_hosts}\""),
        "GlobalKnownHostsFile=/dev/null".into(),
        "HostKeyAlias=ssh-workspace-target".into(),
        "ConnectTimeout=15".into(),
        "ConnectionAttempts=1".into(),
        "ServerAliveInterval=5".into(),
        "ServerAliveCountMax=3".into(),
        "ForwardAgent=no".into(),
        "ClearAllForwardings=yes".into(),
        "NumberOfPasswordPrompts=1".into(),
    ];
    match auth {
        DirectAuth::Password(password) => {
            if password.is_empty() || password.contains(['\0', '\r', '\n']) {
                return Err(AppError::new(
                    "validation",
                    "密码为空或包含当前认证通道不支持的换行符",
                ));
            }
            options.extend(
                [
                    "BatchMode=no",
                    "PreferredAuthentications=password",
                    "PubkeyAuthentication=no",
                ]
                .map(str::to_owned),
            );
        }
        DirectAuth::Key(path) => {
            crate::remote_path::absolute(path)?;
            args.extend(["-i".into(), path.replace('%', "%%")]);
            options.extend(
                [
                    "BatchMode=yes",
                    "PreferredAuthentications=publickey",
                    "IdentitiesOnly=yes",
                    "IdentityAgent=none",
                ]
                .map(str::to_owned),
            );
        }
    }
    for value in options {
        args.extend(["-o".into(), value]);
    }
    Ok(shell_call(
        "ssh",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    ))
}
