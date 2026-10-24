use crate::{native::{Assets, Bounds, NativeViews}, view_operations::ViewOperations, workbench::{View, Workbench}};
use sodeck_core::{Caller, Core, CoreError};
use sodeck_plugin_host::{PluginProcess, RpcError};
use sodeck_plugin_manager::{InstalledPlugin, PluginManager, TrustedKeys};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::{Path, PathBuf}, sync::{Arc, Mutex, Condvar, mpsc, atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering}}, time::{Duration, Instant}};
use tauri::{Manager, WebviewWindowBuilder, WebviewUrl};
use uuid::Uuid;

type Result<T> = std::result::Result<T, CoreError>;
fn err(code:&str,message:impl Into<String>)->CoreError {CoreError::new(code,message)}
fn native_error(message:String)->CoreError {err("NATIVE_ERROR",message)}
fn required<'a>(value:&'a Value,key:&str)->Result<&'a str>{value[key].as_str().filter(|s|!s.is_empty()).ok_or_else(||err("INVALID_PARAMS",format!("缺少 {key}")))}

#[derive(Clone, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Request { api_version:u32, request_id:String, method:String, #[serde(default)]params:Value, timeout_ms:Option<u64> }
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Response {request_id:String, #[serde(skip_serializing_if="Option::is_none")]result:Option<Value>, #[serde(skip_serializing_if="Option::is_none")]error:Option<CoreError>}
impl Response {fn from(id:String,result:Result<Value>)->Self {match result {Ok(result)=>Self{request_id:id,result:Some(result),error:None},Err(error)=>Self{request_id:id,result:None,error:Some(error)}}}}

struct Loaded {package:InstalledPlugin, process:Arc<PluginProcess>, instance:String, assets:Assets, gateway:Arc<Mutex<Option<std::sync::Weak<State>>>>}
struct Binding {
    id:String, view_id:String, generation:u64, window:String, phase:AtomicU8,
    ready:Mutex<bool>, changed:Condvar, geometry:Mutex<(Bounds,bool)>,
}
struct Cached {fingerprint:String, started:Instant, result:Mutex<Option<Response>>, changed:Condvar}
struct SnapshotTransfer {source:String, sender:mpsc::SyncSender<Result<Value>>, data:String, total:usize, next:usize}
struct PendingOpen {created:Instant, server_id:Option<String>, canceled:bool, finished:bool, view_id:Option<String>}
pub struct State {
    core:Arc<Core>, plugins:Mutex<PluginManager>, loaded:Mutex<Option<Arc<Loaded>>>,
    workbench:Mutex<Workbench>, bindings:Mutex<HashMap<String,Arc<Binding>>>, view_operations:ViewOperations,
    snapshots:Mutex<HashMap<String,SnapshotTransfer>>, cache:Mutex<HashMap<String,Arc<Cached>>>,
    pending_opens:Mutex<HashMap<String,PendingOpen>>,
    mounting:Mutex<()>,
    native:NativeViews, app:tauri::AppHandle, data:PathBuf, stopping:AtomicBool, inflight:AtomicUsize,
    _data_lock:fs::File, startup_error:Mutex<Option<String>>,
}

fn plugin_storage(data:&Path,method:&str,params:&Value)->Result<Value> {
    let file=data.join("settings.json");
    let mut value:Value=if file.exists(){serde_json::from_slice(&fs::read(&file).map_err(|_|err("STORAGE_ERROR","无法读取插件设置"))?).map_err(|_|err("STORAGE_ERROR","插件设置已损坏"))?}else{json!({})};
    let key=params["key"].as_str().unwrap_or("settings");
    if key.len()>200 {return Err(err("INVALID_PARAMS","设置键过长"));}
    match method {
        "storage.get"=>Ok(value.get(key).cloned().unwrap_or(Value::Null)),
        "storage.set"=>{
            value[key]=params["value"].clone();
            let bytes=serde_json::to_vec(&value).map_err(|_|err("STORAGE_ERROR","设置编码失败"))?;
            if bytes.len()>1024*1024 {return Err(err("CAPACITY","插件设置超出容量"));}
            fs::create_dir_all(data).map_err(|_|err("STORAGE_ERROR","无法创建插件数据目录"))?;
            let temp=data.join("settings.pending");
            use std::io::Write;
            let mut f=fs::File::create(&temp).map_err(|_|err("STORAGE_ERROR","无法保存插件设置"))?;
            f.write_all(&bytes).and_then(|_|f.sync_all()).map_err(|_|err("STORAGE_ERROR","无法保存插件设置"))?;drop(f);
            fs::rename(temp,file).map_err(|_|err("STORAGE_ERROR","无法提交插件设置"))?;
            Ok(json!({"saved":true}))
        }
        _=>Err(err("UNSUPPORTED_API","不支持的存储方法")),
    }
}
fn spawn_plugin(core:Arc<Core>,package:&InstalledPlugin)->std::result::Result<Arc<Loaded>,String>{
    let instance=Uuid::new_v4().to_string();
    let caller=Caller{source:"plugin".into(),plugin_id:Some(package.manifest.plugin_id.clone()),plugin_version:Some(package.manifest.version.clone()),instance_id:instance.clone(),view_id:None,mount_generation:None};
    let data=package.data_dir.clone();
    let storage_lock=Mutex::new(());
    let gateway=Arc::new(Mutex::new(None::<std::sync::Weak<State>>));let public_gateway=gateway.clone();
    let initializing=Arc::new(AtomicBool::new(true));let init=initializing.clone();
    let callback_core=core.clone();
    let callback=Arc::new(move |method:&str,params:Value|{
        let core=&callback_core;
        let active_gateway=public_gateway.lock().unwrap().as_ref().and_then(|w|w.upgrade());
        let result=if init.load(Ordering::SeqCst) && !matches!(method,"storage.get"|"storage.set"|"settings.get"|"servers.list"|"servers.get"|"diagnostics.get") {
            core.record(&caller,method,&params,||Err(err("INVALID_STATE","插件初始化期间不能启动远端业务")))
        } else if ["storage.get","storage.set"].contains(&method) && params.get("namespace").is_none(){
            core.record(&caller,method,&params,||{let _guard=storage_lock.lock().unwrap();plugin_storage(&data,method,&params)})
        } else if let Some(state)=active_gateway {state.dispatch(&caller,None,method,params)}
        else {core.call(&caller,method,params)};
        result.map_err(|error|serde_json::from_value::<RpcError>(serde_json::to_value(error).unwrap()).unwrap_or_else(|_|RpcError::new("CORE_ERROR","核心调用失败")))
    });
    let process=Arc::new(PluginProcess::spawn(std::env::current_exe().map_err(|e|e.to_string())?,&package.worker_path,callback).map_err(|e|e.to_string())?);
    if let Err(error)=process.initialize(json!({"pluginId":package.manifest.plugin_id,"pluginVersion":package.manifest.version,"instanceId":instance,"dataSchemaVersion":package.manifest.data_schema_version})) {
        process.terminate();core.close_instance(&instance);return Err(error.to_string());
    }
    initializing.store(false,Ordering::SeqCst);
    let assets=match Assets::load(&package.root){Ok(assets)=>assets,Err(error)=>{process.terminate();core.close_instance(&instance);return Err(error);}};
    Ok(Arc::new(Loaded{package:package.clone(),process,instance,assets,gateway}))
}

impl State {
    fn loaded(&self)->Result<Arc<Loaded>>{let loaded=self.loaded.lock().unwrap().clone().ok_or_else(||err("PLUGIN_UNAVAILABLE",self.startup_error.lock().unwrap().clone().unwrap_or_else(||"终端插件尚未安装或不可用".into())))?;if !loaded.process.is_alive(){return Err(err("PLUGIN_CRASHED","终端插件后台已经结束，正在回收其资源"));}Ok(loaded)}
    fn changed(&self){
        let notification=json!({"type":"workbench.changed","payload":{}});
        for window in self.app.webview_windows().values(){let _=window.eval(&format!("window.__sodeckNotify?.({notification});"));}
        let ids=self.bindings.lock().unwrap().values().filter(|b|b.phase.load(Ordering::SeqCst)==1).map(|b|b.id.clone()).collect::<Vec<_>>();
        for id in ids {let _=self.native.notify(id,"workbench.changed",json!({}));}
    }
    fn save(&self)->Result<()> {self.workbench.lock().unwrap().save(&self.data.join("workbench.json")).map_err(|e|err("STORAGE_ERROR",e))}
    fn warning(&self,warning:&Value){
        let notification=json!({"type":"workbench.warning","payload":warning});
        for window in self.app.webview_windows().values(){let _=window.eval(&format!("window.__sodeckNotify?.({notification});"));}
    }
    fn save_committed(&self,mut result:Value,message:&str)->Value{
        result["committed"]=json!(true);
        match self.save(){
            Ok(())=>result["persisted"]=json!(true),
            Err(error)=>{let warning=json!({"code":"STORAGE_ERROR","message":message,"details":{"cause":error.message}});result["persisted"]=json!(false);result["warning"]=warning.clone();self.warning(&warning);}
        }
        self.changed();result
    }
    fn worker(&self,command:&str,args:Value)->Result<Value>{self.loaded()?.process.invoke(command,args).map_err(|e|serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap_or_else(|_|err("PLUGIN_ERROR","终端插件调用失败")))}
    fn binding(&self,id:&str)->Result<Arc<Binding>>{self.bindings.lock().unwrap().get(id).cloned().ok_or_else(||err("INSTANCE_EXPIRED","视图挂载已经结束"))}
    fn view_binding(&self,id:&str)->Option<Arc<Binding>>{self.bindings.lock().unwrap().values().filter(|b|b.view_id==id && matches!(b.phase.load(Ordering::SeqCst),1|2)).max_by_key(|b|b.generation).cloned()}
    fn bind_pty(&self,view:&View)->Result<Value>{
        match (&view.pty_id,view.generation){
            (Some(pty),Some(generation))=>self.core.call(&Caller::shell(),"pty.bind",json!({"ptyId":pty,"generation":generation,"viewId":view.view_id,"mountGeneration":view.mount_generation})),
            _=>Ok(Value::Null),
        }
    }
    fn create_binding(self:&Arc<Self>,view:&View,window:&str,snapshot:Option<Value>,candidate:bool)->Result<Arc<Binding>>{
        let loaded=self.loaded()?;
        let server=self.core.call(&Caller::shell(),"servers.get",json!({"serverId":view.server_id}))?;
        let binding_state=if !candidate {self.bind_pty(view)?}else{Value::Null};
        let snapshot=snapshot.map(|mut state|{
            if state["recovered"].as_bool()==Some(true){
                state["acknowledged"]=binding_state["acknowledged"].clone();
                state["lastInputSeq"]=binding_state["lastInputSeq"].clone();
            }
            state
        });
        let id=Uuid::new_v4().to_string();
        let binding=Arc::new(Binding{id:id.clone(),view_id:view.view_id.clone(),generation:view.mount_generation,window:window.into(),phase:AtomicU8::new(if candidate {0}else{1}),ready:Mutex::new(false),changed:Condvar::new(),geometry:Mutex::new((Bounds::default(),false))});
        self.bindings.lock().unwrap().insert(id.clone(),binding.clone());
        let context=json!({"viewId":view.view_id,"viewType":view.view_type,"workspaceId":view.workspace_id,"serverId":view.server_id,"serverName":server["name"],"pluginVersion":loaded.package.manifest.version,"mountGeneration":view.mount_generation,"sessionId":view.session_id,"ptyId":view.pty_id,"generation":view.generation,"snapshot":snapshot});
        let weak=Arc::downgrade(self);let fail=weak.clone();
        let ipc=Arc::new(move |source:String,body:String|{if let Some(state)=weak.upgrade(){state.receive(source,body);}});
        let failure=Arc::new(move |source:String|{if let Some(state)=fail.upgrade(){std::thread::spawn(move||state.surface_failed(&source));}});
        let result=self.native.create(id.clone(),window.into(),loaded.package.manifest.entry.ui.clone(),context,loaded.assets.clone(),ipc,failure,self.data.join("profiles").join("terminal"));
        if let Err(error)=result {self.bindings.lock().unwrap().remove(&id);if let Some(v)=self.workbench.lock().unwrap().views.iter_mut().find(|v|v.view_id==view.view_id){v.status="viewFailed".into();}return Err(native_error(error));}
        Ok(binding)
    }
    fn receive(self:&Arc<Self>,source:String,body:String){
        #[derive(Deserialize,Default)]#[serde(rename_all="camelCase")]struct Header{request_id:Option<String>,method:Option<String>}
        let header=serde_json::from_str::<Header>(&body).unwrap_or_default();
        let cleanup=matches!(header.method.as_deref(),Some("app.exit"|"workbench.close"|"workbench.cancelOpen"|"operations.cancel"|"pty.close"|"connections.release"|"resources.stop"));
        let admitted=self.inflight.fetch_add(1,Ordering::SeqCst)<if cleanup{128}else{112};
        if !admitted||body.len()>1024*1024 {
            self.inflight.fetch_sub(1,Ordering::SeqCst);
            let caller=self.caller(Some(&source)).unwrap_or_else(|_|Caller{source:"expired-surface".into(),instance_id:source.clone(),plugin_id:None,plugin_version:None,view_id:None,mount_generation:None});
            let result=self.core.record(&caller,header.method.as_deref().unwrap_or("rpc.invalid"),&json!({"requestBytes":body.len()}),||Err(err("CAPACITY",if !admitted{"公开调用队列已满，请稍后重试"}else{"控制请求超过 1 MiB"})));
            let response=Response::from(header.request_id.unwrap_or_else(||"invalid".into()),result);let native=self.native.clone();
            let _=native.post_eval(source,format!("window.__sodeckReceive?.({});",serde_json::to_string(&response).unwrap()));return;
        }
        let state=self.clone();
        std::thread::spawn(move||{
            let response=match serde_json::from_str::<Request>(&body){Ok(request)=>state.request(Some(&source),request),Err(_)=>{let result=state.caller(Some(&source)).and_then(|caller|state.core.record(&caller,"rpc.invalid",&json!({"requestBytes":body.len()}),||Err(err("INVALID_REQUEST","请求格式无效"))));Response::from("invalid".into(),result)}};
            let script=format!("window.__sodeckReceive?.({});",serde_json::to_string(&response).unwrap());
            let _=state.native.eval(source,script);state.inflight.fetch_sub(1,Ordering::SeqCst);
        });
    }
    fn caller(&self,source:Option<&str>)->Result<Caller>{
        if let Some(source)=source {
            let b=self.binding(source)?;let l=self.loaded()?;
            Ok(Caller{source:"plugin".into(),plugin_id:Some(l.package.manifest.plugin_id.clone()),plugin_version:Some(l.package.manifest.version.clone()),instance_id:l.instance.clone(),view_id:Some(b.view_id.clone()),mount_generation:Some(b.generation)})
        } else {Ok(Caller::shell())}
    }
    pub fn request(self:&Arc<Self>,source:Option<&str>,request:Request)->Response{
        let caller=match self.caller(source){Ok(c)=>c,Err(e)=>{let caller=Caller{source:"expired-surface".into(),instance_id:source.unwrap_or("shell").into(),plugin_id:None,plugin_version:None,view_id:None,mount_generation:None};return Response::from(request.request_id,self.core.record(&caller,&request.method,&request.params,||Err(e)));}};
        if request.api_version!=1 || request.request_id.is_empty() || request.request_id.len()>128 || request.method.len()>128 {return Response::from(request.request_id, self.core.record(&caller,&request.method,&request.params,||Err(err("INVALID_REQUEST","请求版本或标识无效"))));}
        if serde_json::to_vec(&request.params).unwrap_or_default().len()>1024*1024-1024 {return Response::from(request.request_id,self.core.record(&caller,&request.method,&json!({"oversize":true}),||Err(err("CAPACITY","控制请求超过 1 MiB"))));}
        let query=request.method.ends_with(".get")||request.method.ends_with(".list")||matches!(request.method.as_str(),"pty.read"|"pty.ack"|"workbench.snapshot"|"workbench.surface"|"app.info")||(request.method=="plugin.invoke"&&request.params["command"].as_str().is_some_and(|c|c.ends_with(".get")||c.ends_with(".list")));
        if query {return Response::from(request.request_id,self.dispatch(&caller,source,&request.method,request.params));}
        let cleanup=matches!(request.method.as_str(),"app.exit"|"workbench.close"|"workbench.cancelOpen"|"operations.cancel"|"pty.close"|"connections.release"|"resources.stop");
        if cleanup&&self.cache.lock().unwrap().len()>=10000{return Response::from(request.request_id,self.dispatch(&caller,source,&request.method,request.params));}
        let key=format!("{}:{}:{}",caller.instance_id,source.unwrap_or("shell"),request.request_id);
        use sha2::{Digest,Sha256};
        let fingerprint=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!([request.method,request.params])).unwrap()));
        let (cached,existing)={let mut cache=self.cache.lock().unwrap();cache.retain(|_,c|c.started.elapsed()<Duration::from_secs(600)||c.result.lock().unwrap().is_none());if let Some(c)=cache.get(&key){(c.clone(),true)}else{
            if cache.len()>=10000 {return Response::from(request.request_id,self.core.record(&caller,&request.method,&request.params,||Err(err("CAPACITY","请求记录已达到容量上限"))));}
            let c=Arc::new(Cached{fingerprint:fingerprint.clone(),started:Instant::now(),result:Mutex::new(None),changed:Condvar::new()});cache.insert(key,c.clone());(c,false)
        }};
        if existing {
            let result=self.core.record(&caller,&request.method,&request.params,||{
                if cached.fingerprint!=fingerprint {return Err(err("REQUEST_CONFLICT","同一请求标识不能提交不同内容"));}
                let guard=cached.result.lock().unwrap();let (guard,_)=cached.changed.wait_timeout_while(guard,Duration::from_millis(request.timeout_ms.unwrap_or(35000).min(180000)),|r|r.is_none()).unwrap();
                match guard.as_ref(){Some(r)=>r.result.clone().ok_or_else(||r.error.clone().unwrap()),None=>Err(err("RESULT_UNKNOWN","操作仍在处理中，请查询状态"))}
            });return Response::from(request.request_id,result);
        }
        let result=if self.stopping.load(Ordering::SeqCst) && request.method!="app.exit" {self.core.record(&caller,&request.method,&request.params,||Err(err("APP_STOPPING","应用正在退出")))}else{self.dispatch(&caller,source,&request.method,request.params.clone())};
        let response=Response::from(request.request_id,result);*cached.result.lock().unwrap()=Some(response.clone());cached.changed.notify_all();response
    }
    fn dispatch(self:&Arc<Self>,caller:&Caller,source:Option<&str>,method:&str,params:Value)->Result<Value>{
        if let Some(source)=source {
            let b=self.binding(source)?;let phase=b.phase.load(Ordering::SeqCst);
            if phase==3 || (matches!(method,"pty.write"|"pty.resize") && phase!=1) {return self.core.record(caller,method,&params,||Err(err("MOUNT_EXPIRED","当前视图没有有效输入绑定")));}
        }
        if method.starts_with("workbench.")||method.starts_with("view.")||method.starts_with("plugin.")||method.starts_with("plugins.")||method.starts_with("clipboard.")||method.starts_with("app.") {
            return self.core.record(caller,method,&params,||self.external(caller,source,method,&params));
        }
        let result=self.core.call(caller,method,params.clone());
        if result.is_ok()&&method=="servers.delete"{
            let id=params["serverId"].as_str().unwrap_or("");let ids=self.workbench.lock().unwrap().views.iter().filter(|v|v.server_id==id).map(|v|v.view_id.clone()).collect::<Vec<_>>();
            for view in ids{let _=self.close_view(&view);}let mut w=self.workbench.lock().unwrap();w.workspaces.remove(id);if w.active_workspace_id.as_deref()==Some(id){w.active_workspace_id=None;}drop(w);let _=self.save();self.changed();
        }
        result
    }
    fn external(self:&Arc<Self>,_caller:&Caller,source:Option<&str>,method:&str,params:&Value)->Result<Value>{
        match method {
            "app.info"=>Ok(json!({"version":"2.0.0-dev.1","platform":std::env::consts::OS,"startupError":self.startup_error.lock().unwrap().clone()})),
            "app.show"=>{if let Some(w)=self.app.get_webview_window("main"){w.show().map_err(|e|native_error(e.to_string()))?;let _=w.unminimize();let _=w.set_focus();}Ok(Value::Null)},
            "app.hide"=>{let w=self.app.get_webview_window("main").ok_or_else(||err("WINDOW_CLOSED","主窗口不可用"))?;w.hide().map_err(|e|native_error(e.to_string()))?;Ok(Value::Null)},
            "app.exit"=>{self.shutdown();let app=self.app.clone();std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(150));app.exit(0);});Ok(Value::Null)},
            "plugins.list"=>Ok(serde_json::to_value(self.plugins.lock().unwrap().status()).unwrap()),
            "plugins.prepareUpdate"=>{
                let path=if let Some(path)=params["path"].as_str().filter(|p|!p.is_empty()){PathBuf::from(path)}else{rfd::FileDialog::new().add_filter("SoDeck 插件",&["sodeck-plugin"]).pick_file().ok_or_else(||err("CANCELLED","已取消选择插件包"))?};
                let manifest=self.plugins.lock().unwrap().prepare_package(path).map_err(|e|err("PACKAGE_INVALID",e.to_string()))?;Ok(serde_json::to_value(manifest).unwrap())
            },
            "plugin.invoke"=>{
                if params.get("pluginId").is_some() {return Err(err("UNSUPPORTED_API","首批不开放跨插件调用"));}
                let command=required(params,"command")?;
                let result=self.worker(command,params.get("args").cloned().unwrap_or(json!({})))?;
                if command=="sessions.reconnect" {let session=params["args"]["sessionId"].as_str();let view={let mut w=self.workbench.lock().unwrap();w.views.iter_mut().find(|v|v.session_id.as_deref()==session).map(|v|{v.pty_id=result["ptyId"].as_str().map(str::to_owned);v.generation=result["generation"].as_u64();v.status="running".into();v.clone()})};if let Some(view)=view{self.bind_pty(&view)?;}self.changed();}
                if command=="sessions.rename" {let session=params["args"]["sessionId"].as_str();if let Some(view)=self.workbench.lock().unwrap().views.iter_mut().find(|v|v.session_id.as_deref()==session){if let Some(title)=result["title"].as_str(){view.title=title.into();if let Some(window)=self.app.get_webview_window(&format!("detached-{}",view.view_id)){let server=self.core.call(&Caller::shell(),"servers.get",json!({"serverId":view.server_id}))?;let _=window.set_title(&format!("{} · {} — SoDeck",server["name"].as_str().unwrap_or("服务器"),title));}}}self.save()?;self.changed();}
                Ok(result)
            },
            "clipboard.readText"=>{let text=arboard::Clipboard::new().and_then(|mut c|c.get_text()).map_err(|_|err("CLIPBOARD_ERROR","无法读取剪贴板"))?;if text.len()>128*1024{return Err(err("CAPACITY","单次剪贴板文本最多 128 KiB，请分段粘贴"));}Ok(json!({"text":text}))},
            "clipboard.writeText"=>{let text=required(params,"text")?;if text.len()>128*1024{return Err(err("CAPACITY","单次复制最多 128 KiB 文本，请分段复制"));}arboard::Clipboard::new().and_then(|mut c|c.set_text(text)).map_err(|_|err("CLIPBOARD_ERROR","无法写入剪贴板"))?;Ok(Value::Null)},
            "workbench.snapshot"=>{
                let mut snapshot=self.workbench.lock().unwrap().snapshot();
                snapshot["pendingOpens"]=Value::Array(self.pending_opens.lock().unwrap().iter().filter(|(_,open)|!open.finished).map(|(id,open)|json!({"operationId":id,"serverId":open.server_id,"cancelling":open.canceled})).collect());
                Ok(snapshot)
            },
            "workbench.activateWorkspace"=>{let id=required(params,"serverId")?;self.core.call(&Caller::shell(),"servers.get",json!({"serverId":id}))?;self.workbench.lock().unwrap().activate_workspace(id).map_err(native_error)?;self.save()?;self.changed();Ok(self.workbench.lock().unwrap().snapshot())},
            "workbench.openTerminal"=>{
                let id=required(params,"serverId")?;
                let operation=match params.get("operationId"){Some(value)=>value.as_str().filter(|id|!id.is_empty()&&id.len()<=128).ok_or_else(||err("INVALID_PARAMS","operationId 无效"))?.to_owned(),None=>Uuid::new_v4().to_string()};
                let result=self.open_terminal(id,&operation);
                if let (Some(source),Err(error))=(source,&result){if ["HOST_KEY_REQUIRED","HOST_KEY_CHANGED","AUTH_REQUIRED","AUTH_FAILED"].contains(&error.code.as_str()){
                    let b=self.binding(source)?;let server=self.core.call(&Caller::shell(),"servers.get",json!({"serverId":id}))?;
                    if let Some(window)=self.app.get_webview_window(&b.window){let event=json!({"type":"auth.required","payload":{"server":server,"error":error,"operationId":operation}});let _=window.eval(&format!("window.__sodeckNotify?.({event});"));}
                }}result
            },
            "workbench.cancelOpen"=>{
                let operation=required(params,"operationId")?;
                if operation.len()>128{return Err(err("INVALID_PARAMS","operationId 无效"));}
                let active={
                    let mut opens=self.pending_opens.lock().unwrap();
                    opens.retain(|_,open|!open.finished||open.created.elapsed()<Duration::from_secs(600));
                    if let Some(open)=opens.get_mut(operation){
                        if let Some(view)=&open.view_id{return Ok(json!({"canceled":false,"viewId":view}));}
                        open.canceled=true;
                        !open.finished
                    }else{
                        if opens.len()>=10000{return Err(err("CAPACITY","取消记录已达到容量上限"));}
                        opens.insert(operation.into(),PendingOpen{created:Instant::now(),server_id:None,canceled:true,finished:true,view_id:None});
                        false
                    }
                };
                let event=json!({"type":"workbench.openCanceled","payload":{"operationId":operation}});
                for window in self.app.webview_windows().values(){let _=window.eval(&format!("window.__sodeckNotify?.({event});"));}
                self.changed();
                if active{self.worker("sessions.cancelOpen",json!({"operationId":operation}))?;}
                Ok(json!({"canceled":true}))
            },
            "workbench.recover"=>{
                let id=required(params,"viewId")?;let _mount=self.mounting.lock().unwrap();
                if self.view_operations.busy(id){return Err(err("MOVE_IN_PROGRESS","正在移动或关闭视图"));}
                let _=self.loaded()?;
                {let mut w=self.workbench.lock().unwrap();let mut candidate=w.clone();let reset=candidate.reset_incompatible_state(id).map_err(native_error)?;let v=candidate.views.iter_mut().find(|v|v.view_id==id).ok_or_else(||err("VIEW_CLOSED","视图已关闭"))?;
                    if v.view_type=="terminal.session"&&v.pty_id.is_none(){v.status="ended".into();w.commit(candidate,&self.data.join("workbench.json")).map_err(|e|err("STORAGE_ERROR",e))?;drop(w);self.changed();return Ok(json!({"ended":true}));}
                    if reset{w.commit(candidate,&self.data.join("workbench.json")).map_err(|e|err("STORAGE_ERROR",e))?;}}
                let view={let mut w=self.workbench.lock().unwrap();let v=w.views.iter_mut().find(|v|v.view_id==id).ok_or_else(||err("VIEW_CLOSED","视图已关闭"))?;if v.status!="viewFailed"{return Err(err("INVALID_STATE","视图尚未失效"));}v.mount_generation+=1;v.status=if v.pty_id.is_some(){"running"}else{"ready"}.into();v.clone()};
                let window=if view.placement=="detached"{format!("detached-{id}")}else{"main".into()};
                if let Err(error)=self.create_binding(&view,&window,Some(json!({"recovered":true})),false){if let Some(v)=self.workbench.lock().unwrap().views.iter_mut().find(|v|v.view_id==id){v.status="viewFailed".into();}return Err(error);}
                Ok(self.save_committed(json!({"viewId":id,"recovered":true}),"视图已恢复，但工作台布局保存失败；当前会话仍可继续使用。"))
            },
            "workbench.activate"=>{let id=required(params,"viewId")?;let v=self.workbench.lock().unwrap().activate(id).map_err(native_error)?;if let Some(b)=self.view_binding(id){let _=self.native.focus(b.id.clone());let _=self.native.notify(b.id.clone(),"view.focus",json!({}));}if v.placement=="detached"{if let Some(w)=self.app.get_webview_window(&format!("detached-{}",v.view_id)){let _=w.show();let _=w.set_focus();}}self.save()?;self.changed();Ok(serde_json::to_value(v).unwrap())},
            "workbench.close"=>{self.close_view(required(params,"viewId")?)?;Ok(Value::Null)},
            "workbench.move"=>{
                if let Some(revision)=params["expectedRevision"].as_u64(){if revision!=self.workbench.lock().unwrap().revision{return Err(err("LAYOUT_CONFLICT","布局已更新，请重新选择目标"));}}
                if params.get("screenPosition").is_some(){self.drop_view(required(params,"viewId")?)}else{self.move_view(required(params,"viewId")?,required(params,"placement")?,params["group"].as_u64().unwrap_or(0) as u8)}
            },
            "workbench.configure"=>{let snapshot={let mut current=self.workbench.lock().unwrap();let mut candidate=current.clone();candidate.configure(params).map_err(native_error)?;current.commit(candidate,&self.data.join("workbench.json")).map_err(|e|err("STORAGE_ERROR",e))?;current.snapshot()};self.changed();Ok(snapshot)},
            "workbench.surface"=>self.present(required(params,"viewId")?,serde_json::from_value(params["bounds"].clone()).map_err(|_|err("INVALID_PARAMS","视图尺寸无效"))?,params["visible"].as_bool().unwrap_or(false)),
            "workbench.modal"=>{let window=if let Some(source)=source{self.binding(source)?.window.clone()}else{params["windowLabel"].as_str().unwrap_or("main").into()};self.native.modal(window,params["open"].as_bool().unwrap_or(false)).map_err(native_error)?;Ok(Value::Null)},
            "view.ready"=>{
                let b=self.binding(source.ok_or_else(||err("INVALID_STATE","需要视图上下文"))?)?;
                *b.ready.lock().unwrap()=true;b.changed.notify_all();
                let guard=b.ready.lock().unwrap();let (_ready,timeout)=b.changed.wait_timeout_while(guard,Duration::from_secs(10),|_|b.phase.load(Ordering::SeqCst)==0).unwrap();
                if timeout.timed_out()||b.phase.load(Ordering::SeqCst)==3 {Err(err("MOVE_CANCELLED","视图挂载未提交"))}else{Ok(json!({"ready":true}))}
            },
            "view.snapshotChunk"|"view.moveRejected"=>self.snapshot_chunk(source,method,params),
            _=>Err(err("UNSUPPORTED_API",format!("未实现的方法：{method}"))),
        }
    }
    fn snapshot_chunk(&self,source:Option<&str>,method:&str,params:&Value)->Result<Value>{
        let source=source.ok_or_else(||err("SOURCE_INVALID","快照必须来自源视图"))?;
        let id=required(params,"operationId")?;
        let mut transfers=self.snapshots.lock().unwrap();
        let t=transfers.get_mut(id).ok_or_else(||err("OPERATION_EXPIRED","迁移操作已结束"))?;
        if t.source!=source{return Err(err("SOURCE_INVALID","快照来源与迁移不一致"));}
        if method=="view.moveRejected" {
            let t=transfers.remove(id).unwrap();let _=t.sender.send(Err(err("MOVE_CANCELLED",params["reason"].as_str().unwrap_or("视图暂时无法移动"))));return Ok(Value::Null);
        }
        let index=params["index"].as_u64().ok_or_else(||err("INVALID_PARAMS","缺少快照块序号"))? as usize;
        let total=params["total"].as_u64().ok_or_else(||err("INVALID_PARAMS","缺少快照块数量"))? as usize;
        let data=params["data"].as_str().ok_or_else(||err("INVALID_PARAMS","快照块无效"))?;
        if total==0||total>64||index!=t.next||(index>0&&total!=t.total)||data.len()>256*1024||t.data.len()+data.len()>4*1024*1024 {
            let t=transfers.remove(id).unwrap();let error=err("SNAPSHOT_INVALID","快照块序号或大小无效");let _=t.sender.send(Err(error.clone()));return Err(error);
        }
        t.total=total;t.next+=1;t.data.push_str(data);
        if t.next==total {
            let t=transfers.remove(id).unwrap();
            let result=serde_json::from_str(&t.data).map_err(|_|err("SNAPSHOT_INVALID","快照不是有效 JSON"));
            if result.is_ok(){self.binding(source)?.phase.store(2,Ordering::SeqCst);}
            let response=result.as_ref().map(|_|json!({"accepted":true})).map_err(Clone::clone);
            let _=t.sender.send(result);response
        } else {Ok(json!({"accepted":true,"next":t.next}))}
    }
    fn open_terminal(self:&Arc<Self>,server_id:&str,operation:&str)->Result<Value>{
        let _=self.loaded()?;
        if self.workbench.lock().unwrap().views.iter().filter(|v|v.view_type=="terminal.session").count()>=8{return Err(err("CAPACITY","最多同时打开 8 个终端"));}
        {
            let mut opens=self.pending_opens.lock().unwrap();
            opens.retain(|_,open|!open.finished||open.created.elapsed()<Duration::from_secs(600));
            if let Some(open)=opens.get(operation){return Err(if open.canceled{err("CANCELED","已取消连接")}else{err("REQUEST_CONFLICT","该新建操作已经使用")});}
            if opens.len()>=10000{return Err(err("CAPACITY","连接操作记录已达到容量上限"));}
            opens.insert(operation.into(),PendingOpen{created:Instant::now(),server_id:Some(server_id.into()),canceled:false,finished:false,view_id:None});
        }
        self.changed();
        let result=(||{
            let session=match self.worker("sessions.create",json!({"serverId":server_id,"workspaceId":server_id,"operationId":operation})){
                Ok(session)=>session,
                Err(error)=>return Err(if self.pending_opens.lock().unwrap().get(operation).is_some_and(|open|open.canceled)&&error.code!="CLEANUP_REQUIRED"{err("CANCELED","已取消连接")}else{error}),
            };
            let result={
                let mut opens=self.pending_opens.lock().unwrap();let open=opens.get_mut(operation).unwrap();
                if open.canceled{Err(err("CANCELED","已取消连接"))}else{
                    self.workbench.lock().unwrap().add_terminal(server_id,&session).map(|view|{open.view_id=Some(view.view_id.clone());view}).map_err(native_error)
                }
            };
            match result {
                Ok(view)=>{
                    let saved=self.worker("sessions.bind",json!({"sessionId":session["sessionId"],"viewId":view.view_id})).and_then(|_|self.save());
                    if let Err(error)=saved{let _=self.workbench.lock().unwrap().close(&view.view_id);let cleanup=self.worker("sessions.close",json!({"sessionId":session["sessionId"]})).map_err(|failure|err("CLEANUP_REQUIRED","新建终端未完成且资源清理失败，请在核心资源页检查").details(json!({"sessionId":session["sessionId"],"cleanupCode":failure.code})));self.pending_opens.lock().unwrap().get_mut(operation).unwrap().view_id=None;cleanup?;return Err(error);}
                    Ok(serde_json::to_value(view).unwrap())
                },
                Err(error)=>{self.worker("sessions.close",json!({"sessionId":session["sessionId"]})).map_err(|failure|err("CLEANUP_REQUIRED","连接已取消但资源清理失败，请在核心资源页检查").details(json!({"sessionId":session["sessionId"],"cleanupCode":failure.code})))?;Err(error)}
            }
        })();
        let result={let mut opens=self.pending_opens.lock().unwrap();let open=opens.get_mut(operation).unwrap();open.finished=true;
            if open.canceled&&result.as_ref().err().is_some_and(|error|error.code!="CLEANUP_REQUIRED"){Err(err("CANCELED","已取消连接"))}else{result}
        };
        self.changed();result
    }
    fn present(self:&Arc<Self>,id:&str,bounds:Bounds,visible:bool)->Result<Value>{
        let _mount=self.mounting.lock().unwrap();
        let view=self.workbench.lock().unwrap().view(id).map_err(native_error)?;
        if view.status=="viewFailed"{return Ok(json!({"failed":true}));}
        let window=if view.placement=="detached"{format!("detached-{id}")}else{"main".into()};
        let binding=match self.view_binding(id){Some(b)=>b,None=>{
            if !visible {return Ok(Value::Null);}
            if view.view_type=="terminal.session"&&view.pty_id.is_none(){return Ok(json!({"ended":true}));}
            self.create_binding(&view,&window,None,false)?
        }};
        if binding.phase.load(Ordering::SeqCst)!=3 {self.native.present(binding.id.clone(),bounds,visible).map_err(native_error)?;*binding.geometry.lock().unwrap()=(bounds,visible);}
        Ok(Value::Null)
    }
    fn close_view(self:&Arc<Self>,id:&str)->Result<()> {
        let _operation=self.view_operations.begin(id).ok_or_else(||err("MOVE_IN_PROGRESS","正在移动或关闭，请稍后重试"))?;
        self.close_view_inner(id)
    }
    fn queue_native_close(self:&Arc<Self>,id:String){
        if !self.view_operations.queue_close(&id){return;}
        let state=self.clone();
        std::thread::spawn(move||{
            let _operation=state.view_operations.wait_close(&id);
            if state.stopping.load(Ordering::SeqCst){return;}
            if let Err(error)=state.close_view_inner(&id){state.warning(&json!({"code":error.code,"message":format!("关闭终端失败：{}",error.message)}));}
        });
    }
    fn close_view_inner(self:&Arc<Self>,id:&str)->Result<()> {
        let view=self.workbench.lock().unwrap().view(id).map_err(native_error)?;
        if let Some(session)=&view.session_id {if let Err(error)=self.worker("sessions.close",json!({"sessionId":session})){if !["PLUGIN_CRASHED","PLUGIN_UNAVAILABLE"].contains(&error.code.as_str()){return Err(error);}if let Some(plugin)=self.loaded.lock().unwrap().as_ref(){self.core.close_instance(&plugin.instance);}}}
        let bindings=self.bindings.lock().unwrap().values().filter(|b|b.view_id==id).cloned().collect::<Vec<_>>();
        for b in bindings{b.phase.store(3,Ordering::SeqCst);b.changed.notify_all();let _=self.native.close(b.id.clone());self.bindings.lock().unwrap().remove(&b.id);}
        self.workbench.lock().unwrap().close(id).map_err(native_error)?;
        if let Some(w)=self.app.get_webview_window(&format!("detached-{id}")){let _=w.destroy();}
        self.save_committed(json!({}),"终端已关闭，但布局未保存；重启后可能恢复已结束的占位。");Ok(())
    }
    fn move_view(self:&Arc<Self>,id:&str,placement:&str,group:u8)->Result<Value>{
        let _operation=self.view_operations.begin(id).ok_or_else(||err("MOVE_IN_PROGRESS","此视图正在移动或等待关闭"))?;
        self.move_inner(id,placement,group)
    }
    fn drop_view(self:&Arc<Self>,id:&str)->Result<Value>{
        #[cfg(windows)] let point={let mut p=windows::Win32::Foundation::POINT::default();unsafe{windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p).map_err(|e|native_error(e.to_string()))?;}tauri::PhysicalPosition::new(p.x,p.y)};
        #[cfg(not(windows))] return Err(err("UNSUPPORTED_PLATFORM","本批仅验证 Windows 拖放"));
        #[cfg(windows)] {
            if let Some(main)=self.app.get_webview_window("main").filter(|w|w.is_visible().unwrap_or(false)){
                let position=main.inner_position().map_err(|e|native_error(e.to_string()))?;let size=main.inner_size().map_err(|e|native_error(e.to_string()))?;
                if point.x>=position.x&&point.y>=position.y&&point.x<position.x+size.width as i32&&point.y<position.y+size.height as i32 {
                    let scale=main.scale_factor().map_err(|e|native_error(e.to_string()))?;let x=(point.x-position.x)as f64/scale;let y=(point.y-position.y)as f64/scale;
                    let candidates=self.bindings.lock().unwrap().values().filter(|b|b.window=="main"&&b.phase.load(Ordering::SeqCst)==1).cloned().collect::<Vec<_>>();
                    let target=candidates.iter().find_map(|b|{let (r,visible)=*b.geometry.lock().unwrap();if visible&&x>=r.x&&y>=r.y&&x<r.x+r.width&&y<r.y+r.height {self.workbench.lock().unwrap().view(&b.view_id).ok().filter(|v|["center","bottom"].contains(&v.placement.as_str()))}else{None}});
                    return self.move_view(id,target.as_ref().map(|v|v.placement.as_str()).unwrap_or("center"),target.as_ref().map(|v|v.group).unwrap_or(0));
                }
            }
            let result=self.move_view(id,"detached",0)?;
            if let Some(window)=self.app.get_webview_window(&format!("detached-{id}")){let _=window.set_position(tauri::PhysicalPosition::new(point.x-80,point.y-20));}
            Ok(result)
        }
    }
    fn move_inner(self:&Arc<Self>,id:&str,placement:&str,group:u8)->Result<Value>{
        let deadline=Instant::now()+Duration::from_secs(10);
        let old=self.workbench.lock().unwrap().view(id).map_err(native_error)?;
        self.workbench.lock().unwrap().validate_placement(&old.view_type,placement).map_err(native_error)?;
        if group>1{return Err(err("UNSUPPORTED_LAYOUT","最多支持两个区域"));}
        let old_window=if old.placement=="detached"{format!("detached-{id}")}else{"main".into()};
        let target_window=if placement=="detached"{format!("detached-{id}")}else{"main".into()};
        if placement=="detached"&&old.placement!="detached"&&self.workbench.lock().unwrap().views.iter().filter(|v|v.placement=="detached").count()>=2{return Err(err("CAPACITY","最多同时打开两个独立终端窗口"));}
        if old_window==target_window {
            let moved={let mut current=self.workbench.lock().unwrap();let mut candidate=current.clone();let moved=candidate.moved(id,placement,group,old.mount_generation).map_err(native_error)?;current.commit(candidate,&self.data.join("workbench.json")).map_err(|e|err("STORAGE_ERROR",e))?;moved};
            self.changed();let mut result=serde_json::to_value(moved).unwrap();result["committed"]=json!(true);result["persisted"]=json!(true);return Ok(result);
        }
        let source=self.view_binding(id).ok_or_else(||err("VIEW_NOT_READY","请先显示此终端再移动"))?;
        let operation=Uuid::new_v4().to_string();let (tx,rx)=mpsc::sync_channel(1);self.snapshots.lock().unwrap().insert(operation.clone(),SnapshotTransfer{source:source.id.clone(),sender:tx,data:String::new(),total:0,next:0});
        let snapshot_result=(||{self.native.notify(source.id.clone(),"view.prepareMove",json!({"operationId":operation})).map_err(native_error)?;rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|err("MOVE_TIMEOUT","终端状态交接超时"))?})();
        self.snapshots.lock().unwrap().remove(&operation);
        let snapshot=match snapshot_result{Ok(v)=>v,Err(e)=>{source.phase.store(1,Ordering::SeqCst);let _=self.native.notify(source.id.clone(),"view.moveCancelled",json!({"operationId":operation}));return Err(e);}};
        let mut target_view=old.clone();target_view.mount_generation+=1;target_view.placement=placement.into();target_view.group=group;
        let mut target:Option<Arc<Binding>>=None;
        let result=(||{
            if placement=="detached" {self.open_detached(&target_view)?;}
            let b=self.create_binding(&target_view,&target_window,Some(snapshot),true)?;target=Some(b.clone());
            let ready=b.ready.lock().unwrap();let (ready,timeout)=b.changed.wait_timeout_while(ready,deadline.saturating_duration_since(Instant::now()),|ready|!*ready).unwrap();
            if timeout.timed_out()||!*ready{return Err(err("MOVE_TIMEOUT","目标终端未准备完成"));}drop(ready);
            if self.stopping.load(Ordering::SeqCst){return Err(err("APP_STOPPING","应用正在退出"));}
            self.bind_pty(&target_view)?;
            let moved=self.workbench.lock().unwrap().moved(id,placement,group,target_view.mount_generation).map_err(native_error)?;
            if placement!="detached" {self.workbench.lock().unwrap().active_workspace_id=Some(old.workspace_id.clone());}
            source.phase.store(3,Ordering::SeqCst);{let _ready=b.ready.lock().unwrap();b.phase.store(1,Ordering::SeqCst);b.changed.notify_all();}
            let _=self.native.notify(source.id.clone(),"view.moveCommitted",json!({"operationId":operation}));
            let _=self.native.close(source.id.clone());self.bindings.lock().unwrap().remove(&source.id);
            if old.placement=="detached"{if let Some(w)=self.app.get_webview_window(&old_window){let _=w.destroy();}}
            Ok(self.save_committed(serde_json::to_value(moved).unwrap(),"移动已完成，但布局未保存；重启后可能恢复原位置。"))
        })();
        if result.is_err() && source.phase.load(Ordering::SeqCst)!=3 {
            if let Some(b)=target{b.phase.store(3,Ordering::SeqCst);b.changed.notify_all();let _=self.native.close(b.id.clone());self.bindings.lock().unwrap().remove(&b.id);}
            if placement=="detached" {if let Some(w)=self.app.get_webview_window(&target_window){let _=w.destroy();}}
            source.phase.store(1,Ordering::SeqCst);let _=self.native.notify(source.id.clone(),"view.moveCancelled",json!({"operationId":operation}));
        }
        result
    }
    fn open_detached(&self,view:&View)->Result<()> {
        let label=format!("detached-{}",view.view_id);
        if self.app.get_webview_window(&label).is_some(){return Ok(());}
        let server=self.core.call(&Caller::shell(),"servers.get",json!({"serverId":view.server_id}))?;
        let mut window=WebviewWindowBuilder::new(&self.app,&label,WebviewUrl::App("index.html".into()))
            .title(format!("{} · {} — SoDeck",server["name"].as_str().unwrap_or("服务器"),view.title))
            .inner_size(960.,640.).min_inner_size(480.,320.)
            .initialization_script(format!("window.__SODECK_CONTEXT__={{detachedViewId:{}}};",serde_json::to_string(&view.view_id).unwrap()))
            .data_directory(self.data.join("profiles/shell"));
        if cfg!(debug_assertions){if let Ok(port)=std::env::var("SODECK_SHELL_DEBUG_PORT"){window=window.additional_browser_args(&format!("--remote-debugging-port={port}"));}}
        window.build().map_err(|e|native_error(e.to_string()))?;
        Ok(())
    }
    fn surface_failed(self:&Arc<Self>,source:&str){
        if let Ok(b)=self.binding(source){b.phase.store(3,Ordering::SeqCst);b.changed.notify_all();self.bindings.lock().unwrap().remove(source);let _=self.native.close(source.into());
            if let Some(v)=self.workbench.lock().unwrap().views.iter_mut().find(|v|v.view_id==b.view_id){v.status="viewFailed".into();}self.changed();}
    }
    fn shutdown(&self){
        if self.stopping.swap(true,Ordering::SeqCst){return;}
        let bindings=self.bindings.lock().unwrap().values().cloned().collect::<Vec<_>>();
        for b in bindings{b.phase.store(3,Ordering::SeqCst);b.changed.notify_all();let _=self.native.close(b.id.clone());}
        if let Some(plugin)=self.loaded.lock().unwrap().take(){let _=plugin.process.stop();self.core.close_instance(&plugin.instance);}
        self.core.shutdown();let _=self.save();
    }
    fn monitor(self:&Arc<Self>){
        let weak=Arc::downgrade(self);std::thread::spawn(move||loop{
            std::thread::sleep(Duration::from_millis(250));let Some(state)=weak.upgrade()else{break};
            if state.stopping.load(Ordering::SeqCst){break;}
            let plugin=state.loaded.lock().unwrap().clone();
            if let Some(plugin)=plugin {if !plugin.process.is_alive(){
                state.core.close_instance(&plugin.instance);
                state.loaded.lock().unwrap().take();
                *state.startup_error.lock().unwrap()=Some("终端插件后台已结束，所属连接已回收；可重启 SoDeck 重新加载插件。".into());
                let mut w=state.workbench.lock().unwrap();for view in &mut w.views {if view.session_id.is_some(){view.status="ended".into();view.session_id=None;view.pty_id=None;view.generation=None;}}drop(w);
                let bindings=state.bindings.lock().unwrap().values().cloned().collect::<Vec<_>>();for b in bindings{b.phase.store(3,Ordering::SeqCst);b.changed.notify_all();let _=state.native.close(b.id.clone());}state.bindings.lock().unwrap().clear();state.changed();break;
            }}
        });
    }
}

#[tauri::command]
async fn rpc(window:tauri::WebviewWindow,state:tauri::State<'_,Arc<State>>,request:Request)->std::result::Result<Response,String>{
    let state=state.inner().clone();let id=request.request_id.clone();
    if window.label()!="main"&&!window.label().starts_with("detached-"){return Ok(Response::from(id,Err(err("SOURCE_INVALID","未知原生调用来源"))));}
    let mut request=request;
    if request.method=="workbench.modal"{request.params["windowLabel"]=Value::String(window.label().into());}
    Ok(tauri::async_runtime::spawn_blocking(move||state.request(None,request)).await.unwrap_or_else(|_|Response::from(id,Err(err("INTERNAL","调用任务意外结束")))))
}

fn bootstrap_paths(app:&tauri::AppHandle)->(PathBuf,PathBuf){
    if let (Some(package),Some(keys))=(std::env::var_os("SODECK_PLUGIN_PACKAGE"),std::env::var_os("SODECK_PLUGIN_KEYS")){return (package.into(),keys.into());}
    let root=app.path().resource_dir().unwrap_or_else(|_|PathBuf::from("."));
    let dir=if root.join("resources/terminal.sodeck-plugin").exists(){root.join("resources")}else{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../resources")};
    (dir.join("terminal.sodeck-plugin"),dir.join("development-public-key.json"))
}
fn read_keys(path:&Path)->std::result::Result<TrustedKeys,String>{
    use base64::Engine;
    let value:Value=serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut keys=TrustedKeys::new();
    if let (Some(id),Some(key))=(value["keyId"].as_str(),value["publicKey"].as_str()) {keys.insert(id.into(),base64::engine::general_purpose::STANDARD.decode(key).map_err(|e|e.to_string())?);}
    else if let Some(map)=value.as_object(){for (id,key) in map {if let Some(key)=key.as_str(){keys.insert(id.into(),base64::engine::general_purpose::STANDARD.decode(key).map_err(|e|e.to_string())?);}}}
    if keys.is_empty(){return Err("开发插件签名公钥无效".into());}Ok(keys)
}
pub fn run()->std::result::Result<(),Box<dyn std::error::Error>>{
    tauri::Builder::default().invoke_handler(tauri::generate_handler![rpc]).setup(|app|{
        let handle=app.handle().clone();
        let data=std::env::var_os("SODECK_DATA_DIR").map(PathBuf::from).unwrap_or(handle.path().app_data_dir()?);
        fs::create_dir_all(&data)?;
        let lock=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(data.join("app.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_|std::io::Error::other("此数据目录已有 SoDeck 实例运行"))?;
        let core=Arc::new(Core::open(data.join("core"))?);
        let (package,keys)=bootstrap_paths(&handle);
        let mut startup_error=None;
        let trusted=match read_keys(&keys){Ok(k)=>k,Err(e)=>{startup_error=Some(e);TrustedKeys::new()}};
        let mut plugins=PluginManager::open(data.join("plugins"),trusted)?;
        if plugins.status().is_empty()&&package.exists(){if let Err(e)=plugins.prepare_package(&package){startup_error=Some(e.to_string());}}
        let init_core=core.clone();
        if let Err(e)=plugins.activate_pending(|candidate,_|{let loaded=spawn_plugin(init_core.clone(),candidate)?;let _=loaded.process.stop();Ok(())}){startup_error=Some(e.to_string());}
        let loaded=match plugins.active("sodeck.terminal"){Ok(Some(p))=>match spawn_plugin(core.clone(),&p){Ok(p)=>Some(p),Err(e)=>{startup_error=Some(e);None}},Ok(None)=>None,Err(e)=>{startup_error=Some(e.to_string());None}};
        let mut workbench=match Workbench::load(&data.join("workbench.json")){Ok(w)=>w,Err(e)=>{startup_error=Some(e);Workbench::default()}};
        workbench.set_declarations(loaded.as_ref().map(|plugin|plugin.package.manifest.contributes.views.clone()).unwrap_or_default()).map_err(std::io::Error::other)?;
        let state=Arc::new(State{core,plugins:Mutex::new(plugins),loaded:Mutex::new(loaded),workbench:Mutex::new(workbench),bindings:Mutex::new(HashMap::new()),view_operations:ViewOperations::default(),snapshots:Mutex::new(HashMap::new()),pending_opens:Mutex::new(HashMap::new()),cache:Mutex::new(HashMap::new()),mounting:Mutex::new(()),native:NativeViews::new(handle.clone()).map_err(std::io::Error::other)?,app:handle.clone(),data,stopping:AtomicBool::new(false),inflight:AtomicUsize::new(0),_data_lock:lock,startup_error:Mutex::new(startup_error)});
        app.manage(state.clone());
        if let Some(plugin)=state.loaded.lock().unwrap().as_ref(){*plugin.gateway.lock().unwrap()=Some(Arc::downgrade(&state));}
        state.monitor();
        let mut main=WebviewWindowBuilder::new(app,"main",WebviewUrl::App("index.html".into())).title("SoDeck v2").inner_size(1280.,820.).min_inner_size(760.,480.).data_directory(state.data.join("profiles/shell"));
        if cfg!(debug_assertions){if let Ok(port)=std::env::var("SODECK_SHELL_DEBUG_PORT"){main=main.additional_browser_args(&format!("--remote-debugging-port={port}"));}}
        main.build()?;
        let restored=state.workbench.lock().unwrap().views.iter().filter(|v|v.placement=="detached").cloned().collect::<Vec<_>>();
        for view in restored {if let Err(error)=state.open_detached(&view){*state.startup_error.lock().unwrap()=Some(error.message);}}
        use tauri::{menu::{Menu,MenuItem},tray::TrayIconBuilder};
        let show=MenuItem::with_id(app,"show","显示主窗口",true,None::<&str>)?;
        let exit=MenuItem::with_id(app,"exit","退出 SoDeck",true,None::<&str>)?;
        let menu=Menu::with_items(app,&[&show,&exit])?;
        let mut pixels=vec![0u8;32*32*4];for p in pixels.chunks_exact_mut(4){p.copy_from_slice(&[71,113,180,255]);}
        let tray=TrayIconBuilder::with_id("sodeck").icon(tauri::image::Image::new_owned(pixels,32,32)).tooltip("SoDeck v2").menu(&menu)
            .on_menu_event(|app,event|match event.id().as_ref(){"show"=>{if let Some(w)=app.get_webview_window("main"){let _=w.show();let _=w.unminimize();let _=w.set_focus();}},"exit"=>{let state=app.state::<Arc<State>>().inner().clone();std::thread::spawn(move||{state.shutdown();state.app.exit(0);});},_=>{}}).build(app);
        if let Err(e)=tray{*state.startup_error.lock().unwrap()=Some(format!("系统托盘不可用：{e}"));}
        Ok(())
    }).on_window_event(|window,event|{
        let Some(state)=window.app_handle().try_state::<Arc<State>>()else{return;};let state=state.inner().clone();
        match event {
            tauri::WindowEvent::CloseRequested{api,..}=>{
                if state.stopping.load(Ordering::SeqCst){return;}
                api.prevent_close();
                if window.label()=="main" {if state.app.tray_by_id("sodeck").is_some(){let _=window.hide();}}
                else if let Some(id)=window.label().strip_prefix("detached-"){state.queue_native_close(id.to_owned());}
            },
            _=>{}
        }
    }).build(tauri::generate_context!())?.run(|app,event|{
        if let tauri::RunEvent::ExitRequested{api,code,..}=event {if let Some(state)=app.try_state::<Arc<State>>(){if !state.stopping.load(Ordering::SeqCst){api.prevent_exit();if code.is_some(){let state=state.inner().clone();std::thread::spawn(move||{state.shutdown();state.app.exit(0);});}}}}
    });
    Ok(())
}
