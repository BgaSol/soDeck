//! Endpoint ownership and native export. Collection/querying stays in core.
use super::*;
use std::io::Write;

struct ExportPermit(String);
#[derive(Clone)]struct ExportTask{job:String,parent:isize}
fn export_dialogs()->&'static Mutex<std::collections::HashMap<String,ExportTask>>{static OPEN:std::sync::OnceLock<Mutex<std::collections::HashMap<String,ExportTask>>>=std::sync::OnceLock::new();OPEN.get_or_init(Default::default)}
impl Drop for ExportPermit{fn drop(&mut self){export_dialogs().lock().unwrap().remove(&self.0);}}
impl State {
    pub(super) fn cancel_window_evidence_export(&self,window:&str){
        let key=format!("{}:{}",self.app_generation,window);
        let task=export_dialogs().lock().unwrap().get(&key).cloned();
        if let Some(task)=task {
            let _=self.core.call(&Caller::shell(),"evidence.cancel",json!({"jobId":task.job}));
            #[cfg(windows)] close_save_dialog(task.parent);
        }
    }
    pub(super) fn stop_evidence_exports(&self){
        let prefix=format!("{}:",self.app_generation);
        let active=export_dialogs().lock().unwrap().iter().filter(|(key,_)|key.starts_with(&prefix)).map(|(_,task)|task.clone()).collect::<Vec<_>>();
        for task in active{
            let _=self.core.call(&Caller::shell(),"evidence.cancel",json!({"jobId":task.job}));
            #[cfg(windows)] close_save_dialog(task.parent);
        }
        let started=Instant::now();
        while export_dialogs().lock().unwrap().keys().any(|key|key.starts_with(&prefix))&&started.elapsed()<Duration::from_secs(2){std::thread::sleep(Duration::from_millis(10));}
    }
    pub(super) fn observation_call(self:&Arc<Self>,caller:&Caller,source:Option<&str>,method:&str,params:Value)->Result<Value>{
        if method=="observability.targets.resolve"&&matches!(params["kind"].as_str(),Some("workspace"|"view")){
            return self.core.record(caller,method,&params,||{
                let kind=required(&params,"kind")?;let id=required(&params,"id")?;let workbench=self.workbench.lock().unwrap();
                let generation=if kind=="workspace"{workbench.workspaces.get(id).map(|w|w.generation.clone()).ok_or_else(||err("STALE_WORKSPACE","工作区已经结束"))?}else{workbench.view(id).map_err(native_error)?.mount_generation.to_string()};
                if params["generation"].as_str().is_some_and(|g|g!=generation){return Err(err("STALE_HANDLE","诊断对象代次已经改变"));}
                self.core.observation_target(kind,id,Some(&generation))
            });
        }
        if matches!(method,"events.v2.open"|"events.v2.ack"|"events.v2.close"){
            return self.core.observation_feed(caller,&self.endpoint_feed_owner(caller,source),method,&params);
        }
        if method=="evidence.export" {
            return self.core.record(caller,method,&params,||self.export_evidence(source,&params));
        }
        self.core.call(caller,method,params)
    }
    pub(super) fn start_observation_pump(self:&Arc<Self>){
        let weak=Arc::downgrade(self);
        std::thread::spawn(move||loop{
            std::thread::sleep(Duration::from_millis(50));
            let Some(state)=weak.upgrade()else{break};if state.stopping.load(Ordering::SeqCst){break;}
            for delivery in state.core.observation_deliveries(){
                let event=&delivery.event;let owner=&delivery.owner;
                if owner.ends_with(":worker:"){
                    if let Some(plugin)=state.all_plugins().into_iter().find(|p|*owner==format!("{}:worker:",p.instance)&&p.process.is_alive()){let _=plugin.process.notify_serialized(event,delivery.payload,delivery.retention);}else{state.core.close_observation_owner(owner);}continue;
                }
                let Ok(payload)=serde_json::from_str::<Value>(&delivery.payload)else{continue};
                if let Some(window)=owner.strip_prefix(&format!("@window:{}:",state.app_generation)){
                    if let Some(window)=state.app.get_webview_window(window){let _=window.eval(&format!("window.__sodeckNotify?.({});",json!({"type":event,"payload":payload})));}
                    else{state.core.close_observation_owner(owner);}continue;
                }
                if owner.starts_with("@dialog:"){
                    let dialog=state.dialogs.lock().unwrap().values().find(|d|d.feed_owner==*owner&&!d.cleaned.load(Ordering::SeqCst)).cloned();
                    if let Some(d)=dialog{let _=state.native.notify(d.source.clone(),event,payload);}
                    else{state.core.close_observation_owner(owner);}continue;
                }
                let plugin=state.all_plugins().into_iter().find(|p|owner.starts_with(&format!("{}:",p.instance))&&p.process.is_alive());
                if let Some(plugin)=plugin{
                    if *owner==format!("{}:worker:",plugin.instance){let _=plugin.process.notify(event,payload);continue;}
                    let binding=state.bindings.lock().unwrap().values().find(|b|*owner==format!("{}:{}:{}",plugin.instance,b.view_id,b.generation)&&b.phase.load(Ordering::SeqCst)==1).cloned();
                    if let Some(binding)=binding{let _=state.native.notify(binding.id.clone(),event,payload);}
                    else{state.core.close_observation_owner(owner);}
                }else{state.core.close_observation_owner(owner);}
            }
        });
    }
    fn export_evidence(self:&Arc<Self>,source:Option<&str>,params:&Value)->Result<Value>{
        if params.as_object().is_none_or(|o|o.len()!=2||!o.contains_key("jobId")||o.get("format")!=Some(&json!("jsonl"))){return Err(err("INVALID_PARAMS","导出只接受证据任务标识"));}
        let package=required(params,"jobId")?.to_owned();
        let source=source.ok_or_else(||err("UI_REQUIRED","导出需要来源窗口"))?.to_owned();
        let window=self.endpoint_window(&source,true)?;
        let parent=self.app.get_webview_window(&window).ok_or_else(||err("SOURCE_INVALID","导出来源窗口已经关闭"))?;
        #[cfg(windows)] let native_parent=parent.hwnd().map_err(|_|err("UI_REQUIRED","原生父窗口不可用"))?.0 as isize;
        #[cfg(not(windows))] let native_parent=0;
        let key=format!("{}:{}",self.app_generation,window);let mut exports=export_dialogs().lock().unwrap();
        if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
        if exports.contains_key(&key){return Err(err("BUSY","此窗口已有保存对话框"));}
        let initial=self.core.begin_evidence_export(&package)?;let job=initial["jobId"].as_str().unwrap().to_owned();
        exports.insert(key.clone(),ExportTask{job:job.clone(),parent:native_parent});drop(exports);let permit=ExportPermit(key);
        let (send,receive)=mpsc::sync_channel(1);
        let dialog_state=Arc::downgrade(self);
        let dialog_job=job.clone();
        if self.app.run_on_main_thread(move||{
            if dialog_state.upgrade().is_none_or(|state|state.stopping.load(Ordering::SeqCst)||state.core.evidence_export_canceled(&dialog_job)){let _=send.send(None);return;}
            let selected=rfd::FileDialog::new().set_parent(&parent).set_title("导出诊断证据").set_file_name("sodeck-evidence.jsonl").add_filter("诊断证据",&["jsonl"]).save_file();
            let _=send.send(selected);
        }).is_err(){let _=self.core.finish_evidence_export(&job,"failed",Some("NATIVE_ERROR"));return Err(err("NATIVE_ERROR","无法打开保存对话框"));}
        let weak=Arc::downgrade(self);let core=self.core.clone();
        std::thread::spawn(move||{
            let _permit=permit;
            let result=(||->Result<()>{
                let started=Instant::now();let target=loop{match receive.recv_timeout(Duration::from_millis(100)){
                    Ok(selected)=>break selected.ok_or_else(||err("CANCELED","已取消导出"))?,
                    Err(mpsc::RecvTimeoutError::Disconnected)=>return Err(err("CANCELED","保存对话框已结束")),
                    Err(mpsc::RecvTimeoutError::Timeout)=>{if core.evidence_export_canceled(&job)||started.elapsed()>Duration::from_secs(300)||weak.upgrade().is_none_or(|state|state.stopping.load(Ordering::SeqCst)||state.endpoint_window(&source,true).is_err()){
                        #[cfg(windows)] close_save_dialog(native_parent);
                        // Keep the per-window permit until the actual modal closes.
                        if receive.recv_timeout(Duration::from_secs(2)).is_ok(){return Err(err("CANCELED","导出已取消"));}
                    }}
                }};
                let state=weak.upgrade().ok_or_else(||err("CANCELED","应用已经结束"))?;
                if state.stopping.load(Ordering::SeqCst)||state.endpoint_window(&source,true).is_err(){return Err(err("CANCELED","导出来源已经结束"));}
                let canceled=||core.evidence_export_canceled(&job)||state.stopping.load(Ordering::SeqCst)||state.endpoint_window(&source,true).is_err();
                core.evidence_export_writing(&job)?;
                export_file(&target,||canceled(),|write|{
                    let mut offset=0;
                    loop{if canceled(){return Err(err("CANCELED","导出已取消"));}let page=core.read_evidence_chunks(&package,offset)?;
                        for chunk in page["chunks"].as_array().unwrap(){write(chunk.as_str().unwrap().as_bytes())?;}
                        match page["nextOffset"].as_u64(){Some(next)=>offset=next,None=>break}
                    }Ok(())
                })
            })();
            let (status,code)=match &result{Ok(())=>("succeeded",None),Err(e)if e.code=="CANCELED"=>("canceled",Some(e.code.as_str())),Err(e)=>("failed",Some(e.code.as_str()))};
            let _=core.finish_evidence_export(&job,status,code);
        });Ok(initial)
    }
}

fn export_file(target:&Path,canceled:impl Fn()->bool,read:impl FnOnce(&mut dyn FnMut(&[u8])->Result<()>)->Result<()>)->Result<()>{
    let temp=target.with_file_name(format!(".sodeck-{}.pending",Uuid::new_v4()));
    let mut options=fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(windows)]{
        use std::os::windows::fs::OpenOptionsExt;
        // Crash/cancel cannot leave a readable partial package behind.
        options.share_mode(7).access_mode(0x40000000|0x00010000).custom_flags(0x04000000);
    }
    let mut file=options.open(&temp).map_err(|_|err("EXPORT_IO","无法创建临时证据文件"))?;
    let result=(||{
        read(&mut |bytes|file.write_all(bytes).map_err(|_|err("EXPORT_IO","无法写入证据")))?;
        file.sync_all().map_err(|_|err("EXPORT_IO","无法同步证据"))?;
        if canceled(){return Err(err("CANCELED","导出已取消"));}
        #[cfg(windows)]{
            use std::os::windows::ffi::OsStrExt;
            use windows::{core::PCWSTR,Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_WRITE_THROUGH}};
            let from:Vec<u16>=temp.as_os_str().encode_wide().chain(Some(0)).collect();let to:Vec<u16>=target.as_os_str().encode_wide().chain(Some(0)).collect();
            // No REPLACE_EXISTING: a race or an existing user file fails safely.
            unsafe{MoveFileExW(PCWSTR(from.as_ptr()),PCWSTR(to.as_ptr()),MOVEFILE_WRITE_THROUGH)}.map_err(|_|err("EXPORT_IO","目标已存在或无法保存证据文件"))?;
        }
        #[cfg(windows)]{
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Storage::FileSystem::{SetFileInformationByHandle,FileDispositionInfoEx,FILE_DISPOSITION_INFO_EX,FILE_DISPOSITION_FLAG_ON_CLOSE};
            use windows::Win32::Foundation::HANDLE;
            let disposition=FILE_DISPOSITION_INFO_EX{Flags:FILE_DISPOSITION_FLAG_ON_CLOSE};
            unsafe{SetFileInformationByHandle(HANDLE(file.as_raw_handle()),FileDispositionInfoEx,&disposition as *const _ as *const _,std::mem::size_of_val(&disposition)as u32)}.map_err(|_|err("EXPORT_IO","无法提交证据文件"))?;
        }
        #[cfg(not(windows))]{fs::hard_link(&temp,target).map_err(|_|err("EXPORT_IO","目标已存在或无法保存证据文件"))?;let _=fs::remove_file(&temp);}
        Ok(())
    })();drop(file);if result.is_err(){let _=fs::remove_file(&temp);}result
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn export_commits_complete_bytes_and_preserves_existing_destinations(){
        let root=std::env::temp_dir().join(format!("sodeck-export-test-{}",Uuid::new_v4()));fs::create_dir(&root).unwrap();let path=root.join("evidence.jsonl");
        export_file(&path,||false,|write|write(b"complete\n")).unwrap();assert_eq!(fs::read(&path).unwrap(),b"complete\n");
        assert!(export_file(&path,||false,|write|write(b"replacement")).is_err());assert_eq!(fs::read(&path).unwrap(),b"complete\n");
        assert!(export_file(&root.join("cancel.jsonl"),||true,|write|write(b"partial")).is_err());assert_eq!(fs::read_dir(&root).unwrap().count(),1);
        fs::remove_file(path).unwrap();fs::remove_dir(root).unwrap();
    }
}

#[cfg(windows)]fn close_save_dialog(parent:isize){
    use windows::{core::BOOL,Win32::{Foundation::{HWND,LPARAM,WPARAM},UI::WindowsAndMessaging::{EnumWindows,GetWindow,GetClassNameW,PostMessageW,GW_OWNER,WM_CLOSE}}};
    unsafe extern "system" fn visit(window:HWND,parent:LPARAM)->BOOL{
        let mut name=[0u16;32];let count=unsafe{GetClassNameW(window,&mut name)} as usize;
        if unsafe{GetWindow(window,GW_OWNER)}.ok()==Some(HWND(parent.0 as *mut _))&&String::from_utf16_lossy(&name[..count])=="#32770"{let _=unsafe{PostMessageW(Some(window),WM_CLOSE,WPARAM(0),LPARAM(0))};}
        true.into()
    }
    let _=unsafe{EnumWindows(Some(visit),LPARAM(parent))};
}
