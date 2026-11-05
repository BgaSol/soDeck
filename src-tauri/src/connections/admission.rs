//! Short, ordered commit of a verified transport into the existing workspace identity.
use super::*;
use crate::workspaces::{Layout, Workspace};

impl AppCore {
    pub(super) fn adopt_connection(
        &self,
        a: &Arc<Attempt>,
        profile: &Profile,
        session: Arc<route::Session>,
    ) -> Result<Context, Error> {
        let servers = self.servers.lock().unwrap();
        if !servers
            .connection_snapshot(&profile.server_id)
            .is_ok_and(|p| p.identity == profile.identity)
        {
            return Err(Error::new("profileChanged", "连接资料已修改，请重新连接"));
        }
        let entries = self.connections.entries.lock().unwrap();
        if self.is_closing()
            || a.cancel.is_cancelled()
            || !entries
                .get(&profile.server_id)
                .is_some_and(|current| Arc::ptr_eq(current, a))
        {
            return Err(Error::cancelled());
        }
        let mut workspaces = self.workspaces.lock().unwrap();
        let mut ui = self.component_ui.lock().unwrap();
        let existing = workspaces.by_server(&profile.server_id).cloned();
        if existing.as_ref().map(|w| &w.context) != a.expected_context.as_ref() {
            return Err(Error::new("workspaceChanged", "工作区已关闭或会话已改变"));
        }
        let node = profile.nodes.last().unwrap();
        let endpoint = format!("{}@{}", node.username, node.endpoint());
        let context = if let Some(w) = existing {
            if ui.frozen(&w.context.workspace_id) {
                return Err(Error::new("workspaceClosing", "工作区正在关闭"));
            }
            let context = workspaces
                .reconnect(&w.context, a.view().attempt_id, endpoint)
                .map_err(|_| Error::new("workspaceChanged", "重连上下文已失效"))?;
            workspaces.set_target_identity(&context.workspace_id, profile.identity.clone());
            ui.rebind_context(&context);
            workspaces
                .activate(&w.window_id, Some(&context.workspace_id))
                .map_err(|_| Error::new("workspaceChanged", "工作区已移动或关闭"))?;
            a.state.lock().unwrap().view.window_id = w.window_id;
            context
        } else {
            workspaces
                .projection(&a.source_window)
                .map_err(|_| Error::cancelled())?;
            let id = self.next_id();
            let (left_width, right_width) = ui.widths();
            let context = Context {
                workspace_id: id.clone(),
                server_id: profile.server_id.clone(),
                connection_id: a.view().attempt_id,
                generation: 1,
            };
            let name = servers
                .workspace_names()
                .remove(&profile.server_id)
                .ok_or_else(|| Error::new("profileChanged", "服务器资料已删除"))?;
            workspaces.adopt(Workspace {
                context: context.clone(),
                name,
                endpoint,
                connected: true,
                target_identity: Some(profile.identity.clone()),
                window_id: "main".into(),
                layout: Layout {
                    left_width,
                    right_width,
                    ..Layout::default()
                },
                view_ids: vec![],
                active_view_id: None,
            });
            a.state.lock().unwrap().view.window_id = "main".into();
            context
        };
        *a.session.lock().unwrap() = Some(session);
        {
            let mut s = a.state.lock().unwrap();
            s.view.context = Some(context.clone());
            s.view.state = "connected".into();
            s.view.stage = "connected".into();
        }
        drop(ui);
        drop(workspaces);
        drop(entries);
        drop(servers);
        self.workspace_changed();
        self.log(None,"info","远程连接已通过认证",Some(serde_json::json!({"serverId":profile.server_id,"connectionId":context.connection_id,"nodeCount":profile.nodes.len()})));
        Ok(context)
    }
}
