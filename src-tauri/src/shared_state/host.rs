use super::*;
use crate::{component_ui::Runtime, AppCore};

impl AppCore {
    pub(crate) fn sync_shared_ui_locked(&self, ui: &Runtime) {
        if let Err(error) = self.shared_state.sync_ui(ui, Instant::now()) {
            self.log(
                None,
                "warn",
                "共享状态协调未完成",
                Some(json!({"code":error.code,"error":error.message})),
            );
        }
    }
    pub(crate) fn state_process_request(
        &self,
        plugin: &str,
        instance: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value, Error> {
        let view_id = text(params, "viewId")?;
        let expected: Context = serde_json::from_value(params["expectedContext"].clone())
            .map_err(|_| Error::invalid("缺少有效 expectedContext"))?;
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *_workbench;

        workspaces
            .validate_identity(&expected)
            .map_err(|e| Error::new("staleContext", e))?;
        let view = ui
            .resource_view(view_id)
            .map_err(|e| Error::new("sourceStopped", e))?;
        if view.plugin_id != plugin
            || view.plugin_instance_id != instance
            || view.context != expected
        {
            return Err(Error::new("staleContext", "来源或 Context 已改变"));
        }
        self.sync_shared_ui_locked(&ui);
        let owner = Owner {
            context: expected,
            producer: Producer::Plugin {
                plugin_id: plugin.into(),
                plugin_instance_id: instance.into(),
                view_id: view_id.into(),
            },
            target: DeliveryTarget::Process {
                instance_id: instance.into(),
                view_id: view_id.into(),
            },
        };
        self.state_dispatch(&owner, method.trim_start_matches("state."), params)
    }
    pub fn state_surface_request(
        &self,
        window: &str,
        surface: &str,
        params: &Value,
    ) -> Result<Value, Error> {
        let expected: Context = serde_json::from_value(params["expectedContext"].clone())
            .map_err(|_| Error::invalid("缺少有效 expectedContext"))?;
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *_workbench;

        let s = ui
            .surfaces
            .get(surface)
            .filter(|s| s.window == window && s.ready)
            .ok_or_else(|| Error::new("staleContext", "界面尚未就绪或挂载已失效"))?;
        let view = ui
            .resource_view(&s.view)
            .map_err(|e| Error::new("sourceStopped", e))?;
        if view.context != expected {
            return Err(Error::new("staleContext", "界面 Context 已改变"));
        }
        // During a pending move the original owner can still consume/ACK facts.
        let owned = workspaces.contains(window, &expected.workspace_id)
            || ui
                .transfers
                .values()
                .any(|t| t.movement.target == window && t.views.contains(&view.id));
        if !owned {
            return Err(Error::new("staleContext", "工作区已移出此窗口"));
        }
        if ["put", "remove"].contains(&params["op"].as_str().unwrap_or_default())
            && s.phase != crate::component_ui::SurfacePhase::Active
        {
            return Err(Error::new("staleContext", "候选或交接中的界面不能发布状态"));
        }
        self.sync_shared_ui_locked(&ui);
        let owner = Owner {
            context: expected,
            producer: Producer::Plugin {
                plugin_id: view.plugin_id.clone(),
                plugin_instance_id: view.plugin_instance_id.clone(),
                view_id: view.id.clone(),
            },
            target: DeliveryTarget::Surface {
                window_id: window.into(),
                surface_id: surface.into(),
            },
        };
        self.state_dispatch(&owner, text(params, "op")?, params)
    }
    fn state_dispatch(&self, owner: &Owner, op: &str, p: &Value) -> Result<Value, Error> {
        let now = Instant::now();
        let hub = &self.shared_state;
        match op {
            "get" => {
                let producer = p
                    .get("producer")
                    .map(|p| {
                        serde_json::from_value(p.clone())
                            .map_err(|_| Error::invalid("producer 无效"))
                    })
                    .transpose()?;
                hub.get(
                    owner,
                    text(p, "topic")?,
                    text(p, "key")?,
                    producer.as_ref(),
                    now,
                )
            }
            "put" => hub.put(
                owner,
                text(p, "topic")?,
                text(p, "key")?,
                parse_revision(&p["expectedRevision"])?,
                p.get("value")
                    .cloned()
                    .ok_or_else(|| Error::invalid("缺少 value"))?,
                crate::core::now(),
                now,
            ),
            "remove" => hub.remove(
                owner,
                text(p, "topic")?,
                text(p, "key")?,
                parse_revision(&p["expectedRevision"])?,
                crate::core::now(),
                now,
            ),
            "list" if p.get("cursor").is_some() => {
                hub.page(owner, text(p, "cursor")?, p["release"] == true, now)
            }
            "list" => hub.list(owner, parse_filter(p)?, now),
            "subscribe" => hub.subscribe(
                owner,
                text(p, "clientSubscriptionId")?,
                parse_filter(p)?,
                p.get("acknowledgedRevision")
                    .map(parse_revision)
                    .transpose()?,
                now,
            ),
            "unsubscribe" => {
                hub.unsubscribe(
                    owner,
                    p["subscriptionId"].as_str(),
                    p["clientSubscriptionId"].as_str(),
                    now,
                )?;
                Ok(json!({"released":true}))
            }
            "ack" => hub.acknowledge(
                owner,
                text(p, "subscriptionId")?,
                text(p, "batchId")?,
                p["part"]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| Error::invalid("part 无效"))?,
                p.get("appliedRevision").map(parse_revision).transpose()?,
                now,
            ),
            _ => Err(Error::new("methodNotFound", "未知共享状态操作")),
        }
    }
}
fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str, Error> {
    p[key]
        .as_str()
        .ok_or_else(|| Error::invalid(format!("缺少 {key}")))
}
fn parse_filter(p: &Value) -> Result<Filter, Error> {
    serde_json::from_value(p["filter"].clone()).map_err(|_| Error::invalid("filter 无效"))
}
