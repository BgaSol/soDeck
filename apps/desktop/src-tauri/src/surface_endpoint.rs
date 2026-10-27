//! Host-authenticated routes. A temporary dialog has its own source/feed owner,
//! but its caller and resources remain bound to the parent logical view.
use super::*;

impl State {
    pub(super) fn dialog_endpoint(&self, source: &str) -> Option<Arc<plugin_dialogs::Session>> {
        self.dialogs.lock().unwrap().values().find(|d| d.source == source && !d.cleaned.load(Ordering::SeqCst)).cloned()
    }
    pub(super) fn endpoint_caller(&self, source: &str) -> Result<Option<Caller>> {
        let Some(dialog) = self.dialog_endpoint(source) else { return Ok(None); };
        if !self.dialog_parent_valid(&dialog) { return Err(err("SOURCE_INVALID", "弹窗来源已失效")); }
        Ok(Some(dialog.caller.clone()))
    }
    pub(super) fn endpoint_window(&self, source: &str, surface: bool) -> Result<String> {
        if let Some(dialog) = self.dialog_endpoint(source) {
            if !self.dialog_parent_valid(&dialog) { return Err(err("SOURCE_INVALID", "弹窗来源已失效")); }
            return Ok(if surface {dialog.label.clone()} else {dialog.owner.clone()});
        }
        if let Some(window) = source.strip_prefix("@window:") {
            if !self.workbench.lock().unwrap().windows.contains_key(window)
                && !window.strip_prefix("detached-").is_some_and(|id| self.view_binding(id).is_some()) {
                return Err(err("SOURCE_INVALID", "窗口来源未登记"));
            }
            return Ok(window.into());
        }
        Ok(self.binding(source)?.window.clone())
    }
    pub(super) fn endpoint_notify(&self, source: &str, event: &str, payload: Value) {
        let script = format!("window.__sodeckNotify?.({});", json!({"type":event,"payload":payload}));
        if let Some(window) = source.strip_prefix("@window:").and_then(|id|self.app.get_webview_window(id)) {
            let _=window.eval(&script);
        } else { let _=self.native.post_eval(source.into(), script); }
    }
    pub(super) fn endpoint_feed_owner(&self, caller: &Caller, source: Option<&str>) -> String {
        if let Some(dialog) = source.and_then(|s|self.dialog_endpoint(s)) {return dialog.feed_owner.clone();}
        source.and_then(|s|s.strip_prefix("@window:")).map(|id|format!("@window:{}:{}", self.app_generation,id)).unwrap_or_else(||communication_services::caller_key(caller))
    }
}
