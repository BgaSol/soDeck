use super::*;
use sodeck_plugin_manager::enablement::Change;
use sha2::{Digest,Sha256};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Registry { pub operations:VecDeque<PackageOperation> }
pub(super) struct PackageOperation { id:String, request:Value, pub result:Value }
pub(super) fn check(kind:&str,method:&str,value:&Value)->Result<()> {
    if serde_json::to_vec(value).unwrap().len()>64*1024{return Err(err("CAPACITY","插件管理数据超出容量"));}
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../../contracts/plugin-management-v1.schema.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||err("UNSUPPORTED_API","未知插件管理方法"))?;
    sodeck_core::contract::check(schema,&schema["$defs"][name],value,method).map_err(|e|err(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},e))
}
pub(super) fn manager_error(e:sodeck_plugin_manager::Error)->CoreError {
    let code=match &e{sodeck_plugin_manager::Error::Fault(code,_)=>*code,_=>"PACKAGE_INVALID"};
    // I/O errors may contain local archive paths. Keep those out of public errors/audit.
    let message:String=match &e {sodeck_plugin_manager::Error::Io(_)=>"无法读取或保存插件文件".into(),_=>e.to_string().chars().take(512).collect()};
    err(code,message)
}
impl State {
    pub(super) fn plugin_management_key(&self)->sodeck_communication::Key {
        sodeck_communication::Key {contract:"plugins.management.v1".into(),scope:self.app_generation.clone(),object:"plugins".into()}
    }
    fn management_snapshot(&self)->(String,Vec<Value>,Value) {
        let (mut rows,preference)={let manager=self.plugins.lock().unwrap();(manager.management_rows(),manager.enablement_snapshot())};
        let running=self.all_plugins().into_iter().filter(|p|p.process.is_alive()).map(|p|p.package.manifest.plugin_id.clone()).collect::<std::collections::HashSet<_>>();
        for row in &mut rows {
            let id=row["pluginId"].as_str().unwrap();
            row["runtimeStatus"]=json!(if running.contains(id){"running"}else if row["bootEnabled"]==false{"disabled"}else if !row["selected"].is_null()||!row["error"].is_null(){"failed"}else{"pending"});
            if row["runtimeStatus"]=="failed"&&row["error"].is_null(){row["error"]=json!(row["selected"]["error"].as_str().unwrap_or("插件未启动或后台已结束"));}
            if let Some(e)=row["error"].as_str(){row["error"]=json!(e.chars().take(512).collect::<String>());}
            for version in ["selected","pending"] {if let Some(e)=row[version]["error"].as_str(){row[version]["error"]=json!(e.chars().take(512).collect::<String>());}}
        }
        let revision=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!([rows,preference])).unwrap()));
        (revision,rows,preference)
    }
    pub(super) fn publish_plugin_management(&self)->Result<()> {
        let (revision,_,preference)=self.management_snapshot();
        self.communication.lock().unwrap().put(self.plugin_management_key(),json!({"appGeneration":self.app_generation}),json!({"catalogRevision":revision,"enablementRevision":preference["revision"],"storageStatus":preference["storageStatus"]})).map_err(|e|err(e.0,"插件状态同步失败"))?;
        Ok(())
    }
    pub(super) fn plugin_management_call(self:&Arc<Self>,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        check("x-methods",method,p)?;
        let result=match method {
            "plugins.management.list"=>{
                let (revision,mut rows,_)=self.management_snapshot();
                if let Some(query)=p["query"].as_str().filter(|q|!q.trim().is_empty()) {
                    let words=query.to_lowercase().split_whitespace().map(str::to_owned).collect::<Vec<_>>();
                    rows.retain(|r|{let text=format!("{} {} {}",r["pluginId"].as_str().unwrap(),r["displayName"].as_str().unwrap(),r["runtimeStatus"].as_str().unwrap()).to_lowercase();words.iter().all(|w|text.contains(w))});
                }
                let offset=if let Some(cursor)=p["cursor"].as_str() {
                    let (r,n)=cursor.split_once(':').ok_or_else(||err("INVALID_PARAMS","无效插件游标"))?;
                    if r!=revision{return Err(err("CONFLICT","插件列表已改变，请重新加载"));}
                    n.parse::<usize>().map_err(|_|err("INVALID_PARAMS","无效插件游标"))?
                }else{0};
                if offset>rows.len(){return Err(err("INVALID_PARAMS","插件游标超出范围"));}
                let mut end=offset;let mut bytes=256;
                for row in rows.iter().skip(offset).take(p["limit"].as_u64().unwrap_or(50) as usize) {let size=serde_json::to_vec(row).unwrap().len()+1;if bytes+size>60*1024{break;}bytes+=size;end+=1;}
                json!({"catalogRevision":revision,"items":rows[offset..end],"nextCursor":if end<rows.len(){Some(format!("{revision}:{end}"))}else{None}})
            },
            "plugins.enablement.get"=>self.plugins.lock().unwrap().enablement_snapshot(),
            "plugins.enablement.validate"=>{
                let changes:Vec<Change>=serde_json::from_value(p["changes"].clone()).map_err(|_|err("INVALID_PARAMS","启用更改无效"))?;
                self.plugins.lock().unwrap().validate_enablement(required(p,"generation")?,required(p,"expectedRevision")?,&changes).map_err(manager_error)?;
                json!({"valid":true})
            },
            "plugins.enablement.commit"|"plugins.enablement.repair"=>{
                let _shutdown=self.shutdown_lock.lock().unwrap();
                if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
                let operation=required(p,"operationId")?;let key=format!("core:plugins/{operation}");
                let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(p).unwrap()));
                {
                    let mut settings=self.settings.lock().unwrap();
                    if settings.exiting{return Err(err("SHUTTING_DOWN","应用正在退出"));}
                    if settings.operations.values().any(|o|o.running&&o.page=="core:plugins"){return Err(err("BUSY","插件配置正在保存或导入"));}
                    if settings.operations.len()>=128{settings.operations.retain(|_,o|o.running);}
                settings.operations.insert(key.clone(),settings::registry::Operation{page:"core:plugins".into(),digest,running:true,result:json!({"state":"running"})});
                }
                let saved=(||{
                    let mut manager=self.plugins.lock().unwrap();
                    if method=="plugins.enablement.repair"{manager.repair_enablement(required(p,"diagnosticToken")?,operation).map_err(manager_error)}
                    else {let changes:Vec<Change>=serde_json::from_value(p["changes"].clone()).map_err(|_|err("INVALID_PARAMS","启用更改无效"))?;
                        manager.commit_enablement(required(p,"generation")?,required(p,"expectedRevision")?,&changes,operation).map_err(manager_error)}
                })();
                if let Some(op)=self.settings.lock().unwrap().operations.get_mut(&key){op.running=false;op.result=match &saved{Ok(v)=>json!({"state":"saved","result":v}),Err(e)=>json!({"state":"rejected","reason":e.message})};}
                let result=saved?;let _=self.publish_plugin_management();result
            },
            "plugins.enablement.operation.get"=>{
                let saved=self.plugins.lock().unwrap().enablement_operation(required(p,"generation")?,required(p,"operationId")?);
                if saved["state"]=="saved"{saved}else{self.settings.lock().unwrap().operations.get(&format!("core:plugins/{}",required(p,"operationId")?)).map(|o|o.result.clone()).unwrap_or(saved)}
            },
            "plugins.package.prepare"=>self.prepare_plugin_operation(caller,p)?,
            "plugins.package.operation.get"=>self.plugin_management.lock().unwrap().operations.iter().find(|o|o.id==p["operationId"].as_str().unwrap()).map(|o|o.result.clone()).unwrap_or_else(||json!({"operationId":p["operationId"],"state":"unknown"})),
            _=>return Err(err("UNSUPPORTED_API","未知插件管理方法")),
        };
        check("x-results",method,&result)?;Ok(result)
    }
    pub(super) fn legacy_prepare_plugin(self:&Arc<Self>,caller:&Caller,p:&Value)->Result<Value>{
        let mut request=json!({"operationId":Uuid::new_v4().to_string()});
        if let Some(path)=p["path"].as_str().filter(|p|!p.is_empty()){request["path"]=json!(path);}
        let operation=self.prepare_plugin_operation(caller,&request)?;
        let started=Instant::now();
        loop {
            let value=self.plugin_management.lock().unwrap().operations.iter().find(|o|o.id==operation["operationId"].as_str().unwrap()).map(|o|o.result.clone()).ok_or_else(||err("RESULT_UNKNOWN","请在设置中核对导入结果"))?;
            match value["state"].as_str(){
                Some("prepared")=>return self.plugins.lock().unwrap().package_manifest(value["pluginId"].as_str().unwrap(),value["version"].as_str().unwrap()).map(|m|serde_json::to_value(m).unwrap()).ok_or_else(||err("RESULT_UNKNOWN","请核对候选版本")),
                Some("cancelled")=>return Err(err("CANCELLED","已取消选择插件包")),
                Some("failed")=>return Err(err("PACKAGE_INVALID",value["error"].as_str().unwrap_or("插件包无效"))),
                _=>{}
            }
            if started.elapsed()>Duration::from_secs(120){return Err(err("RESULT_UNKNOWN","导入仍在进行，请在设置中核对结果"));}
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn prepare_plugin_operation(self:&Arc<Self>,caller:&Caller,p:&Value)->Result<Value> {
        if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
        let id=required(p,"operationId")?.to_owned();
        let initial=json!({"operationId":id,"state":if p["path"].is_string(){"verifying"}else{"selecting"}});
        let key=format!("core:plugins/package/{id}");
        {
            let mut registry=self.plugin_management.lock().unwrap();
            if let Some(old)=registry.operations.iter().find(|o|o.id==id) {return if old.request==*p{Ok(old.result.clone())}else{Err(err("CONFLICT","同一导入操作不能选择不同插件包"))};}
            if registry.operations.iter().any(|o|matches!(o.result["state"].as_str(),Some("selecting"|"verifying"))){return Err(err("BUSY","正在导入另一个插件包"));}
            let mut settings=self.settings.lock().unwrap();
            if settings.exiting||settings.operations.values().any(|o|o.running&&o.page=="core:plugins"){return Err(err("BUSY","插件配置正在保存或应用退出中"));}
            if settings.operations.len()>=128{settings.operations.retain(|_,o|o.running);}
                settings.operations.insert(key.clone(),settings::registry::Operation{page:"core:plugins".into(),digest:id.clone(),running:true,result:json!({"state":"running"})});
            registry.operations.push_back(PackageOperation{id:id.clone(),request:p.clone(),result:initial.clone()});
            while registry.operations.len()>128{registry.operations.pop_front();}
        }
        let state=self.clone();let caller=caller.clone();let path=p["path"].as_str().map(str::to_owned);
        std::thread::spawn(move||{
            let result=(||->Result<Value>{
                let path=match path {Some(p)=>PathBuf::from(p),None=>match rfd::FileDialog::new().add_filter("SoDeck 插件",&["sodeck-plugin"]).pick_file(){Some(p)=>p,None=>return Ok(json!({"operationId":id,"state":"cancelled"}))}};
                state.package_operation_status(&id,json!({"operationId":id,"state":"verifying"}));
                let stager=state.plugins.lock().unwrap().stager();
                let staged=stager.stage(&path).map_err(manager_error)?;
                if state.stopping.load(Ordering::SeqCst){return Err(err("CANCELLED","应用已退出，未发布候选包"));}
                let _shutdown=state.shutdown_lock.lock().unwrap();
                let mut manager=state.plugins.lock().unwrap();
                if state.stopping.load(Ordering::SeqCst){return Err(err("CANCELLED","应用已退出，未发布候选包"));}
                let manifest=state.updates.accept_package(&mut manager,staged,sodeck_updater::Origin::Local,&id,false,None).map_err(|e|err(&e.code,e.message))?;
                let row=manager.management_rows().into_iter().find(|r|r["pluginId"]==manifest.plugin_id).unwrap();
                Ok(json!({"operationId":id,"state":"prepared","pluginId":manifest.plugin_id,"version":manifest.version,"signature":row["pending"]["signature"],"digest":row["pending"]["digest"]}))
            })();
            let value=match result{Ok(v)=>v,Err(e)=>json!({"operationId":id,"state":"failed","code":e.code,"error":e.message})};
            state.package_operation_status(&id,value.clone());
            if let Some(op)=state.settings.lock().unwrap().operations.get_mut(&key){op.running=false;op.result=json!({"state":"rejected","reason":"导入操作已结束，请查询插件导入回执"});}
            let _=state.publish_plugin_management();
            let _=state.settings_catalog(&Caller::shell(),&json!({}));
            let _=state.core.record(&caller,"plugins.package.completed",&json!({"operationId":id,"pluginId":value["pluginId"],"version":value["version"],"state":value["state"],"code":value["code"]}),||Ok(value.clone()));
        });
        Ok(initial)
    }
    fn package_operation_status(&self,id:&str,value:Value){if let Some(op)=self.plugin_management.lock().unwrap().operations.iter_mut().find(|o|o.id==id){op.result=value;}}
}
