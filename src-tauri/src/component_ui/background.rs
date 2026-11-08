//! Logical mounts run independently of browser surfaces, with two bounded workers.
use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
pub(crate) fn start_background(core: &Arc<AppCore>) {
    let (tx, rx) = mpsc::sync_channel(1);
    *core.background_wake.lock().unwrap() = Some(tx);
    let weak = Arc::downgrade(core);
    std::thread::spawn(move || {
        let workers = Arc::new(AtomicUsize::new(0));
        let (done, completed) = mpsc::sync_channel::<(String, bool)>(2);
        let mut mounts: HashMap<String, (u8, Option<Instant>)> = HashMap::new();
        loop {
            let _ = rx.recv_timeout(Duration::from_secs(1));
            let Some(core) = weak.upgrade() else { break };
            // One existing monitor handles all close deadlines, including global
            // closing. Repeated closes must not create sleeping watchdog threads.
            core.expire_close_inputs(Instant::now());
            if core.is_closing() {
                continue;
            }
            core.executions.reap();
            if !core.snapshot().plugins.iter().any(|p| {
                p.status == PluginStatus::Running
                    && p.manifest
                        .ui
                        .as_ref()
                        .is_some_and(|u| u.views.iter().any(|v| v.background_on_workspace))
            }) {
                continue;
            }
            while let Ok((key, ok)) = completed.try_recv() {
                if let Some((attempt, next)) = mounts.get_mut(&key) {
                    *next = if ok || *attempt >= 3 {
                        None
                    } else {
                        Some(
                            Instant::now() + Duration::from_secs(if *attempt == 1 { 1 } else { 5 }),
                        )
                    };
                }
            }
            let candidates: Vec<_> = {
                let ui = core.component_ui.lock().unwrap();
                ui.workspaces
                    .values()
                    .filter(|w| {
                        !w.views
                            .first()
                            .is_some_and(|v| ui.frozen(&v.context.workspace_id))
                    })
                    .flat_map(|w| &w.views)
                    .filter(|v| {
                        ui.definitions
                            .get(&v.definition_key)
                            .is_some_and(|d| d.available && d.definition.background_on_workspace)
                    })
                    .cloned()
                    .collect()
            };
            let key =
                |v: &View| format!("{}/{}/{}", v.plugin_instance_id, v.id, v.context.generation);
            let live: HashSet<_> = candidates.iter().map(&key).collect();
            mounts.retain(|k, _| live.contains(k));
            for view in candidates {
                if workers.load(Ordering::Acquire) >= 2 {
                    break;
                }
                let k = key(&view);
                let entry = mounts.entry(k.clone()).or_insert((0, Some(Instant::now())));
                if !entry.1.is_some_and(|at| at <= Instant::now()) {
                    continue;
                }
                entry.0 += 1;
                entry.1 = None;
                workers.fetch_add(1, Ordering::AcqRel);
                let core = Arc::downgrade(&core);
                let done = done.clone();
                let workers = workers.clone();
                std::thread::spawn(move || {
                    let ok = core.upgrade().is_some_and(|c| {
                        let response = c.ui_plugin_call(&view, "ui.mount", Value::Null);
                        let mut ui = c.component_ui.lock().unwrap();
                        if !ui.view(&view.id).is_ok_and(|v| {
                            v.plugin_instance_id == view.plugin_instance_id
                                && v.context == view.context
                        }) {
                            return false;
                        }
                        match response {
                            Ok(data) => {
                                if let Some(v) = ui
                                    .workspaces
                                    .values_mut()
                                    .flat_map(|w| &mut w.views)
                                    .find(|v| v.id == view.id)
                                {
                                    v.error = None;
                                }
                                ui.update_metadata(&view.id, &data["metadata"]).is_ok()
                            }
                            Err(e) => {
                                if let Some(v) = ui
                                    .workspaces
                                    .values_mut()
                                    .flat_map(|w| &mut w.views)
                                    .find(|v| v.id == view.id)
                                {
                                    v.error = Some(e);
                                }
                                false
                            }
                        }
                    });
                    let _ = done.send((k, ok));
                    workers.fetch_sub(1, Ordering::AcqRel);
                    if let Some(c) = core.upgrade() {
                        c.wake_background();
                    }
                });
            }
        }
    });
}
impl AppCore {
    pub(crate) fn wake_background(&self) {
        if let Some(tx) = self.background_wake.lock().unwrap().as_ref() {
            let _ = tx.try_send(());
        }
    }
}
