use super::*;
#[derive(Default)]
pub(super) struct ExitReview {pending:Option<Value>,report:Option<Value>,acknowledged:bool}
impl ExitReview {pub(super) fn supersede(&mut self){self.pending=None;self.acknowledged=false;}}
impl State {
    pub(super) fn clear_settings_review(&self,generation:&str){let mut review=self.settings_exit.lock().unwrap();if review.pending.as_ref().is_some_and(|p|p["generation"]==generation){review.pending=None;review.acknowledged=false;}if review.report.as_ref().is_some_and(|p|p["generation"]==generation){review.report=None;}}
    pub(super) fn settings_request_close(self:&Arc<Self>,reason:&str)->Result<Value>{
        if reason=="exit" {self.begin_close_review(window_close_review::Action::Exit,None)?;return Ok(json!({"accepted":true}));}
        self.ensure_review_mutable()?;
        let current=self.settings_window.lock().unwrap().current.clone();
        let Some(window)=current else{return Ok(json!({"accepted":true}));};
        let intent=json!({"generation":window.generation,"ticket":Uuid::new_v4().to_string(),"reason":reason});
        {
            // A new close/exit intent supersedes an earlier one. In particular,
            // an in-flight "keep editing" reply must not swallow a later exit.
            let mut review=self.settings_exit.lock().unwrap();
            review.pending=Some(intent.clone());review.acknowledged=false;
        }
        self.endpoint_notify(&window.source(),"settings.closeRequested",intent.clone());self.show_window(settings_window::LABEL);
        let weak=Arc::downgrade(self);
        std::thread::spawn(move||{
            std::thread::sleep(Duration::from_secs(3));let Some(state)=weak.upgrade()else{return};
            if {let review=state.settings_exit.lock().unwrap();review.pending.as_ref()!=Some(&intent)||review.acknowledged}{return;}
            // First button and dismissal keep the application; never infer consent from a timeout.
            let response=rfd::MessageDialog::new().set_title("SoDeck · 设置窗口未响应").set_description("暂时无法确认设置草稿。继续编辑会保留应用；放弃将丢弃尚未应用的更改。已开始保存的配置不会回滚。")
                .set_buttons(rfd::MessageButtons::OkCancelCustom("继续编辑".into(),"放弃更改".into())).show();
            if state.settings_exit.lock().unwrap().pending.as_ref()!=Some(&intent){return;}
            if response==rfd::MessageDialogResult::Custom("放弃更改".into()) {
                if !state.settings.lock().unwrap().operations.values().any(|o|o.running){let _=state.finish_settings_close(&intent);}
            }else{state.settings_exit.lock().unwrap().pending=None;}
        });
        Ok(json!({"accepted":true}))
    }
    pub(super) fn settings_close_call(self:&Arc<Self>,source:Option<&str>,method:&str,p:&Value)->Result<Value>{
        let source=source.ok_or_else(||err("SOURCE_INVALID","缺少设置窗口来源"))?;
        self.settings_window_for_source(source)?.ok_or_else(||err("SOURCE_INVALID","仅设置窗口可确认草稿"))?;
        if method=="ui.settings.close.request"{return self.settings_request_close(required(p,"reason")?);}
        let generation=self.settings_window.lock().unwrap().current.as_ref().map(|w|w.generation.clone()).ok_or_else(||err("WINDOW_CLOSED","设置窗口已关闭"))?;
        if p["generation"]!=generation{return Err(err("SOURCE_INVALID","设置窗口代次失效"));}
        if let Some(result)=self.coordinated_settings_response(method,p){return result;}
        if method=="ui.settings.session.report"{let mut review=self.settings_exit.lock().unwrap();if review.pending.as_ref().is_some_and(|i|i["ticket"]==p["closeTicket"]){review.acknowledged=true;}review.report=Some(p.clone());return Ok(json!({"accepted":true}));}
        let intent={let review=self.settings_exit.lock().unwrap();let intent=review.pending.as_ref().ok_or_else(||err("STALE_OPERATION","关闭操作已结束"))?;
            if intent["ticket"]!=p["ticket"]||intent["generation"]!=p["generation"]{return Err(err("SOURCE_INVALID","关闭凭据不匹配"));}
            if review.report.as_ref().is_none_or(|r|r["draftRevision"]!=p["draftRevision"]){return Err(err("CONFLICT","草稿状态已改变，请重新核对"));}intent.clone()};
        match required(p,"decision")? {
            "cancel"|"review"=>{let mut review=self.settings_exit.lock().unwrap();if review.pending.as_ref()==Some(&intent){review.pending=None;}},
            "discard"=>{
                if self.settings.lock().unwrap().operations.values().any(|o|o.running){return Err(err("BUSY","设置仍在保存，请等待结果"));}
                self.finish_settings_close(&intent)?;
            },
            _=>return Err(err("INVALID_PARAMS","关闭决定无效")),
        }
        Ok(json!({"accepted":true}))
    }
    fn finish_settings_close(self:&Arc<Self>,intent:&Value)->Result<()> {
        {
            let mut review=self.settings_exit.lock().unwrap();if review.pending.as_ref()!=Some(intent){return Err(err("STALE_OPERATION","关闭请求已过期"));}
            if intent["reason"]=="exit"{let mut settings=self.settings.lock().unwrap();if settings.operations.values().any(|o|o.running){return Err(err("BUSY","设置仍在保存，请等待结果"));}settings.exiting=true;}
            review.pending=None;review.report=None;
        }
        if let Err(error)=self.destroy_settings(None){self.settings.lock().unwrap().exiting=false;return Err(error);}
        if intent["reason"]=="exit" {
            self.dispatch(&Caller::shell(),None,"app.exit",json!({}))?;
        }Ok(())
    }
}
