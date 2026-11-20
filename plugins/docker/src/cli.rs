//! The single Docker argv and startup-check construction path.
use crate::{
    commands,
    connection::{Endpoint, EngineOs, Snapshot},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{program::PreparedProgram, Client, RpcError};
pub const REMOVE_ENV: [&str; 5] = [
    "DOCKER_HOST",
    "DOCKER_CONTEXT",
    "DOCKER_TLS_VERIFY",
    "DOCKER_CERT_PATH",
    "DOCKER_API_VERSION",
];
pub fn prepare(
    client: &Client,
    view: &str,
    target: &Snapshot,
    args: &[&str],
    container: Option<&str>,
    terminal: bool,
) -> Result<PreparedProgram, RpcError> {
    let host = target.endpoint.uri();
    let mut checks = vec![
        json!({"argv":["--host",host,"info","--format","{{.ID}}"],"expectedStdout":target.engine_id}),
    ];
    if let Some(id) = container {
        checks.push(json!({"argv":["--host",host,"container","inspect","--format","{{.Id}}",id],"expectedStdout":id}));
    }
    raw(
        client,
        view,
        &target.context,
        &target.environment,
        &target.cli,
        Some(&target.endpoint),
        args,
        checks,
        terminal,
    )
}
pub fn raw(
    client: &Client,
    view: &str,
    context: &Value,
    environment: &Value,
    file: &str,
    endpoint: Option<&Endpoint>,
    args: &[&str],
    checks: Vec<Value>,
    terminal: bool,
) -> Result<PreparedProgram, RpcError> {
    let mut argv = Vec::<String>::new();
    if let Some(endpoint) = endpoint {
        argv.extend(["--host".into(), endpoint.uri()]);
    }
    argv.extend(args.iter().map(|s| s.to_string()));
    PreparedProgram::prepare(
        client,
        view,
        context,
        environment,
        json!({"file":file,"argv":argv,"removeEnv":if endpoint.is_some(){json!(REMOVE_ENV)}else{json!([])},"checks":checks}),
        terminal,
    )
}
pub fn startup_error(status: &Value) -> Option<String> {
    if status["programStarted"] != false {
        return None;
    }
    Some(match status["checkFailure"]["index"].as_u64() {
        Some(1) => "Docker Engine 身份已改变或不可访问，请重新识别连接".into(),
        Some(2) => "原容器已不存在或身份改变，请从资源树重新打开".into(),
        _ => format!(
            "Docker CLI 未能启动：{}",
            status["programError"]["message"]
                .as_str()
                .unwrap_or("远端程序不可用")
        ),
    })
}
pub fn outcome(status: &Value, stderr: &[u8]) -> (&'static str, String) {
    if let Some(message) = startup_error(status) {
        return ("failed", message);
    }
    if status["remoteExitConfirmed"] != true || status["state"] != "completed" {
        return ("unknown", "结果未确认，请刷新；不会自动重发".into());
    }
    if status["exitCode"] == 0 {
        ("succeeded", "命令成功".into())
    } else {
        (
            "failed",
            commands::text(&String::from_utf8_lossy(stderr), 2048),
        )
    }
}
pub fn terminal_args(
    target: &Snapshot,
    container: &str,
    shell: &str,
) -> Result<Vec<String>, String> {
    commands::id(container)?;
    if shell.is_empty() || shell.len() > 256 || shell.chars().any(char::is_control) {
        return Err("容器程序路径无效".into());
    }
    match target.os {
        EngineOs::Linux if !shell.starts_with('/') => {
            return Err("Linux 容器 Shell 必须是绝对路径".into())
        }
        EngineOs::Windows
            if shell.starts_with('/')
                || (!crate::connection::absolute(shell)
                    && (shell.chars().any(char::is_whitespace)
                        || shell.contains(['/', '\\', ':']))) =>
        {
            return Err("Windows 容器程序须为一个可执行名称或盘符绝对路径".into())
        }
        _ => {}
    }
    Ok([
        "container",
        "exec",
        "--interactive",
        "--tty",
        container,
        shell,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outcomes_require_both_startup_and_confirmed_completion() {
        assert_eq!(
            outcome(
                &json!({"programStarted":false,"checkFailure":{"index":1}}),
                b""
            )
            .0,
            "failed"
        );
        assert_eq!(
            outcome(
                &json!({"state":"failed","remoteExitConfirmed":true,"exitCode":0}),
                b""
            )
            .0,
            "unknown"
        );
        assert_eq!(
            outcome(
                &json!({"state":"completed","remoteExitConfirmed":true,"exitCode":0}),
                b""
            )
            .0,
            "succeeded"
        );
        assert_eq!(
            outcome(
                &json!({"state":"completed","remoteExitConfirmed":true,"exitCode":1}),
                b"daemon refused"
            )
            .1,
            "daemon refused"
        );
    }
    #[test]
    fn container_program_uses_the_engine_system() {
        let mut target = crate::connection::Target {
            context: json!({}),
            revision: 1,
            environment: json!({"kind":"windows"}),
            cli: "docker.exe".into(),
            endpoint: Endpoint::Pipe("docker_engine".into()),
            engine_id: "engine".into(),
            os: EngineOs::Linux,
            source: "manual".into(),
        };
        let id = "a".repeat(64);
        assert!(terminal_args(&std::sync::Arc::new(target.clone()), &id, "/bin/sh").is_ok());
        assert!(terminal_args(&std::sync::Arc::new(target.clone()), &id, "cmd.exe").is_err());
        target.os = EngineOs::Windows;
        let target = std::sync::Arc::new(target);
        for shell in [
            "cmd.exe",
            "powershell.exe",
            "C:\\Program Files\\Shell\\shell.exe",
        ] {
            assert!(terminal_args(&target, &id, shell).is_ok(), "{shell}");
        }
        for shell in ["/bin/sh", "cmd.exe /c whoami", "C:cmd.exe", ""] {
            assert!(terminal_args(&target, &id, shell).is_err(), "{shell}");
        }
    }
}
