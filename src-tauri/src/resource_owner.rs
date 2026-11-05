//! Shared ownership checks for native plugin resources.
use crate::{workspaces::Context, AppCore};
#[derive(Clone)]
pub(crate) struct Owner {
    pub plugin: String,
    pub instance: String,
    pub view: String,
    pub context: Context,
}
impl AppCore {
    pub(crate) fn resource_owner(
        &self,
        plugin: &str,
        instance: &str,
        view: &str,
    ) -> Result<Owner, String> {
        if self.is_closing() {
            return Err("应用正在退出".into());
        }
        let live = self
            .instances
            .lock()
            .unwrap()
            .get(plugin)
            .map(|i| i.view())
            .is_some_and(|p| {
                p.instance_id.as_deref() == Some(instance)
                    && p.status == crate::model::PluginStatus::Running
            });
        if !live {
            return Err("插件实例已停止".into());
        }
        let ui = self.component_ui.lock().unwrap();
        let v = ui.resource_view(view)?;
        if v.plugin_id != plugin || v.plugin_instance_id != instance {
            return Err("资源视图不属于此插件实例".into());
        }
        if ui.frozen(&v.context.workspace_id) {
            return Err("工作区正在关闭".into());
        }
        Ok(Owner {
            plugin: plugin.into(),
            instance: instance.into(),
            view: view.into(),
            context: v.context.clone(),
        })
    }
}
