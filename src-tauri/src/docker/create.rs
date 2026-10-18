use super::{executor, inspect, model::*, operations::Prepared, query};
use crate::{
    model::{AppError, AppResult},
    remote_read, ssh,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub image_id: String,
    pub name: String,
    pub ports: Vec<Publish>,
    pub environment: Vec<Environment>,
    pub mounts: Vec<MountInput>,
    pub network: Option<String>,
    pub restart_policy: String,
    pub maximum_retry_count: u32,
    pub command: Vec<String>,
    pub working_directory: String,
    pub start: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Publish {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Environment {
    pub key: String,
    pub value: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountInput {
    pub kind: String,
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}
impl Spec {
    pub fn validate(&self) -> AppResult<()> {
        super::images::validate_id(&self.image_id)?;
        Action::Rename {
            name: self.name.clone(),
        }
        .validate()?;
        Action::RestartPolicy {
            policy: self.restart_policy.clone(),
            maximum_retry_count: self.maximum_retry_count,
        }
        .validate()?;
        if self.ports.len() > 64
            || self.environment.len() > 200
            || self.mounts.len() > 32
            || self.command.len() > 128
        {
            return Err(AppError::new("limit", "创建参数数量超过上限"));
        }
        let mut keys = HashSet::new();
        let mut bytes = 0;
        for env in &self.environment {
            bytes += env.key.len() + env.value.len();
            if env.key.is_empty()
                || env.key.len() > 255
                || !env
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || env.key.as_bytes()[0].is_ascii_digit()
                || !keys.insert(&env.key)
                || env.value.contains(['\n', '\r', '\0'])
            {
                return Err(AppError::new(
                    "validation",
                    "环境变量键须唯一且有效；值不能包含换行或 NUL",
                ));
            }
        }
        if bytes > 128000 {
            return Err(AppError::new("limit", "环境变量超过 128 KB"));
        }
        let mut ports = HashSet::new();
        for p in &self.ports {
            if p.host_port == 0
                || p.container_port == 0
                || !["tcp", "udp"].contains(&p.protocol.as_str())
                || p.host_ip.parse::<std::net::IpAddr>().is_err()
                || !ports.insert((&p.host_ip, p.host_port, &p.protocol))
            {
                return Err(AppError::new(
                    "validation",
                    "请检查发布端口、协议与 IPv4／IPv6 地址；同一端口不能重复绑定",
                ));
            }
        }
        let mut mounts = HashSet::new();
        for m in &self.mounts {
            if !["bind", "volume"].contains(&m.kind.as_str())
                || !m.destination.starts_with('/')
                || m.source.is_empty()
                || !mounts.insert(&m.destination)
                || [m.source.as_str(), m.destination.as_str()].iter().any(|s| {
                    s.len() > 4096 || s.contains([',', '"']) || s.chars().any(char::is_control)
                })
                || m.kind == "bind" && !m.source.starts_with('/')
            {
                return Err(AppError::new("validation", "挂载须使用绝对容器路径；Bind 来源须为绝对路径。路径不能重复或包含逗号、引号、控制字符"));
            }
        }
        if self.network.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 255
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        }) || self
            .command
            .iter()
            .any(|s| s.len() > 8192 || s.contains('\0'))
            || !self.working_directory.is_empty()
                && (!self.working_directory.starts_with('/')
                    || self.working_directory.len() > 4096
                    || self.working_directory.chars().any(char::is_control))
        {
            return Err(AppError::new("validation", "网络、工作目录或命令参数无效"));
        }
        Ok(())
    }
}
fn resource(
    session: &ssh2::Session,
    endpoint: &str,
    kind: &str,
    id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Value> {
    let template = if kind == "volume" {
        r#"{"Name":{{json .Name}},"CreatedAt":{{json .CreatedAt}},"Driver":{{json .Driver}}}"#
    } else {
        r#"{"Id":{{json .Id}},"Name":{{json .Name}},"Driver":{{json .Driver}}}"#
    };
    let r = remote_read::exec(
        session,
        &format!(
            "{} {kind} inspect --format {} {}",
            query::docker(endpoint),
            ssh::quote(template),
            ssh::quote(id)
        ),
        check,
    )?;
    if r.code != 0 {
        return Err(AppError::new(
            "notFound",
            format!("所选 {kind} 不存在或不可访问"),
        ));
    }
    serde_json::from_str(&r.stdout).map_err(|_| AppError::new("format", "资源身份不可用"))
}
pub fn prepare(
    session: &ssh2::Session,
    engine: &EngineBinding,
    spec: &Spec,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Prepared> {
    spec.validate()?;
    let image = super::images::resolve_local(session, &engine.endpoint, &spec.image_id, check)?;
    if image.as_deref() != Some(&spec.image_id) {
        return Err(AppError::new("notFound", "镜像不存在；请先拉取镜像"));
    }
    let exists = remote_read::exec(
        session,
        &format!(
            "{} container inspect --format '{{{{.Id}}}}' {}",
            query::docker(&engine.endpoint),
            ssh::quote(&spec.name)
        ),
        check,
    )?;
    if exists.code == 0 {
        return Err(AppError::new("conflict", "容器名称已存在，请选择其他名称"));
    }
    if !exists.stderr.to_lowercase().contains("no such") {
        return Err(AppError::new("docker", "无法确认容器名称是否可用"));
    }
    let mut resources = vec![];
    if spec.mounts.iter().any(|m| m.kind == "bind") {
        super::links::ensure_local(session, engine, check)?;
    }
    for m in &spec.mounts {
        if m.kind == "bind" {
            let sftp = session.sftp()?;
            let metadata = sftp.stat(Path::new(&m.source)).map_err(|_| {
                AppError::new(
                    "notFound",
                    format!("Bind 来源不存在或不可访问：{}", m.source),
                )
            })?;
            resources.push(json!([m.source, metadata.perm, metadata.uid, metadata.gid]));
        } else {
            resources.push(resource(
                session,
                &engine.endpoint,
                "volume",
                &m.source,
                check,
            )?);
        }
    }
    if let Some(network) = &spec.network {
        resources.push(resource(
            session,
            &engine.endpoint,
            "network",
            network,
            check,
        )?);
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([spec, resources])).unwrap())
    );
    let mut consequences = vec![
        format!(
            "使用本地镜像 {} 创建容器 {}{}。",
            spec.image_id,
            spec.name,
            if spec.start {
                "并启动"
            } else {
                "，暂不启动"
            }
        ),
        format!(
            "发布 {} 个端口，挂载 {} 个路径或卷，传入 {} 个环境变量。",
            spec.ports.len(),
            spec.mounts.len(),
            spec.environment.len()
        ),
        "创建后启动失败会保留容器和诊断信息，不自动删除或重复创建。".into(),
    ];
    consequences.extend(spec.ports.iter().map(|p| {
        format!(
            "发布 {}:{} → {}/{}",
            p.host_ip, p.host_port, p.container_port, p.protocol
        )
    }));
    consequences.extend(spec.mounts.iter().map(|m| {
        format!(
            "{} {} → {}（{}）",
            m.kind,
            m.source,
            m.destination,
            if m.read_only { "只读" } else { "读写" }
        )
    }));
    Ok(Prepared {
        name: spec.name.clone(),
        fingerprint,
        consequences,
        command: String::new(),
        private_config: None,
        batch: None,
        timeout: 300,
    })
}
fn command(endpoint: &str, spec: &Spec, request_id: &str, env: Option<&str>) -> String {
    let mut c = format!(
        "{} container create --name {} --label {}",
        query::docker(endpoint),
        ssh::quote(&spec.name),
        ssh::quote(&format!("io.sodeck.create.request={request_id}"))
    );
    let restart = if spec.restart_policy == "on-failure" && spec.maximum_retry_count > 0 {
        format!("on-failure:{}", spec.maximum_retry_count)
    } else {
        spec.restart_policy.clone()
    };
    c.push_str(&format!(" --restart {}", ssh::quote(&restart)));
    if let Some(env) = env {
        c.push_str(&format!(" --env-file {}", ssh::quote(env)));
    }
    for p in &spec.ports {
        let host = if p.host_ip.contains(':') {
            format!("[{}]", p.host_ip)
        } else {
            p.host_ip.clone()
        };
        c.push_str(&format!(
            " --publish {}",
            ssh::quote(&format!(
                "{host}:{}:{}/{}",
                p.host_port, p.container_port, p.protocol
            ))
        ));
    }
    for m in &spec.mounts {
        c.push_str(&format!(
            " --mount {}",
            ssh::quote(&format!(
                "type={},src={},dst={}{}",
                m.kind,
                m.source,
                m.destination,
                if m.read_only { ",readonly" } else { "" }
            ))
        ));
    }
    if let Some(network) = &spec.network {
        c.push_str(&format!(" --network {}", ssh::quote(network)));
    }
    if !spec.working_directory.is_empty() {
        c.push_str(&format!(
            " --workdir {}",
            ssh::quote(&spec.working_directory)
        ));
    }
    c.push_str(&format!(" {}", ssh::quote(&spec.image_id)));
    for arg in &spec.command {
        c.push_str(&format!(" {}", ssh::quote(arg)));
    }
    c
}
pub fn execute(
    session: &ssh2::Session,
    endpoint: &str,
    spec: &Spec,
    request_id: &str,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
    created: &impl Fn(&str),
) -> AppResult<ssh::ExecResult> {
    let env_path = if spec.environment.is_empty() {
        None
    } else {
        Some(format!(
            "/tmp/.sodeck-env-{}",
            uuid::Uuid::new_v4().simple()
        ))
    };
    let mut command = command(endpoint, spec, request_id, env_path.as_deref());
    if let Some(path) = &env_path {
        check()?;
        let sftp = session.sftp()?;
        let mut file = sftp.open_mode(
            Path::new(path),
            ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE,
            0o600,
            ssh2::OpenType::File,
        )?;
        let content = spec
            .environment
            .iter()
            .map(|e| format!("{}={}\n", e.key, e.value))
            .collect::<String>();
        if let Err(e) = file.write_all(content.as_bytes()) {
            drop(file);
            let _ = sftp.unlink(Path::new(path));
            return Err(e.into());
        }
        drop(file);
        let cleanup = format!(
            "rm -f -- {} || printf '__SODECK_TEMP_CLEANUP_FAILED__\\n' >&2",
            ssh::quote(path)
        );
        command = format!(
            "sh -c {}",
            ssh::quote(&format!("trap {} EXIT; {command}", ssh::quote(&cleanup)))
        );
    }
    let result = executor::execute_progress(
        session,
        &command,
        Duration::from_secs(300),
        attempted,
        check,
        &|_| {},
    );
    let mut result = match result {
        Ok(result) => result,
        Err(e) => {
            if let Some(path) = &env_path {
                if !attempted.load(Ordering::SeqCst) {
                    if let Ok(sftp) = session.sftp() {
                        let _ = sftp.unlink(Path::new(path));
                    }
                }
                return Err(AppError::new(
                    &e.kind,
                    format!(
                        "{}；临时环境文件 {path} 应由远端命令结束时清理。请核对后检查是否残留。",
                        e.message
                    ),
                ));
            }
            return Err(e);
        }
    };
    let cleanup_warning = result.stderr.contains("__SODECK_TEMP_CLEANUP_FAILED__");
    if result.code != 0 {
        result.stderr = format!(
            "创建命令返回退出码 {}，请检查名称、挂载和网络设置。",
            result.code
        );
        if cleanup_warning {
            result.stderr.push_str(&format!(
                " 临时配置清理未完成：{}",
                env_path.as_deref().unwrap_or_default()
            ));
        }
        result.stdout.clear();
        return Ok(result);
    }
    let id = result.stdout.trim();
    validate_id(id).map_err(|_| {
        AppError::new(
            "unknown",
            "创建命令已完成，但未取得有效容器 ID；请按操作标识核对",
        )
    })?;
    created(id);
    attempted.store(false, Ordering::SeqCst);
    let detail = inspect::container(session, endpoint, id, check)?;
    if detail.name != spec.name {
        return Err(AppError::new("identity", "创建的容器名称不匹配，请核对"));
    }
    if spec.start {
        let mut start = executor::execute_progress(
            session,
            &format!(
                "{} container start {}",
                query::docker(endpoint),
                ssh::quote(id)
            ),
            Duration::from_secs(300),
            attempted,
            check,
            &|_| {},
        )?;
        if start.code != 0 {
            start.stderr = format!(
                "容器 {id} 已创建，启动失败（退出码 {}）。容器已保留，请查看详情排障。",
                start.code
            );
            start.stdout.clear();
        }
        result = start;
    }
    if cleanup_warning {
        result.stderr.push_str(&format!(
            " 临时配置清理未完成：{}",
            env_path.unwrap_or_default()
        ));
    }
    Ok(result)
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    request_id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, Option<String>)> {
    let result = remote_read::exec(
        session,
        &format!(
            "{} container ls -a --no-trunc --filter {} --format '{{{{.ID}}}}'",
            query::docker(endpoint),
            ssh::quote(&format!("label=io.sodeck.create.request={request_id}"))
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new("docker", "无法核对创建标识"));
    }
    let ids: Vec<_> = result.stdout.lines().filter(|s| !s.is_empty()).collect();
    if ids.is_empty() {
        return Ok((
            "当前未发现带此创建标识的容器；这不代表远端命令以后不会完成".into(),
            None,
        ));
    }
    if ids.len() != 1 {
        return Err(AppError::new(
            "conflict",
            "多个容器具有相同创建标识，无法确定原对象",
        ));
    }
    let d = inspect::container(session, endpoint, ids[0], check)?;
    Ok((format!("容器 {} · {}", d.id, d.state), d.health))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> Spec {
        Spec {
            image_id: format!("sha256:{}", "a".repeat(64)),
            name: "web".into(),
            ports: vec![],
            environment: vec![],
            mounts: vec![],
            network: None,
            restart_policy: "no".into(),
            maximum_retry_count: 0,
            command: vec![],
            working_directory: String::new(),
            start: true,
        }
    }
    #[test]
    fn create_secrets_are_never_shell_arguments_and_paths_do_not_inject_mount_options() {
        let mut s = spec();
        s.environment.push(Environment {
            key: "PASSWORD".into(),
            value: "secret;$(whoami)".into(),
        });
        s.command = vec!["sh".into(), "-c".into(), "printf '%s' '$value'".into()];
        s.validate().unwrap();
        let cmd = command(
            "unix:///var/run/docker.sock",
            &s,
            "request",
            Some("/tmp/private-env"),
        );
        assert!(!cmd.contains("secret"));
        assert!(cmd.contains("--env-file '/tmp/private-env'"));
        s.mounts.push(MountInput {
            kind: "bind".into(),
            source: "/data,readonly=false".into(),
            destination: "/data".into(),
            read_only: true,
        });
        assert!(s.validate().is_err());
        s.mounts.clear();
        s.environment.push(Environment {
            key: "PASSWORD".into(),
            value: "other".into(),
        });
        assert!(s.validate().is_err());
        s.environment.pop();
        s.environment[0].value = "one\ntwo".into();
        assert!(s.validate().is_err());
    }
}
