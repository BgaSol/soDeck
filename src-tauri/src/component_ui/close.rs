//! Blocking fixture adapters to the production lifecycle, without separate plans.
use super::*;
use crate::lifecycle::{Action, Phase, Target};
use std::sync::Arc;
impl AppCore {
    pub fn ui_close_prepare(
        self: &Arc<Self>,
        window: &str,
        workspaces: Vec<String>,
        view: Option<String>,
    ) -> Result<Value, String> {
        let target = if let Some(id) = view {
            Target::Views { ids: vec![id] }
        } else if workspaces.len() == 1 {
            Target::Workspace {
                id: workspaces[0].clone(),
            }
        } else {
            Target::Application {
                action: Action::Exit,
            }
        };
        self.close_fixture_prepare(window, target)
    }
    /// Explicit generated-fixture/package-smoke input, never a desktop RPC endpoint.
    pub fn close_fixture_prepare(
        self: &Arc<Self>,
        window: &str,
        target: Target,
    ) -> Result<Value, String> {
        let op = self.close_prepare(window, &self.next_id(), target)?;
        {
            let mut ui = self.ui_state();
            let op = ui.lifecycle.operations.get_mut(&op.id).unwrap();
            op.inputs.values_mut().for_each(|v| *v = Some(json!({})));
            op.working = true;
        }
        self.close_check_views(&op.id);
        let op = self.close_status(window, &op.id)?;
        Ok(json!({"token":op.id,"views":op.views,"dirty":op.dirty}))
    }
    pub fn ui_close_finish(
        self: &Arc<Self>,
        token: &str,
        ids: Vec<String>,
        decision: &str,
    ) -> Result<(), String> {
        let window = {
            let ui = self.ui_state();
            let op = ui.lifecycle.operations.get(token).ok_or("关闭操作已失效")?;
            if ids != op.views {
                return Err("关闭页面集合已改变".into());
            }
            op.window.clone()
        };
        if decision == "cancel" {
            self.close_abort(token);
            return Ok(());
        }
        if !matches!(decision, "save" | "discard") {
            return Err("关闭决定无效".into());
        }
        {
            let mut ui = self.ui_state();
            let op = ui.lifecycle.operations.get_mut(token).unwrap();
            op.working = true;
            op.decision = Some(decision.into());
        }
        self.close_execute(token, decision);
        let op = self.close_status(&window, token)?;
        if op.phase == Phase::Completed {
            Ok(())
        } else {
            let error = op.error.unwrap_or("关闭未完成".into());
            if op.phase == Phase::Deciding {
                self.close_abort(token);
            }
            Err(error)
        }
    }
    pub(super) fn component_close_owner(&self, window: &str, token: &str) -> Result<(), String> {
        self.close_status(window, token).and_then(|op| {
            if op.window == window {
                Ok(())
            } else {
                Err("关闭操作不属于此窗口".into())
            }
        })
    }
}
