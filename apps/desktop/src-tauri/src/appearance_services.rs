use super::*;
use sodeck_communication::Key;

impl State {
    pub(super) fn theme_background(&self)->tauri::window::Color {let (r,g,b,a)=crate::theme::background(&self.theme_snapshot(true));tauri::window::Color(r,g,b,a)}
    pub(super) fn refresh_native_theme(&self){
        let theme=self.theme_snapshot(true);let (r,g,b,a)=crate::theme::background(&theme);
        for (id,window) in self.app.webview_windows(){self.cancel_menus(&id);if !id.starts_with("plugin-dialog-"){let _=window.set_background_color(Some(tauri::window::Color(r,g,b,a)));}}
        let live:std::collections::HashSet<_>=self.all_plugins().iter().filter(|p|p.package.manifest.requires.capabilities.iter().any(|c|c=="ui.theme.live.v1")).map(|p|p.package.manifest.plugin_id.clone()).collect();
        let views:std::collections::HashSet<_>=self.workbench.lock().unwrap().views.iter().filter(|v|live.contains(&v.plugin_id)).map(|v|v.view_id.clone()).collect();
        let ids=self.bindings.lock().unwrap().iter().filter(|(_,b)|views.contains(&b.view_id)).map(|(id,_)|id.clone()).collect();
        self.native.backgrounds(ids,(r,g,b,a));
    }
    pub(super) fn theme_key(&self)->Key{Key{contract:"app.theme.v1".into(),scope:self.app_generation.clone(),object:"theme".into()}}
    pub(super) fn supports_live_theme(&self, caller:&Caller)->bool{
        caller.plugin_id.as_ref().is_none_or(|id|self.loaded_for(id).is_ok_and(|p|p.package.manifest.requires.capabilities.iter().any(|c|c=="ui.theme.live.v1")))
    }
    pub(super) fn theme_snapshot(&self, live:bool)->Value{
        let theme=self.theme.lock().unwrap();if live{theme.effective.clone()}else{crate::theme::legacy(&theme.effective)}
    }
    fn theme_preference(&self, theme:&crate::theme::Appearance)->Value{
        let old=self.all_plugins().iter().filter(|p|!p.package.manifest.requires.capabilities.iter().any(|c|c=="ui.theme.live.v1")).map(|p|p.package.manifest.plugin_id.clone()).collect::<Vec<_>>();
        json!({"mode":theme.mode,"themeId":theme.effective["id"],"revision":theme.revision.to_string(),"highContrast":theme.effective["highContrast"],"testOverride":theme.test_override,"legacyPlugins":old})
    }
    fn theme_catalog(&self,theme:&crate::theme::Appearance)->Value{
        let preference=self.theme_preference(theme);
        let partial=self.all_plugins().iter().filter(|p|{let c=&p.package.manifest.requires.capabilities;c.iter().any(|c|c=="ui.theme.live.v1")&&!c.iter().any(|c|c=="ui.theme.dopamine.v1")}).map(|p|p.package.manifest.plugin_id.clone()).collect::<Vec<_>>();
        let catalog=crate::theme::catalog();
        let entries=catalog["themes"].as_array().unwrap().iter().map(|entry|{let mut preview=crate::theme::by_id(entry["id"].as_str().unwrap()).unwrap();preview["revision"]=json!("1");preview["highContrast"]=json!(false);preview["testOverride"]=json!(false);json!({"id":entry["id"],"label":entry["label"],"mode":entry["mode"],"preview":preview})}).collect::<Vec<_>>();
        json!({"revision":theme.revision.to_string(),"selectedThemeId":theme.selected_id,"effectiveThemeId":theme.effective["id"],"defaultThemeId":catalog["defaultThemeId"],"highContrast":theme.effective["highContrast"],"testOverride":theme.test_override,"legacyPlugins":preference["legacyPlugins"],"partialStylePlugins":partial,"themes":entries})
    }
    pub(super) fn appearance_service(&self, caller:&Caller,method:&str,p:&Value)->Result<Value>{
        if method=="ui.theme.get"{return Ok(self.theme_snapshot(self.supports_live_theme(caller)));}
        if !self.supports_live_theme(caller){return Err(err("UNSUPPORTED_API","此插件未声明实时主题契约"));}
        let mut theme=self.theme.lock().unwrap();
        match method{
            "ui.theme.preference.get"=>Ok(self.theme_preference(&theme)),
            "ui.theme.catalog.get"=>Ok(self.theme_catalog(&theme)),
            "ui.theme.select"|"ui.theme.choose"=>{
                let expected=required(p,"expectedRevision")?;
                if expected!=theme.revision.to_string(){return Err(err("REVISION_CONFLICT","外观已在其他窗口更改，请重新选择。"));}
                let catalog=method=="ui.theme.choose";
                let id=if catalog{required(p,"themeId")?}else{match required(p,"mode")?{"light"=>"soft-light","dark"=>"soft-dark",_=>return Err(err("INVALID_PARAMS","未知明暗模式"))}};
                if id==theme.selected_id&&!theme.test_override&&!theme.preference_invalid{return Ok(if catalog{self.theme_catalog(&theme)}else{self.theme_preference(&theme)});}
                let next=theme.chosen(id).map_err(|e|err("INVALID_PARAMS",e))?;
                // The theme slot is reserved at startup. No Hub/workbench lock
                // is held during file I/O; the theme mutex serializes writers.
                crate::theme::save_selection(&self.data,&theme,id,&Uuid::new_v4().to_string(),"legacy").map_err(|e|err("PERSISTENCE_FAILED",format!("外观未更改：{e}")))?;
                self.communication.lock().unwrap().put(self.theme_key(),json!({"appGeneration":self.app_generation}),next.effective.clone()).map_err(|e|err(e.0,"主题状态提交失败"))?;
                *theme=next;let result=if catalog{self.theme_catalog(&theme)}else{self.theme_preference(&theme)};drop(theme);
                self.refresh_native_theme();
                let _=self.appearance_settings_page();
                Ok(result)
            }
            _=>Err(err("METHOD_NOT_FOUND","未知主题接口")),
        }
    }
    pub(super) fn refresh_system_theme(&self){
        let Ok(mut theme)=self.theme.try_lock()else{return;};
        let next=theme.render();if next==theme.effective{return;}
        let Some(revision)=theme.revision.checked_add(1)else{return;};
        let mut next=next;next["revision"]=json!(revision.to_string());
        if self.communication.lock().unwrap().put(self.theme_key(),json!({"appGeneration":self.app_generation}),next.clone()).is_ok(){theme.revision=revision;theme.effective=next;drop(theme);for id in self.app.webview_windows().keys(){self.cancel_menus(id);}}
    }
}
