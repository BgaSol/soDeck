use super::super::model::DirectEndpoint;
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

pub fn ssh_arguments(
    endpoint: &DirectEndpoint,
    known_hosts: &str,
    optional: &[String],
) -> AppResult<Vec<String>> {
    crate::remote_path::validate_host(&endpoint.host)?;
    if endpoint.username.contains(['\0', '\n', '\r']) || endpoint.username.is_empty() {
        return Err(AppError::new("validation", "直连账号无效"));
    }
    let quoted_path = known_hosts
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut options = vec![
        "BatchMode=yes".into(),
        "StrictHostKeyChecking=yes".into(),
        "VerifyHostKeyDNS=no".into(),
        format!("UserKnownHostsFile=\"{quoted_path}\""),
        "GlobalKnownHostsFile=/dev/null".into(),
        "HostKeyAlias=ssh-workspace-target".into(),
        "ProxyCommand=none".into(),
        "ControlMaster=no".into(),
        "ControlPath=none".into(),
        "ClearAllForwardings=yes".into(),
        "ForwardAgent=no".into(),
        "ForwardX11=no".into(),
        "PermitLocalCommand=no".into(),
        "RequestTTY=no".into(),
        "CanonicalizeHostname=no".into(),
        format!("Hostname={}", endpoint.host),
        "ConnectTimeout=12".into(),
        "ConnectionAttempts=1".into(),
        "ServerAliveInterval=5".into(),
        "ServerAliveCountMax=3".into(),
    ];
    let algorithm = endpoint
        .public_key
        .split_whitespace()
        .next()
        .ok_or_else(|| AppError::new("hostKey", "缺少目标主机公钥"))?;
    options.push(format!(
        "HostKeyAlgorithms={}",
        if algorithm == "ssh-rsa" {
            "rsa-sha2-512,rsa-sha2-256"
        } else {
            algorithm
        }
    ));
    options.extend(optional.iter().cloned());
    let mut args = vec![
        "-T".into(),
        "-p".into(),
        endpoint.port.to_string(),
        "-l".into(),
        endpoint.username.clone(),
    ];
    for option in options {
        args.extend(["-o".into(), option]);
    }
    args.push(endpoint.host.clone());
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_arguments_are_literal_including_trailing_newlines() {
        let value = "space ' $(false)\\\n";
        let command = shell_call("printf", &["%s", value]);
        let output = std::process::Command::new("sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert_eq!(output.stdout, value.as_bytes());
    }
}
