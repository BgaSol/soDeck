//! Two-phase review: permissions do not dispose drafts. Only a successful action commits.
use super::*;
use std::cell::Cell;
thread_local!{static COMMITTING:Cell<bool>=const{Cell::new(false)};}
struct CommitScope;
impl Drop for CommitScope{fn drop(&mut self){COMMITTING.with(|v|v.set(false));}}
#[derive(Clone)]
pub(super) enum Action {
    Exit, Home(String), Close(Vec<String>),
    InstallUpdate{operation_id:String,restart_only:bool},
    Move{ids:Vec<String>,target:String,index:usize,active:Option<String>,operation:Option<String>,request:Value},
}
#[derive(Clone)]
struct Participant { session:String, revision:Option<String>, acknowledged:bool, allowed:bool, settings:bool, operations:Vec<String> }
pub(super) struct Review {ticket:String,action:Action,targets:Value,participants:HashMap<String,Participant>,finishing:bool,created:Instant}
impl State {
    pub(super) fn review_targets(&self,action:&Action)->Value{
        let workbench=self.workbench.lock().unwrap();
        let mut windows=match action{
            Action::Exit|Action::InstallUpdate{..}=>workbench.windows.keys().cloned().collect::<Vec<_>>(),
            Action::Home(window)=>vec![window.clone()],
            Action::Close(ids)|Action::Move{ids,..}=>ids.iter().filter_map(|id|workbench.workspaces.get(id).map(|w|w.owner_window_id.clone())).collect(),
        };
        if let Action::Move{target,..}=action{windows.push(target.clone());}
        windows.sort();windows.dedup();
        let mut ids=match action{Action::Exit|Action::InstallUpdate{..}=>workbench.workspaces.keys().cloned().collect::<Vec<_>>(),Action::Close(ids)|Action::Move{ids,..}=>ids.clone(),Action::Home(_)=>vec![]};
        ids.sort();
        json!({"windows":windows.iter().map(|id|json!([id,workbench.windows.get(id).map(|w|&w.home_generation)])).collect::<Vec<_>>(),
            "workspaces":ids.iter().map(|id|json!([id,workbench.workspaces.get(id).map(|w|(&w.generation,&w.owner_window_id))])).collect::<Vec<_>>()})
    }
    pub(super) fn ensure_review_mutable(&self)->Result<()>{
        if !COMMITTING.with(Cell::get)&&self.close_review.lock().unwrap().is_some(){return Err(err("CLOSE_REVIEW_REQUIRED","请先完成当前关闭确认"));}Ok(())
    }
    pub(super) fn begin_close_review(self:&Arc<Self>,action:Action,_source:Option<&str>)->Result<bool>{
        if COMMITTING.with(Cell::get){return Ok(false);}
        let _intent=self.close_intent.lock().unwrap();
        let previous=self.close_review.lock().unwrap().as_ref().map(|r|(r.ticket.clone(),matches!(action,Action::Exit)&&matches!(r.action,Action::Exit)&&!r.finishing));
        if let Some((ticket,replace))=previous{
            // A fresh exit must not disappear behind an in-flight "keep editing" reply.
            // The old ticket cannot authorize or cancel this new intent.
            if replace{self.cancel_close_review_ticket(&ticket);}else{return Err(err("CLOSE_REVIEW_REQUIRED","请先处理当前关闭确认"));}
        }
        let windows={let w=self.workbench.lock().unwrap();match &action{
            Action::Exit|Action::InstallUpdate{..}=>w.windows.keys().cloned().collect::<Vec<_>>(),
            Action::Home(window)=>vec![window.clone()],
            Action::Close(ids)=>w.windows.iter().filter(|(id,r)|id.as_str()!="main"&&!r.workspace_ids.is_empty()&&r.workspace_ids.iter().all(|id|ids.contains(id))).map(|(id,_)|id.clone()).collect(),
            Action::Move{ids,target,..}=>w.windows.iter().filter(|(id,r)|id.as_str()!="main"&&*id!=target&&!r.workspace_ids.is_empty()&&r.workspace_ids.iter().all(|id|ids.contains(id))).map(|(id,_)|id.clone()).collect(),
        }};
        let mut participants={let registry=self.connection_home.lock().unwrap();windows.iter().filter_map(|id|registry.sessions.get(id).map(|s|(id.clone(),Participant{session:s.id.clone(),revision:None,acknowledged:false,allowed:false,settings:false,operations:vec![]}))).collect::<HashMap<_,_>>()};
        if matches!(action,Action::Exit|Action::InstallUpdate{..}){if let Some(s)=self.settings_window.lock().unwrap().current.as_ref(){participants.insert(settings_window::LABEL.into(),Participant{session:s.generation.clone(),revision:None,acknowledged:false,allowed:false,settings:true,operations:vec![]});}}
        if participants.is_empty(){return Ok(false);}
        let ticket=Uuid::new_v4().to_string();let copy=participants.clone();
        let targets=self.review_targets(&action);
        *self.close_review.lock().unwrap()=Some(Review{ticket:ticket.clone(),action,targets,participants,finishing:false,created:Instant::now()});
        // A direct settings-close ticket must not compete with this app-wide review.
        if matches!(self.close_review.lock().unwrap().as_ref().unwrap().action,Action::Exit|Action::InstallUpdate{..}){self.settings_exit.lock().unwrap().supersede();}
        for (window,participant) in copy{
            if participant.settings{
                self.endpoint_notify(&format!("@window:settings/{}",participant.session),"settings.closeRequested",json!({"generation":participant.session,"ticket":ticket,"reason":"exit"}));
            }else{
                self.endpoint_notify(&format!("@window:{window}"),"connectionHome.closeRequested",json!({"pageSession":participant.session,"ticket":ticket}));
            }
        }
        let weak=Arc::downgrade(self);std::thread::spawn(move||{
            std::thread::sleep(Duration::from_secs(3));let Some(state)=weak.upgrade()else{return};
            let missing={let review=state.close_review.lock().unwrap();review.as_ref().filter(|r|r.ticket==ticket&&!r.finishing).map(|r|r.participants.iter().filter(|(_,p)|!p.acknowledged).map(|(w,_)|w.clone()).collect::<Vec<_>>()).unwrap_or_default()};
            if missing.is_empty(){return;}
            let answer=rfd::MessageDialog::new().set_title("SoDeck · 页面暂未响应").set_description("暂时无法核对未保存的配置。继续编辑会保留窗口；放弃将丢失这些窗口的草稿，已开始的保存可能完成。")
                .set_buttons(rfd::MessageButtons::OkCancelCustom("继续编辑".into(),"放弃并关闭".into())).show();
            if answer!=rfd::MessageDialogResult::Custom("放弃并关闭".into()){state.cancel_close_review_ticket(&ticket);return;}
            if state.settings.lock().unwrap().operations.values().any(|o|o.running){state.cancel_close_review_ticket(&ticket);state.warning(&json!({"code":"BUSY","message":"设置仍在保存，已保留全部窗口；请等待保存结果后关闭"}));return;}
            {let mut review=state.close_review.lock().unwrap();let Some(r)=review.as_mut().filter(|r|r.ticket==ticket&&!r.finishing)else{return};for id in &missing{if let Some(p)=r.participants.get_mut(id){if !p.acknowledged{p.allowed=true;p.revision=Some("unresponsive".into());}}}}
            state.finish_close_review_if_ready();
        });
        Ok(true)
    }
    pub(super) fn review_report(&self,window:&str,session:&str,p:&Value){
        let cancel={let mut review=self.close_review.lock().unwrap();let Some(r)=review.as_mut()else{return};let Some(participant)=r.participants.get_mut(window).filter(|v|v.session==session)else{return};
            let revision=p["draftRevision"].as_str().unwrap_or("").to_owned();
            if participant.allowed&&participant.revision.as_ref()!=Some(&revision){Some(r.ticket.clone())}else{
                participant.revision=Some(revision);participant.acknowledged|=p["closeTicket"]==r.ticket;
                participant.operations=p[if participant.settings{"unresolvedOperationIds"}else{"operations"}].as_array().map(|v|v.iter().filter_map(|s|s.as_str().map(str::to_owned)).collect()).unwrap_or_default();None
            }};
        if let Some(ticket)=cancel{self.cancel_close_review_ticket(&ticket);}
    }
    pub(super) fn review_respond(self:&Arc<Self>,window:&str,session:&str,p:&Value)->Result<()> {
        let ticket=required(p,"ticket")?;let decision=required(p,"decision")?;
        {let mut review=self.close_review.lock().unwrap();let r=review.as_mut().filter(|r|r.ticket==ticket&&!r.finishing).ok_or_else(||err("STALE_OPERATION","关闭请求已结束"))?;
            let participant=r.participants.get_mut(window).filter(|v|v.session==session).ok_or_else(||err("SOURCE_INVALID","关闭会话不匹配"))?;
            if participant.revision.as_deref()!=p["draftRevision"].as_str(){return Err(err("CONFLICT","草稿已变化，请重新核对"));}
            if matches!(decision,"cancel"|"review"){drop(review);self.cancel_close_review_ticket(ticket);return Ok(());}
            if !participant.operations.is_empty()&&!(decision=="abandonWait"&&r.created.elapsed()>=Duration::from_secs(10)){return Err(err("BUSY","配置仍在保存，请先核对结果"));}
            participant.allowed=true;
        }
        self.finish_close_review_if_ready();Ok(())
    }
    pub(super) fn coordinated_settings_response(self:&Arc<Self>,method:&str,p:&Value)->Option<Result<Value>>{
        let session=p["generation"].as_str()?;
        let participating=self.close_review.lock().unwrap().as_ref().is_some_and(|r|r.participants.get("settings").is_some_and(|v|v.session==session));
        if !participating{return None;}
        if method=="ui.settings.session.report"{self.review_report("settings",session,p);return Some(Ok(json!({"accepted":true})));}
        if method=="ui.settings.close.respond"{return Some(self.review_respond("settings",session,p).map(|_|json!({"accepted":true})));}
        None
    }
    pub(super) fn cancel_close_review(&self){let ticket=self.close_review.lock().unwrap().as_ref().map(|r|r.ticket.clone());if let Some(ticket)=ticket{self.cancel_close_review_ticket(&ticket);}}
    pub(super) fn cancel_update_close_review(&self,id:&str){
        let ticket=self.close_review.lock().unwrap().as_ref().filter(|r|matches!(&r.action,Action::InstallUpdate{operation_id,..} if operation_id==id)).map(|r|r.ticket.clone());
        if let Some(ticket)=ticket{self.cancel_close_review_ticket(&ticket);}
    }
    fn cancel_close_review_ticket(&self,ticket:&str){
        let review={let mut state=self.close_review.lock().unwrap();if state.as_ref().is_none_or(|r|r.ticket!=ticket||r.finishing){return;}state.take().unwrap()};
        self.notify_close_outcome(&review,false);
    }
    fn notify_close_outcome(&self,review:&Review,committed:bool){
        if !committed {if let Action::InstallUpdate{operation_id,..}=&review.action{self.cancel_update_review(operation_id,None);}}
        for (window,p) in &review.participants{
            let source=if p.settings{format!("@window:settings/{}",p.session)}else{format!("@window:{window}")};
            self.endpoint_notify(&source,if p.settings{"settings.closeFinished"}else{"connectionHome.closeFinished"},json!({"ticket":review.ticket,"committed":committed}));
        }
    }
    fn finish_close_review_if_ready(self:&Arc<Self>){
        let (ticket,action,targets)={let mut state=self.close_review.lock().unwrap();let Some(r)=state.as_mut().filter(|r|!r.finishing&&r.participants.values().all(|p|p.allowed))else{return};
            let pages=self.connection_home.lock().unwrap();let settings=self.settings_window.lock().unwrap();
            let valid=r.participants.iter().all(|(window,p)|if p.settings{settings.current.as_ref().is_some_and(|s|s.generation==p.session)}else{pages.sessions.get(window).is_some_and(|s|s.id==p.session&&(p.revision.as_deref()==Some("unresponsive")||p.revision.as_deref()==Some(s.revision.as_str())))});
            if !valid||r.targets!=self.review_targets(&r.action){let review=state.take().unwrap();drop(settings);drop(pages);drop(state);self.notify_close_outcome(&review,false);return;}
            if matches!(r.action,Action::Exit|Action::InstallUpdate{..}){let mut settings=self.settings.lock().unwrap();if settings.operations.values().any(|o|o.running){let review=state.take().unwrap();drop(settings);drop(pages);drop(state);self.notify_close_outcome(&review,false);return;}settings.exiting=true;}
            r.finishing=true;(r.ticket.clone(),r.action.clone(),r.targets.clone())};
        let state=self.clone();std::thread::spawn(move||{
            COMMITTING.with(|v|v.set(true));let _scope=CommitScope;
            let result=(||{if targets!=state.review_targets(&action){return Err(err("STALE_OPERATION","关闭目标已变化，请重新核对"));}match action {
                Action::Exit=>{
                    let views=state.workbench.lock().unwrap().views.clone();
                    for v in views{if state.loaded_for(&v.plugin_id).is_ok(){let result=state.worker("lifecycle.prepareClose",json!({"viewId":v.view_id,"workspaceId":v.workspace_id,"pluginData":v.runtime_data}))?;if result["allow"]!=true{return Err(err("CANCELED","插件取消了退出"));}}}
                    if targets!=state.review_targets(&Action::Exit){return Err(err("STALE_OPERATION","工作区已变化，已保留全部窗口"));}
                    state.dispatch(&Caller::shell(),None,"app.exit",json!({}))
                },
                Action::Home(window)=>state.close_home_committed(&window),
                Action::InstallUpdate{operation_id,restart_only}=>state.complete_update_review(&operation_id,restart_only),
                Action::Close(ids)=>state.close_workspaces(&ids),
                Action::Move{ids,target,index,active,operation,request}=>state.move_workspace_batch(ids,&target,index,active.as_deref(),operation.as_deref(),request),
            }})();
            let review={let mut current=state.close_review.lock().unwrap();if current.as_ref().is_none_or(|r|r.ticket!=ticket){return;}current.take().unwrap()};
            state.notify_close_outcome(&review,result.is_ok());
            if result.is_err(){state.settings.lock().unwrap().exiting=false;}
            if let Err(e)=result{state.warning(&json!({"code":e.code,"message":e.message}));}
        });
    }
}
