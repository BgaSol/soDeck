//! Temporary owned native surfaces: no workbench view, worker, or resource lease.
use super::*;

pub(super) struct Session {
    pub id: String,
    pub source: String,
    pub label: String,
    pub owner: String,
    pub parent: String,
    pub caller: Caller,
    pub feed_owner: String,
    pub cleaned: AtomicBool,
    request_id: String,
    request: Value,
    opened: Instant,
    outcome: Mutex<Value>,
    finished: Mutex<Option<Instant>>,
    suspended: AtomicBool,
}
impl Session {
    fn result(&self) -> Value { self.outcome.lock().unwrap().clone() }
    fn result_for(&self,source:&str)->Value {
        // The parent may start authentication as soon as it observes completion.
        // Never expose that result while the temporary input barrier still exists.
        if source==self.parent&&self.terminal()&&!self.cleaned.load(Ordering::SeqCst){json!({"sessionId":self.id,"status":"open"})}else{self.result()}
    }
    pub(super) fn terminal(&self) -> bool { self.finished.lock().unwrap().is_some() }
}

impl State {
    pub(super) fn dialog_parent_valid(&self, d: &Session) -> bool {
        if self.stopping.load(Ordering::SeqCst) || self.app.get_webview_window(&d.owner).is_none() {return false;}
        let Ok(parent)=self.binding(&d.parent) else{return false;};
        if parent.phase.load(Ordering::SeqCst)!=1 || parent.generation!=d.caller.mount_generation.unwrap_or(0) || parent.window!=d.owner {return false;}
        self.plugin_for_view(&parent.view_id).is_ok_and(|p|p.instance==d.caller.instance_id)
            && self.workbench.lock().unwrap().view(&parent.view_id).is_ok_and(|v|v.mount_generation==parent.generation)
    }
    pub(super) fn dialog_call(self:&Arc<Self>,caller:&Caller,source:Option<&str>,method:&str,p:&Value)->Result<Value>{
        let source=source.ok_or_else(||err("INVALID_CONTEXT","弹窗需要有效插件表面"))?;
        if method=="ui.dialog.open" { return self.open_plugin_dialog(caller,source,p); }
        let id=required(p,"sessionId")?;
        let d=self.dialogs.lock().unwrap().get(id).cloned().ok_or_else(||err("DIALOG_EXPIRED","弹窗回执已过期"))?;
        if source!=d.source && source!=d.parent {return Err(err("SOURCE_INVALID","弹窗不属于此表面"));}
        if d.terminal() {return Ok(d.result_for(source));}
        if !self.dialog_parent_valid(&d) {self.finish_plugin_dialog(&d,"cancelled",None);return Ok(d.result_for(source));}
        match method {
            "ui.dialog.ready"=>{
                if source!=d.source {return Err(err("INVALID_CONTEXT","仅弹窗可报告就绪"));}
                let _transition=self.dialog_transition.lock().unwrap();
                if d.terminal(){return Ok(d.result());}
                d.outcome.lock().unwrap()["status"]=json!("open");
                if !d.suspended.load(Ordering::SeqCst){
                    let result=self.show_plugin_dialog(&d);drop(_transition);
                    if let Err(error)=result {self.finish_plugin_dialog(&d,"failed",Some(json!({"code":"DIALOG_START_FAILED","message":error.message})));}
                }
            }
            "ui.dialog.complete"=>{
                if source!=d.source {return Err(err("INVALID_CONTEXT","仅弹窗可提交结果"));}
                if serde_json::to_vec(&p["result"]).unwrap().len()>16*1024{return Err(err("CAPACITY","弹窗结果超过16 KiB"));}
                self.finish_plugin_dialog(&d,"completed",Some(p["result"].clone()));
            }
            "ui.dialog.close"=>{self.finish_plugin_dialog(&d,"cancelled",None);}
            "ui.dialog.result"=>{}
            _=>return Err(err("UNSUPPORTED_API","未知弹窗方法")),
        }
        Ok(d.result_for(source))
    }
    fn open_plugin_dialog(self:&Arc<Self>,caller:&Caller,source:&str,p:&Value)->Result<Value>{
        if source.starts_with('@') {return Err(err("INVALID_CONTEXT","请从主插件表面打开弹窗"));}
        let parent=self.binding(source)?;
        if parent.phase.load(Ordering::SeqCst)!=1 || !parent.geometry.lock().unwrap().1 {return Err(err("INVALID_CONTEXT","来源表面不可见"));}
        let plugin=self.plugin_for_view(&parent.view_id)?;
        let declaration=plugin.package.manifest.contributes.presentation.as_ref().and_then(|c|c.dialogs.iter().find(|d|d.id==p["dialogId"])).cloned().ok_or_else(||err("INVALID_PARAMS","插件未声明此弹窗"))?;
        if serde_json::to_vec(&p["initialState"]).unwrap().len()>16*1024 {return Err(err("CAPACITY","弹窗初始状态超过16 KiB"));}
        let _transition=self.dialog_transition.lock().unwrap();
        let request_id=required(p,"requestId")?;
        {
            let mut dialogs=self.dialogs.lock().unwrap();
            dialogs.retain(|_,d|d.finished.lock().unwrap().is_none_or(|t|t.elapsed()<Duration::from_secs(60)));
            if let Some(d)=dialogs.values().find(|d|d.parent==source&&d.request_id==request_id) {
                if d.request!=*p{return Err(err("REQUEST_CONFLICT","弹窗标识已用于其他内容"));}
                return Ok(d.result_for(source));
            }
            if dialogs.len()>=256 {return Err(err("CAPACITY","弹窗回执容量已满，请稍后重试"));}
            if dialogs.values().filter(|d|!d.cleaned.load(Ordering::SeqCst)).count()>=8
                || dialogs.values().any(|d|d.owner==parent.window&&!d.cleaned.load(Ordering::SeqCst))
                || self.shell_modals.lock().unwrap().contains(&parent.window) {
                return Err(err("UI_BUSY","当前窗口已有弹窗，请先完成它"));
            }
        }
        let id=Uuid::new_v4().to_string();
        let d=Arc::new(Session{id:id.clone(),source:format!("@dialog:{id}"),label:format!("plugin-dialog-{id}"),owner:parent.window.clone(),parent:source.into(),caller:caller.clone(),feed_owner:format!("@dialog:{}:{id}",caller.instance_id),cleaned:AtomicBool::new(false),request_id:request_id.into(),request:p.clone(),opened:Instant::now(),outcome:Mutex::new(json!({"sessionId":id,"status":"opening"})),finished:Mutex::new(None),suspended:AtomicBool::new(false)});
        // Register before any page can execute. The blank Tauri container is never a shell endpoint.
        self.dialogs.lock().unwrap().insert(id.clone(),d.clone());
        let result:Result<()>=(||{
            self.cancel_menus(&d.owner);
            let owner=self.app.get_webview_window(&d.owner).ok_or_else(||err("WINDOW_CLOSED","所属窗口已关闭"))?;
            // WebView2 may stall its creation callback when this path is a file.
            // Fail before holding a native builder on an unusable profile root.
            let profile=self.data.join("profiles/dialog-host");
            std::fs::create_dir_all(&profile).map_err(|e|native_error(format!("无法创建弹窗配置目录：{e}")))?;
            let mut builder=WebviewWindowBuilder::new(&self.app,&d.label,WebviewUrl::External("about:blank".parse().unwrap()))
                .background_color(tauri::window::Color(0,0,0,0))
                .title(&declaration.title).decorations(false).transparent(true).shadow(false).skip_taskbar(true).resizable(false).visible(false)
                .data_directory(profile);
            #[cfg(windows)] {builder=builder.owner(&owner).map_err(|e|native_error(e.to_string()))?;}
            let window=builder.build().map_err(|e|native_error(e.to_string()))?;
            window.set_size(owner.inner_size().map_err(|e|native_error(e.to_string()))?).map_err(|e|native_error(e.to_string()))?;
            window.set_position(owner.inner_position().map_err(|e|native_error(e.to_string()))?).map_err(|e|native_error(e.to_string()))?;
            let view=self.workbench.lock().unwrap().view(&parent.view_id).map_err(native_error)?;
            let context=json!({"diagnosticContextVersion":1,"observationCreditBytes":self.core.grant_observation_credit(&caller.instance_id,&d.feed_owner,32*1024),"pluginId":view.plugin_id,"pluginVersion":plugin.package.manifest.version,"viewId":view.view_id,"viewType":view.view_type,"mountGeneration":parent.generation,"workspaceId":view.workspace_id,"workspaceGeneration":self.workbench.lock().unwrap().workspaces.get(&view.workspace_id).map(|w|w.generation.clone()),"serverId":view.server_id,"windowId":d.owner,"surfaceWindowId":d.label,"theme":self.theme_snapshot(true),"dialog":{"sessionId":id,"dialogId":declaration.id,"title":declaration.title,"width":declaration.width,"height":declaration.height,"initialState":p["initialState"]}});
            let weak=Arc::downgrade(self);let fail=weak.clone();
            self.native.create(d.source.clone(),d.label.clone(),plugin.package.manifest.entry.ui.clone(),context,plugin.assets.clone(),Arc::new(move|s,b|{if let Some(state)=weak.upgrade(){state.receive(s,b);}}),Arc::new(move|s|{if let Some(state)=fail.upgrade(){std::thread::spawn(move||{if let Some(d)=state.dialog_endpoint(&s){state.finish_plugin_dialog(&d,"failed",Some(json!({"code":"DIALOG_START_FAILED","message":"弹窗表面已结束"})));}});}}),self.data.join("profiles").join(&view.plugin_id)).map_err(native_error)?;
            Ok(())
        })();
        drop(_transition);
        if let Err(error)=result {self.finish_plugin_dialog(&d,"failed",Some(json!({"code":"DIALOG_START_FAILED","message":error.message})));}
        Ok(d.result_for(source))
    }
    fn show_plugin_dialog(&self,d:&Session)->Result<()> {
        let owner=self.app.get_webview_window(&d.owner).ok_or_else(||err("WINDOW_CLOSED","所属窗口已关闭"))?;
        let popup=self.app.get_webview_window(&d.label).ok_or_else(||err("DIALOG_START_FAILED","弹窗不可用"))?;
        let size=owner.inner_size().map_err(|e|native_error(e.to_string()))?;
        let scale=owner.scale_factor().map_err(|e|native_error(e.to_string()))?;
        popup.set_size(size).map_err(|e|native_error(e.to_string()))?;
        popup.set_position(owner.inner_position().map_err(|e|native_error(e.to_string()))?).map_err(|e|native_error(e.to_string()))?;
        self.native.present(d.source.clone(),Bounds{x:0.,y:0.,width:size.width as f64/scale,height:size.height as f64/scale},true).map_err(native_error)?;
        popup.show().map_err(|e|native_error(e.to_string()))?;
        owner.set_enabled(false).map_err(|e|native_error(e.to_string()))?;
        popup.set_focus().map_err(|e|native_error(e.to_string()))?;
        self.native.focus(d.source.clone()).map_err(native_error)?;
        Ok(())
    }
    pub(super) fn finish_plugin_dialog(self:&Arc<Self>,d:&Arc<Session>,status:&str,value:Option<Value>){
        {
            let mut finished=d.finished.lock().unwrap();if finished.is_some(){return;}*finished=Some(Instant::now());
            let mut result=json!({"sessionId":d.id,"status":status});
            if let Some(value)=value {result[if status=="failed"{"error"}else{"result"}]=value;}
            *d.outcome.lock().unwrap()=result;
        }
        // Let the completing page receive its RPC response before destroying it.
        let state=self.clone();let d=d.clone();
        std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(40));state.cleanup_plugin_dialog(&d);});
    }
    fn cleanup_plugin_dialog(&self,d:&Session){
        let _transition=self.dialog_transition.lock().unwrap();
        if d.cleaned.load(Ordering::SeqCst){return;}
        self.cancel_menus(&d.label);
        self.communication.lock().unwrap().remove_owner(&d.feed_owner);
        self.core.close_observation_owner(&d.feed_owner);
        let _=self.native.close(d.source.clone());
        let focused=self.app.get_webview_window(&d.label).is_some_and(|w|w.is_focused().unwrap_or(false));
        if let Some(popup)=self.app.get_webview_window(&d.label){let _=popup.destroy();}
        if let Some(owner)=self.app.get_webview_window(&d.owner){let _=owner.set_enabled(true);if focused{let _=owner.set_focus();}}
        d.cleaned.store(true,Ordering::SeqCst);
        if self.dialog_parent_valid(d) {
            self.endpoint_notify(&d.parent,"ui.dialog.closed",d.result());
            if focused&&!self.shell_modals.lock().unwrap().contains(&d.owner){let _=self.native.focus(d.parent.clone());}
        }
    }
    pub(super) fn cancel_plugin_dialogs(&self,view:Option<&str>){
        let dialogs=self.dialogs.lock().unwrap().values().filter(|d|!d.cleaned.load(Ordering::SeqCst)&&view.is_none_or(|id|d.caller.view_id.as_deref()==Some(id))).cloned().collect::<Vec<_>>();
        for d in dialogs {
            {let mut finished=d.finished.lock().unwrap();if finished.is_none(){*finished=Some(Instant::now());*d.outcome.lock().unwrap()=json!({"sessionId":d.id,"status":"cancelled"});}}
            self.cleanup_plugin_dialog(&d);
        }
    }
    pub(super) fn maintain_plugin_dialogs(self:&Arc<Self>){
        let dialogs=self.dialogs.lock().unwrap().values().cloned().collect::<Vec<_>>();
        for d in dialogs {
            if d.terminal(){continue;}
            if !self.dialog_parent_valid(&d){self.finish_plugin_dialog(&d,"cancelled",None);}
            else if d.result()["status"]=="opening"&&d.opened.elapsed()>Duration::from_secs(10){self.finish_plugin_dialog(&d,"failed",Some(json!({"code":"DIALOG_START_FAILED","message":"弹窗未能及时就绪"})));}
        }
        self.dialogs.lock().unwrap().retain(|_,d|d.finished.lock().unwrap().is_none_or(|t|t.elapsed()<Duration::from_secs(60)));
    }
    pub(super) fn shell_modal(self:&Arc<Self>,window:&str,open:bool)->Result<()> {
        let _transition=self.dialog_transition.lock().unwrap();
        {let mut modals=self.shell_modals.lock().unwrap();if open{modals.insert(window.into());}else{modals.remove(window);}}
        let dialogs=self.dialogs.lock().unwrap().values().filter(|d|d.owner==window&&!d.terminal()).cloned().collect::<Vec<_>>();
        for d in dialogs {
            d.suspended.store(open,Ordering::SeqCst);
            if open {
                if let Some(popup)=self.app.get_webview_window(&d.label){let _=popup.hide();}
                if let Some(owner)=self.app.get_webview_window(window){let _=owner.set_enabled(true);let _=owner.set_focus();}
            }else if self.dialog_parent_valid(&d)&&d.result()["status"]=="open" {self.show_plugin_dialog(&d)?;}
        }
        self.native.modal(window.into(),open).map_err(native_error)
    }
    pub(super) fn dialog_window_event(self:&Arc<Self>,label:&str,event:&tauri::WindowEvent)->bool {
        if !label.starts_with("plugin-dialog-"){return false;}
        let d=self.dialogs.lock().unwrap().values().find(|d|d.label==label).cloned();
        if let Some(d)=d {match event {
            tauri::WindowEvent::CloseRequested{api,..}=>{api.prevent_close();self.endpoint_notify(&d.source,"ui.dialog.cancelRequested",json!({"sessionId":d.id}));}
            tauri::WindowEvent::Destroyed=>{if !d.terminal(){self.finish_plugin_dialog(&d,"cancelled",None);}}
            _=>{}
        }}
        true
    }
    pub(super) fn align_plugin_dialogs(&self,owner:&str){
        let _transition=self.dialog_transition.lock().unwrap();
        let dialogs=self.dialogs.lock().unwrap().values().filter(|d|d.owner==owner&&!d.terminal()&&!d.suspended.load(Ordering::SeqCst)).cloned().collect::<Vec<_>>();
        for d in dialogs {
            if let (Some(parent),Some(popup))=(self.app.get_webview_window(owner),self.app.get_webview_window(&d.label)) {
                if let (Ok(size),Ok(position),Ok(scale))=(parent.inner_size(),parent.inner_position(),parent.scale_factor()){
                    let _=popup.set_size(size);let _=popup.set_position(position);
                    let _=self.native.present(d.source.clone(),Bounds{x:0.,y:0.,width:size.width as f64/scale,height:size.height as f64/scale},d.result()["status"]=="open");
                }
            }
        }
    }
}
