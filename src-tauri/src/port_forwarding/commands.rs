use super::*;
use crate::{connection::ConnectionAnswer, model::ServerView};
use serde::Serialize;
use tauri::{
    ipc::Channel, Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_opener::OpenerExt;

fn owner(state: &State, window: &WebviewWindow, generation: u64) -> AppResult<WindowOwner> {
    state.ensure_running()?;
    let owner = state.owner(window.label());
    if window.label() != "port-forwarding"
        || owner.generation != generation
        || state
            .closed_windows
            .lock()
            .unwrap()
            .contains(window.label())
    {
        return Err(AppError::new("permission", "管理窗口已失效，请重新打开"));
    }
    Ok(owner)
}
fn read_rule(state: &State, id: &str, revision: u32) -> AppResult<Rule> {
    let rule = state.port_forwarding.with_store(|s| s.get(id))?;
    if rule.revision != revision {
        return Err(AppError::new("conflict", "规则已修改，请刷新"));
    }
    Ok(rule)
}
fn plan(state: &State, rule: &Rule) -> AppResult<Arc<ConnectionPlan>> {
    let path = state
        .database
        .lock()
        .unwrap()
        .resolve_connection(&rule.settings.server_id)?;
    Ok(Arc::new(ConnectionPlan::new(path)?))
}

#[tauri::command]
pub async fn open_port_forward_window(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    server_id: Option<String>,
    seed: Option<ForwardSeed>,
) -> AppResult<()> {
    state.ensure_running()?;
    // Server profiles are safe metadata; independent session windows may preselect theirs.
    if let Some(id) = &server_id {
        state.database.lock().unwrap().get(id)?;
    }
    if let Some(seed) = &seed {
        if server_id.is_none()
            || seed.name.len() > 255
            || seed.target_port == 0
            || seed.target_host.parse::<std::net::IpAddr>().is_err()
        {
            return Err(AppError::new("validation", "转发草稿参数无效"));
        }
    }
    *state.port_forwarding.open_request.lock().unwrap() = Some(OpenRequest {
        id: uuid::Uuid::new_v4().to_string(),
        server_id,
        seed,
    });
    if let Some(existing) = window.app_handle().get_webview_window("port-forwarding") {
        existing
            .unminimize()
            .and_then(|_| existing.show())
            .and_then(|_| existing.set_focus())
            .map_err(|e| AppError::new("window", e))?;
        return Ok(());
    }
    if state
        .port_forwarding
        .window_pending
        .swap(true, Ordering::SeqCst)
    {
        return Ok(());
    }
    state
        .closed_windows
        .lock()
        .unwrap()
        .remove("port-forwarding");
    let result = crate::window_chrome::configure(
        window.app_handle(),
        WebviewWindowBuilder::new(
            window.app_handle(),
            "port-forwarding",
            WebviewUrl::App("index.html?port-forwarding=1".into()),
        ),
    )
    .title("端口转发与代理 · SoDeck")
    .inner_size(1100.0, 760.0)
    .min_inner_size(800.0, 560.0)
    .build();
    state
        .port_forwarding
        .window_pending
        .store(false, Ordering::SeqCst);
    result.map(|_| ()).map_err(|e| AppError::new("window", e))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    generation: u64,
    servers: Vec<ServerView>,
}
#[tauri::command]
pub fn port_forward_bootstrap(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<Bootstrap> {
    let generation = state.owner(window.label()).generation;
    owner(&state, &window, generation)?;
    Ok(Bootstrap {
        generation,
        servers: state
            .database
            .lock()
            .unwrap()
            .list()?
            .into_iter()
            .map(|s| s.view())
            .collect(),
    })
}
#[tauri::command]
pub fn subscribe_port_forwards(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    output: Channel<super::observe::Frame>,
) -> AppResult<String> {
    let _gate = state.configuration.lock().unwrap();
    let owner = owner(&state, &window, generation)?;
    state
        .port_forwarding
        .subscribe(state.inner().clone(), owner, output)
}
#[tauri::command]
pub fn ack_port_forward_frame(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    subscription_id: String,
    revision: u64,
) -> AppResult<()> {
    state.port_forwarding.ack(
        &subscription_id,
        revision,
        &owner(&state, &window, generation)?,
    )
}
#[tauri::command]
pub fn unsubscribe_port_forwards(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    subscription_id: String,
) -> AppResult<()> {
    let owner = owner(&state, &window, generation)?;
    let mut observers = state.port_forwarding.observers.lock().unwrap();
    if observers
        .get(&subscription_id)
        .is_some_and(|s| s.owner.generation == owner.generation)
    {
        if let Some(s) = observers.remove(&subscription_id) {
            s.stop();
        }
    }
    Ok(())
}
#[tauri::command]
pub fn port_forward_summary(state: TauriState<Arc<State>>) -> Summary {
    state.port_forwarding.summary()
}

#[tauri::command]
pub fn save_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    mut rule: Rule,
) -> AppResult<Rule> {
    let _gate = state.configuration.lock().unwrap();
    owner(&state, &window, generation)?;
    rule.validate()?;
    plan(&state, &rule)?;
    state
        .port_forwarding
        .intents
        .lock()
        .unwrap()
        .remove(&rule.id);
    state.port_forwarding.with_store(|s| s.save(rule))
}
#[tauri::command]
pub fn start_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    revision: u32,
) -> AppResult<String> {
    let _gate = state.configuration.lock().unwrap();
    let owner = owner(&state, &window, generation)?;
    let rule = read_rule(&state, &id, revision)?;
    let plan = plan(&state, &rule)?;
    state.port_forwarding.intents.lock().unwrap().remove(&id);
    state
        .port_forwarding
        .start(state.inner().clone(), rule, plan, owner)
}
#[tauri::command]
pub fn stop_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    owner(&state, &window, generation)?;
    state.port_forwarding.intents.lock().unwrap().remove(&id);
    let run = state.port_forwarding.run(&id, &run_id)?;
    run.stop();
    Ok(())
}
#[tauri::command]
pub async fn apply_port_forward(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
    revision: u32,
    delete: bool,
) -> AppResult<()> {
    let state = state.inner().clone();
    let (run, intent, owner) = {
        let _gate = state.configuration.lock().unwrap();
        let owner = owner(&state, &window, generation)?;
        read_rule(&state, &id, revision)?;
        let run = state.port_forwarding.run(&id, &run_id)?;
        let intent = uuid::Uuid::new_v4().to_string();
        state
            .port_forwarding
            .intents
            .lock()
            .unwrap()
            .insert(id.clone(), intent.clone());
        run.stop();
        (run, intent, owner)
    };
    crate::blocking(move || {
        run.wait()?;
        let _gate = state.configuration.lock().unwrap();
        state.ensure_running()?;
        if state.port_forwarding.intents.lock().unwrap().get(&id) != Some(&intent) {
            return Err(AppError::new("conflict", "操作已被后续修改或停止取消"));
        }
        state.port_forwarding.intents.lock().unwrap().remove(&id);
        let rule = read_rule(&state, &id, revision)?;
        if delete {
            state
                .port_forwarding
                .with_store(|s| s.delete(&id, revision))?;
            state.port_forwarding.runs.lock().unwrap().remove(&id);
        } else {
            let plan = plan(&state, &rule)?;
            state
                .port_forwarding
                .start(state.clone(), rule, plan, owner)?;
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub fn delete_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    revision: u32,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    owner(&state, &window, generation)?;
    if state
        .port_forwarding
        .runs
        .lock()
        .unwrap()
        .get(&id)
        .is_some_and(|r| !r.done.load(Ordering::SeqCst))
    {
        return Err(AppError::new("busy", "请先停止此规则再删除"));
    }
    state
        .port_forwarding
        .with_store(|s| s.delete(&id, revision))?;
    state.port_forwarding.runs.lock().unwrap().remove(&id);
    Ok(())
}
#[tauri::command]
pub fn resume_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    let owner = owner(&state, &window, generation)?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    if run.view.lock().unwrap().phase != "attention" {
        return Err(AppError::new("conflict", "当前状态不需要重新认证"));
    }
    run.control(Control::Resume(owner))
}
#[tauri::command]
pub fn answer_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
    attempt_id: String,
    challenge_id: String,
    answer: Option<ConnectionAnswer>,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    let owner = owner(&state, &window, generation)?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    let active_owner = run.owner.lock().unwrap();
    if active_owner.generation != owner.generation || run.stopped.load(Ordering::SeqCst) {
        return Err(AppError::new("challenge", "认证已过期"));
    }
    drop(active_owner);
    let interaction = run.interaction.lock().unwrap();
    let (_, auth) = interaction
        .as_ref()
        .filter(|(id, _)| id == &attempt_id)
        .ok_or_else(|| AppError::new("challenge", "认证尝试已过期"))?;
    if let Some(answer) = answer {
        auth.answer(&challenge_id, answer)?;
    } else {
        let view = run.view.lock().unwrap();
        let challenge_matches = view.prompt.as_ref().is_some_and(|p| match &p.event {
            connection::ConnectionEvent::AuthRequired { challenge } => challenge.id == challenge_id,
            connection::ConnectionEvent::HostKeyRequired { challenge } => {
                challenge.id == challenge_id
            }
            _ => false,
        });
        if !challenge_matches {
            return Err(AppError::new("challenge", "认证提示已过期"));
        }
        run.control(Control::CancelAuth(attempt_id))?;
    }
    Ok(())
}
#[tauri::command]
pub fn test_port_forward(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
    cancel_probe_id: Option<String>,
) -> AppResult<String> {
    let _gate = state.configuration.lock().unwrap();
    owner(&state, &window, generation)?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    if run.rule.settings.service.is_proxy() {
        return Err(AppError::new("validation", "内网代理请使用代理测试"));
    }
    if let Some(id) = cancel_probe_id {
        run.control(Control::CancelProbe(id))?;
        return Ok(String::new());
    }
    let mut view = run.view.lock().unwrap();
    if view.phase != "ready" || view.probe_id.is_some() {
        return Err(AppError::new("busy", "请等待转发就绪或当前测试结束"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    // Reserve before enqueueing so duplicate IPC calls cannot return phantom probes.
    view.probe_id = Some(id.clone());
    drop(view);
    *run.probe_owner.lock().unwrap() = Some(state.owner(window.label()));
    if let Err(error) = run.control(Control::Probe(id.clone())) {
        run.view.lock().unwrap().probe_id = None;
        return Err(error);
    }
    Ok(id)
}
#[tauri::command]
pub fn test_network_proxy(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
    request: proxy::ProbeRequest,
) -> AppResult<String> {
    let _gate = state.configuration.lock().unwrap();
    let owner = owner(&state, &window, generation)?;
    proxy::validate_target(&request.target.host, request.target.port)?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    if !run.rule.settings.service.is_proxy() {
        return Err(AppError::new("validation", "此规则不是内网代理"));
    }
    let mut view = run.view.lock().unwrap();
    if view.phase != "ready" || view.probe_id.is_some() {
        return Err(AppError::new("busy", "请等待代理就绪或当前测试结束"));
    }
    let probe_id = uuid::Uuid::new_v4().to_string();
    view.probe_id = Some(probe_id.clone());
    drop(view);
    *run.probe_owner.lock().unwrap() = Some(owner);
    if let Err(e) = run.control(Control::ProxyProbe(probe_id.clone(), request)) {
        run.view.lock().unwrap().probe_id = None;
        return Err(e);
    }
    Ok(probe_id)
}
#[tauri::command]
pub fn cancel_network_proxy_test(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
    probe_id: String,
) -> AppResult<()> {
    let _gate = state.configuration.lock().unwrap();
    let current = owner(&state, &window, generation)?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    if !run.rule.settings.service.is_proxy()
        || !run
            .probe_owner
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|o| o.label == current.label && o.generation == current.generation)
    {
        return Err(AppError::new("permission", "此测试不属于当前窗口"));
    }
    run.control(Control::CancelProxyProbe(probe_id))
}
#[tauri::command]
pub fn open_port_forward_web(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    generation: u64,
    id: String,
    run_id: String,
) -> AppResult<()> {
    owner(&state, &window, generation)?;
    let rule = state.port_forwarding.with_store(|s| s.get(&id))?;
    if rule.settings.service.is_proxy() {
        return Err(AppError::new("validation", "请在客户端中配置本地代理地址"));
    }
    let scheme = rule
        .web_scheme
        .as_deref()
        .filter(|s| matches!(*s, "http" | "https"))
        .ok_or_else(|| AppError::new("validation", "请先选择 HTTP 或 HTTPS"))?;
    let run = state.port_forwarding.run(&id, &run_id)?;
    let view = run.view.lock().unwrap();
    if view.phase != "ready" || run.stopped.load(Ordering::SeqCst) {
        return Err(AppError::new("disconnected", "转发尚未就绪"));
    }
    let address: std::net::SocketAddr = view
        .actual_address
        .as_deref()
        .unwrap_or("")
        .parse()
        .map_err(|_| AppError::new("validation", "无效的本地监听地址"))?;
    if !address.ip().is_loopback() {
        return Err(AppError::new("validation", "仅允许本地回环地址"));
    }
    window
        .app_handle()
        .opener()
        .open_url(format!("{scheme}://{address}/"), None::<&str>)
        .map_err(|e| AppError::new("open", e))
}
