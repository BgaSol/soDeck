use super::*;
use super::registry::{authorize,plugin_page,Operation};
use sodeck_plugin_manager::settings_descriptor::Entry;
use sha2::{Digest,Sha256};
fn digest(p:&Value)->String {let mut p=p.clone();p.as_object_mut().unwrap().remove("operationId");if let Some(changes)=p["changes"].as_array_mut(){changes.sort_by(|a,b|a["key"].as_str().cmp(&b["key"].as_str()));}format!("{:x}",Sha256::digest(serde_json::to_vec(&p).unwrap()))}
impl State {
    pub(in super::super) fn settings_publish(&self,snapshot:Value)->Result<Value>{
        if serde_json::to_vec(&snapshot).unwrap().len()>24*1024{return Err(err("CAPACITY","设置状态超出容量"));}
        let key=self.settings_key(required(&snapshot,"pageId")?);
        let mut hub=self.communication.lock().unwrap();
        if let Ok(previous)=hub.get(&key){
            if previous.value["providerEpoch"]==snapshot["providerEpoch"] && previous.value["settingsRevision"].as_str().and_then(|s|s.parse::<u64>().ok())>snapshot["settingsRevision"].as_str().and_then(|s|s.parse::<u64>().ok()) {return Ok(previous.value.clone());}
        }
        hub.put(key,json!({"appGeneration":self.app_generation}),snapshot.clone()).map_err(|e|err(e.0,"设置已保存，状态同步待恢复"))?;Ok(snapshot)
    }
    pub(in super::super) fn settings_provider_changed(&self,plugin:&str){
        let _=self.publish_plugin_management();
        let loaded=self.loaded_for(plugin);
        if let Ok(loaded)=loaded {
            if let Ok(Some(descriptor))=&loaded.settings_descriptor{for page in &descriptor.pages{if let Ok(snapshot)=loaded.preferences.read(&loaded.instance,&page.id){let _=self.settings_publish(snapshot);}}}
        }else{
            let mut hub=self.communication.lock().unwrap();let prefix=format!("plugin:{plugin}:");let keys=hub.keys().filter(|k|k.contract=="settings.page.v1"&&k.object.starts_with(&prefix)).cloned().collect::<Vec<_>>();
            for key in keys{if let Ok(snapshot)=hub.get(&key){let mut value=snapshot.value.clone();value["syncState"]=json!("unavailable");let _=hub.put(key,json!({"appGeneration":self.app_generation}),value);}}
        }
        let _=self.settings_catalog(&Caller::shell(),&json!({}));
    }
    pub(in super::super) fn settings_call(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        check("x-methods",method,p)?;
        let result=if method=="settings.catalog.list"{self.settings_catalog(caller,p)?}else{
            let page=required(p,"pageId")?;authorize(caller,page)?;
            match method {
                "settings.page.get"=>self.settings_page(page)?,
                "settings.validate"=>self.settings_validate(page,p)?,
                "settings.commit"=>self.settings_commit(page,p)?,
                "settings.operation.get"=>self.settings_operation(page,p)?,
                "settings.sync.retry"=>self.settings_sync(page,p)?,
                _=>return Err(err("UNSUPPORTED_API","未知设置方法")),
            }
        };check("x-results",method,&result)?;Ok(result)
    }
    pub(super) fn settings_page(&self,page:&str)->Result<Value>{
        if page=="core:appearance"{return self.appearance_settings_page();}
        let (loaded,definition)=self.settings_provider(page)?;
        let snapshot=self.invoke_settings(&loaded,"settings.provider.read",json!({"groupId":definition.id,"descriptor":loaded.settings_descriptor.as_ref().unwrap()}))?;
        check("x-results","storage.preferences.read",&snapshot)?;
        if snapshot["pageId"]!=page||snapshot["providerEpoch"]!=loaded.instance||snapshot["storageGeneration"]!=loaded.preferences.generation(){return Err(err("PROTOCOL_ERROR","设置快照来源不匹配"));}
        let snapshot=self.settings_publish(snapshot)?;
        Ok(json!({"entry":{"pageId":page,"title":definition.title,"owner":"plugin","pluginId":loaded.package.manifest.plugin_id,"pluginName":loaded.package.manifest.display_name(),"pluginVersion":loaded.package.manifest.version,"order":definition.order,"available":true},"definition":definition,"snapshot":snapshot}))
    }
    fn settings_validate(&self,page:&str,p:&Value)->Result<Value>{
        if page=="core:appearance"{return self.appearance_settings_validate(p);}
        let (loaded,definition)=self.settings_provider(page)?;
        if p["providerEpoch"]!=loaded.instance||p["schemaVersion"]!=definition.schema_version{return Err(err("STALE_PROVIDER","设置描述已更新，请重新载入"));}
        let changes:Vec<Entry>=serde_json::from_value(p["changes"].clone()).map_err(|e|err("INVALID_PARAMS",e.to_string()))?;
        definition.validate_changes(&changes).map_err(|e|err("INVALID_PARAMS",e))?;
        let snapshot=loaded.preferences.read(&loaded.instance,&definition.id)?;
        if snapshot["settingsRevision"]!=p["expectedRevision"]{return Err(err("CONFLICT","设置已在其他位置改变，请载入最新值"));}
        let result=self.invoke_settings(&loaded,"settings.provider.validate",json!({"groupId":definition.id,"changes":changes}))?;
        check("x-results","settings.validate",&result)?;Ok(result)
    }
    fn settings_commit(&self,page:&str,p:&Value)->Result<Value>{
        let operation=required(p,"operationId")?;let digest=digest(p);let key=format!("{page}/{operation}");
        // Check durable receipts before the revision check, including after restart.
        if page=="core:appearance"{if let Some(receipt)=crate::theme::operation(&self.data,operation).map_err(|e|err("STORAGE_ERROR",e))?{if receipt["digest"]!=digest{return Err(err("CONFLICT","操作编号已被使用"));}return Ok(receipt["result"].clone());}}
        else {let (loaded,definition)=self.settings_provider(page)?;if let Some(r)=loaded.preferences.receipt(&definition.id,operation)?{if r.digest!=digest{return Err(err("CONFLICT","操作编号已被使用"));}return Ok(r.result(false));}}
        {
            let mut r=self.settings.lock().unwrap();
            if r.exiting{return Err(err("SHUTTING_DOWN","应用正在退出"));}
            if let Some(op)=r.operations.get(&key){if op.digest!=digest{return Err(err("CONFLICT","操作编号已被使用"));}if op.running{return Err(err("BUSY","正在核对此设置操作"));}}
            if r.operations.values().filter(|o|o.running).count()>=8||r.operations.values().any(|o|o.running&&o.page==page){return Err(err("BUSY","设置正在保存"));}
            if r.operations.len()>=128{r.operations.retain(|_,o|o.running);}
            r.operations.insert(key.clone(),Operation{page:page.into(),digest:digest.clone(),running:true,result:json!({"state":"running"})});
        }
        let result=(|| {
            let validation=self.settings_validate(page,p)?;
            if !validation["errors"].as_array().unwrap().is_empty(){return Err(err("VALIDATION_FAILED",validation.to_string()));}
            self.communication.lock().unwrap().reserve(self.settings_key(page),32*1024).map_err(|e|err(e.0,"设置状态容量不足"))?;
            if page=="core:appearance"{return self.appearance_settings_commit(p,&digest);}
            let (loaded,definition)=self.settings_provider(page)?;
            let ticket=Uuid::new_v4().to_string();
            loaded.preferences.issue(plugin_preferences::Ticket{id:ticket.clone(),epoch:loaded.instance.clone(),group:definition.clone(),expected:required(p,"expectedRevision")?.into(),operation:operation.into(),digest:digest.clone(),keys:p["changes"].as_array().unwrap().iter().map(|v|v["key"].as_str().unwrap().to_owned()).collect(),applied:None,saved:false})?;
            let callback=self.invoke_settings(&loaded,"settings.provider.commit",json!({"groupId":definition.id,"changes":p["changes"],"ticket":ticket}));
            // Revocation and writes share the file lock; a late callback cannot start a second write.
            let finished=loaded.preferences.finish(&ticket);
            if let Some(receipt)=loaded.preferences.receipt(&definition.id,operation)? {
                let applied=finished.and_then(|t|t.applied).is_some_and(|r|r==receipt.persisted_revision);
                let snapshot=loaded.preferences.read(&loaded.instance,&definition.id).map(|mut s|{s["syncState"]=json!(if applied{"current"}else{"pending"});s});
                let published=snapshot.and_then(|s|self.settings_publish(s)).is_ok();
                return Ok(receipt.result(applied&&published));
            }
            callback?;Err(err("PROTOCOL_ERROR","插件未提供持久化回执"))
        })();
        if let Some(op)=self.settings.lock().unwrap().operations.get_mut(&key){op.running=false;op.result=match &result{Ok(value)=>json!({"state":"saved","result":value}),Err(error)=>json!({"state":"rejected","reason":error.message})};}
        result
    }
    fn settings_operation(&self,page:&str,p:&Value)->Result<Value>{
        let operation=required(p,"operationId")?;
        if page=="core:appearance"{return self.appearance_settings_operation(p);}
        let (plugin,group)=plugin_page(page)?;
        let package=self.plugins.lock().unwrap().active(plugin).map_err(|e|err("STORAGE_ERROR",e.to_string()))?;
        let Some(package)=package else{return Ok(json!({"state":"unknown","reason":"插件当前数据代次不可用"}));};
        let store=plugin_preferences::Store::shared(&package.data_dir);
        if p["storageGeneration"]!=store.generation(){return Ok(json!({"state":"unknown","reason":"插件数据代次已改变"}));}
        if let Some(r)=store.receipt(group,operation)?{return Ok(json!({"state":"saved","result":r.result(false)}));}
        let r=self.settings.lock().unwrap();Ok(r.operations.get(&format!("{page}/{operation}")).map(|o|o.result.clone()).unwrap_or_else(||json!({"state":"unknown","reason":"没有可核对的持久化回执，请载入当前值"})))
    }
    fn settings_sync(&self,page:&str,p:&Value)->Result<Value>{
        if page=="core:appearance"{let snapshot=self.appearance_settings_page()?["snapshot"].clone();if snapshot["settingsRevision"]!=p["settingsRevision"]{return Err(err("CONFLICT","设置修订已改变"));}return self.settings_publish(snapshot);}
        let (loaded,definition)=self.settings_provider(page)?;let snapshot=loaded.preferences.read(&loaded.instance,&definition.id)?;
        if snapshot["settingsRevision"]!=p["settingsRevision"]{return Err(err("CONFLICT","设置修订已改变"));}
        let id=Uuid::new_v4().to_string();loaded.preferences.issue(plugin_preferences::Ticket{id:id.clone(),epoch:loaded.instance.clone(),group:definition.clone(),expected:required(p,"settingsRevision")?.into(),operation:id.clone(),digest:String::new(),keys:Default::default(),applied:None,saved:false})?;
        let result=self.invoke_settings(&loaded,"settings.provider.sync",json!({"groupId":definition.id,"ticket":id}));let ticket=loaded.preferences.finish(&id);result?;
        if ticket.and_then(|t|t.applied).is_none(){return Err(err("SYNC_PENDING","设置已保存，插件同步待恢复"));}self.settings_publish(snapshot)
    }
}
