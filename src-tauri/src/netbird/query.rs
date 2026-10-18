use super::{model::*, parser, runner};
use crate::{
    model::{AppError, AppResult},
    ssh::quote,
    traffic::now,
};
use serde_json::Value;

pub const DAEMON: &str = "unix:///var/run/netbird.sock";
pub const ENV: &str =
    "env -i PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin LANG=C";
fn cli_base(path: &str, args: &str) -> String {
    format!(
        "{ENV} HOME=\"$HOME\" {} --daemon-addr {} {args} </dev/null",
        quote(path),
        quote(DAEMON)
    )
}
pub fn cli(path: &str, args: &str) -> String {
    let command = cli_base(path, args);
    if args.ends_with("--help") {
        command
    } else {
        format!("test -S /var/run/netbird.sock && {command}")
    }
}
pub fn validate_path(path: &str) -> AppResult<()> {
    if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(AppError::new("validation", "请输入有效的绝对路径"));
    }
    Ok(())
}
#[derive(Clone, Debug, Default)]
pub struct Service {
    pub signature: String,
    pub invocation: String,
    pub active: String,
    pub writable: bool,
    pub owns_socket: bool,
}
pub fn service(
    session: &ssh2::Session,
    path: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Service> {
    let r = runner::read(session, &format!("{ENV} systemctl show netbird.service --no-pager --property=LoadState,ActiveState,ExecStart,Environment,FragmentPath,InvocationID,MainPID,User,EnvironmentFiles; id -u"), check)?;
    let field = |name: &str| {
        r.stdout
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{name}=")))
            .unwrap_or("")
            .to_string()
    };
    let exec = field("ExecStart");
    let environment = field("Environment");
    // Bind the unit command separately from its volatile PID/start-time fields.
    // Only a root-owned native unit with a matching listening process can be controlled.
    let argv = exec
        .split("argv[]=")
        .nth(1)
        .and_then(|s| s.split(" ;").next())
        .unwrap_or("");
    let executable = exec
        .split("path=")
        .nth(1)
        .and_then(|s| s.split(" ;").next())
        .unwrap_or("");
    let args: Vec<_> = argv.split_whitespace().collect();
    let conventional = executable == path
        && args.first().copied() == Some(path)
        && args.windows(2).any(|pair| pair == ["service", "run"])
        && exec.matches("argv[]=").count() == 1;
    let root = r.stdout.lines().last() == Some("0");
    let pid = field("MainPID").parse::<u32>().ok().filter(|pid| *pid > 0);
    let owns_socket = if root && field("ActiveState") == "active" {
        let sockets = runner::read(session, "ss -H -xlpn", check)?;
        sockets.code == 0
            && pid.is_some_and(|pid| {
                sockets.stdout.lines().any(|line| {
                    line.split_whitespace().any(|value| {
                        value == "/var/run/netbird.sock" || value == "/run/netbird.sock"
                    }) && line.contains(&format!("pid={pid},"))
                })
            })
    } else {
        false
    };
    let writable = field("LoadState") == "loaded"
        && conventional
        && environment.is_empty()
        && field("EnvironmentFiles").is_empty()
        && ["", "root"].contains(&field("User").as_str())
        && root
        && (field("ActiveState") != "active" || owns_socket);
    Ok(Service {
        signature: parser::hash(&format!(
            "{executable}|{argv}|{environment}|{}",
            field("FragmentPath")
        )),
        invocation: field("InvocationID"),
        active: field("ActiveState"),
        writable,
        owns_socket,
    })
}
pub fn detect(
    session: &ssh2::Session,
    path: Option<&str>,
    old: &Snapshot,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Snapshot> {
    let os = runner::read(session, "uname -s", check)?;
    if os.stdout.trim() != "Linux" {
        return Ok(Snapshot {
            collection_status: "unsupported".into(),
            error: Some("首版支持 Linux 原生 NetBird 客户端".into()),
            ..Default::default()
        });
    }
    let path = if let Some(path) = path {
        validate_path(path)?;
        path.to_string()
    } else {
        let r = runner::read(session, "command -v netbird", check)?;
        if r.code != 0 || r.stdout.is_empty() {
            return Ok(Snapshot {
                installation: "missing".into(),
                collection_status: "unavailable".into(),
                error: Some("未检测到 NetBird，可在诊断中指定 CLI 路径".into()),
                ..Default::default()
            });
        }
        r.stdout.trim().to_string()
    };
    validate_path(&path)?;
    let executable = runner::read(
        session,
        &format!(
            "test -x {} && readlink -f -- {}",
            quote(&path),
            quote(&path)
        ),
        check,
    )?;
    if executable.code != 0 {
        return Err(AppError::new("permission", "NetBird CLI 不存在或不可执行"));
    }
    let path = executable.stdout.trim();
    validate_path(path)?;
    let help = runner::read(session, &cli(path, "status --help"), check)?;
    let has_json =
        help.code == 0 && help.stdout.contains("--json") && help.stdout.contains("--daemon-addr");
    let mut svc = service(session, path, check).unwrap_or_default();
    let raw = runner::read(
        session,
        &cli(path, if has_json { "status --json" } else { "status" }),
        check,
    )?;
    let value: Value = serde_json::from_str(&raw.stdout).unwrap_or(Value::Null);
    let key = parser::text(&value, "publicKey");
    let profile = parser::text(&value, "profileName");
    let signature = parser::hash(&format!(
        "{path}|{DAEMON}|{:?}|{:?}|{:?}|{:?}|{}",
        key,
        profile,
        parser::text(&value, "cliVersion"),
        parser::text(&value, "daemonVersion"),
        svc.signature
    ));
    let mut snapshot = if has_json && raw.code == 0 {
        parser::parse(&raw.stdout, now())?
    } else {
        Snapshot {
            collection_status: if has_json {
                "unavailable"
            } else {
                "unsupported"
            }
            .into(),
            installation: "installed".into(),
            service: if svc.active == "inactive" || svc.active == "failed" {
                "stopped"
            } else {
                "unknown"
            }
            .into(),
            error: Some(
                if !has_json {
                    "当前 CLI 不支持结构化状态，仅提供原始状态查看"
                } else if raw.stderr.to_lowercase().contains("permission") {
                    "没有权限访问 NetBird daemon，请在终端处理权限"
                } else {
                    "无法访问 NetBird daemon，请检查服务状态"
                }
                .into(),
            ),
            ..Default::default()
        }
    };
    let prior = old
        .binding
        .as_ref()
        .filter(|b| b.cli_path == path && b.service.signature == svc.signature);
    if has_json
        && raw.code != 0
        && prior.is_some()
        && old.updated_at.is_some()
        && snapshot.service != "stopped"
    {
        return Err(AppError::new(
            if raw.stderr.to_lowercase().contains("permission") {
                "permission"
            } else {
                "unavailable"
            },
            snapshot.error.as_deref().unwrap_or("无法读取客户端状态"),
        ));
    }
    // A stopped daemon may retain a previously verified service binding, never invent a new client identity.
    let stopped = snapshot.service == "stopped" && prior.is_some();
    let signature = if stopped {
        prior.unwrap().signature.clone()
    } else {
        signature
    };
    let token = prior
        .filter(|b| b.signature == signature)
        .map(|b| b.token.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let unambiguous = runner::read(session, "test ! -d /var/run/netbird", check)?.code == 0;
    let timed_up = runner::read(session, "timeout --version", check)
        .is_ok_and(|r| r.code == 0 && r.stdout.contains("GNU coreutils"));
    let known = unambiguous
        && key.is_some()
        && snapshot.collection_status == "ready"
        && snapshot.connection != "unknown"
        && has_json;
    let up_help = if known {
        runner::read(session, &cli(path, "up --help"), check)?.stdout
    } else {
        String::new()
    };
    let down_help = if known {
        runner::read(session, &cli(path, "down --help"), check)?.stdout
    } else {
        String::new()
    };
    let capabilities = Capabilities {
        json: has_json,
        up: known
            && timed_up
            && profile.as_deref() == Some("default")
            && svc.writable
            && snapshot.connection == "disconnected"
            && up_help.contains("--no-browser"),
        down: known
            && matches!(snapshot.connection.as_str(), "connected" | "connecting")
            && down_help.contains("Usage:"),
        service_start: stopped
            && svc.writable
            && unambiguous
            && prior.is_some_and(|b| b.service.owns_socket),
        service_restart: known
            && svc.writable
            && svc.active == "active"
            && !svc.invocation.is_empty(),
        reason: if !unambiguous {
            Some("检测到 NetBird 实例目录，首版仅支持明确的默认 daemon，操作保持只读".into())
        } else if !known && !stopped {
            Some("客户端身份或状态不完整，操作保持只读".into())
        } else if !svc.writable {
            Some("服务操作需要 root 和已确认的默认 systemd 单元".into())
        } else if snapshot.connection == "disconnected"
            && (!timed_up
                || profile.as_deref() != Some("default")
                || !up_help.contains("--no-browser"))
        {
            Some(
                "连接操作需要默认 Profile、支持非浏览器登录的 CLI 和 GNU timeout；可在终端手动连接"
                    .into(),
            )
        } else {
            None
        },
    };
    if stopped {
        svc.owns_socket = prior.is_some_and(|b| b.service.owns_socket);
        snapshot.local_peer = old.local_peer.clone();
        snapshot.peers = old.peers.clone();
        snapshot.peers_complete = old.peers_complete;
        snapshot.reported_total = old.reported_total;
        snapshot.updated_at = old.updated_at;
    }
    snapshot.binding = Some(Binding {
        token,
        cli_path: path.into(),
        daemon_address: DAEMON.into(),
        client_version: parser::text(&value, "cliVersion")
            .or_else(|| prior.and_then(|b| b.client_version.clone())),
        daemon_version: parser::text(&value, "daemonVersion")
            .or_else(|| prior.and_then(|b| b.daemon_version.clone())),
        profile: profile.or_else(|| prior.and_then(|b| b.profile.clone())),
        capabilities,
        signature,
        service: svc,
    });
    check()?;
    Ok(snapshot)
}
pub fn allowed(snapshot: &Snapshot, action: &Action) -> bool {
    snapshot.binding.as_ref().is_some_and(|b| match action {
        Action::Up => b.capabilities.up,
        Action::Down => b.capabilities.down,
        Action::ServiceStart => b.capabilities.service_start,
        Action::ServiceRestart => b.capabilities.service_restart,
    })
}
pub fn operation_command(binding: &Binding, action: &Action) -> String {
    match action {
        Action::Up => format!(
            "test -S /var/run/netbird.sock && {ENV} HOME=\"$HOME\" timeout -k 2 20 sh -c {}",
            quote(&cli_base(&binding.cli_path, "up --no-browser"))
        ),
        Action::Down => cli(&binding.cli_path, "down"),
        Action::ServiceStart => format!("{ENV} systemctl --no-ask-password start netbird.service"),
        Action::ServiceRestart => {
            format!("{ENV} systemctl --no-ask-password restart netbird.service")
        }
    }
}
