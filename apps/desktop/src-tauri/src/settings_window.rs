//! App-level settings identity is independent from workspaces and plugin views.
use super::*;
pub(super) const LABEL:&str="settings";
#[derive(Clone)]
pub(super) struct SettingsWindow {
    pub generation:String,
    pub revision:u64,
    pub page_id:String,
    pub origin:Option<String>,
}
#[derive(Default)]
pub(super) struct SettingsWindowRegistry {pub current:Option<SettingsWindow>,position:Option<tauri::PhysicalPosition<i32>>,size:Option<tauri::PhysicalSize<u32>>,receipts:HashMap<(String,String),(Value,Value,Instant)>}
impl SettingsWindow {
    pub fn source(&self)->String {format!("@window:{LABEL}/{}",self.generation)}
    pub fn context(&self)->Value {json!({"generation":self.generation,"navigationRevision":self.revision.to_string(),"pageId":self.page_id})}
}
impl State {
    pub(super) fn settings_registered(&self,label:&str)->bool {
        label==LABEL&&self.settings_window.lock().unwrap().current.is_some()
    }
    pub(super) fn settings_source(&self)->Option<String> {self.settings_window.lock().unwrap().current.as_ref().map(SettingsWindow::source)}
    pub(super) fn settings_window_for_source(&self,source:&str)->Result<Option<String>> {
        if !source.starts_with(&format!("@window:{LABEL}")){return Ok(None);}
        if self.settings_window.lock().unwrap().current.as_ref().is_none_or(|w|w.source()!=source) {
            return Err(err("SOURCE_INVALID","设置窗口已关闭或换代"));
        }
        Ok(Some(LABEL.into()))
    }
    pub(super) fn settings_window_call(self:&Arc<Self>,source:Option<&str>,method:&str,p:&Value)->Result<Value> {
        settings::check("x-methods",method,p)?;
        if matches!(method,"ui.settings.session.report"|"ui.settings.close.request"|"ui.settings.close.respond"){return self.settings_close_call(source,method,p);}
        if method=="ui.settings.context.get" {
            let source=source.ok_or_else(||err("SOURCE_INVALID","缺少设置窗口来源"))?;
            self.settings_window_for_source(source)?.ok_or_else(||err("SOURCE_INVALID","仅设置窗口可获取导航"))?;
            return self.settings_window.lock().unwrap().current.as_ref().map(SettingsWindow::context).ok_or_else(||err("WINDOW_CLOSED","设置已关闭"));
        }
        if method!="ui.settings.open" {return Err(err("UNSUPPORTED_API","未知设置窗口操作"));}
        let page_id=p["pageId"].as_str().unwrap_or("core:appearance").to_owned();
        let origin=source.map(|s|self.endpoint_window(s,true)).transpose()?;
        let _transition=self.settings_transition.lock().unwrap();
        if let Some(source)=source {self.endpoint_window(source,true)?;}
        if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
        let operation=(source.unwrap_or("@shell").to_owned(),p["operationId"].as_str().unwrap().to_owned());
        {
            let mut registry=self.settings_window.lock().unwrap();registry.receipts.retain(|_,(_,_,created)|created.elapsed()<Duration::from_secs(600));
            if let Some((request,result,_))=registry.receipts.get(&operation) {
                if request!=p{return Err(err("CONFLICT","同一打开操作不能使用不同目标"));}
                if registry.current.as_ref().is_none_or(|current|result["generation"]!=current.generation){return Err(err("WINDOW_CLOSED","该操作打开的设置窗口已经关闭"));}
                let result=result.clone();drop(registry);self.show_window(LABEL);return Ok(result);
            }
        }
        let title="设置";
        let context={let mut registry=self.settings_window.lock().unwrap();
            if let Some(window)=registry.current.as_mut() {
                window.revision=window.revision.checked_add(1).ok_or_else(||err("CAPACITY","导航代次已用尽"))?;
                window.page_id=page_id.clone();window.origin=origin.clone();Some(window.context())
            }else{None}};
        if let Some(context)=context {
            let window=self.app.get_webview_window(LABEL).ok_or_else(||err("WINDOW_CLOSED","设置窗口已关闭"))?;
            window.set_title(&title).map_err(|e|native_error(e.to_string()))?;
            window.eval(&format!("window.__sodeckNotify?.({});",json!({"type":"settings.navigate","payload":context}))).map_err(|e|native_error(e.to_string()))?;
            self.show_window(LABEL);self.remember_settings_open(operation,p,&context);return Ok(context);
        }
        let record=SettingsWindow{generation:Uuid::new_v4().to_string(),revision:1,page_id,origin};
        let context=record.context();let generation=record.generation.clone();
        self.settings_window.lock().unwrap().current=Some(record);
        let state=self.clone();let created_generation=generation.clone();let initial=context.clone();let (tx,rx)=mpsc::sync_channel(1);
        let scheduled=self.app.run_on_main_thread(move||{
            let result=(||{
                if state.stopping.load(Ordering::SeqCst)||state.settings_window.lock().unwrap().current.as_ref().is_none_or(|r|r.generation!=created_generation){return Err(err("CANCELED","设置窗口创建已取消"));}
                let mut builder=WebviewWindowBuilder::new(&state.app,LABEL,WebviewUrl::App("index.html?settings".into()))
                    .title(title).decorations(false).visible(false).inner_size(1000.,720.).min_inner_size(640.,480.)
                    .icon(crate::brand::icon(false)).map_err(|e|native_error(e.to_string()))?
                    .background_color(state.theme_background())
                    .initialization_script(crate::theme::bootstrap(&state.theme_snapshot(true)))
                    .initialization_script(format!("window.__SODECK_SETTINGS_CONTEXT__={initial};"))
                    .data_directory(state.data.join("profiles/shell"));
                if cfg!(debug_assertions)||crate::startup_config::qa_build(){if let Some(port)=state.app.state::<crate::startup_config::DebugConfig>().shell_port{builder=builder.additional_browser_args(&format!("--remote-debugging-port={port}"));}}
                let (position,size)={let registry=state.settings_window.lock().unwrap();(registry.position,registry.size)};
                let window=builder.build().map_err(|e|native_error(e.to_string()))?;
                if let Some(size)=size{let _=window.set_size(size);}if let Some(position)=position{let _=window.set_position(position);}
                let weak=Arc::downgrade(&state);let ended_generation=created_generation.clone();
                window.on_window_event(move|event|{if let Some(state)=weak.upgrade(){let mut r=state.settings_window.lock().unwrap();if r.current.as_ref().is_some_and(|w|w.generation==ended_generation){match event{tauri::WindowEvent::Moved(p)=>r.position=Some(*p),tauri::WindowEvent::Resized(s) if s.width>0&&s.height>0=>r.size=Some(*s),_=>{}}}}if matches!(event,tauri::WindowEvent::Destroyed){if let Some(state)=weak.upgrade(){
                    let mut registry=state.settings_window.lock().unwrap();
                    if registry.current.as_ref().is_some_and(|r|r.generation==ended_generation){registry.current=None;}
                    drop(registry);state.clear_settings_review(&ended_generation);let owner=format!("@window:{}:{LABEL}/{ended_generation}",state.app_generation);state.communication.lock().unwrap().remove_owner(&owner);state.core.release_communication_owner(&owner);
                }}});
                if state.stopping.load(Ordering::SeqCst)||state.settings_window.lock().unwrap().current.as_ref().is_none_or(|r|r.generation!=created_generation){let _=window.destroy();return Err(err("CANCELED","设置窗口创建已取消"));}
                window.show().map_err(|e|native_error(e.to_string()))?;let _=window.set_focus();Ok(())
            })();let _=tx.send(result);
        });
        let result=scheduled.map_err(|e|native_error(e.to_string())).and_then(|_|rx.recv_timeout(Duration::from_secs(15)).map_err(|_|err("TIMEOUT","设置窗口创建超时"))?);
        if let Err(error)=result {let mut registry=self.settings_window.lock().unwrap();if registry.current.as_ref().is_some_and(|r|r.generation==generation){registry.current=None;}return Err(error);}
        self.remember_settings_open(operation,p,&context);Ok(context)
    }
    fn remember_settings_open(&self,key:(String,String),request:&Value,result:&Value) {
        let mut registry=self.settings_window.lock().unwrap();
        if registry.receipts.len()>=256 {if let Some(oldest)=registry.receipts.iter().min_by_key(|(_,(_,_,time))|*time).map(|(key,_)|key.clone()){registry.receipts.remove(&oldest);}}
        registry.receipts.insert(key,(request.clone(),result.clone(),Instant::now()));
    }
    pub(super) fn destroy_settings(&self,source:Option<&str>)->Result<Value> {
        let _transition=self.settings_transition.lock().unwrap();
        if let Some(source)=source {self.settings_window_for_source(source)?.ok_or_else(||err("SOURCE_INVALID","设置窗口来源无效"))?;}
        let record=self.settings_window.lock().unwrap().current.take();
        if let Some(record)=record {
            self.clear_settings_review(&record.generation);
            let owner=format!("@window:{}:{LABEL}/{}",self.app_generation,record.generation);
            self.communication.lock().unwrap().remove_owner(&owner);self.core.release_communication_owner(&owner);
            self.cancel_menus(LABEL);
            if let Some(window)=self.app.get_webview_window(LABEL){window.destroy().map_err(|e|native_error(e.to_string()))?;}
            if let Some(origin)=record.origin {if let Some(window)=self.app.get_webview_window(&origin){if window.is_visible().unwrap_or(false){let _=window.set_focus();}}}
        }
        Ok(json!({"closed":true}))
    }
}
