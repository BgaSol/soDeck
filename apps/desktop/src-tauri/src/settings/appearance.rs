use super::*;
pub(super) fn definition()->Value {
    let catalog=crate::theme::catalog();let options=catalog["themes"].as_array().unwrap().iter().map(|t|json!({"value":t["id"],"label":t["label"]})).collect::<Vec<_>>();
    json!({"id":"appearance","title":"外观","order":0,"schemaVersion":1,"sections":[{"id":"theme","title":"应用主题","fields":[{"key":"themeId","label":"主题","type":"enum","default":catalog["defaultThemeId"],"effect":"immediate","options":options,"keywords":["颜色","明暗","主题"]}]}]})
}
fn snapshot(theme:&crate::theme::Appearance,epoch:&str)->Value {
    json!({"pageId":"core:appearance","providerEpoch":epoch,"storageGeneration":theme.storage_generation,"schemaVersion":1,"settingsRevision":theme.config_revision.to_string(),"values":[{"key":"themeId","value":theme.selected_id}],"syncState":"current","warnings":if theme.read_only{vec!["偏好格式较新，当前版本只读"]}else if theme.preference_invalid{vec!["偏好文件无法读取；应用时会备份原文件并修复"]}else{vec![]}})
}
impl State {
    pub(in super::super) fn appearance_settings_page(&self)->Result<Value>{
        let value={let theme=self.theme.lock().unwrap();snapshot(&theme,&self.app_generation)};self.settings_publish(value.clone())?;
        Ok(json!({"entry":{"pageId":"core:appearance","title":"外观","owner":"core","order":0,"available":true},"definition":definition(),"snapshot":value}))
    }
    pub(super) fn appearance_settings_validate(&self,p:&Value)->Result<Value>{
        let theme=self.theme.lock().unwrap();
        if theme.read_only{return Err(err("READ_ONLY","外观偏好格式较新，当前版本只读"));}
        if p["providerEpoch"]!=self.app_generation||p["schemaVersion"]!=1{return Err(err("STALE_PROVIDER","设置来源已更新"));}
        if p["expectedRevision"]!=theme.config_revision.to_string(){return Err(err("CONFLICT","外观已在其他位置改变，请载入最新值"));}
        let definition:sodeck_plugin_manager::settings_descriptor::Page=serde_json::from_value(definition()).unwrap();
        definition.validate_changes(&serde_json::from_value::<Vec<sodeck_plugin_manager::settings_descriptor::Entry>>(p["changes"].clone()).map_err(|e|err("INVALID_PARAMS",e.to_string()))?).map_err(|e|err("INVALID_PARAMS",e))?;
        Ok(json!({"errors":[]}))
    }
    pub(super) fn appearance_settings_commit(&self,p:&Value,digest:&str)->Result<Value>{
        let mut theme=self.theme.lock().unwrap();
        if p["expectedRevision"]!=theme.config_revision.to_string(){return Err(err("CONFLICT","外观已在其他位置改变"));}
        let id=p["changes"].as_array().unwrap().iter().find(|e|e["key"]=="themeId").and_then(|e|e["value"].as_str()).unwrap_or(&theme.selected_id);
        let next=theme.chosen(id).map_err(|e|err("INVALID_PARAMS",e))?;
        let mut result=crate::theme::save_selection(&self.data,&theme,id,required(p,"operationId")?,digest).map_err(|e|err("PERSISTENCE_FAILED",e))?;
        *theme=next;let settings=snapshot(&theme,&self.app_generation);let effective=theme.effective.clone();drop(theme);
        let published=self.communication.lock().unwrap().put(self.theme_key(),json!({"appGeneration":self.app_generation}),effective).is_ok();
        let settings_published=self.settings_publish(settings).is_ok();
        if published&&settings_published{result["synchronization"]=json!("current");}self.refresh_native_theme();Ok(result)
    }
    pub(super) fn appearance_settings_operation(&self,p:&Value)->Result<Value>{
        if p["storageGeneration"]!=self.theme.lock().unwrap().storage_generation{return Ok(json!({"state":"unknown","reason":"外观存储代次已改变"}));}
        Ok(match crate::theme::operation(&self.data,required(p,"operationId")?).map_err(|e|err("STORAGE_ERROR",e))?{Some(r)=>json!({"state":"saved","result":r["result"]}),None=>json!({"state":"unknown","reason":"未找到持久化回执"})})
    }
}
