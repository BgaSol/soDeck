use crate::connection::{self, SharedContext};
use crate::ssh_transport::Cancellation;
use crate::{
    model::{AppError, AppResult},
    ssh,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DockerStatus {
    pub state: String,
    pub message: String,
    pub endpoint: String,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub name: String,
    pub size: String,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContainerTarget {
    pub id: String,
    pub name: String,
    pub endpoint: String,
}
pub fn detect(spec: &SharedContext) -> AppResult<DockerStatus> {
    let connection = connection::connect_verified(
        spec,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::DockerMonitor,
    )?;
    detect_on(connection.session(), &|| Ok(()))
}
pub(crate) fn detect_on(
    session: &ssh2::Session,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<DockerStatus> {
    let found = query(session, "command -v docker >/dev/null 2>&1", check)?;
    if found.code != 0 {
        return Ok(DockerStatus {
            state: "missing".into(),
            message: "当前账号未找到 docker 命令".into(),
            endpoint: String::new(),
        });
    }
    let endpoint=query(session,"if [ -n \"$DOCKER_HOST\" ] && [ -z \"$DOCKER_CONTEXT\" ]; then printf '%s' \"$DOCKER_HOST\"; else docker context inspect --format '{{.Endpoints.docker.Host}}'; fi", check)?;
    if endpoint.code != 0 || endpoint.stdout.trim().is_empty() {
        return Ok(DockerStatus {
            state: "error".into(),
            message: format!("无法确认 Docker 目标：{}", endpoint.stderr.trim()),
            endpoint: String::new(),
        });
    }
    let endpoint = endpoint.stdout.trim().to_string();
    let result = query(
        session,
        &format!(
            "{} info --format '{{{{json .ServerVersion}}}}'",
            docker(&endpoint)
        ),
        check,
    )?;
    if result.code == 0 {
        Ok(DockerStatus {
            state: "running".into(),
            message: "Docker 引擎可访问".into(),
            endpoint,
        })
    } else {
        let text = format!("{}{}", result.stderr, result.stdout);
        let state = if text.to_lowercase().contains("permission denied") {
            "permission"
        } else {
            "unreachable"
        };
        Ok(DockerStatus {
            state: state.into(),
            message: text.trim().into(),
            endpoint,
        })
    }
}
pub(crate) fn docker(endpoint: &str) -> String {
    format!(
        "env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {}",
        ssh::quote(endpoint)
    )
}
fn field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
fn json_lines(output: &str) -> AppResult<Vec<Value>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|e| AppError::new("docker", format!("Docker 返回格式无效：{e}")))
        })
        .collect()
}
pub fn images(spec: &SharedContext, endpoint: &str) -> AppResult<Vec<Image>> {
    let connection = connection::connect_verified(
        spec,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::DockerMonitor,
    )?;
    images_on(connection.session(), endpoint, &|| Ok(()))
}
pub(crate) fn images_on(
    session: &ssh2::Session,
    endpoint: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<Image>> {
    let result = query(
        session,
        &format!(
            "{} image ls --no-trunc --format '{{{{json .}}}}'",
            docker(endpoint)
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new("docker", result.stderr));
    }
    Ok(json_lines(&result.stdout)?
        .iter()
        .map(|value| {
            let repository = field(value, "Repository");
            let tag = field(value, "Tag");
            let id = field(value, "ID");
            Image {
                id: id.clone(),
                name: if repository == "<none>" {
                    id
                } else {
                    format!("{repository}:{tag}")
                },
                size: field(value, "Size"),
            }
        })
        .collect())
}
pub fn containers(spec: &SharedContext, endpoint: &str) -> AppResult<Vec<Container>> {
    let connection = connection::connect_verified(
        spec,
        &Cancellation::default(),
        crate::traffic::TrafficPurpose::DockerMonitor,
    )?;
    containers_on(connection.session(), endpoint, &|| Ok(()))
}
pub(crate) fn containers_on(
    session: &ssh2::Session,
    endpoint: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<Container>> {
    let result = query(
        session,
        &format!(
            "{} ps -a --no-trunc --format '{{{{json .}}}}'",
            docker(endpoint)
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new("docker", result.stderr));
    }
    let mut values: Vec<_> = json_lines(&result.stdout)?
        .iter()
        .map(|v| Container {
            id: field(v, "ID"),
            name: field(v, "Names"),
            image: field(v, "Image"),
            state: field(v, "State"),
            status: field(v, "Status"),
        })
        .collect();
    values.sort_by(|a, b| {
        (a.state != "running")
            .cmp(&(b.state != "running"))
            .then(a.name.cmp(&b.name))
    });
    Ok(values)
}
fn query(
    session: &ssh2::Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ssh::ExecResult> {
    crate::remote_read::exec(session, command, check)
}
pub(crate) fn unhealthy_on(
    session: &ssh2::Session,
    endpoint: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Vec<String>> {
    let result = query(
        session,
        &format!(
            "{} ps -a --no-trunc --filter health=unhealthy --format '{{{{.ID}}}}'",
            docker(endpoint)
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new("docker", result.stderr));
    }
    let ids: Vec<String> = result
        .stdout
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_owned())
        .collect();
    if ids
        .iter()
        .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(AppError::new("docker", "Docker 健康状态返回格式无效"));
    }
    Ok(ids)
}

pub fn terminal_command_on(session: &ssh2::Session, target: &ContainerTarget) -> AppResult<String> {
    if target.id.len() != 64 || !target.id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::new("validation", "无效的容器 ID"));
    }
    let prefix = docker(&target.endpoint);
    let id = ssh::quote(&target.id);
    let result = ssh::exec(
        session,
        &format!("{prefix} inspect --type container --format '{{{{json .State}}}}' {id}"),
    )?;
    if result.code != 0 {
        return Err(AppError::new("docker", result.stderr));
    }
    let state: Value =
        serde_json::from_str(result.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
    if state["Running"] != true || state["Paused"] == true || state["Restarting"] == true {
        return Err(AppError::new("docker", "容器当前不处于可进入的运行状态"));
    }
    for shell in ["/bin/bash", "/bin/sh"] {
        let result = ssh::exec(session, &format!("{prefix} exec {id} {shell} -c 'exit 0'"))?;
        if result.code == 0 {
            return Ok(format!("{prefix} exec -it {id} {shell}"));
        }
        let text = format!("{}{}", result.stderr, result.stdout);
        let lower = text.to_lowercase();
        if !(lower.contains("no such file")
            || lower.contains("executable file not found")
            || lower.contains("not found in $path"))
        {
            return Err(AppError::new("docker", text));
        }
    }
    Err(AppError::new(
        "docker",
        "容器中没有可用的 /bin/bash 或 /bin/sh",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_quote_remote_parameters() {
        assert_eq!(ssh::quote("a'b"), "'a'\\''b'");
        assert!(docker("unix:///run/a'b").contains("'\\''"));
    }
    #[test]
    fn parses_docker_json_lines() {
        let values = json_lines("{\"State\":\"running\"}\n{\"State\":\"exited\"}\n").unwrap();
        assert_eq!(values.len(), 2);
        assert!(json_lines("bad output").is_err());
    }
}
