use super::*;
impl State {
    fn menu_notification(&self, source: &str, result: &crate::presentation::ResultState) {
        let payload = serde_json::to_value(result).unwrap();
        if let Some(window) = source.strip_prefix("@window:").and_then(|id|self.app.get_webview_window(id)) {
            let _ = window.eval(&format!("window.__sodeckNotify?.({});",json!({"type":"ui.menu.closed","payload":payload})));
        } else { let _ = self.native.post_eval(source.into(),format!("window.__sodeckNotify?.({});",json!({"type":"ui.menu.closed","payload":payload}))); }
    }
    pub(super) fn cancel_menus(&self, window: &str) {
        let results = self.menus.cancel_window(window);
        if results.is_empty() { return; }
        crate::presentation::cancel();
        for (source,result) in results { self.menu_notification(&source,&result); }
    }
    pub(super) fn presentation_service(self: &Arc<Self>, source: Option<&str>, method: &str, params: &Value) -> Result<Value> {
        if method == "ui.theme.get" { return Ok(self.theme.clone()); }
        let source = source.ok_or_else(||err("INVALID_CONTEXT","菜单需要有效界面来源"))?;
        let window_id = self.source_window(Some(source),params)?;
        let id = required(params,"menuId")?;
        match method {
            "ui.menu.result" => Ok(serde_json::to_value(self.menus.get(id,source).map_err(|e|err("MOUNT_EXPIRED",e))?).unwrap()),
            "ui.menu.close" => {
                let result = self.menus.get(id,source).map_err(|e|err("MOUNT_EXPIRED",e))?;
                if result.status == "open" { self.cancel_menus(&window_id); }
                Ok(json!({"closed":true}))
            }
            "ui.menu.open" => {
                let items: Vec<crate::presentation::Item> = serde_json::from_value(params["items"].clone()).map_err(|_|err("INVALID_PARAMS","菜单格式无效"))?;
                let mut ids=std::collections::BTreeSet::new();
                if items.is_empty() || items.iter().any(|i| !ids.insert(&i.id) || i.label.chars().any(|c|c.is_control()) || i.shortcut.as_ref().is_some_and(|s|s.chars().any(char::is_control))) {return Err(err("INVALID_PARAMS","菜单为空或项目无效"));}
                let window=self.app.get_webview_window(&window_id).ok_or_else(||err("WINDOW_CLOSED","窗口已关闭"))?;
                let dpi=window.scale_factor().map_err(|e|native_error(e.to_string()))?;
                let position=window.inner_position().map_err(|e|native_error(e.to_string()))?;
                let bounds=if source.starts_with("@window:"){Bounds::default()}else{self.binding(source)?.geometry.lock().unwrap().0};
                let point=(position.x+((bounds.x+params["anchor"]["x"].as_f64().unwrap())*dpi).round() as i32,position.y+((bounds.y+params["anchor"]["y"].as_f64().unwrap())*dpi).round() as i32);
                #[cfg(windows)] let hwnd=window.hwnd().map_err(|e|native_error(e.to_string()))?.0 as usize;
                #[cfg(not(windows))] let hwnd=0usize;
                for id in self.app.webview_windows().keys() { self.cancel_menus(id); }
                self.menus.begin(id,source,&window_id).map_err(|e|err("INVALID_PARAMS",e))?;
                let weak=Arc::downgrade(self);let source=source.to_string();let menu_id=id.to_string();let theme=self.theme.clone();
                let menu_source=source.clone();
                let completed: crate::presentation::Completed=Arc::new(move|selected|{
                    if let Some(state)=weak.upgrade(){
                        let valid=source.starts_with("@window:") || state.binding(&source).is_ok_and(|b|b.phase.load(Ordering::SeqCst)==1);
                        if let Some(result)=state.menus.finish(&menu_id,if valid{selected}else{None}){state.menu_notification(&source,&result);}
                    }
                });
                let menus=self.menus.clone();let menu_id=id.to_string();
                if menus.get(&menu_id,&menu_source).is_ok_and(|r|r.status=="open") {crate::presentation::show(hwnd,point,items,theme,dpi,completed);}else{completed(None);}

                Ok(json!({"menuId":id}))
            }
            _=>Err(err("METHOD_NOT_FOUND","未知呈现接口")),
        }
    }
}
