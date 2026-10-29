use super::{catalog,context,descriptor,model,schema::Partition};
use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
pub(super) fn definition(name:&str)->Result<descriptor::Definition>{
    let id=json!({"type":"string","classification":"safe-id","maxLength":128});
    let number=json!({"type":"integer","classification":"count","minimum":0,"maximum":9007199254740991u64});
    let fields=match name{
        "call.started"|"call.finished"|"call.interrupted"=>json!({"method":id,"errorCode":id,"reconciled":{"type":"boolean","classification":"state"},"summary":{"type":"object","classification":"structure","properties":{"fieldCount":number,"byteLength":number,"inputSeq":number,"generation":number,"bytesRead":number,"bytesWritten":number,"durationMs":number,"settingsOwner":id,"settingsPage":id,"settingsRevision":id,"persistedRevision":id,"persistence":id,"synchronization":id,"pluginId":id,"version":id,"state":id,"code":id,"changedKeyCount":number,"changedKeys":{"type":"array","classification":"structure","maxItems":32,"items":id}}},"traceId":id,"operationId":id}),
        "quality.segment.ended"=>json!({"connectionId":id,"generation":id}),
        "quality.response.sample"=>json!({"method":id,"routeRevision":id,"connectionId":id,"generation":id,"segmentId":id,"sample":{"type":"object","classification":"structure","properties":{"observedAt":number,"outcome":id,"sent":{"type":"boolean","classification":"state"},"durationMs":{"type":"number","classification":"duration","minimum":0},"queueMs":number,"openMs":number}}}),
        "connections.route.phase"|"connections.route.stage"=>json!({"originInstanceId":id,"originPluginId":id,"operationId":id,"workspaceId":id,"routeRevision":id,"accessMode":id,"ownerKind":id,"nodeId":id,"configRevision":number,"stepIndex":number,"stepId":id,"stage":id,"attemptId":id,"round":number,"durationMs":number,"state":id,"errorCode":id,"traceId":id}),
        "core.observation.cleanup.completed"=>json!({"jobId":id,"removed":number,"protected":number,"state":id}),
        "core.observation.policy.changed"=>json!({"revision":id,"changedFields":{"type":"array","classification":"structure","maxItems":7,"items":{"type":"string","classification":"enum","maxLength":32,"enum":["logLevel","debugUntil","factRetentionDays","logRetentionDays","diagnosticRetentionDays","sourceOverrides"]}}}),
        "core.observation.recovered"=>json!({"reasonCode":id,"originalTime":number}),
        "core.configuration.changed"=>json!({"method":id,"objectId":id,"revision":number,"changedFieldCount":number}),
        "core.network.completed"=>json!({"method":id,"operationId":id,"outcome":id,"errorCode":id}),
        "core.resource.created"|"core.resource.released"|"core.connection.created"|"core.connection.ended"=>json!({"method":id,"resourceRef":id}),
        "core.workspace.created"|"core.workspace.closed"|"core.view.created"|"core.view.closed"=>json!({"objectRef":id,"generation":id}),
        "core.plugin.started"|"core.plugin.ended"|"core.plugin.failed"=>json!({"phase":{"type":"string","classification":"enum","maxLength":32,"enum":["prepare","activate","stop","crash"]},"reasonCode":id}),
        "core.app.started"|"core.app.stopping"=>json!({}),
        "host.console.summary"=>json!({"trace":number,"debug":number,"info":number,"warn":number,"error":number,"fatal":number,"contentNotCollected":{"type":"boolean","classification":"state"}}),
        _=>return Err(CoreError::new("TYPE_UNSUPPORTED","未知核心事件")),
    };
    Ok(serde_json::from_value(json!({"type":name,"typeVersion":1,"kind":if name.starts_with("call."){"call"}else if matches!(name,"connections.route.phase"|"connections.route.stage"|"core.network.completed"){"domain"}else if name=="host.console.summary"{"log"}else if name.starts_with("core.observation."){"health"}else{"lifecycle"},"level":"info","fields":fields}))?)
}
impl Core {
    /// Accepts only static core event types and whitelisted, finite fields.
    pub fn observe_host_event(&self,caller:&Caller,name:&str,fields:Value)->Result<()> {
        self.observability.observe_internal(caller,name,fields)?;
        if name=="core.plugin.failed"{self.observability.request_automatic(caller,&json!([]),None);}Ok(())
    }
    pub(crate) fn observe_committed(&self,caller:&Caller,method:&str,p:&Value,result:&Value){
        if matches!(method,"servers.create"|"servers.update"|"servers.save"|"servers.delete"|"serverGroups.create"|"serverGroups.update"|"serverGroups.delete"|"servers.catalog.save"|"servers.catalog.delete"|"servers.catalog.move"|"servers.routes.update"|"proxies.catalog.save"|"proxies.catalog.remove"|"credentials.configuration.set"|"proxies.socks5.create"|"proxies.socks5.update"|"proxies.socks5.delete"|"auth.socks5.set"|"auth.ssh.set"|"auth.clear"|"hosts.trust"|"settings.set"){
            let mut fields=json!({"method":method,"changedFieldCount":p.as_object().map_or(0,|v|v.len())});
            if let Some(id)=result["serverId"].as_str().or(result["id"].as_str()).or(p["serverId"].as_str()).filter(|id|descriptor::identifier(id)){fields["objectId"]=json!(id);}
            if let Some(revision)=result["revision"].as_u64(){fields["revision"]=json!(revision);}
            let _=self.observe_host_event(caller,"core.configuration.changed",fields);
        }
    }
    pub fn observe_plugin_lifecycle(&self,instance:&str,phase:&str,code:Option<&str>){
        let source=self.observability.catalog.lock().unwrap().sources.get(instance).map(|(plugin,version,_)|Caller{source:"plugin".into(),plugin_id:Some(plugin.clone()),plugin_version:Some(version.clone()),instance_id:instance.into(),view_id:None,mount_generation:None});
        if let Some(source)=source{let mut fields=json!({"phase":phase});if let Some(code)=code{fields["reasonCode"]=json!(code);}let _=self.observe_host_event(&source,if code.is_some(){"core.plugin.failed"}else{"core.plugin.ended"},fields);}
    }
}

impl super::Observability{
    pub(super) fn observe_internal(&self,caller:&Caller,name:&str,fields:Value)->Result<()>{self.observe_internal_targets(caller,name,fields,vec![])}
    pub(crate) fn observe_internal_targets(&self,caller:&Caller,name:&str,fields:Value,targets:Vec<Value>)->Result<()>{
        let definition=definition(name)?;descriptor::check_fields(&definition.fields,&fields).map_err(|_|CoreError::new("INVALID_EVENT","核心事件字段无效"))?;
        let store=self;let key=store.signing_key.clone();let mut event=model::envelope(caller,&store.boot_id,&store.epoch,definition.kind.as_deref().unwrap(),name,fields);
        event["targets"]=json!(targets);
        event["configEpoch"]=json!(store.config_epoch);event["provenance"]=json!(if name=="host.console.summary"{"plugin-reported"}else{"core-observed"});
        if let Some(current)=context::DiagnosticScope::capture(){context::attach(&mut event,&current);}
        let partition=if name=="host.console.summary"{Partition::Logs}else{Partition::Facts};
        store.write(partition,8192,move|db|{db.admit(partition,8192)?;let connection=if partition==Partition::Facts{&mut db.facts}else{&mut db.logs};let tx=connection.transaction()?;catalog::save_type(&tx,"","events",&definition)?;model::insert(&tx,&key,if partition==Partition::Facts{"f"}else{"l"},&mut event,None)?;tx.commit()?;Ok(Value::Null)})?;Ok(())
    }
}

impl super::Observability {
    pub(crate) fn connection_lifecycle(&self,id:&str,generation:u64,server:&str,created:bool){
        let generation=generation.to_string();let target=super::targets::reference(&self.signing_key,&self.boot_id,"resource",id,Some(&generation));
        let server=super::targets::reference(&self.signing_key,&self.config_epoch,"server",server,Some(&self.config_epoch));
        let caller=Caller{source:"core".into(),instance_id:"connection-broker".into(),plugin_id:None,plugin_version:None,view_id:None,mount_generation:None};
        let _=self.observe_internal_targets(&caller,if created{"core.connection.created"}else{"core.connection.ended"},json!({"method":"connections.lifecycle","resourceRef":target["ref"]}),vec![target,server]);
    }
}

impl super::Observability {
    /// Domain owners enqueue safe facts without blocking their I/O/control locks.
    pub(crate) fn network_completed(&self,caller:&Caller,method:&str,operation:&str,result:&Result<Value>,resources:Vec<(String,u64)>,context:Option<Value>){
        let outcome=if result.as_ref().is_err_and(|e|e.code=="CANCELED"){"canceled"}else if result.is_ok(){"succeeded"}else{"failed"};
        let mut fields=json!({"method":method,"operationId":operation,"outcome":outcome});
        if let Err(error)=result{fields["errorCode"]=json!(error.code);}
        let Ok(definition)=definition("core.network.completed")else{return};
        if descriptor::check_fields(&definition.fields,&fields).is_err(){return;}
        let mut event=model::envelope(caller,&self.boot_id,&self.epoch,"domain","core.network.completed",fields);event["configEpoch"]=json!(self.config_epoch);event["provenance"]=json!("core-observed");event["operationId"]=json!(operation);event["outcome"]=json!(outcome);if let Err(error)=result{event["errorCode"]=json!(error.code);}
        event["targets"]=json!(resources.into_iter().take(8).map(|(id,generation)|super::targets::reference(&self.signing_key,&self.boot_id,"resource",&id,Some(&generation.to_string()))).collect::<Vec<_>>());
        if let Some(context)=context{context::attach(&mut event,&context);}
        if outcome=="failed"{self.request_automatic(caller,&event["targets"],None);}
        let key=self.signing_key.clone();let _=self.submit_partition(Partition::Facts,false,16*1024,move|db|{db.admit(Partition::Facts,16*1024)?;let tx=db.facts.transaction()?;catalog::save_type(&tx,"","events",&definition)?;model::insert(&tx,&key,"f",&mut event,None)?;tx.commit()?;Ok(Value::Null)});
    }
}
