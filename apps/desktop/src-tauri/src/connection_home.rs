//! System navigation belongs to the workbench; drafts never cross this boundary.
use super::*;
use crate::window_registry::Selection;

#[derive(Clone)]
pub(super) struct PageSession {
    pub id:String,pub renderer:String,pub revision:String,pub dirty:u64,pub operations:Vec<String>,pub picker:bool,
}
#[derive(Default)]
pub(super) struct Registry {
    pub sessions:HashMap<String,PageSession>,
    pub receipts:HashMap<(String,String),(Value,Value,Instant)>,
}
impl State {
    pub(super) fn home_snapshot(&self,window:&str)->Result<Value>{
        let w=self.workbench.lock().unwrap();
        let page=w.windows.get(window);
        let mut snapshot=w.snapshot(window);
        snapshot["pendingOpens"]=json!(self.pending_opens.lock().unwrap().iter().filter(|(_,o)|!o.finished&&o.window_id==window).map(|(id,o)|json!({"operationId":id,"serverId":o.server_id,"commandId":o.command_id,"cancelling":o.canceled})).collect::<Vec<_>>());
        Ok(json!({"workbench":snapshot,"page":{"open":page.is_some_and(|p|p.home_open),"selected":page.is_some_and(|p|p.selection==Selection::ConnectionHome),"generation":page.map(|p|p.home_generation.as_str()).unwrap_or("detached")},"navigationRevision":w.revision.to_string()}))
    }
    pub(super) fn page_allows_surface(&self,view:&View,window:&str)->bool{
        if view.placement=="detached"{return true;}
        let w=self.workbench.lock().unwrap();
        w.windows.get(window).is_some_and(|r|r.selection!=Selection::ConnectionHome&&(view.workspace_id=="@app"||r.selected_workspace()==Some(&view.workspace_id)))
    }
    pub(super) fn connection_home_call(self:&Arc<Self>,source:Option<&str>,method:&str,p:&Value)->Result<Value>{
        sodeck_core::connection_home_contract::validate("x-methods",method,p)?;
        let source_window=self.source_window(source,&json!({}))?;
        if method=="ui.connectionHome.snapshot"{return self.home_snapshot(&source_window);}
        if method=="ui.connectionHome.open"{return self.open_connection_home(source,&source_window,p);}
        // Only the actual shell renderer can report/answer for its own session.
        if source!=Some(format!("@window:{source_window}").as_str())||!self.workbench.lock().unwrap().windows.contains_key(&source_window){return Err(err("SOURCE_INVALID","连接页会话需要工作台界面来源"));}
        if method=="ui.connectionHome.session.attach"{
            self.ensure_review_mutable()?;
            let renderer=required(p,"rendererId")?;let mut registry=self.connection_home.lock().unwrap();
            if let Some(s)=registry.sessions.get(&source_window).filter(|s|s.renderer==renderer){return Ok(json!({"pageSession":s.id,"replacedDirty":false}));}
            let previous=registry.sessions.get(&source_window).is_some_and(|s|s.dirty>0||!s.operations.is_empty());
            let session=Uuid::new_v4().to_string();
            registry.sessions.insert(source_window.clone(),PageSession{id:session.clone(),renderer:renderer.into(),revision:"0".into(),dirty:0,operations:vec![],picker:false});
            drop(registry);self.cancel_close_review();
            return Ok(json!({"pageSession":session,"replacedDirty":previous}));
        }
        let session={let registry=self.connection_home.lock().unwrap();registry.sessions.get(&source_window).filter(|s|Some(s.id.as_str())==p["pageSession"].as_str()||method=="ui.connectionHome.close.request").cloned().ok_or_else(||err("SOURCE_INVALID","连接页会话已失效"))?};
        match method {
            "ui.connectionHome.session.report"=>{
                let mut registry=self.connection_home.lock().unwrap();let s=registry.sessions.get_mut(&source_window).filter(|s|s.id==session.id).ok_or_else(||err("SOURCE_INVALID","页面会话已改变"))?;
                s.revision=required(p,"draftRevision")?.into();s.dirty=p["dirtyCount"].as_u64().unwrap();s.operations=serde_json::from_value(p["operations"].clone()).map_err(|_|err("INVALID_PARAMS","保存操作摘要无效"))?;
                drop(registry);self.review_report(&source_window,&session.id,p);
                Ok(json!({"accepted":true}))
            },
            "ui.connectionHome.close.request"=>{
                if !self.begin_close_review(window_close_review::Action::Home(source_window.clone()),source)?{return self.close_home_committed(&source_window);}
                Ok(json!({"accepted":true}))
            },
            "ui.connectionHome.close.respond"=>{
                self.review_respond(&source_window,&session.id,p)?;Ok(json!({"accepted":true}))
            },
            "ui.connectionHome.pickPrivateKey"=>{
                let window=self.app.get_webview_window(&source_window).ok_or_else(||err("WINDOW_CLOSED","窗口已关闭"))?;
                {let mut registry=self.connection_home.lock().unwrap();let s=registry.sessions.get_mut(&source_window).unwrap();if s.picker{return Err(err("BUSY","文件选择器已打开"));}s.picker=true;}
                let path=rfd::FileDialog::new().set_parent(&window).set_title("选择 SSH 私钥").pick_file();
                let mut registry=self.connection_home.lock().unwrap();let s=registry.sessions.get_mut(&source_window).filter(|s|s.id==session.id).ok_or_else(||err("SOURCE_INVALID","页面已重新载入"))?;s.picker=false;
                Ok(match path {Some(path)=>json!({"status":"selected","path":path.to_string_lossy()}),None=>json!({"status":"canceled"})})
            },
            _=>Err(err("UNSUPPORTED_API","未知连接页操作")),
        }
    }
    fn open_connection_home(self:&Arc<Self>,source:Option<&str>,origin:&str,p:&Value)->Result<Value>{
        self.ensure_review_mutable()?;
        let _navigation=self.home_navigation.lock().unwrap();
        let target=if let Some(target)=p["targetWindowId"].as_str(){target.to_owned()}else if let Some(id)=origin.strip_prefix("detached-"){
            let w=self.workbench.lock().unwrap();w.view(id).ok().and_then(|v|w.owner(&v.workspace_id).ok()).unwrap_or("main".into())
        }else if self.workbench.lock().unwrap().windows.contains_key(origin){origin.into()}else{"main".into()};
        if self.shell_modals.lock().unwrap().contains(&target)||self.dialogs.lock().unwrap().values().any(|d|d.owner==target&&!d.terminal()){return Err(err("MODAL_ACTIVE","请先完成当前对话框"));}
        if p["intent"]["kind"]=="editServer"{self.core.call(&Caller::shell(),"servers.get",json!({"serverId":p["intent"]["serverId"]}))?;}
        let key=(target.clone(),format!("{}:{}",source.unwrap_or("@shell"),required(p,"operationId")?));
        {let mut registry=self.connection_home.lock().unwrap();registry.receipts.retain(|_,(_,_,t)|t.elapsed()<Duration::from_secs(600));
            if let Some((request,result,_))=registry.receipts.get(&key){if request!=p{return Err(err("OPERATION_CONFLICT","导航操作标识已被使用"));}let result=result.clone();drop(registry);let w=self.workbench.lock().unwrap();if w.windows.get(&target).is_none_or(|r|!r.home_open||result["generation"]!=r.home_generation){return Err(err("STALE_OPERATION","此导航页面已关闭"));}return Ok(result);}
            if registry.receipts.keys().filter(|(w,_)|w==&target).count()>=256{return Err(err("CAPACITY","导航请求过多，请稍后重试"));}
        }
        let result={
            let _mount=self.mounting.lock().unwrap();
            if !self.workbench.lock().unwrap().windows.contains_key(&target){return Err(err("WINDOW_CLOSED","目标工作台已关闭"));}
            let bindings=self.bindings.lock().unwrap().values().filter(|b|b.window==target).cloned().collect::<Vec<_>>();
            for b in bindings{let (bounds,visible)=*b.geometry.lock().unwrap();if visible{self.native.present(b.id.clone(),bounds,false).map_err(native_error)?;b.geometry.lock().unwrap().1=false;}}
            let mut w=self.workbench.lock().unwrap();let r=w.windows.get_mut(&target).unwrap();r.select_home();let generation=r.home_generation.clone();w.revision+=1;
            json!({"windowId":target,"generation":generation,"navigationRevision":w.revision.to_string()})
        };
        self.connection_home.lock().unwrap().receipts.insert(key,(p.clone(),result.clone(),Instant::now()));
        let ids=self.workbench.lock().unwrap().views.iter().filter(|v|v.placement!="detached").map(|v|v.view_id.clone()).collect::<Vec<_>>();for id in ids{self.publish_presentation(&id);}
        self.endpoint_notify(&format!("@window:{target}"),"connectionHome.navigate",json!({"intent":p["intent"],"generation":result["generation"],"navigationRevision":result["navigationRevision"]}));
        self.show_window(&target);self.update_window_titles();self.changed();Ok(result)
    }
    pub(super) fn close_home_committed(&self,window:&str)->Result<Value>{
        let mut w=self.workbench.lock().unwrap();let page=w.windows.get_mut(window).ok_or_else(||err("WINDOW_CLOSED","窗口已关闭"))?;
        page.close_home();w.revision+=1;drop(w);self.update_window_titles();self.changed();Ok(json!({"accepted":true}))
    }
}
