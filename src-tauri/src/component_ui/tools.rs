//! Host-owned tool slots and local layout receipts; no plugin calls.
use super::*;
use std::time::{Duration, Instant};

pub(crate) struct ToolReceipt {
    fingerprint: Vec<u8>,
    pub window: String,
    pub context: Context,
    pub host: String,
    pub revision: u64,
    pub result: Value,
    ended_at: Instant,
}
impl ToolReceipt {
    pub fn expired(&self) -> bool {
        self.ended_at.elapsed() > Duration::from_secs(300)
    }
    pub fn wire(&self) -> Value {
        let mut result = self.result.clone();
        if self.expired() {
            result["status"] = json!("expired");
        }
        result
    }
}
impl Runtime {
    pub(crate) fn seed_tool_slots(&mut self, workspace: &str, window: &str, server: &str) {
        let Some(w) = self.workspaces.get_mut(workspace) else {
            return;
        };
        let mut definitions: Vec<_> = self
            .definitions
            .values()
            .filter(|d| d.available && d.definition.container == "sidebar")
            .collect();
        definitions.sort_by_key(|d| {
            (
                w.preferences
                    .get(&d.key)
                    .map(|p| p.order)
                    .unwrap_or(d.definition.startup_order),
                &d.key,
            )
        });
        for d in definitions {
            if w.hosts.values().any(|h| h.tool_side(&d.key).is_some())
                || w.views
                    .iter()
                    .any(|v| v.definition_key == d.key && w.view_window(&v.id).is_some())
            {
                continue;
            }
            if self
                .memory
                .tool_slot(server, &d.key)
                .is_some_and(|slot| w.hosts.values().all(|h| h.layout_slot != slot))
            {
                continue;
            }
            let side = w
                .preferences
                .get(&d.key)
                .map(|p| p.side.as_str())
                .unwrap_or(&d.definition.default_side);
            if let Some(host) = w.hosts.get_mut(window) {
                if host.left.entries.len() + host.right.entries.len() >= 128 {
                    continue;
                }
                let tools = if side == "left" {
                    &mut host.left
                } else {
                    &mut host.right
                };
                tools.entries.push(crate::workbench_layout::ToolSlot {
                    definition_key: d.key.clone(),
                    view_id: None,
                });
                host.revision += 1;
            }
        }
    }
    pub(crate) fn tool_projection(&self, w: &WorkspaceUi) -> Value {
        json!(self.definitions.values().filter(|d| d.definition.container == "sidebar").filter_map(|d| {
            let placed = w.hosts.iter().find_map(|(window, host)| {
                for (side, tools) in [("left", &host.left), ("right", &host.right)] {
                    if let Some((order, entry)) = tools.entries.iter().enumerate().find(|(_, e)| e.definition_key == d.key) {
                        return Some((entry.view_id.clone(), json!({"windowId":window,"hostId":host.id,"side":side,"order":order}), entry.view_id.is_some() && entry.view_id == tools.active_view_id));
                    }
                }
                w.views.iter().find(|v| v.definition_key == d.key && host.contains(&v.id)).map(|v| (Some(v.id.clone()), json!({"windowId":window,"hostId":host.id,"groupId":host.group_of(&v.id)}), host.active_view().as_ref() == Some(&v.id)))
            });
            let (view, location, active) = placed.unwrap_or((None, Value::Null, false));
            Some(json!({"key":d.key,"viewId":view,"location":location,"active":active,"available":d.available,"pinned":w.preferences.get(&d.key).is_some_and(|p|p.pinned)}))
        }).collect::<Vec<_>>())
    }
}
impl AppCore {
    pub(crate) fn ui_move_tool(
        &self,
        window: &str,
        context: &Context,
        action: Value,
    ) -> Result<Value, String> {
        let request = action["requestId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or("移动请求 ID 无效")?
            .to_owned();
        use sha2::{Digest, Sha256};
        let fingerprint = Sha256::digest(
            serde_json::to_vec(&json!([window, context, action])).map_err(|e| e.to_string())?,
        )
        .to_vec();
        let mut state = self.workbench_state();
        if let Some(receipt) = state.ui.tool_receipts.get(&request) {
            if receipt.fingerprint != fingerprint {
                return Err("移动 requestId 已用于不同目标或参数".into());
            }
            return Ok(receipt.wire());
        }
        if state.ui.move_receipts.contains_key(&request) {
            return Err("移动 requestId 已被占用".into());
        }
        if state.ui.move_receipts.len() + state.ui.tool_receipts.len() >= 4096 {
            return Err("移动请求记录已满".into());
        }
        let outcome = (|| -> Result<(String, u64), String> {
            state.workspaces.validate_owned(window, context)?;
            let ui = &mut state.ui;
            if self.is_closing() || ui.frozen(&context.workspace_id) {
                return Err("工作区正在关闭".into());
            }
            let key = action["key"].as_str().ok_or("缺少工具 key")?;
            let d = ui
                .definitions
                .get(key)
                .filter(|d| d.available && d.definition.container == "sidebar")
                .ok_or("只有可用工具可以移动")?;
            let w = ui
                .workspaces
                .get(&context.workspace_id)
                .ok_or("工作台已关闭")?;
            let mut host = w.host(window)?.clone();
            let source = host
                .tool_side(key)
                .ok_or("工具位置已改变，请重新读取")?
                .to_owned();
            if action["revision"].as_u64() != Some(host.revision) {
                return Err("布局已改变，请重新读取".into());
            }
            let side = action["side"].as_str().ok_or("缺少停靠侧")?;
            let view = [&host.left, &host.right]
                .into_iter()
                .flat_map(|t| &t.entries)
                .find(|e| e.definition_key == key)
                .and_then(|e| e.view_id.as_deref());
            if let Some(id) = view {
                let v = ui.resource_view(id)?;
                if &v.context != context || v.plugin_instance_id != d.plugin_instance_id {
                    return Err("工具实例或连接已改变".into());
                }
            }
            ui.layout_admission(&context.workspace_id, window, None, view, Some(&source))?;
            ui.layout_admission(&context.workspace_id, window, None, view, Some(side))?;
            if !action["beforeKey"].is_null() && !action["beforeKey"].is_string() {
                return Err("工具顺序参数无效".into());
            }
            host.move_tool(key, side, action["beforeKey"].as_str())?;
            let committed = (host.id.clone(), host.revision);
            ui.workspaces
                .get_mut(&context.workspace_id)
                .unwrap()
                .hosts
                .insert(window.into(), host);
            Ok(committed)
        })();
        let (host, revision, result) = match outcome {
            Ok((host, revision)) => (
                host,
                revision,
                json!({"id":request,"status":"completed","revision":revision,"persistence":"pending"}),
            ),
            Err(error) => (
                String::new(),
                0,
                json!({"id":request,"status":"failed","error":error}),
            ),
        };
        let completed = result["status"] == "completed";
        state.ui.tool_receipts.insert(
            request.clone(),
            ToolReceipt {
                fingerprint,
                window: window.into(),
                context: context.clone(),
                host,
                revision,
                result,
                ended_at: Instant::now(),
            },
        );
        self.sync_shared_ui_locked(&state.ui);
        drop(state);
        if completed {
            self.workspace_changed();
        }
        Ok(self.ui_state().tool_receipts[&request].wire())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_receipts_keep_their_original_result_and_do_not_become_new_operations() {
        let receipt = ToolReceipt {
            fingerprint: vec![],
            window: "main".into(),
            context: Context {
                workspace_id: "w".into(),
                server_id: "s".into(),
                connection_id: "c".into(),
                generation: 1,
            },
            host: "h".into(),
            revision: 4,
            result: json!({"status":"completed","revision":4,"persistence":"saved"}),
            ended_at: Instant::now() - Duration::from_secs(301),
        };
        assert!(receipt.expired());
        assert_eq!(receipt.wire()["status"], "expired");
        assert_eq!(receipt.wire()["revision"], 4);
        assert_eq!(receipt.result["status"], "completed");
    }
}
