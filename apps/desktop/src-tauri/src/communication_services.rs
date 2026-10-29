use super::*;
use sodeck_communication::Key;
use std::sync::OnceLock;

pub(super) fn check(kind:&str, method:&str, value:&Value)->Result<()> {
    static SCHEMA:OnceLock<Value>=OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(sodeck_communication::STATE_SCHEMA).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||err("UNSUPPORTED_API","未知状态契约方法"))?;
    sodeck_core::contract::check(schema,&schema["$defs"][name],value,method).map_err(|message|err("INVALID_PARAMS",message))
}
fn fault(e:sodeck_communication::Fault)->CoreError {err(e.0,"状态订阅已失效或超出容量，请重新获取状态")}
pub(super) fn presentation_key(view:&str,scope:&Value)->Key {
    Key{contract:"view.presentation.v1".into(),scope:scope["workspaceGeneration"].as_str().unwrap_or_default().into(),object:view.into()}
}
pub(super) fn caller_key(caller:&Caller)->String {
    format!("{}:{}:{}",caller.instance_id,caller.view_id.as_deref().unwrap_or("worker"),caller.mount_generation.map_or(String::new(),|n|n.to_string()))
}

impl State {
    pub(super) fn state_call(&self,caller:&Caller,source:Option<&str>,method:&str,p:&Value)->Result<Value>{
        check("x-methods",method,p)?;
        let window=source.filter(|s|s.starts_with("@window:")).map(|s|self.endpoint_window(s,true)).transpose()?;
        if let Some(window)=&window {if self.app.get_webview_window(window).is_none(){return Err(err("SOURCE_INVALID","窗口已关闭"));}}
        else {self.state_caller(caller)?;}
        let owner=self.endpoint_feed_owner(caller,source);
        if p["contract"]=="updates.state.v1" {
            self.publish_updates();let key=self.updates_key();let mut hub=self.communication.lock().unwrap();
            let result=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&result)?;return Ok(result);
        }
        if p["contract"]=="plugins.management.v1" {
            self.publish_plugin_management()?;let key=self.plugin_management_key();let mut hub=self.communication.lock().unwrap();
            let result=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&result)?;return Ok(result);
        }
        if matches!(p["contract"].as_str(),Some("settings.catalog.v1"|"settings.page.v1")) {
            let page=if p["contract"]=="settings.catalog.v1" {self.settings_catalog(caller,&json!({}))?;"catalog"}else{
                let page=required(&p["target"],"pageId")?;
                self.settings_call(caller,"settings.page.get",&json!({"pageId":page}))?;page
            };
            let mut hub=self.communication.lock().unwrap();let key=self.settings_key(page);
            let result=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&result)?;return Ok(result);
        }
        if p["contract"]=="communication.quality.v1" {
            let key=Key{contract:"communication.quality.v1".into(),scope:self.app_generation.clone(),object:Core::quality_object_id(&p["target"])?};
            let value=self.core.quality_state(&p["target"])?;let mut hub=self.communication.lock().unwrap();
            hub.prune_unused_monitor_objects();
            if hub.get(&key).ok().is_none_or(|s|s.value["revision"].as_str().and_then(|s|s.parse::<u64>().ok())<=value["revision"].as_str().and_then(|s|s.parse::<u64>().ok())) {
                hub.put(key.clone(),json!({"appGeneration":self.app_generation}),value).map_err(fault)?;
            }
            let result=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&result)?;return Ok(result);
        }
        if p["contract"]=="communication.summary.v1" {
            let key=Key{contract:"communication.summary.v1".into(),scope:self.app_generation.clone(),object:Core::communication_object_id(&p["target"])?};
            let value=self.core.communication_snapshot(&p["target"])?;
            let mut hub=self.communication.lock().unwrap();
            hub.prune_unused_monitor_objects();
            // A sampler update may win while the first snapshot is being acquired.
            if hub.get(&key).ok().is_none_or(|s|s.value["revision"].as_str().and_then(|s|s.parse::<u64>().ok())<=value["revision"].as_str().and_then(|s|s.parse::<u64>().ok())) {
                hub.put(key.clone(),json!({"appGeneration":self.app_generation}),value).map_err(fault)?;
            }
            let value=if method=="state.get" {serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else {hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&value)?;return Ok(value);
        }
        if p["contract"]=="servers.catalog.v2" {
            let key=Key{contract:"servers.catalog.v2".into(),scope:self.app_generation.clone(),object:"catalog".into()};
            let revision=self.core.catalog_revision()?;
            let mut hub=self.communication.lock().unwrap();
            hub.put(key.clone(),json!({"appGeneration":self.app_generation}),json!({"catalogRevision":revision})).map_err(fault)?;
            let value=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&value)?;return Ok(value);
        }
        if p["contract"]=="app.theme.v1" {
            if !self.supports_live_theme(caller){return Err(err("UNSUPPORTED_API","插件未声明实时主题契约"));}
            let mut hub=self.communication.lock().unwrap();let key=self.theme_key();
            let value=if method=="state.get"{serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else{hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&value)?;return Ok(value);
        }
        if matches!(p["contract"].as_str(),Some("connections.route.v2"|"connections.diagnostics.v1")) {
            let workspace=required(&p["target"],"workspaceId")?;
            let generation=required(&p["target"],"workspaceGeneration")?;
            let w=self.workbench.lock().unwrap();
            let ws=w.workspaces.get(workspace).ok_or_else(||err("STALE_WORKSPACE","工作区已结束"))?;
            if ws.generation!=generation {return Err(err("STALE_WORKSPACE","工作区已换代"));}
            if window.is_none() && caller.plugin_id.as_ref().is_none_or(|id|!w.views.iter().any(|v|v.workspace_id==workspace&&v.plugin_id==*id)) {return Err(err("SOURCE_INVALID","连接状态不属于调用者工作区"));}
            let value=if p["contract"]=="connections.diagnostics.v1"{self.core.connection_diagnostics_snapshot(&p["target"])?}else{self.core.route_snapshot(workspace,workspace)?};
            let key=Key{contract:required(p,"contract")?.into(),scope:generation.into(),object:workspace.into()};
            let mut hub=self.communication.lock().unwrap();
            hub.put(key.clone(),p["target"].clone(),value).map_err(fault)?;
            let value=if method=="state.get" {serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else {hub.open_for(&owner,window.as_ref().map_or(caller.instance_id.as_str(),|_|owner.as_str()),&key,Instant::now()).map_err(fault)?};
            check("x-results",method,&value)?;return Ok(value);
        }
        if window.is_some() && matches!(method,"state.get"|"state.open"){return Err(err("SOURCE_INVALID","窗口不支持此状态订阅"));}
        let result=match method {
            "state.ack"|"state.close"=>{
                let mut hub=self.communication.lock().unwrap();
                let id=required(p,"subscriptionId")?;let epoch=required(p,"feedEpoch")?;
                if method=="state.close" {hub.close(&owner,id,epoch).map_err(fault)?;}
                else {let sequence=required(p,"sequence")?.parse().map_err(|_|err("INVALID_CURSOR","无效订阅序号"))?;hub.ack(&owner,id,epoch,sequence).map_err(fault)?;}
                json!({"accepted":true})
            }
            "state.get"|"state.open"=>{
                let contract=required(p,"contract")?;
                let target=&p["target"];
                let key=match contract {
                    "net.group.v1"=>{
                        let value=self.core.network_group_state(caller,target)?;
                        let key=Key{contract:contract.into(),scope:required(&target["scope"],"workspaceGeneration")?.into(),object:required(&value["group"],"id")?.into()};
                        self.communication.lock().unwrap().put(key.clone(),target["scope"].clone(),value).map_err(fault)?;
                        key
                    }
                    "view.presentation.v1"=>{
                        let view=required(target,"viewId")?;
                        let value=self.extended_workbench(caller,None,"views.presentation",&json!({"viewId":view}))?;
                        if value["scope"]!=target["scope"] {return Err(err("STALE_WORKSPACE","视图工作区代次已变化"));}
                        if caller.view_id.as_deref().is_some_and(|id|id!=view){return Err(err("SOURCE_INVALID","请求不属于调用视图"));}
                        let key=presentation_key(view,&value["scope"]);
                        let mut hub=self.communication.lock().unwrap();
                        // A delayed initial query must not replace a newer published presentation.
                        if hub.get(&key).ok().is_none_or(|old|old.value["revision"].as_u64()<=value["revision"].as_u64()) {
                            hub.put(key.clone(),value["scope"].clone(),value).map_err(fault)?;
                        }
                        key
                    }
                    "workspace.directory.v1"=>{
                        let workspace=required(target,"workspaceId")?;let generation=required(target,"workspaceGeneration")?;
                        {let w=self.workbench.lock().unwrap();
                        let ws=w.workspaces.get(workspace).ok_or_else(||err("STALE_WORKSPACE","工作区已结束"))?;
                        if ws.generation!=generation||caller.plugin_id.as_ref().is_none_or(|owner|!w.views.iter().any(|v|v.plugin_id==*owner&&v.workspace_id==workspace)){
                            return Err(err("SOURCE_INVALID","目录订阅工作区归属无效"));
                        }
                        if caller.view_id.as_deref().is_some_and(|id|w.view(id).map_or(true,|v|v.workspace_id!=workspace)){return Err(err("SOURCE_INVALID","目录订阅不属于视图"));}}
                        self.sync_directories();
                        directory_context::key(workspace,generation)
                    }
                    _=>return Err(err("UNSUPPORTED_API","未知状态契约")),
                };
                // Serialize final registration with workspace/view teardown.
                let w=self.workbench.lock().unwrap();
                if !w.workspaces.values().any(|ws|ws.generation==key.scope) ||
                    (key.contract=="view.presentation.v1" && w.view(&key.object).is_err()) {
                    return Err(err("STALE_WORKSPACE","状态所属工作区或视图已结束"));
                }
                if let Some(id)=&caller.view_id {
                    let view=w.view(id).map_err(native_error)?;
                    if Some(view.mount_generation)!=caller.mount_generation{return Err(err("SOURCE_INVALID","调用表面已换代"));}
                }
                self.state_participants.lock().unwrap().entry(key.scope.clone()).or_default().insert(caller.instance_id.clone());
                let mut hub=self.communication.lock().unwrap();
                if method=="state.get" {serde_json::to_value(hub.get(&key).map_err(fault)?.as_ref()).unwrap()}
                else {hub.open_for(&owner,&caller.instance_id,&key,Instant::now()).map_err(fault)?}
            }
            _=>return Err(err("UNSUPPORTED_API","未知状态方法")),
        };
        check("x-results",method,&result)?;Ok(result)
    }
    pub(super) fn state_caller(&self,caller:&Caller)->Result<()> {
        let id=caller.plugin_id.as_deref().ok_or_else(||err("SOURCE_INVALID","状态接口需要插件实例"))?;
        if self.loaded_for(id)?.instance!=caller.instance_id{return Err(err("INSTANCE_EXPIRED","插件实例已换代"));}
        Ok(())
    }
    pub(super) fn publish_state_presentation(&self,value:Value){
        let Some(id)=value["viewId"].as_str() else{return;};
        let key=presentation_key(id,&value["scope"]);
        let mut hub=self.communication.lock().unwrap();
        if hub.get(&key).ok().is_none_or(|old|old.value["revision"].as_u64()<=value["revision"].as_u64()) {
            if hub.put(key.clone(),value["scope"].clone(),value).is_err(){hub.remove(&key);}
        }
    }

    /// One bounded pump for all state feeds. No plugin callback runs under Hub,
    /// workbench, binding, or registry locks. Weak ownership allows shutdown.
    pub(super) fn start_state_pump(self:&Arc<Self>){
        let weak=Arc::downgrade(self);
        std::thread::spawn(move||{let mut liveness=Instant::now();let mut appearance=Instant::now();let mut traffic_revision=u64::MAX;let mut quality_revision=u64::MAX;loop{
            std::thread::sleep(Duration::from_millis(10));
            let Some(state)=weak.upgrade()else{break;};
            if state.stopping.load(Ordering::SeqCst){break;}
            let revision=state.core.communication_revision();
            if revision!=traffic_revision {state.sync_communication_summaries();traffic_revision=revision;}
            let revision=state.core.quality_revision();
            if revision!=quality_revision{state.sync_quality_states();quality_revision=revision;}
            if liveness.elapsed()>=Duration::from_millis(100){state.maintain_plugin_dialogs();state.sync_catalog();state.sync_directories();state.sync_connection_routes();state.sync_network_groups();liveness=Instant::now();}
            if appearance.elapsed()>=Duration::from_millis(500){state.refresh_system_theme();state.expire_application_menus();appearance=Instant::now();}
            let plugins:HashMap<_,_>=state.all_plugins().into_iter().map(|p|(p.instance.clone(),p)).collect();
            for (instance,payload) in state.core.pty_observer_notices(){
                if let Some(plugin)=plugins.get(&instance) {
                    if plugin.process.notify("stream.readable",payload.clone()).is_err(){state.core.retry_pty_observer_notice(payload["ptyId"].as_str().unwrap());}
                }
            }
            let deliveries=state.communication.lock().unwrap().drain(Instant::now());
            for delivery in deliveries {
                if let Some(id)=delivery.owner.strip_prefix(&format!("@window:{}:",state.app_generation)) {
                    let result=state.endpoint_window(&format!("@window:{id}"),true).ok().and_then(|id|state.app.get_webview_window(&id)).map(|w|w.eval(&format!("window.__sodeckNotify?.({});",json!({"type":delivery.event,"payload":delivery.payload}))));
                    let mut hub=state.communication.lock().unwrap();
                    match result{Some(Ok(()))=>hub.delivered(&delivery.subscription_id,delivery.event),Some(Err(_))=>hub.delivery_failed(&delivery.subscription_id),None=>hub.remove_owner(&delivery.owner)}
                    continue;
                }
                if delivery.owner.starts_with("@dialog:") {
                    let dialog=state.dialogs.lock().unwrap().values().find(|d|d.feed_owner==delivery.owner&&!d.cleaned.load(Ordering::SeqCst)).cloned();
                    let delivered=dialog.as_ref().is_some_and(|d|state.native.notify(d.source.clone(),delivery.event,delivery.payload.clone()).is_ok());
                    let mut hub=state.communication.lock().unwrap();
                    if delivered {hub.delivered(&delivery.subscription_id,delivery.event);}else if dialog.is_none(){hub.remove_owner(&delivery.owner);}else{hub.delivery_failed(&delivery.subscription_id);}
                    continue;
                }
                let plugin=delivery.owner.split(':').next().and_then(|id|plugins.get(id));
                let result=if let Some(plugin)=plugin {
                    let worker=format!("{}:worker:",plugin.instance);
                    if delivery.owner==worker {plugin.process.notify(delivery.event,delivery.payload.clone()).map_err(|_|())}
                    else {
                        let binding=state.bindings.lock().unwrap().values().find(|b|delivery.owner==format!("{}:{}:{}",plugin.instance,b.view_id,b.generation)&&b.phase.load(Ordering::SeqCst)!=3).cloned();
                        binding.map_or(Err(()),|b|state.native.notify(b.id.clone(),delivery.event,delivery.payload.clone()).map_err(|_|()))
                    }
                }else{Err(())};
                if result.is_err(){
                    let mut hub=state.communication.lock().unwrap();
                    hub.delivery_failed(&delivery.subscription_id);
                    if plugin.is_none(){hub.remove_owner(&delivery.owner);}
                }else{state.communication.lock().unwrap().delivered(&delivery.subscription_id,delivery.event);}
            }
        }});
    }
    fn sync_communication_summaries(&self) {
        let keys={let mut hub=self.communication.lock().unwrap();hub.prune_unused_monitor_objects();hub.keys().filter(|k|k.contract=="communication.summary.v1").cloned().collect::<Vec<_>>()};
        for key in keys {
            let previous=self.communication.lock().unwrap().get(&key);
            let Ok(previous)=previous else {continue};
            // Constant-size cached counters; no public RPC, history SQL or resource list construction.
            let result=self.core.communication_snapshot(&json!({"target":previous.value["target"],"layer":previous.value["layer"]}));
            let mut hub=self.communication.lock().unwrap();
            if hub.get(&key).is_err(){continue;}
            match result {Ok(value)=>{if hub.put(key.clone(),json!({"appGeneration":self.app_generation}),value).is_err(){hub.remove(&key);}},Err(_)=>hub.remove(&key)}
        }
    }
    fn sync_quality_states(&self) {
        let keys={let mut hub=self.communication.lock().unwrap();hub.prune_unused_monitor_objects();hub.keys().filter(|k|k.contract=="communication.quality.v1").cloned().collect::<Vec<_>>()};
        for key in keys {
            let Ok(previous)=self.communication.lock().unwrap().get(&key)else{continue};
            let mut target=json!({"configEpoch":previous.value["configEpoch"]});
            if previous.value["serverId"].is_string(){target["serverId"]=previous.value["serverId"].clone();}
            let result=self.core.quality_state(&target);let mut hub=self.communication.lock().unwrap();
            if hub.get(&key).is_err(){continue;}
            match result{Ok(value)=>{if hub.put(key.clone(),json!({"appGeneration":self.app_generation}),value).is_err(){hub.remove(&key);}},Err(_)=>hub.remove(&key)}
        }
    }
    fn sync_connection_routes(&self) {
        let keys=self.communication.lock().unwrap().keys().filter(|k|matches!(k.contract.as_str(),"connections.route.v2"|"connections.diagnostics.v1")).cloned().collect::<Vec<_>>();
        for key in keys {
            let w=self.workbench.lock().unwrap();
            let Some(ws)=w.workspaces.get(&key.object).filter(|ws|ws.generation==key.scope)else{continue};
            if let Ok(value)=if key.contract=="connections.diagnostics.v1"{self.core.connection_diagnostics_snapshot(&json!({"workspaceId":key.object,"workspaceGeneration":ws.generation}))}else{self.core.route_snapshot(&key.object,&key.object)} {
                let scope=json!({"workspaceId":key.object,"workspaceGeneration":ws.generation});
                let _=self.communication.lock().unwrap().put(key,scope,value);
            }
        }
    }
    fn sync_catalog(&self) {
        let key=Key{contract:"servers.catalog.v2".into(),scope:self.app_generation.clone(),object:"catalog".into()};
        let exists=self.communication.lock().unwrap().get(&key).is_ok();
        if exists {if let Ok(revision)=self.core.catalog_revision(){let _=self.communication.lock().unwrap().put(key,json!({"appGeneration":self.app_generation}),json!({"catalogRevision":revision}));}}
    }
    fn sync_network_groups(&self) {
        let snapshots=self.core.network_feed_snapshots();
        let mut hub=self.communication.lock().unwrap();
        let keys:Vec<_>=hub.keys().filter(|k|k.contract=="net.group.v1").cloned().collect();
        for key in keys {
            if let Some((scope,value))=snapshots.iter().find(|(s,v)|s.workspace_generation==key.scope&&v["group"]["id"]==key.object) {
                let _=hub.put(key,json!(scope),value.clone());
            } else {hub.remove(&key);}
        }
    }
    pub(super) fn prune_state_feeds(&self){
        let (scopes,views)={let w=self.workbench.lock().unwrap();(w.workspaces.values().map(|ws|ws.generation.clone()).collect::<std::collections::HashSet<_>>(),w.views.iter().map(|v|v.view_id.clone()).collect::<std::collections::HashSet<_>>())};
        let expired_participants={let mut participants=self.state_participants.lock().unwrap();
            let expired:Vec<_>=participants.keys().filter(|scope|!scopes.contains(*scope)).cloned().collect();
            expired.into_iter().map(|scope|{let owners=participants.remove(&scope).unwrap();(scope,owners)}).collect::<Vec<_>>()};
        for (scope,owners) in expired_participants {
            for plugin in self.all_plugins().into_iter().filter(|p|owners.contains(&p.instance)) {
                if plugin.process.notify("workspace.closed",json!({"workspaceGeneration":scope})).is_err(){plugin.process.terminate();}
            }
        }
        let mut hub=self.communication.lock().unwrap();
        let expired:Vec<_>=hub.keys().filter(|key|!matches!(key.contract.as_str(),"updates.state.v1"|"plugins.management.v1"|"app.theme.v1"|"settings.catalog.v1"|"settings.page.v1"|"servers.catalog.v2"|"communication.summary.v1"|"communication.quality.v1")&&(!scopes.contains(&key.scope)||(key.contract=="view.presentation.v1"&&!views.contains(&key.object)))).cloned().collect();
        for key in expired {hub.remove(&key);}
        drop(hub);
        let plugins=self.all_plugins();
        let mut live=plugins.iter().map(|p|format!("{}:worker:",p.instance)).collect::<std::collections::HashSet<_>>();
        let view_plugins:HashMap<_,_>=self.workbench.lock().unwrap().views.iter().map(|v|(v.view_id.clone(),v.plugin_id.clone())).collect();
        for binding in self.bindings.lock().unwrap().values().filter(|b|b.phase.load(Ordering::SeqCst)!=3){
            if let Some(plugin)=plugins.iter().find(|p|view_plugins.get(&binding.view_id)==Some(&p.package.manifest.plugin_id)){
                live.insert(format!("{}:{}:{}",plugin.instance,binding.view_id,binding.generation));
            }
        }
        live.extend(self.dialogs.lock().unwrap().values().filter(|d|!d.cleaned.load(Ordering::SeqCst)).map(|d|d.feed_owner.clone()));
        live.extend(self.app.webview_windows().keys().filter(|id|id.as_str()!=monitor_window::LABEL&&id.as_str()!=settings_window::LABEL).map(|id|format!("@window:{}:{}",self.app_generation,id)));
        if let Some(source)=self.settings_source(){live.insert(format!("@window:{}:{}",self.app_generation,source.strip_prefix("@window:").unwrap()));}
        if let Some(source)=self.monitor_source(){live.insert(format!("@window:{}:{}",self.app_generation,source.strip_prefix("@window:").unwrap()));}
        self.communication.lock().unwrap().retain_owners(&live);
        self.core.retain_observation_owners(&live);
    }
}
