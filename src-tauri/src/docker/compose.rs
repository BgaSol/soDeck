use super::{model::*, operations::Prepared, query};
use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    remote_read, ssh, State,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Association {
    pub id: String,
    pub revision: u32,
    pub server_id: String,
    pub endpoint: String,
    pub daemon_id: Option<String>,
    pub project: String,
    pub directory: String,
    pub files: Vec<String>,
    pub env_files: Vec<String>,
    pub profiles: Vec<String>,
}
pub fn project_name(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_lowercase() && !value.as_bytes()[0].is_ascii_digit()
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(AppError::new(
            "validation",
            "项目名须以小写字母或数字开头，只能包含小写字母、数字、连字符与下划线",
        ));
    }
    Ok(())
}
impl Association {
    pub fn validate(&self) -> AppResult<()> {
        project_name(&self.project)?;
        if self.files.is_empty()
            || self.files.len() > 16
            || self.env_files.len() > 16
            || self.profiles.len() > 32
        {
            return Err(AppError::new(
                "validation",
                "请提供 1–16 个有序 Compose 配置文件",
            ));
        }
        for path in std::iter::once(&self.directory)
            .chain(self.files.iter())
            .chain(self.env_files.iter())
        {
            if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
                return Err(AppError::new(
                    "validation",
                    "配置和工作目录须为远端绝对路径，不包含控制字符",
                ));
            }
        }
        if self.profiles.iter().any(|p| {
            p.is_empty()
                || p.len() > 128
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        }) {
            return Err(AppError::new("validation", "Profile 名称无效"));
        }
        Ok(())
    }
    fn prefix(&self, endpoint: &str, include_files: bool) -> String {
        let mut command = format!(
            "{} compose --project-name {} --project-directory {}",
            query::docker(endpoint),
            ssh::quote(&self.project),
            ssh::quote(&self.directory)
        );
        if include_files {
            for file in &self.files {
                command.push_str(&format!(" --file {}", ssh::quote(file)));
            }
            for file in &self.env_files {
                command.push_str(&format!(" --env-file {}", ssh::quote(file)));
            }
        }
        for profile in &self.profiles {
            command.push_str(&format!(" --profile {}", ssh::quote(profile)));
        }
        command
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Start,
    Stop,
    Restart,
    Pull,
    Apply,
    Down,
}
impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Start => "启动项目",
            Self::Stop => "停止项目",
            Self::Restart => "重启项目",
            Self::Pull => "更新项目镜像",
            Self::Apply => "应用项目配置",
            Self::Down => "停止并移除项目",
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: String,
    pub name: String,
    pub service: String,
    pub state: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub containers: Vec<Service>,
    pub directory: String,
    pub files: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub available: bool,
    pub message: String,
    pub projects: Vec<Project>,
    pub associations: Vec<Association>,
}
fn read(
    session: &ssh2::Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let result = remote_read::exec(session, command, check)?;
    if result.code != 0 {
        return Err(AppError::new(
            "docker",
            "Compose 查询未完成，请检查插件、配置路径及文件权限",
        ));
    }
    Ok(result.stdout)
}
pub fn projects(
    session: &ssh2::Session,
    endpoint: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<Project>> {
    let template = r#"{"id":{{json .ID}},"name":{{json .Names}},"state":{{json .State}},"project":{{json (.Label "com.docker.compose.project")}},"service":{{json (.Label "com.docker.compose.service")}},"directory":{{json (.Label "com.docker.compose.project.working_dir")}},"files":{{json (.Label "com.docker.compose.project.config_files")}}}"#;
    let output = read(
        session,
        &format!(
            "{} container ls -a --no-trunc --filter label=com.docker.compose.project --format {}",
            query::docker(endpoint),
            ssh::quote(template)
        ),
        check,
    )?;
    let mut grouped: BTreeMap<String, Project> = BTreeMap::new();
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line)
            .map_err(|_| AppError::new("format", "Compose 项目数据格式无效"))?;
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
        let name = text("project");
        if name.is_empty() {
            continue;
        }
        validate_id(&text("id"))?;
        grouped
            .entry(name.clone())
            .or_insert_with(|| Project {
                name,
                containers: vec![],
                directory: text("directory"),
                files: text("files")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
            .containers
            .push(Service {
                id: text("id"),
                name: text("name"),
                service: text("service"),
                state: text("state"),
            });
    }
    for project in grouped.values_mut() {
        project.containers.sort_by(|a, b| a.id.cmp(&b.id));
    }
    Ok(grouped.into_values().collect())
}
pub fn overview(
    state: &State,
    spec: &SharedContext,
    session: &ssh2::Session,
    engine: &EngineBinding,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Overview> {
    let version = remote_read::exec(
        session,
        &format!(
            "{} compose version --short",
            query::docker(&engine.endpoint)
        ),
        check,
    )?;
    Ok(Overview {
        available: version.code == 0,
        message: if version.code == 0 {
            String::new()
        } else {
            "当前服务器没有可用的 Docker Compose 插件；仍可查看已发现的项目与容器。".into()
        },
        projects: projects(session, &engine.endpoint, check)?,
        associations: state.docker.with_store(|s| s.list(&spec.target().id))?,
    })
}
fn association(
    state: &State,
    spec: &SharedContext,
    engine: &EngineBinding,
    id: &str,
    revision: u32,
    project: &str,
) -> AppResult<Association> {
    let a = state.docker.with_store(|s| s.get(id))?;
    if a.server_id != spec.target().id
        || a.revision != revision
        || a.project != project
        || a.endpoint != engine.endpoint
        || a.daemon_id != engine.daemon_id
    {
        return Err(AppError::new(
            "conflict",
            "项目关联或引擎已变化，请重新核对关联设置",
        ));
    }
    a.validate()?;
    Ok(a)
}
pub fn prepare(
    state: &State,
    spec: &SharedContext,
    session: &ssh2::Session,
    engine: &EngineBinding,
    project: &str,
    mode: &Mode,
    id: &str,
    revision: u32,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Prepared> {
    let a = association(state, spec, engine, id, revision, project)?;
    let prefix = a.prefix(&engine.endpoint, true);
    let help = read(session, &format!("{prefix} config --help"), check)?;
    if !help.contains("--format") {
        return Err(AppError::new(
            "unavailable",
            "当前 Compose 不支持结构化配置预检，请升级 Compose 插件",
        ));
    }
    if *mode == Mode::Apply && !help.contains("--resolve-image-digests") {
        return Err(AppError::new(
            "unavailable",
            "当前 Compose 不支持固定镜像 digest，暂不能安全应用配置",
        ));
    }
    let raw = read(
        session,
        &format!(
            "{prefix} config --format json{}",
            if *mode == Mode::Apply {
                " --resolve-image-digests"
            } else {
                ""
            }
        ),
        check,
    )?;
    let config: Value =
        serde_json::from_str(&raw).map_err(|_| AppError::new("format", "Compose 配置解析失败"))?;
    let services = config["services"]
        .as_object()
        .ok_or_else(|| AppError::new("validation", "配置中没有服务"))?;
    if services.is_empty() || services.len() > 200 {
        return Err(AppError::new("limit", "项目须包含 1–200 个服务"));
    }
    if *mode == Mode::Apply {
        let up_help = read(session, &format!("{prefix} up --help"), check)?;
        if !up_help.contains("--pull") || !up_help.contains("--no-build") {
            return Err(AppError::new(
                "unavailable",
                "当前 Compose 不支持受控镜像部署选项",
            ));
        }
        for (name, service) in services {
            if service.get("build").is_some() {
                return Err(AppError::new(
                    "unavailable",
                    format!("服务 {name} 包含 build，请使用已构建并拉取的镜像"),
                ));
            }
            let image = service["image"]
                .as_str()
                .ok_or_else(|| AppError::new("validation", format!("服务 {name} 缺少镜像")))?;
            super::images::validate_reference(image)?;
            if !image.contains("@sha256:") {
                return Err(AppError::new(
                    "unavailable",
                    format!("服务 {name} 无法固定为镜像 digest"),
                ));
            }
            super::images::resolve_local(session, &engine.endpoint, image, check)?.ok_or_else(
                || {
                    AppError::new(
                        "notFound",
                        format!("服务 {name} 的目标镜像尚未拉取，请先更新项目镜像"),
                    )
                },
            )?;
        }
    }
    let existing = projects(session, &engine.endpoint, check)?
        .into_iter()
        .find(|p| p.name == project)
        .map(|p| p.containers)
        .unwrap_or_default();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::json!([a, config, existing])).unwrap())
    );
    let names = services.keys().cloned().collect::<Vec<_>>().join("、");
    let mut consequences = vec![format!(
        "项目 {project}；配置服务：{names}。当前发现 {} 个容器。",
        existing.len()
    )];
    consequences.push(format!(
        "工作目录：{}；配置按顺序合并：{}。",
        a.directory,
        a.files.join(" → ")
    ));
    if !a.profiles.is_empty() {
        consequences.push(format!("启用的 Profiles：{}。", a.profiles.join("、")));
    }
    for (name, service) in services {
        if let Some(image) = service["image"].as_str() {
            consequences.push(format!("服务 {name} · 镜像 {image}"));
        }
    }
    for container in &existing {
        let scope = if services.contains_key(&container.service) {
            "现有服务容器"
        } else {
            "孤立服务，保留"
        };
        consequences.push(format!("{scope} · {} · {}", container.name, container.id));
    }
    if matches!(mode, Mode::Down | Mode::Apply) {
        if let Some(networks) = config["networks"].as_object() {
            for (key, network) in networks {
                let name = network["name"].as_str().unwrap_or(key);
                let effect = if network["external"] == true {
                    "外部网络，保留"
                } else if *mode == Mode::Down {
                    "项目网络，未被其他容器使用时移除"
                } else {
                    "项目网络，按配置创建或复用"
                };
                consequences.push(format!("{effect} · {name}"));
            }
        }
    }
    let suffix = match mode {
        Mode::Start => {
            consequences.push("仅启动已有服务容器；缺失的容器请使用“应用配置”创建。".into());
            "start"
        }
        Mode::Stop => {
            consequences.push("停止现有服务，保留容器、网络和数据卷。".into());
            "stop"
        }
        Mode::Restart => {
            consequences.push("重启现有服务；不会应用配置文件变更。".into());
            "restart"
        }
        Mode::Pull => {
            consequences
                .push("只拉取项目镜像。完成后请重新预览并应用配置，才会更新运行中的服务。".into());
            "pull"
        }
        Mode::Apply => {
            consequences.push("按预览中的配置和镜像 digest 创建或重建服务，可能中断服务。使用本地已拉取镜像，不执行构建或额外拉取。".into());
            "up -d --no-build --pull never"
        }
        Mode::Down => {
            consequences.push(
                "停止并删除项目容器与项目网络。保留数据卷、镜像与孤立服务；本地关联设置继续保留。"
                    .into(),
            );
            "down"
        }
    };
    // Execute a frozen, resolved configuration for every action. Raw environment
    // values are never included in IPC previews, task metadata or shell arguments.
    Ok(Prepared {
        name: project.into(),
        fingerprint,
        consequences,
        command: String::new(),
        batch: None,
        private_config: Some(super::operations::PrivateConfig {
            prefix: a.prefix(&engine.endpoint, false),
            // compose config already escapes literal dollars for a subsequent
            // parse. Re-escaping here would change the actual environment values.
            content: config.to_string(),
            suffix: suffix.into(),
        }),
        timeout: 1800,
    })
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    project: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let containers = projects(session, endpoint, check)?
        .into_iter()
        .find(|p| p.name == project)
        .map(|p| p.containers)
        .unwrap_or_default();
    Ok(format!(
        "当前 {} 个项目容器，{} 个运行中；健康状态请查看各服务详情",
        containers.len(),
        containers.iter().filter(|c| c.state == "running").count()
    ))
}
