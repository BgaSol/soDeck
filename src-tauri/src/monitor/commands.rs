use super::*;
use tauri::{State as TauriState, WebviewWindow};

pub(super) fn validate(interest: &Interest) -> AppResult<()> {
    if interest.mount_paths.len() > 4096
        || interest
            .mount_paths
            .iter()
            .any(|p| !p.starts_with('/') || p.len() > 16384 || p.contains('\0'))
    {
        return Err(AppError::new("validation", "监控挂载点请求超出限制"));
    }
    Ok(())
}
#[tauri::command]
pub fn subscribe_monitor(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    session_id: String,
    events: Channel<MonitorEvent>,
    interest: Interest,
) -> AppResult<String> {
    crate::moves::workspace_only(&window)?;
    validate(&interest)?;
    let spec = state.spec(&session_id, window.label())?;
    state.monitor.subscribe(
        state.inner(),
        spec,
        session_id,
        window.label().into(),
        events,
        interest,
    )
}
#[tauri::command]
pub fn update_monitor_interest(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    interest: Interest,
) -> AppResult<()> {
    validate(&interest)?;
    let entries = state.monitor.entries.lock().unwrap();
    {
        let mut observers = state.monitor.observers.lock().unwrap();
        if let Some(sub) = observers.get_mut(&subscription_id) {
            if sub.window != window.label() || !subscriber_valid(&state, sub) {
                return Err(AppError::new("permission", "此窗口无权访问该监控订阅"));
            }
            sub.interest = interest.clone();
            for entry in entries.values() {
                if let Some(active) = entry
                    .core
                    .lock()
                    .unwrap()
                    .subscribers
                    .get_mut(&subscription_id)
                {
                    active.interest = interest.clone();
                }
            }
            return Ok(());
        }
    }
    drop(entries);
    let entry = state
        .monitor
        .owned(&state, &subscription_id, window.label())?;
    if let Some(sub) = entry
        .core
        .lock()
        .unwrap()
        .subscribers
        .get_mut(&subscription_id)
    {
        sub.interest = interest;
    }
    Ok(())
}
#[tauri::command]
pub fn get_monitor_history(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    minutes: u32,
) -> AppResult<HistoryPage> {
    if !matches!(minutes, 5 | 15 | 60) {
        return Err(AppError::new("validation", "无效的监控时间范围"));
    }
    let entry = state
        .monitor
        .owned(&state, &subscription_id, window.label())?;
    let c = entry.core.lock().unwrap();
    Ok(HistoryPage {
        generation: entry.generation.clone(),
        points: c
            .history
            .since(now().saturating_sub(u64::from(minutes) * 60_000)),
        truncated: c.history.truncated,
        traffic: entry
            .traffic
            .history(now().saturating_sub(u64::from(minutes) * 60_000)),
    })
}
#[tauri::command]
pub fn refresh_monitor(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
    scope: String,
) -> AppResult<()> {
    if !matches!(scope.as_str(), "all" | "metrics" | "mounts" | "docker") {
        return Err(AppError::new("validation", "无效的刷新范围"));
    }
    let entry = state
        .monitor
        .owned(&state, &subscription_id, window.label())?;
    let mut c = entry.core.lock().unwrap();
    if matches!(scope.as_str(), "all" | "metrics") {
        c.refresh = true;
    }
    if matches!(scope.as_str(), "all" | "mounts") {
        c.schedule.refresh();
        for v in c.mounts.usage.values_mut() {
            v.stale = true;
        }
    }
    if matches!(scope.as_str(), "all" | "docker") && !c.docker.refreshing {
        c.docker_refresh += 1;
    }
    drop(c);
    if matches!(scope.as_str(), "all" | "mounts") {
        entry.publish_mounts(&state);
    }
    Ok(())
}
#[tauri::command]
pub fn unsubscribe_monitor(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    subscription_id: String,
) -> AppResult<()> {
    // Unsubscribe remains idempotent after ownership moves or the session closes.
    let entries = state.monitor.entries.lock().unwrap();
    state
        .monitor
        .observers
        .lock()
        .unwrap()
        .retain(|id, sub| id != &subscription_id || sub.window != window.label());
    for entry in entries.values() {
        let mut c = entry.core.lock().unwrap();
        if c.subscribers
            .get(&subscription_id)
            .is_some_and(|s| s.window == window.label())
        {
            c.subscribers.remove(&subscription_id);
        }
    }
    Ok(())
}
