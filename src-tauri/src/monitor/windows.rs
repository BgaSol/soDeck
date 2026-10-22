use super::*;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPreferences {
    #[serde(default = "default_minutes")]
    minutes: u32,
    #[serde(default)]
    network_interface: String,
    #[serde(default)]
    selected_metric: String,
}
fn default_minutes() -> u32 {
    5
}

#[derive(Clone)]
pub(super) struct WindowBinding {
    pub identity: String,
    pub server_name: String,
    pub preferences: WindowPreferences,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    server_name: String,
    preferences: WindowPreferences,
}

#[tauri::command]
pub async fn open_monitor_window(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    mut preferences: WindowPreferences,
) -> AppResult<()> {
    crate::moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    if !matches!(preferences.minutes, 5 | 15 | 60) {
        preferences.minutes = 5;
    }
    if !matches!(
        preferences.selected_metric.as_str(),
        "cpu" | "memory" | "network"
    ) {
        preferences.selected_metric = "cpu".into();
    }
    if preferences.network_interface.len() > 256 {
        return Err(AppError::new("validation", "无效的网卡名称"));
    }
    let (label, create) = {
        let mut windows = state.monitor.windows.lock().unwrap();
        if let Some((label, _)) = windows
            .iter()
            .find(|(_, binding)| binding.identity == spec.identity())
        {
            (label.clone(), false)
        } else {
            let label = format!("monitor-{}", uuid::Uuid::new_v4());
            windows.insert(
                label.clone(),
                WindowBinding {
                    identity: spec.identity().into(),
                    server_name: spec.target().name.clone(),
                    preferences,
                },
            );
            (label, true)
        }
    };
    if !create {
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.unminimize();
            existing
                .set_focus()
                .map_err(|e| AppError::new("window", e))?;
        }
        return Ok(());
    }
    let result = crate::window_chrome::configure(
        &app,
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html?monitor=1".into())),
    )
    .title(crate::brand::window_title(
        &app,
        &format!("{} · 监控", spec.target().name),
    ))
    .inner_size(1080.0, 720.0)
    .min_inner_size(420.0, 400.0)
    .build();
    if let Err(error) = result {
        state.monitor.close_monitor_window(&label);
        return Err(AppError::new("window", error));
    }
    Ok(())
}

#[tauri::command]
pub fn monitor_window_info(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<WindowInfo> {
    let binding = state.monitor.window_binding(window.label())?;
    Ok(WindowInfo {
        server_name: binding.server_name,
        preferences: binding.preferences,
    })
}

#[tauri::command]
pub fn subscribe_monitor_window(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    events: Channel<MonitorEvent>,
    interest: Interest,
) -> AppResult<String> {
    commands::validate(&interest)?;
    state
        .monitor
        .subscribe_window(&state, window.label(), events, interest)
}

impl MonitorManager {
    fn window_binding(&self, window: &str) -> AppResult<WindowBinding> {
        self.windows
            .lock()
            .unwrap()
            .get(window)
            .cloned()
            .ok_or_else(|| AppError::new("permission", "此监控窗口的目标已失效"))
    }

    fn subscribe_window(
        &self,
        state: &State,
        window: &str,
        output: Channel<MonitorEvent>,
        interest: Interest,
    ) -> AppResult<String> {
        let _gate = state.configuration.lock().unwrap();
        state.ensure_running()?;
        // Registry order is entries -> observers. No native window call runs under these locks.
        let entries = self.entries.lock().unwrap();
        let binding = self.window_binding(window)?;
        let id = uuid::Uuid::new_v4().to_string();
        let subscriber = Subscriber {
            session: String::new(),
            identity: Some(binding.identity.clone()),
            window: window.into(),
            window_generation: state.owner(window).generation,
            output: output.clone(),
            interest,
        };
        self.observers
            .lock()
            .unwrap()
            .insert(id.clone(), subscriber.clone());
        if let Some(entry) = entries.get(&binding.identity) {
            let mut core = entry.core.lock().unwrap();
            core.subscribers.insert(id.clone(), subscriber);
            let _ = output.send(MonitorEvent::Connection {
                generation: entry.generation.clone(),
                connected: true,
            });
            let _ = output.send(entry.snapshot(&core));
        } else {
            let _ = output.send(MonitorEvent::Connection {
                generation: String::new(),
                connected: false,
            });
        }
        Ok(id)
    }

    pub fn close_monitor_window(&self, window: &str) {
        // Removing the target and subscribers under the registry lock prevents
        // a late subscribe command from recreating an observer after close.
        let entries = self.entries.lock().unwrap();
        self.windows.lock().unwrap().remove(window);
        self.observers
            .lock()
            .unwrap()
            .retain(|_, sub| sub.window != window);
        for entry in entries.values() {
            entry
                .core
                .lock()
                .unwrap()
                .subscribers
                .retain(|_, sub| sub.window != window);
        }
    }

    pub(super) fn attach_observers(&self, state: &State, identity: &str, entry: &Entry) {
        let observers = self.observers.lock().unwrap().clone();
        let mut core = entry.core.lock().unwrap();
        for (id, subscriber) in observers {
            if subscriber.identity.as_deref() != Some(identity)
                || !subscriber_valid(state, &subscriber)
                || core.subscribers.contains_key(&id)
            {
                continue;
            }
            let _ = subscriber.output.send(MonitorEvent::Connection {
                generation: entry.generation.clone(),
                connected: true,
            });
            let _ = subscriber.output.send(entry.snapshot(&core));
            core.subscribers.insert(id, subscriber);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(state: &State, label: &str, identity: &str) {
        state.monitor.windows.lock().unwrap().insert(
            label.into(),
            WindowBinding {
                identity: identity.into(),
                server_name: "Test server".into(),
                preferences: WindowPreferences::default(),
            },
        );
    }

    fn collector() -> (Channel<MonitorEvent>, Arc<Mutex<Vec<serde_json::Value>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        (
            Channel::new(move |body| {
                if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                    sink.lock().unwrap().push(serde_json::from_str(&json)?);
                }
                Ok(())
            }),
            events,
        )
    }

    #[test]
    fn observer_requires_registered_window_and_shares_the_existing_entry() {
        let state = crate::connection::tests::state();
        let (output, _) = collector();
        assert!(state
            .monitor
            .subscribe_window(&state, "main", output, Interest::default())
            .is_err());
        register(&state, "monitor-one", "identity-a");
        let entry = Arc::new(Entry::new(
            state.monitor.jobs.clone(),
            state.traffic.account("identity-a"),
        ));
        state
            .monitor
            .entries
            .lock()
            .unwrap()
            .insert("identity-a".into(), entry.clone());
        let (output, received) = collector();
        let id = state
            .monitor
            .subscribe_window(&state, "monitor-one", output, Interest::default())
            .unwrap();
        assert!(Arc::ptr_eq(
            &entry,
            &state.monitor.owned(&state, &id, "monitor-one").unwrap()
        ));
        assert!(state.monitor.owned(&state, &id, "main").is_err());
        assert_eq!(state.monitor.entries.lock().unwrap().len(), 1);
        assert_eq!(state.monitor.jobs.load(Ordering::SeqCst), 0);
        assert_eq!(received.lock().unwrap()[0]["connected"], true);
        state.monitor.close_monitor_window("monitor-one");
        assert!(state.monitor.observers.lock().unwrap().is_empty());
        assert!(entry.core.lock().unwrap().subscribers.is_empty());
        assert!(!entry.cancel.is_cancelled());
    }

    #[test]
    fn observer_survives_disconnect_but_only_reattaches_to_its_identity() {
        let state = crate::connection::tests::state();
        register(&state, "monitor-one", "identity-a");
        let (output, received) = collector();
        let id = state
            .monitor
            .subscribe_window(&state, "monitor-one", output, Interest::default())
            .unwrap();
        assert_eq!(received.lock().unwrap()[0]["connected"], false);
        let entry = Arc::new(Entry::new(
            state.monitor.jobs.clone(),
            state.traffic.account("identity-a"),
        ));
        state.monitor.attach_observers(&state, "identity-b", &entry);
        assert!(entry.core.lock().unwrap().subscribers.is_empty());
        state.monitor.attach_observers(&state, "identity-a", &entry);
        assert!(entry.core.lock().unwrap().subscribers.contains_key(&id));
        state
            .monitor
            .entries
            .lock()
            .unwrap()
            .insert("identity-a".into(), entry.clone());
        state.monitor.close_session("last-session");
        assert!(entry.cancel.is_cancelled());
        assert!(state.monitor.entries.lock().unwrap().is_empty());
        assert!(state.monitor.observers.lock().unwrap().contains_key(&id));
        assert_eq!(received.lock().unwrap().last().unwrap()["connected"], false);
        let next = Arc::new(Entry::new(
            state.monitor.jobs.clone(),
            state.traffic.account("identity-a"),
        ));
        state.monitor.attach_observers(&state, "identity-a", &next);
        assert!(next.core.lock().unwrap().subscribers.contains_key(&id));
        assert_ne!(next.generation, entry.generation);
        state.monitor.stop_window("monitor-one");
        assert!(state.monitor.observers.lock().unwrap().is_empty());
    }

    #[test]
    fn reloaded_window_cannot_use_old_subscription() {
        let state = crate::connection::tests::state();
        register(&state, "monitor-one", "identity-a");
        let entry = Arc::new(Entry::new(
            state.monitor.jobs.clone(),
            state.traffic.account("identity-a"),
        ));
        state
            .monitor
            .entries
            .lock()
            .unwrap()
            .insert("identity-a".into(), entry);
        let (output, _) = collector();
        let id = state
            .monitor
            .subscribe_window(&state, "monitor-one", output, Interest::default())
            .unwrap();
        state
            .window_generations
            .lock()
            .unwrap()
            .insert("monitor-one".into(), 1);
        assert!(state.monitor.owned(&state, &id, "monitor-one").is_err());
        state.monitor.stop_window("monitor-one");
        assert!(state.monitor.window_binding("monitor-one").is_ok());
        assert!(state.monitor.observers.lock().unwrap().is_empty());
    }
}
