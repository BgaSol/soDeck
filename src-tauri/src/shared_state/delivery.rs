use super::*;
use crate::AppCore;
use std::time::Duration;
struct Pending {
    target: DeliveryTarget,
    frame: Value,
    deadline: Instant,
    attempts: u8,
    receiver: Option<mpsc::Receiver<Result<Value, String>>>,
}
pub(crate) fn start(core: &Arc<AppCore>, wake: mpsc::Receiver<()>) {
    let core = Arc::downgrade(core);
    std::thread::spawn(move || {
        let mut pending: HashMap<String, Pending> = HashMap::new();
        loop {
            let _ = wake.recv_timeout(Duration::from_millis(25));
            let Some(core) = core.upgrade() else { break };
            let now = Instant::now();
            let plans = {
                let _workspaces = core.workspaces.lock().unwrap();
                let ui = core.component_ui.lock().unwrap();
                core.sync_shared_ui_locked(&ui);
                core.shared_state.deliveries(now)
            };
            pending.retain(|id, _| plans.iter().any(|p| &p.subscription_id == id));
            let mut per_instance: HashMap<String, usize> = HashMap::new();
            for p in pending.values().filter(|p| p.receiver.is_some()) {
                if let DeliveryTarget::Process { instance_id, .. } = &p.target {
                    *per_instance.entry(instance_id.clone()).or_default() += 1;
                }
            }
            for plan in plans {
                let id = &plan.subscription_id;
                if pending.get(id).is_some_and(|p| p.frame != plan.frame) {
                    pending.remove(id);
                }
                if let Some(p) = pending.get_mut(id) {
                    if plan.frame["resetRequired"] == true {
                        p.receiver = None;
                        continue;
                    }
                    if let Some(rx) = &p.receiver {
                        match rx.try_recv() {
                            Ok(Ok(reply))
                                if reply["accepted"] == true
                                    && reply["batchId"] == plan.frame["batchId"]
                                    && reply["part"] == plan.frame["part"] =>
                            {
                                let owner = match &plan.target {
                                    DeliveryTarget::Process {
                                        instance_id,
                                        view_id,
                                    } => {
                                        let ui = core.component_ui.lock().unwrap();
                                        ui.resource_view(view_id).ok().map(|v| Owner {
                                            context: v.context.clone(),
                                            producer: Producer::Plugin {
                                                plugin_id: v.plugin_id.clone(),
                                                plugin_instance_id: instance_id.clone(),
                                                view_id: view_id.clone(),
                                            },
                                            target: plan.target.clone(),
                                        })
                                    }
                                    _ => None,
                                };
                                if let Some(owner) = owner {
                                    if serde_json::to_value(&owner.context).ok().as_ref()
                                        == Some(&plan.frame["context"])
                                    {
                                        if core
                                            .shared_state
                                            .acknowledge(
                                                &owner,
                                                id,
                                                plan.frame["batchId"].as_str().unwrap_or(""),
                                                plan.frame["part"].as_u64().unwrap_or(0) as usize,
                                                reply
                                                    .get("appliedRevision")
                                                    .map(parse_revision)
                                                    .transpose()
                                                    .ok()
                                                    .flatten(),
                                                now,
                                            )
                                            .is_err()
                                        {
                                            core.shared_state.reset_delivery(id, now);
                                        }
                                    }
                                }
                                pending.remove(id);
                                continue;
                            }
                            Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {
                                p.receiver = None;
                                p.deadline = now
                                    + Duration::from_millis(if p.attempts == 1 {
                                        100
                                    } else {
                                        500
                                    });
                            }
                            Err(mpsc::TryRecvError::Empty) => {}
                        }
                    }
                    if now < p.deadline {
                        continue;
                    }
                    if plan.frame["resetRequired"] == true {
                        continue;
                    }
                    if p.attempts >= 3 {
                        core.shared_state.reset_delivery(id, now);
                        pending.remove(id);
                        continue;
                    }
                }
                if pending.len() >= 64 && !pending.contains_key(id) {
                    continue;
                }
                let mut receiver = None;
                match &plan.target {
                    DeliveryTarget::Process { instance_id, .. } => {
                        let count = per_instance.entry(instance_id.clone()).or_default();
                        if *count >= 4 {
                            continue;
                        }
                        *count += 1;
                        let instance = core
                            .instances
                            .lock()
                            .unwrap()
                            .values()
                            .find(|i| i.view().instance_id.as_ref() == Some(instance_id))
                            .cloned();
                        if let Some(instance) = instance {
                            receiver = instance.enqueue_state(plan.frame.clone()).ok();
                        }
                    }
                    DeliveryTarget::Surface {
                        window_id,
                        surface_id,
                    } => core.event(
                        "component-state-changed",
                        json!({"windowId":window_id,"surfaceId":surface_id,"data":plan.frame}),
                    ),
                }
                let attempts = pending.get(id).map_or(1, |p| p.attempts + 1);
                pending.insert(
                    id.clone(),
                    Pending {
                        target: plan.target,
                        frame: plan.frame,
                        deadline: now + Duration::from_secs(5),
                        attempts,
                        receiver,
                    },
                );
            }
        }
    });
}
