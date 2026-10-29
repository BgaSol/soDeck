use super::*;
use sodeck_plugin_manager::settings_descriptor::Page;
#[derive(Default)]
pub(in super::super) struct Registry {pub revision:u64,pub catalog:Vec<Value>,pub operations:HashMap<String,Operation>,pub exiting:bool}
pub(in super::super) struct Operation {pub page:String,pub digest:String,pub running:bool,pub result:Value}
pub(super) fn authorize(caller:&Caller,page:&str)->Result<()> {
    if let Some(rest)=page.strip_prefix("plugin:") {
        let (owner,_)=rest.split_once(':').ok_or_else(||err("INVALID_PARAMS","设置页标识无效"))?;
        if caller.plugin_id.as_ref().is_some_and(|id|id!=owner){return Err(err("SOURCE_INVALID","不允许跨插件访问设置"));}
    } else if !matches!(page,"core:appearance"|"core:application"|"core:plugins"|"core:updates"){return Err(err("SETTINGS_UNAVAILABLE","设置页不存在"));}Ok(())
}
pub(super) fn plugin_page(page:&str)->Result<(&str,&str)>{page.strip_prefix("plugin:").and_then(|s|s.split_once(':')).ok_or_else(||err("INVALID_PARAMS","插件设置页标识无效"))}
impl State {
    pub(in super::super) fn settings_catalog(&self,caller:&Caller,p:&Value)->Result<Value> {
        let mut items=vec![json!({"pageId":"core:appearance","title":"外观","owner":"core","order":0,"available":true,"definition":appearance::definition()}),json!({"pageId":"core:application","title":"应用与数据","owner":"core","order":10,"available":true}),json!({"pageId":"core:plugins","title":"插件管理","owner":"core","order":20,"available":true})];
        let packages={let manager=self.plugins.lock().unwrap();manager.status().into_iter().filter(|s|s.active_version.is_some()).filter_map(|s|manager.selected_metadata(&s.plugin_id)).collect::<Vec<_>>()};
        items.push(json!({"pageId":"core:updates","title":"更新","owner":"core","order":30,"available":true}));
        for package in packages {
            let id=&package.manifest.plugin_id;
            if package.manifest.contributes.settings.is_none(){continue;}
            let descriptor=package.settings.and_then(|d|d.ok_or_else(||"缺少设置描述".into()));
            let loaded=self.loaded_for(id);
            match descriptor {
                Ok(descriptor)=>for page in descriptor.pages {
                    let mut entry=json!({"pageId":format!("plugin:{id}:{}",page.id),"title":page.title,"owner":"plugin","pluginId":id,"pluginName":package.manifest.display_name(),"pluginVersion":package.manifest.version,"order":page.order,"available":loaded.is_ok(),"definition":page});
                    if let Err(error)=&loaded{entry["reason"]=json!(if !self.plugins.lock().unwrap().boot_enabled(id){"插件已停用，启用并重启后可配置"}else{&error.message});}items.push(entry);
                },
                Err(reason)=>items.push(json!({"pageId":format!("plugin:{id}:unavailable"),"title":package.manifest.display_name(),"owner":"plugin","pluginId":id,"pluginName":package.manifest.display_name(),"pluginVersion":package.manifest.version,"order":100000,"available":false,"reason":reason.chars().take(512).collect::<String>()})),
            }
        }
        items.sort_by(|a,b|a["order"].as_i64().cmp(&b["order"].as_i64()).then_with(||a["pageId"].as_str().cmp(&b["pageId"].as_str())));
        for entry in items.iter_mut().skip(128){entry["available"]=json!(false);entry["reason"]=json!("设置页面数量达到上限");entry.as_object_mut().unwrap().remove("definition");}
        let revision={let mut registry=self.settings.lock().unwrap();if registry.catalog!=items{registry.revision+=1;registry.catalog=items.clone();}registry.revision};
        self.communication.lock().unwrap().put(self.settings_key("catalog"),json!({"appGeneration":self.app_generation}),json!({"catalogRevision":revision.to_string()})).map_err(|e|err(e.0,"设置目录超出容量"))?;
        items.retain(|entry|authorize(caller,entry["pageId"].as_str().unwrap()).is_ok());
        let start=if let Some(cursor)=p["cursor"].as_str(){let (r,i)=cursor.split_once(':').ok_or_else(||err("INVALID_PARAMS","设置目录游标无效"))?;if r!=revision.to_string(){return Err(err("CONFLICT","设置目录已改变，请重新加载"));}i.parse::<usize>().map_err(|_|err("INVALID_PARAMS","目录游标无效"))?}else{0};
        if start>items.len(){return Err(err("INVALID_PARAMS","目录游标超出范围"));}
        let mut end=start;let mut bytes=256;for item in items.iter().skip(start).take(p["limit"].as_u64().unwrap_or(50) as usize){let size=serde_json::to_vec(item).unwrap().len()+1;if bytes+size>60*1024{break;}bytes+=size;end+=1;}
        Ok(json!({"catalogRevision":revision.to_string(),"items":items[start..end],"nextCursor":if end<items.len(){Some(format!("{revision}:{end}"))}else{None}}))
    }
    pub(in super::super) fn settings_key(&self,page:&str)->sodeck_communication::Key {sodeck_communication::Key{contract:if page=="catalog"{"settings.catalog.v1"}else{"settings.page.v1"}.into(),scope:self.app_generation.clone(),object:page.into()}}
    pub(super) fn settings_provider(&self,page:&str)->Result<(Arc<Loaded>,Page)> {
        let (id,group)=plugin_page(page)?;let loaded=self.loaded_for(id)?;
        let definition=loaded.settings_descriptor.as_ref().map_err(|e|err("SETTINGS_UNAVAILABLE",e.clone()))?.as_ref().and_then(|d|d.pages.iter().find(|p|p.id==group)).cloned().ok_or_else(||err("SETTINGS_UNAVAILABLE","插件没有声明此设置组"))?;
        Ok((loaded,definition))
    }
    pub(super) fn invoke_settings(&self,loaded:&Loaded,method:&str,args:Value)->Result<Value> {
        static READS:AtomicUsize=AtomicUsize::new(0);
        struct ReadPermit(bool);impl Drop for ReadPermit{fn drop(&mut self){if self.0{READS.fetch_sub(1,Ordering::SeqCst);}}}
        let _permit=if method=="settings.provider.read"{let before=READS.fetch_add(1,Ordering::SeqCst);if before>=4{READS.fetch_sub(1,Ordering::SeqCst);return Err(err("BUSY","设置读取繁忙，请重试"));}ReadPermit(true)}else{ReadPermit(false)};
        let owner=Caller{source:"plugin".into(),plugin_id:Some(loaded.package.manifest.plugin_id.clone()),plugin_version:Some(loaded.package.manifest.version.clone()),instance_id:loaded.instance.clone(),view_id:None,mount_generation:None};
        let metadata=self.core.transfer_diagnostic_context(&owner)?;
        let result=loaded.process.invoke_with_context(method,args,Duration::from_secs(if method=="settings.provider.commit"{10}else{5}),metadata).map_err(|e|err(&e.code,e.message))?;
        if self.loaded_for(&loaded.package.manifest.plugin_id)?.instance!=loaded.instance{return Err(err("STALE_PROVIDER","设置提供者已重建"));}Ok(result)
    }
}
