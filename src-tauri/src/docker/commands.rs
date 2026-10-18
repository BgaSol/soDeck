use super::model::*;
use crate::{connection::WindowOwner, model::AppResult, State};
use std::sync::Arc;
use tauri::{ipc::Channel, State as TauriState, WebviewWindow};
fn owner(state: &State, window: &WebviewWindow) -> AppResult<WindowOwner> {
    crate::moves::workspace_only(window)?;
    state.ensure_running()?;
    Ok(state.owner(window.label()))
}

#[tauri::command]
pub fn subscribe_docker_management(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    events: Channel<Frame>,
) -> AppResult<String> {
    let owner = owner(&state, &window)?;
    state
        .docker
        .subscribe(state.inner(), session_id, owner, events)
}
#[tauri::command]
pub fn update_docker_management_interest(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    container_id: Option<String>,
) -> AppResult<()> {
    state.docker.interest(
        &state,
        &subscription_id,
        &owner(&state, &window)?,
        container_id,
    )
}
#[tauri::command]
pub fn ack_docker_management(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    revision: u64,
) -> AppResult<()> {
    state
        .docker
        .ack(&state, &subscription_id, &owner(&state, &window)?, revision)
}
#[tauri::command]
pub fn unsubscribe_docker_management(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
) -> AppResult<()> {
    state
        .docker
        .unsubscribe(&subscription_id, &state.owner(window.label()));
    Ok(())
}
#[tauri::command]
pub fn refresh_docker_management(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
) -> AppResult<()> {
    state
        .docker
        .refresh(&state, &session_id, &owner(&state, &window)?)
}
#[tauri::command]
pub fn list_docker_operations(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    before: Option<String>,
) -> AppResult<HistoryPage> {
    state.docker.history(
        &state,
        &session_id,
        &owner(&state, &window)?,
        before.as_deref(),
    )
}

#[tauri::command]
pub async fn prepare_docker_operation(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    target_id: String,
    action: Action,
) -> AppResult<Preview> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state.docker.prepare(
            &state,
            &session_id,
            &owner,
            &engine_token,
            &target_id,
            action,
        )
    })
    .await
}
#[tauri::command]
pub fn submit_docker_operation(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    request: OperationRequest,
) -> AppResult<String> {
    state
        .docker
        .submit(state.inner(), &session_id, owner(&state, &window)?, request)
}
#[tauri::command]
pub fn reconcile_docker_operation(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    operation_id: String,
) -> AppResult<()> {
    state.docker.reconcile(
        state.inner(),
        &session_id,
        &owner(&state, &window)?,
        &operation_id,
    )
}
#[tauri::command]
pub fn acknowledge_docker_operation(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    operation_id: String,
) -> AppResult<()> {
    state
        .docker
        .acknowledge(&state, &session_id, &owner(&state, &window)?, &operation_id)
}
#[tauri::command]
pub fn docker_operation_summary(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Summary> {
    crate::main_only(&window)?;
    Ok(state.docker.summary())
}

#[tauri::command]
pub async fn get_docker_container_diagnostics(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    container_id: String,
    section: String,
    reveal: bool,
) -> AppResult<String> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        use crate::{
            connection, model::AppError, remote_read, ssh, ssh_transport::Cancellation,
            traffic::TrafficPurpose,
        };
        validate_id(&container_id)?;
        let spec = state.spec(&session_id, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        if engine.token != engine_token {
            return Err(AppError::new("conflict", "Docker 引擎已变化"));
        }
        let check = || {
            state.spec(&session_id, &owner.label)?;
            if state.owner(&owner.label).generation != owner.generation
                || state.monitor.docker_binding(spec.identity())? != engine
            {
                return Err(AppError::new("permission", "原窗口或引擎已变化"));
            }
            Ok(())
        };
        let connection = connection::connect_verified(
            &spec,
            &Cancellation::default(),
            TrafficPurpose::DockerMonitor,
        )?;
        super::tasks::verify_engine(connection.session(), &engine, &check)?;
        let prefix = super::query::docker(&engine.endpoint);
        let command = match section.as_str() {
            "processes" => format!("{prefix} top {}", ssh::quote(&container_id)),
            "configuration" => format!("{prefix} container inspect {}", ssh::quote(&container_id)),
            _ => return Err(AppError::new("validation", "无效的诊断分组")),
        };
        let result = remote_read::exec(connection.session(), &command, &check)?;
        if result.code != 0 {
            return Err(AppError::new(
                "docker",
                super::inspect::clean(&result.stderr, 2000),
            ));
        }
        let text = if section == "configuration" {
            let mut v: serde_json::Value = serde_json::from_str(&result.stdout)
                .map_err(|_| AppError::new("format", "配置格式不可用"))?;
            if v[0]["Id"] != container_id {
                return Err(AppError::new("identity", "容器身份不匹配"));
            }
            if !reveal {
                if let Some(env) = v[0]["Config"]["Env"].as_array_mut() {
                    for value in env {
                        let key = value.as_str().unwrap_or("").split('=').next().unwrap_or("");
                        *value = format!("{key}=••••••").into();
                    }
                }
                if let Some(labels) = v[0]["Config"]["Labels"].as_object_mut() {
                    for value in labels.values_mut() {
                        *value = "••••••".into();
                    }
                }
                // Raw command args and health output may also contain credentials.
                for key in ["Cmd", "Entrypoint", "Healthcheck"] {
                    if !v[0]["Config"][key].is_null() {
                        v[0]["Config"][key] = "••••••".into();
                    }
                }
                v[0]["Args"] = "••••••".into();
                v[0]["State"]["Health"]["Log"] = serde_json::json!([]);
                let config = &v[0]["Config"];
                v[0] = serde_json::json!({
                    "Id": container_id,
                    "Image": config["Image"], "User": config["User"], "WorkingDir": config["WorkingDir"],
                    "Entrypoint": config["Entrypoint"], "Cmd": config["Cmd"], "Env": config["Env"], "Labels": config["Labels"],
                });
            }
            serde_json::to_string_pretty(&v[0]).unwrap_or_default()
        } else {
            super::inspect::clean(&result.stdout, 128000)
        };
        check()?;
        Ok(text)
    })
    .await
}

#[tauri::command]
pub async fn get_docker_image_detail(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    image_id: String,
) -> AppResult<super::images::ImageDetail> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        read_engine(
            &state,
            &session_id,
            &owner,
            &engine_token,
            |session, engine, check| {
                super::images::detail(session, &engine.endpoint, &image_id, &|| check())
            },
        )
    })
    .await
}
pub(super) fn read_engine<T>(
    state: &State,
    session_id: &str,
    owner: &WindowOwner,
    token: &str,
    read: impl FnOnce(&ssh2::Session, &EngineBinding, &dyn Fn() -> AppResult<()>) -> AppResult<T>,
) -> AppResult<T> {
    use crate::{
        connection, model::AppError, ssh_transport::Cancellation, traffic::TrafficPurpose,
    };
    state.ensure_running()?;
    let spec = state.spec(session_id, &owner.label)?;
    let engine = state.monitor.docker_binding(spec.identity())?;
    if engine.token != token {
        return Err(AppError::new("conflict", "Docker 引擎已变化"));
    }
    let check = || {
        state.ensure_running()?;
        state.spec(session_id, &owner.label)?;
        if state.owner(&owner.label).generation != owner.generation
            || state.monitor.docker_binding(spec.identity())? != engine
        {
            return Err(AppError::new("permission", "原窗口或引擎已变化"));
        }
        Ok(())
    };
    let connection = connection::connect_verified(
        &spec,
        &Cancellation::default(),
        TrafficPurpose::DockerMonitor,
    )?;
    super::tasks::verify_engine(connection.session(), &engine, &check)?;
    let value = read(connection.session(), &engine, &check)?;
    check()?;
    Ok(value)
}

#[tauri::command]
pub async fn get_docker_host_link(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    container_id: String,
    request: super::links::LinkRequest,
) -> AppResult<super::links::HostLink> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        read_engine(
            &state,
            &session_id,
            &owner,
            &engine_token,
            |session, engine, check| {
                super::links::resolve(session, engine, &container_id, &request, &|| check())
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn get_docker_compose_projects(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
) -> AppResult<super::compose::Overview> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let spec = state.spec(&session_id, &owner.label)?;
        read_engine(
            &state,
            &session_id,
            &owner,
            &engine_token,
            |session, engine, check| {
                super::compose::overview(&state, &spec, session, engine, &|| check())
            },
        )
    })
    .await
}
#[tauri::command]
pub async fn save_docker_compose_association(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    mut association: super::compose::Association,
) -> AppResult<super::compose::Association> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let spec = state.spec(&session_id, &owner.label)?;
        let engine = state.monitor.docker_binding(spec.identity())?;
        association.server_id = spec.target().id.clone();
        association.endpoint = engine.endpoint.clone();
        association.daemon_id = engine.daemon_id.clone();
        association.validate()?;
        read_engine(
            &state,
            &session_id,
            &owner,
            &engine_token,
            |session, _, check| {
                // SFTP validates only paths on the SSH host where the Compose CLI reads.
                let sftp = session.sftp()?;
                for path in association.files.iter().chain(association.env_files.iter()) {
                    check()?;
                    if !sftp.stat(std::path::Path::new(path))?.is_file() {
                        return Err(crate::model::AppError::new(
                            "validation",
                            "Compose 或 env 路径不是普通文件",
                        ));
                    }
                }
                if !sftp
                    .stat(std::path::Path::new(&association.directory))?
                    .is_dir()
                {
                    return Err(crate::model::AppError::new(
                        "validation",
                        "工作目录不存在或不可访问",
                    ));
                }
                Ok(())
            },
        )?;
        let _gate = state.configuration.lock().unwrap();
        state.spec(&session_id, &owner.label)?;
        if state.owner(&owner.label).generation != owner.generation
            || state.monitor.docker_binding(spec.identity())? != engine
        {
            return Err(crate::model::AppError::new("conflict", "窗口或引擎已变化"));
        }
        if state.docker.has_server(&spec.target().id) {
            return Err(crate::model::AppError::new(
                "busy",
                "请等待正在进行的 Docker 操作完成后修改关联",
            ));
        }
        state.docker.with_store(|store| {
            if !association.id.is_empty()
                && store.get(&association.id)?.server_id != spec.target().id
            {
                return Err(crate::model::AppError::new(
                    "permission",
                    "此关联不属于当前服务器",
                ));
            }
            if store.list(&spec.target().id)?.iter().any(|a| {
                a.id != association.id
                    && a.endpoint == association.endpoint
                    && a.project == association.project
            }) {
                return Err(crate::model::AppError::new(
                    "conflict",
                    "此项目已有配置关联，请编辑已有记录",
                ));
            }
            let saved = store.save(association)?;
            state.docker.catalog_changed();
            Ok(saved)
        })
    })
    .await
}
#[tauri::command]
pub fn remove_docker_compose_association(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    association_id: String,
    revision: u32,
) -> AppResult<()> {
    let owner = owner(&state, &window)?;
    let _gate = state.configuration.lock().unwrap();
    let spec = state.spec(&session_id, &owner.label)?;
    if state.docker.has_server(&spec.target().id) {
        return Err(crate::model::AppError::new(
            "busy",
            "请等待正在进行的 Docker 操作完成后解除关联",
        ));
    }
    state.docker.with_store(|store| {
        if store.get(&association_id)?.server_id != spec.target().id {
            return Err(crate::model::AppError::new(
                "permission",
                "此关联不属于当前服务器",
            ));
        }
        store.remove(&association_id, revision)?;
        state.docker.catalog_changed();
        Ok(())
    })
}

#[tauri::command]
pub async fn prepare_docker_create(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    specification: super::create::Spec,
) -> AppResult<Preview> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        state
            .docker
            .prepare_create(&state, &session_id, &owner, &engine_token, specification)
    })
    .await
}

#[tauri::command]
pub async fn get_docker_resources(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
) -> AppResult<super::resources::Overview> {
    let owner = owner(&state, &window)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        read_engine(
            &state,
            &session_id,
            &owner,
            &engine_token,
            |session, engine, check| {
                super::resources::overview(session, &engine.endpoint, &|| check())
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn prepare_docker_transfer(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    engine_token: String,
    kind: super::transfer::Kind,
    target_id: String,
    container_path: String,
    conflict: String,
) -> AppResult<Option<Preview>> {
    let owner = owner(&state, &window)?;
    state.spec(&session_id, &owner.label)?;
    super::transfer::validate(&kind, &target_id, &container_path, &conflict)?;
    let state = state.inner().clone();
    crate::blocking(move || {
        let file = if kind.upload() {
            rfd::FileDialog::new().set_title(kind.label()).pick_file()
        } else {
            rfd::FileDialog::new()
                .set_title(kind.label())
                .set_file_name(if matches!(kind, super::transfer::Kind::ImageSave) {
                    "image.tar"
                } else {
                    "container-files.tar"
                })
                .save_file()
        };
        let Some(local) = file else { return Ok(None) };
        let input = super::transfer::selection(kind, target_id, container_path, local, conflict)?;
        state
            .docker
            .prepare_transfer(&state, &session_id, &owner, &engine_token, input)
            .map(Some)
    })
    .await
}
