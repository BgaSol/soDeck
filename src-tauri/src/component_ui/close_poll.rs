use super::*;
impl AppCore {
    pub fn ui_close_poll(&self, window: &str, token: &str, save: bool) -> Result<Value, String> {
        self.component_close_owner(window, token)?;
        let views = {
            let ui = self.component_ui.lock().unwrap();
            let plan = ui.plans.get(token).ok_or("关闭确认已失效")?;
            plan.views
                .iter()
                .map(|id| ui.view(id).cloned())
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut waiting = Vec::new();
        for view in views {
            let file_pending = if save {
                false
            } else {
                self.files.cancel_pending(&view.id)
            };
            let reply = self.ui_plugin_call(
                &view,
                if save { "ui.save" } else { "ui.beforeClose" },
                if save {
                    Value::Null
                } else {
                    json!({"cancelTasks":true})
                },
            );
            let reply = match reply {
                Ok(reply) => reply,
                Err(error) if save => return Err(error),
                Err(_) => json!({"metadata":{"busy":false}}),
            };
            self.component_ui
                .lock()
                .unwrap()
                .update_metadata(&view.id, &reply["metadata"])?;
            if file_pending
                || reply["metadata"]["busy"] == true
                || (save && reply["metadata"]["dirty"] == true)
            {
                waiting.push(json!({"id":view.id,"title":view.title,"busy":reply["metadata"]["busy"],"dirty":reply["metadata"]["dirty"]}));
            }
        }
        Ok(json!({"waiting":waiting}))
    }
}
