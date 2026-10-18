use super::{model::*, query};
use crate::{
    model::{AppError, AppResult},
    remote_read, ssh,
};
use serde_json::Value;
pub fn sample(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ContainerStats> {
    validate_id(id)?;
    let result = remote_read::exec(
        session,
        &format!(
            "{} stats --no-stream --no-trunc --format '{{{{json .}}}}' {}",
            query::docker(endpoint),
            ssh::quote(id)
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new(
            "docker",
            super::inspect::clean(&result.stderr, 2000),
        ));
    }
    let v: Value = serde_json::from_str(result.stdout.trim())
        .map_err(|_| AppError::new("format", "资源数据格式不可用"))?;
    if v["ID"].as_str() != Some(id) {
        return Err(AppError::new("identity", "资源采样的容器身份不匹配"));
    }
    let text = |key: &str| v[key].as_str().unwrap_or("—").to_string();
    let percent = |key: &str| {
        v[key]
            .as_str()
            .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    Ok(ContainerStats {
        cpu_percent: percent("CPUPerc"),
        memory: text("MemUsage"),
        memory_percent: percent("MemPerc"),
        network_io: text("NetIO"),
        block_io: text("BlockIO"),
        processes: text("PIDs"),
    })
}
