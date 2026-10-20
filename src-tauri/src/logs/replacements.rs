use super::*;
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replacement {
    pub id: String,
    pub name: String,
    pub created: String,
    pub project: String,
    pub service: String,
    pub instance: String,
}
fn identity(v: &serde_json::Value) -> Option<(String, String, String)> {
    let get = |key: &str| v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    Some((
        get("com.docker.compose.project")?,
        get("com.docker.compose.service")?,
        get("com.docker.compose.container-number")?,
    ))
}
pub(super) fn discover(
    spec: &SharedContext,
    target: &crate::docker::ContainerTarget,
    root: &std::path::Path,
    source: &Source,
) -> AppResult<Vec<Replacement>> {
    let Some(labels) = lifecycle::labels(root, &source.source_id)? else {
        return Ok(vec![]);
    };
    let Some((project, service, instance)) = identity(&labels) else {
        return Ok(vec![]);
    };
    let Some(expected) = lifecycle::metadata(root, &source.source_id, "daemonId")? else {
        return Err(AppError::new(
            "logs",
            "尚未记录引擎身份，请从 Docker 列表打开新容器",
        ));
    };
    let cancel = Cancellation::default();
    let connection =
        connection::connect_verified(spec, &cancel, crate::traffic::TrafficPurpose::Logs)?;
    let session = connection.session();
    let prefix = format!(
        "env -u DOCKER_CONTEXT -u DOCKER_HOST docker --host {}",
        ssh::quote(&target.endpoint)
    );
    let daemon = ssh::exec(
        session,
        &format!("{prefix} info --format '{{{{json .ID}}}}'"),
    )?;
    if daemon.code != 0 {
        return Err(AppError::new("docker", daemon.stderr));
    }
    let current: String =
        serde_json::from_str(daemon.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
    if current != expected {
        return Err(AppError::new(
            "logs",
            "Docker 引擎身份已变化，请重新连接后选择目标容器",
        ));
    }
    let filters = [
        ("com.docker.compose.project", &project),
        ("com.docker.compose.service", &service),
        ("com.docker.compose.container-number", &instance),
    ]
    .into_iter()
    .map(|(k, v)| format!("--filter {}", ssh::quote(&format!("label={k}={v}"))))
    .collect::<Vec<_>>()
    .join(" ");
    let found = ssh::exec(
        session,
        &format!("{prefix} container ls -a --no-trunc {filters} --format '{{{{json .ID}}}}'"),
    )?;
    if found.code != 0 {
        return Err(AppError::new("docker", found.stderr));
    }
    let mut result = Vec::new();
    for line in found.stdout.lines().take(50) {
        let id: String = serde_json::from_str(line).map_err(|e| AppError::new("docker", e))?;
        validate_id(&id)?;
        if id == target.id {
            continue;
        }
        let inspect = ssh::exec(
            session,
            &format!(
                "{prefix} inspect --type container --format '{{{{json .}}}}' {}",
                ssh::quote(&id)
            ),
        )?;
        if inspect.code != 0 {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(inspect.stdout.trim()).map_err(|e| AppError::new("docker", e))?;
        if identity(&v["Config"]["Labels"])
            != Some((project.clone(), service.clone(), instance.clone()))
        {
            continue;
        }
        result.push(Replacement {
            id,
            name: v["Name"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('/')
                .into(),
            created: v["Created"].as_str().unwrap_or("").into(),
            project: project.clone(),
            service: service.clone(),
            instance: instance.clone(),
        });
    }
    Ok(result)
}
fn context(
    state: &State,
    label: &str,
) -> AppResult<(SharedContext, crate::docker::ContainerTarget, Source)> {
    let windows = state.logs.windows.lock().unwrap();
    let w = windows
        .get(label)
        .ok_or_else(|| AppError::new("window", "日志窗口已失效"))?;
    Ok((
        w.spec
            .clone()
            .ok_or_else(|| AppError::new("logs", "请先连接服务器后从 Docker 列表恢复采集"))?,
        w.target
            .clone()
            .ok_or_else(|| AppError::new("logs", "没有在线容器来源"))?,
        w.source.clone(),
    ))
}
#[tauri::command]
pub async fn logs_replacements(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<Vec<Replacement>> {
    let (spec, target, source) = context(&state, window.label())?;
    let root = root(&app)?;
    crate::blocking(move || discover(&spec, &target, &root, &source)).await
}
#[tauri::command]
pub async fn open_log_replacement(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    container_id: String,
) -> AppResult<()> {
    validate_id(&container_id)?;
    let (spec, target, source) = context(&state, window.label())?;
    let root = root(&app)?;
    let discovery = spec.clone();
    let endpoint = target.endpoint.clone();
    let next = crate::blocking(move || {
        discover(&discovery, &target, &root, &source)?
            .into_iter()
            .find(|v| v.id == container_id)
            .ok_or_else(|| AppError::new("logs", "替代容器已变化，请重新查找"))
    })
    .await?;
    let target = crate::docker::ContainerTarget {
        id: next.id,
        name: next.name,
        endpoint,
    };
    let source = Source::new(&spec, &target);
    open_window(&app, &state, source, Some(spec), Some(target))
}
