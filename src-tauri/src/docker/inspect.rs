use super::{model::*, query};
use crate::{
    model::{AppError, AppResult},
    remote_read, ssh,
    traffic::now,
};
use serde_json::Value;

// Project only the fields needed for this pane. Config.Env and health log output
// never enter shared monitor frames or operation history.
// Docker's raw JSON fallback uses missingkey=error. Use index for fields omitted
// by the Engine; even `if .State.Health` fails when there is no healthcheck.
const FORMAT: &str = r#"{"Id":{{json .Id}},"Name":{{json .Name}},"Image":{{json .Config.Image}},"State":{"Status":{{json .State.Status}},"ExitCode":{{json .State.ExitCode}},"OOMKilled":{{json .State.OOMKilled}},"Error":{{json .State.Error}},"StartedAt":{{json .State.StartedAt}},"FinishedAt":{{json .State.FinishedAt}},"Health":{{with index .State "Health"}}{"Status":{{json .Status}},"FailingStreak":{{json .FailingStreak}}}{{else}}null{{end}}},"Created":{{json .Created}},"RestartCount":{{json .RestartCount}},"RestartPolicy":{{json .HostConfig.RestartPolicy}},"AutoRemove":{{json .HostConfig.AutoRemove}},"StopTimeout":{{json (index .Config "StopTimeout")}},"Ports":{{json .NetworkSettings.Ports}},"Networks":{{json .NetworkSettings.Networks}},"Mounts":{{json .Mounts}}}"#;

pub fn container(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ContainerDetail> {
    validate_id(id)?;
    let result = remote_read::docker_query(
        session,
        &format!(
            "{} container inspect --format {} {}",
            query::docker(endpoint),
            ssh::quote(FORMAT),
            ssh::quote(id)
        ),
        "container-inspect",
        check,
    )?;
    if result.code != 0 {
        let kind = if result.stderr.to_lowercase().contains("no such") {
            "notFound"
        } else {
            "docker"
        };
        return Err(AppError::new(kind, clean(&result.stderr, 4096)));
    }
    parse(&result.stdout, id)
}
pub fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(limit)
        .collect()
}
fn text(v: &Value, key: &str) -> String {
    clean(v[key].as_str().unwrap_or_default(), 8192)
}
pub fn parse(output: &str, expected: &str) -> AppResult<ContainerDetail> {
    let v: Value = serde_json::from_str(output)
        .map_err(|_| AppError::new("format", "Docker 容器详情格式无效"))?;
    let id = text(&v, "Id");
    if id != expected || !v["State"]["Status"].is_string() {
        return Err(AppError::new(
            "identity",
            "容器身份或状态无效，请刷新后重试",
        ));
    }
    let s = &v["State"];
    let mut ports = Vec::new();
    if let Some(values) = v["Ports"].as_object() {
        for (port, bindings) in values {
            if let Some(bindings) = bindings.as_array().filter(|b| !b.is_empty()) {
                for binding in bindings {
                    ports.push(Port {
                        container_port: port.clone(),
                        host_ip: binding["HostIp"].as_str().map(str::to_owned),
                        host_port: binding["HostPort"].as_str().map(str::to_owned),
                    });
                }
            } else {
                ports.push(Port {
                    container_port: port.clone(),
                    host_ip: None,
                    host_port: None,
                });
            }
        }
    }
    let mounts = v["Mounts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Mount {
            kind: text(m, "Type"),
            name: m["Name"].as_str().map(str::to_owned),
            source: text(m, "Source"),
            destination: text(m, "Destination"),
            read_only: m["RW"] != true,
        })
        .collect();
    let networks = v["Networks"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, n)| Network {
            name: name.clone(),
            id: text(n, "NetworkID"),
            address: text(n, "IPAddress"),
            ipv6: text(n, "GlobalIPv6Address"),
        })
        .collect();
    Ok(ContainerDetail {
        id,
        name: text(&v, "Name").trim_start_matches('/').into(),
        image: text(&v, "Image"),
        state: text(s, "Status"),
        health: s["Health"]["Status"].as_str().map(str::to_owned),
        failing_streak: s["Health"]["FailingStreak"].as_u64(),
        exit_code: s["ExitCode"].as_i64(),
        oom_killed: s["OOMKilled"] == true,
        error: text(s, "Error"),
        restart_count: v["RestartCount"].as_u64().unwrap_or(0),
        restart_policy: text(&v["RestartPolicy"], "Name"),
        restart_maximum: v["RestartPolicy"]["MaximumRetryCount"]
            .as_u64()
            .unwrap_or(0),
        auto_remove: v["AutoRemove"] == true,
        stop_timeout: v["StopTimeout"].as_i64(),
        created: text(&v, "Created"),
        started_at: text(s, "StartedAt"),
        finished_at: text(s, "FinishedAt"),
        ports,
        mounts,
        networks,
        updated_at: now(),
        stats: None,
        stats_loading: false,
        stats_error: None,
    })
}

/// Confirmation covers every field that changes the scope/consequences of a delete.
pub fn fingerprint(detail: &ContainerDetail) -> String {
    serde_json::json!([
        detail.id,
        detail.state,
        detail.auto_remove,
        detail.mounts,
        detail.created
    ])
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires Docker CLI and Python, but no running Docker Engine"]
    fn docker_inspect_templates_with_real_cli() {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/docker_inspect_cli.py");
        let output = std::process::Command::new(
            std::env::var("SSH_TEST_PYTHON").unwrap_or_else(|_| "python3".into()),
        )
        .arg(script)
        .arg(FORMAT)
        .arg(super::super::images::FORMAT)
        .output()
        .expect("run Docker CLI inspect fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["images"].as_array().unwrap().len(), 4);
        for image in result["images"].as_array().unwrap() {
            super::super::images::validate_id(image["Id"].as_str().unwrap()).unwrap();
            assert_eq!(image["Size"].as_u64(), Some(9_007_199_254_740_993));
        }
        let containers = result["containers"].as_array().unwrap();
        let expected = [
            (None, None, None),
            (None, None, None),
            (Some("starting"), Some(0), Some(0)),
            (Some("healthy"), Some(0), Some(-1)),
            (Some("unhealthy"), Some(3), Some(10)),
        ];
        assert_eq!(containers.len(), expected.len());
        for (value, (health, streak, timeout)) in containers.iter().zip(expected) {
            let detail = parse(&value.to_string(), &"a".repeat(64)).unwrap();
            assert_eq!(detail.health.as_deref(), health);
            assert_eq!(detail.failing_streak, streak);
            assert_eq!(detail.stop_timeout, timeout);
            assert_eq!(detail.state, "running");
            assert_eq!(detail.ports.len(), 1);
            assert!(detail.ports[0].host_port.is_none());
        }
    }
}
