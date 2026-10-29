//! App-level monitor identity is independent from workspaces and plugin views.
use super::*;
pub(super) const LABEL:&str="communication-monitor";
#[derive(Clone)]
pub(super) struct MonitorWindow {
    pub generation:String,
    pub revision:u64,
    pub target:Value,
    pub origin:Option<String>,
}
#[derive(Default)]
pub(super) struct MonitorWindowRegistry {pub current:Option<MonitorWindow>,receipts:HashMap<(String,String),(Value,Value,Instant)>}
impl MonitorWindow {
    pub fn source(&self)->String {format!("@window:{LABEL}/{}",self.generation)}
    pub fn context(&self)->Value {json!({"generation":self.generation,"navigationRevision":self.revision.to_string(),"target":self.target})}
}
impl State {
    pub(super) fn monitor_registered(&self,label:&str)->bool {
        label==LABEL&&self.monitor_window.lock().unwrap().current.is_some()
    }
    pub(super) fn monitor_source(&self)->Option<String> {self.monitor_window.lock().unwrap().current.as_ref().map(MonitorWindow::source)}
    pub(super) fn monitor_window_for_source(&self,source:&str)->Result<Option<String>> {
        if !source.starts_with(&format!("@window:{LABEL}")){return Ok(None);}
        if self.monitor_window.lock().unwrap().current.as_ref().is_none_or(|w|w.source()!=source) {
            return Err(err("SOURCE_INVALID","通信监控窗口已关闭或换代"));
        }
        Ok(Some(LABEL.into()))
    }
    pub(super) fn monitor_call(self:&Arc<Self>,source:Option<&str>,method:&str,p:&Value)->Result<Value> {
        Core::validate_communication("x-methods",method,p)?;
        if method=="ui.communicationMonitor.get" {
            let source=source.ok_or_else(||err("SOURCE_INVALID","缺少监控窗口来源"))?;
            self.monitor_window_for_source(source)?.ok_or_else(||err("SOURCE_INVALID","仅监控窗口可获取导航"))?;
            return self.monitor_window.lock().unwrap().current.as_ref().map(MonitorWindow::context).ok_or_else(||err("WINDOW_CLOSED","通信监控已关闭"));
        }
        if method!="ui.communicationMonitor.open" {return Err(err("UNSUPPORTED_API","未知监控窗口操作"));}
        let target=p.get("target").cloned().unwrap_or_else(||json!({}));
        let summary=self.core.communication_snapshot(&target)?;
        let target=json!({"target":summary["target"],"layer":summary["layer"]});
        let origin=source.map(|s|self.endpoint_window(s,true)).transpose()?;
        let _transition=self.monitor_transition.lock().unwrap();
        if let Some(source)=source {self.endpoint_window(source,true)?;}
        if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
        let operation=(source.unwrap_or("@shell").to_owned(),p["operationId"].as_str().unwrap().to_owned());
        {
            let mut registry=self.monitor_window.lock().unwrap();registry.receipts.retain(|_,(_,_,created)|created.elapsed()<Duration::from_secs(600));
            if let Some((request,result,_))=registry.receipts.get(&operation) {
                if request!=p{return Err(err("CONFLICT","同一打开操作不能使用不同目标"));}
                if registry.current.as_ref().is_none_or(|current|result["generation"]!=current.generation){return Err(err("WINDOW_CLOSED","该操作打开的监控窗口已经关闭"));}
                let result=result.clone();drop(registry);self.show_window(LABEL);return Ok(result);
            }
        }
        let scope=if let Some(server)=target["target"]["serverId"].as_str(){
            self.core.call(&Caller::shell(),"servers.get",json!({"serverId":server})).ok().and_then(|v|v["name"].as_str().map(str::to_owned)).unwrap_or_else(||format!("服务器 {}",server.chars().take(8).collect::<String>()))
        }else{"全部服务器".into()};
        let title=format!("通信监控 · {scope}");
        let context={let mut registry=self.monitor_window.lock().unwrap();
            if let Some(window)=registry.current.as_mut() {
                window.revision=window.revision.checked_add(1).ok_or_else(||err("CAPACITY","导航代次已用尽"))?;
                window.target=target.clone();window.origin=origin.clone();Some(window.context())
            }else{None}};
        if let Some(context)=context {
            let window=self.app.get_webview_window(LABEL).ok_or_else(||err("WINDOW_CLOSED","通信监控窗口已关闭"))?;
            window.set_title(&title).map_err(|e|native_error(e.to_string()))?;
            window.eval(&format!("window.__sodeckNotify?.({});",json!({"type":"communicationMonitor.navigate","payload":context}))).map_err(|e|native_error(e.to_string()))?;
            self.show_window(LABEL);self.remember_monitor_open(operation,p,&context);return Ok(context);
        }
        let record=MonitorWindow{generation:Uuid::new_v4().to_string(),revision:1,target,origin};
        let context=record.context();let generation=record.generation.clone();
        self.monitor_window.lock().unwrap().current=Some(record);
        let state=self.clone();let created_generation=generation.clone();let initial=context.clone();let (tx,rx)=mpsc::sync_channel(1);
        let scheduled=self.app.run_on_main_thread(move||{
            let result=(||{
                if state.stopping.load(Ordering::SeqCst)||state.monitor_window.lock().unwrap().current.as_ref().is_none_or(|r|r.generation!=created_generation){return Err(err("CANCELED","监控窗口创建已取消"));}
                let mut builder=WebviewWindowBuilder::new(&state.app,LABEL,WebviewUrl::App("index.html?communication-monitor".into()))
                    .title(&title).decorations(false).visible(false).inner_size(1000.,720.).min_inner_size(720.,520.)
                    .icon(crate::brand::icon(false)).map_err(|e|native_error(e.to_string()))?
                    .background_color(state.theme_background())
                    .initialization_script(crate::theme::bootstrap(&state.theme_snapshot(true)))
                    .initialization_script(format!("window.__SODECK_MONITOR_CONTEXT__={initial};"))
                    .data_directory(state.data.join("profiles/shell"));
                if cfg!(debug_assertions){if let Some(port)=state.app.state::<crate::startup_config::DebugConfig>().shell_port{builder=builder.additional_browser_args(&format!("--remote-debugging-port={port}"));}}
                let window=builder.build().map_err(|e|native_error(e.to_string()))?;
                let weak=Arc::downgrade(&state);let ended_generation=created_generation.clone();
                window.on_window_event(move|event|{if matches!(event,tauri::WindowEvent::Destroyed){if let Some(state)=weak.upgrade(){
                    let mut registry=state.monitor_window.lock().unwrap();
                    if registry.current.as_ref().is_some_and(|r|r.generation==ended_generation){registry.current=None;}
                    drop(registry);let owner=format!("@window:{}:{LABEL}/{ended_generation}",state.app_generation);state.communication.lock().unwrap().remove_owner(&owner);state.core.release_communication_owner(&owner);
                }}});
                if state.stopping.load(Ordering::SeqCst)||state.monitor_window.lock().unwrap().current.as_ref().is_none_or(|r|r.generation!=created_generation){let _=window.destroy();return Err(err("CANCELED","监控窗口创建已取消"));}
                window.show().map_err(|e|native_error(e.to_string()))?;let _=window.set_focus();Ok(())
            })();let _=tx.send(result);
        });
        let result=scheduled.map_err(|e|native_error(e.to_string())).and_then(|_|rx.recv_timeout(Duration::from_secs(15)).map_err(|_|err("TIMEOUT","监控窗口创建超时"))?);
        if let Err(error)=result {let mut registry=self.monitor_window.lock().unwrap();if registry.current.as_ref().is_some_and(|r|r.generation==generation){registry.current=None;}return Err(error);}
        self.remember_monitor_open(operation,p,&context);Ok(context)
    }
    fn remember_monitor_open(&self,key:(String,String),request:&Value,result:&Value) {
        let mut registry=self.monitor_window.lock().unwrap();
        if registry.receipts.len()>=256 {if let Some(oldest)=registry.receipts.iter().min_by_key(|(_,(_,_,time))|*time).map(|(key,_)|key.clone()){registry.receipts.remove(&oldest);}}
        registry.receipts.insert(key,(request.clone(),result.clone(),Instant::now()));
    }
    pub(super) fn close_monitor(&self,source:Option<&str>)->Result<Value> {
        let _transition=self.monitor_transition.lock().unwrap();
        if let Some(source)=source {self.monitor_window_for_source(source)?.ok_or_else(||err("SOURCE_INVALID","监控窗口来源无效"))?;}
        let record=self.monitor_window.lock().unwrap().current.take();
        if let Some(record)=record {
            let owner=format!("@window:{}:{LABEL}/{}",self.app_generation,record.generation);
            self.communication.lock().unwrap().remove_owner(&owner);self.core.release_communication_owner(&owner);
            self.cancel_menus(LABEL);
            if let Some(window)=self.app.get_webview_window(LABEL){window.destroy().map_err(|e|native_error(e.to_string()))?;}
            if let Some(origin)=record.origin {if let Some(window)=self.app.get_webview_window(&origin){if window.is_visible().unwrap_or(false){let _=window.set_focus();}}}
        }
        Ok(json!({"closed":true}))
    }
}
