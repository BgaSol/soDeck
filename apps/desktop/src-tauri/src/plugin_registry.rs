use super::*;

impl State {
    pub(super) fn all_plugins(&self) -> Vec<Arc<Loaded>> {
        let mut plugins = self
            .loaded
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        plugins.sort_by(|a, b| {
            a.package
                .manifest
                .plugin_id
                .cmp(&b.package.manifest.plugin_id)
        });
        plugins
    }
    pub(super) fn loaded_for(&self, id: &str) -> Result<Arc<Loaded>> {
        let plugin = self
            .loaded
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| err("PLUGIN_UNAVAILABLE", format!("插件 {id} 未加载")))?;
        if !plugin.process.is_alive() {
            return Err(err("PLUGIN_CRASHED", format!("插件 {id} 后台已结束")));
        }
        Ok(plugin)
    }
    pub(super) fn plugin_for_view(&self, id: &str) -> Result<Arc<Loaded>> {
        let owner = self
            .workbench
            .lock()
            .unwrap()
            .view(id)
            .map_err(native_error)?
            .plugin_id;
        self.loaded_for(&owner)
    }
    pub(super) fn command_plugin(&self, caller: &Caller, params: &Value) -> Result<Arc<Loaded>> {
        let owner = if let Some(view) = params["viewId"].as_str() {
            Some(
                self.workbench
                    .lock()
                    .unwrap()
                    .view(view)
                    .map_err(native_error)?
                    .plugin_id,
            )
        } else {
            params["pluginId"]
                .as_str()
                .map(str::to_owned)
                .or(caller.plugin_id.clone())
        };
        if let Some(owner) = owner {
            if caller.plugin_id.as_ref().is_some_and(|id| *id != owner)
                || params["pluginId"].as_str().is_some_and(|id| id != owner)
            {
                return Err(err("SOURCE_INVALID", "不允许跨插件调用"));
            }
            return self.loaded_for(&owner);
        }
        let command = required(params, "commandId")?;
        let candidates = self
            .all_plugins()
            .into_iter()
            .filter(|p| {
                p.package
                    .manifest
                    .contributes
                    .commands
                    .iter()
                    .any(|c| c.id == command)
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(err(
                if candidates.is_empty() {
                    "UNSUPPORTED_API"
                } else {
                    "AMBIGUOUS_COMMAND"
                },
                "命令没有唯一所属插件，请指定 pluginId",
            ));
        }
        Ok(candidates[0].clone())
    }
    pub(super) fn worker_for(&self, plugin: &str, command: &str, args: Value) -> Result<Value> {
        let timeout = if command == "command.execute" {
            match args["context"]["serverId"].as_str() { Some(id)=>self.core.route_attempt_timeout(id)?+std::time::Duration::from_secs(60), None=>std::time::Duration::from_secs(180) }
        } else { std::time::Duration::from_secs(180) };
        let loaded=self.loaded_for(plugin)?;
        let owner=Caller{source:"plugin".into(),plugin_id:Some(plugin.into()),plugin_version:Some(loaded.package.manifest.version.clone()),instance_id:loaded.instance.clone(),view_id:None,mount_generation:None};
        let metadata=self.core.transfer_diagnostic_context(&owner)?;
        loaded.process.invoke_with_context(command, args, timeout, metadata)
            .map_err(|e| {
                serde_json::from_value(serde_json::to_value(e).unwrap())
                    .unwrap_or_else(|_| err("PLUGIN_ERROR", "插件调用失败"))
            })
    }
    pub(super) fn worker(&self, command: &str, args: Value) -> Result<Value> {
        let context = args.get("context").unwrap_or(&args);
        let owner = if let Some(view) = context["viewId"].as_str() {
            self.workbench
                .lock()
                .unwrap()
                .view(view)
                .map_err(native_error)?
                .plugin_id
        } else if let Some(op) = context["operationId"].as_str() {
            self.pending_opens
                .lock()
                .unwrap()
                .get(op)
                .map(|o| o.plugin_id.clone())
                .ok_or_else(|| err("STALE_OPERATION", "操作已结束"))?
        } else if let Some(plugin) = context["pluginId"].as_str() {
            plugin.into()
        } else {
            return Err(err("SOURCE_INVALID", "插件调用缺少所属视图或操作"));
        };
        self.worker_for(&owner, command, args)
    }
    pub(super) fn monitor(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let Some(state) = weak.upgrade() else {
                break;
            };
            if state.stopping.load(Ordering::SeqCst) {
                break;
            }
            for plugin in state.all_plugins() {
                if !plugin.process.is_alive() {
                    let _mount = state.mounting.lock().unwrap();
                    if state.stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    state.core.observe_plugin_lifecycle(&plugin.instance,"crash",Some("PLUGIN_CRASHED"));
                    state.core.close_instance(&plugin.instance);
                    state.communication.lock().unwrap().remove_instance(&plugin.instance);
                    state
                        .loaded
                        .lock()
                        .unwrap()
                        .remove(&plugin.package.manifest.plugin_id);
                    state.settings_provider_changed(&plugin.package.manifest.plugin_id);
                    let affected = {
                        let mut w = state.workbench.lock().unwrap();
                        let ids = w
                            .views
                            .iter_mut()
                            .filter(|v| v.plugin_id == plugin.package.manifest.plugin_id)
                            .map(|v| {
                                v.status = "viewFailed".into();
                                v.failure_reason = Some("插件后台已结束，可重新加载".into());
                                v.runtime_data = Value::Null;
                                v.view_id.clone()
                            })
                            .collect::<Vec<_>>();
                        for id in &ids {
                            w.resources.remove(id);
                        }
                        ids
                    };
                    let bindings = state
                        .bindings
                        .lock()
                        .unwrap()
                        .values()
                        .filter(|b| affected.contains(&b.view_id))
                        .cloned()
                        .collect::<Vec<_>>();
                    for b in bindings {
                        b.phase.store(3, Ordering::SeqCst);
                        b.changed.notify_all();
                        let _ = state.native.close(b.id.clone());
                        state.bindings.lock().unwrap().remove(&b.id);
                    }
                    state.changed();
                }
            }
        });
    }
    pub(super) fn recover_plugin(self: &Arc<Self>, id: &str) -> Result<()> {
        if self.loaded_for(id).is_ok() {
            return Ok(());
        }
        let package = self
            .plugins
            .lock()
            .unwrap()
            .load_for_run(id)
            .map_err(|e| err("PLUGIN_UNAVAILABLE", e.to_string()))?
            .ok_or_else(|| err("PLUGIN_UNAVAILABLE", "插件包不可用"))?;
        let plugin =
            spawn_plugin(self.core.clone(), &package).map_err(|e| err("PLUGIN_ERROR", e))?;
        *plugin.gateway.lock().unwrap() = Some(Arc::downgrade(self));
        self.loaded.lock().unwrap().insert(id.into(), plugin);
        self.settings_provider_changed(id);
        Ok(())
    }
}
