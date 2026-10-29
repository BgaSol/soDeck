use super::*;
use domain::store::CoreHandoff;
use tauri_plugin_updater::UpdaterExt;

impl State {
    pub(super) fn verify_installer(&self,target:&Target,path:&Path)->domain::Result<(tauri_plugin_updater::Update,Vec<u8>)> {
        let Target::Core{release,installer}=target else {return Err(domain::error("INVALID_PARAMS","目标不是核心安装包"));};
        let key=self.updates.build.updater_key.as_ref().ok_or_else(||domain::error("UPDATE_NOT_CONFIGURED","未配置核心更新公钥"))?;
        let updater=self.app.updater_builder().pubkey(key)
            .endpoints(vec!["https://github.com/BgaSol/soDeck/releases".parse().unwrap()]).map_err(|_|domain::error("UPDATE_SOURCE_INVALID","更新适配器地址无效"))?
            .build().map_err(|_|domain::error("UPDATE_SOURCE_INVALID","无法创建核心安装适配器"))?;
        let metadata=json!({"version":release.app_version,"url":installer.asset.url().as_str(),"signature":installer.signature});
        let update=updater.restore_update(metadata).map_err(|_|domain::error("UPDATE_SIGNATURE_INVALID","核心更新元数据或版本签名无效"))?.ok_or_else(||domain::error("UPDATE_INCOMPATIBLE","当前核心已不是此安装包的升级起点"))?;
        let bytes=domain::atomic::read(path,MAX_INSTALLER)?;
        update.verify(&bytes).map_err(|_|domain::error("UPDATE_SIGNATURE_INVALID","核心安装包签名无效"))?;
        Ok((update,bytes))
    }
    pub(super) fn prepare_update_install(self:Arc<Self>,id:&str,plan:UpdatePlan,cancel:CancellationToken,work:work::Guard) {
        let mut immediate_review=false;
        let result=(||->domain::Result<Stage>{
            let _install=self.updates.install_lock.try_lock().map_err(|_|domain::error("BUSY","另一个更新正在准备安装"))?;
            let mut paths=BTreeMap::new();
            for target in plan.components.iter().filter(|c|c.selected).filter_map(|c|c.target.as_ref()) {
                if cancel.is_cancelled(){return Err(domain::error("CANCELLED","安装准备已取消"));}
                let path=self.updates.verify_target(target,&plan.catalog_revision)?;
                if matches!(target,Target::Core{..}){self.verify_installer(target,&path)?;}
                paths.insert(target.id().to_owned(),path);
            }
            let core=plan.component_ids.iter().any(|id|id=="core");
            if core {
                if cancel.is_cancelled(){return Err(domain::error("CANCELLED","安装准备已取消"));}
                self.updates.with(|r|{r.reviews.insert(id.into(),plan.clone());r.store.update_operation(id,|o|o.stage=Stage::Reviewing)?;Ok(())})?;
                let action=window_close_review::Action::InstallUpdate{operation_id:id.into(),restart_only:false};
                immediate_review=!self.begin_close_review(action,None).map_err(|e|domain::error(&e.code,e.message))?;
                return Ok(Stage::Reviewing);
            }
            let _gate=self.shutdown_lock.lock().unwrap();
            if self.stopping.load(Ordering::SeqCst){return Err(domain::error("CANCELLED","应用正在退出"));}
            let mut manager=self.plugins.lock().unwrap();
            let fresh=self.updates.plan(&manager.selection_snapshot(),&plan.component_ids)?;
            if fresh.plan_revision!=plan.plan_revision{return Err(domain::error("CONFLICT","插件选择或来源已变化，请重新规划"));}
            let mut failure=None;let mut waiting=false;
            for target in plan.components.iter().filter(|c|c.selected).filter_map(|c|c.target.as_ref()) {
                if cancel.is_cancelled(){failure=Some(domain::error("CANCELLED","其余安装准备已取消"));break;}
                let result=self.updates.install_plugin(&mut manager,target,&plan.catalog_revision,id,&paths[target.id()]);
                match result {Ok(stage)=>{waiting|=stage==Stage::WaitingForCore;self.update_component(id,target,stage,target.asset().size,None);},Err(e)=>{self.update_component(id,target,Stage::Failed,0,Some(e.clone()));failure=Some(e);}}
            }
            drop(manager);let _=self.publish_plugin_management();
            failure.map_or(Ok(if waiting{Stage::WaitingForCore}else{Stage::Ready}),Err)
        })();
        // The handoff must not wait for itself when it drains update producers.
        drop(work);
        if immediate_review&&matches!(result,Ok(Stage::Reviewing)) {
            if let Err(e)=self.complete_update_review(id,false){self.cancel_update_review(id,Some(domain::error(&e.code,e.message)));}
        }
        if !matches!(result,Ok(Stage::Reviewing)){self.updates.finish(id,result);}
        self.publish_updates();
    }
    pub(super) fn request_update_restart(self:&Arc<Self>,id:&str,owner:&str,p:&Value)->Result<()> {
        self.ensure_review_mutable()?;
        let plan=self.updates.plan(&self.plugins.lock().unwrap().selection_snapshot(),&[]).map_err(fault)?;
        if p["planRevision"]!=plan.plan_revision{return Err(err("CONFLICT","待重启状态已变化，请重新核对"));}
        self.updates.with(|r|{
            if !r.store.state.installs.values().any(|i|i.stage==Stage::Ready){return Err(domain::error("CONFLICT","尚未准备需要重启的插件更新"));}
            r.store.begin(id,owner,"app.restartForUpdates",p)?;r.reviews.insert(id.into(),plan);
            r.cancellations.insert(id.into(),self.updates.cancel.child_token());
            r.store.update_operation(id,|o|o.stage=Stage::Reviewing)?;Ok(())
        }).map_err(fault)?;
        if !self.begin_close_review(window_close_review::Action::InstallUpdate{operation_id:id.into(),restart_only:true},None)? {
            let state=self.clone();let id=id.to_owned();std::thread::spawn(move||{if let Err(e)=state.complete_update_review(&id,true){state.cancel_update_review(&id,Some(domain::error(&e.code,e.message)));}});
        }
        Ok(())
    }
    pub(in crate::app) fn cancel_update_review(&self,id:&str,error:Option<domain::Error>) {
        let _=self.updates.with(|r|{r.reviews.remove(id);Ok(())});
        self.updates.finish(id,Err(error.unwrap_or_else(||domain::error("CANCELLED","已取消安装重启，下载文件仍保留"))));self.publish_updates();
    }
    pub(in crate::app) fn complete_update_review(self:&Arc<Self>,id:&str,restart_only:bool)->Result<Value> {
        let plan=self.updates.with(|r|r.reviews.get(id).cloned().ok_or_else(||domain::error("CONFLICT","安装审查已失效"))).map_err(fault)?;
        let action=window_close_review::Action::InstallUpdate{operation_id:id.into(),restart_only};
        let targets=self.review_targets(&action);
        let views=self.workbench.lock().unwrap().views.clone();
        for v in views {if self.loaded_for(&v.plugin_id).is_ok(){let response=self.worker("lifecycle.prepareClose",json!({"viewId":v.view_id,"workspaceId":v.workspace_id,"pluginData":v.runtime_data}))?;if response["allow"]!=true{return Err(err("CANCELLED","插件取消了重启"));}}}
        let mut paths=BTreeMap::new();let mut native=None;
        for target in plan.components.iter().filter(|c|c.selected).filter_map(|c|c.target.as_ref()) {
            let path=self.updates.verify_target(target,&plan.catalog_revision).map_err(fault)?;
            if matches!(target,Target::Core{..}){native=Some(self.verify_installer(target,&path).map_err(fault)?);}
            paths.insert(target.id().to_owned(),path);
        }
        {
            let _gate=self.shutdown_lock.lock().unwrap();
            if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
            if targets!=self.review_targets(&action){return Err(err("STALE_OPERATION","工作区已变化，已保留全部窗口"));}
            self.updates.with(|r|{if !r.reviews.contains_key(id)||r.cancellations.get(id).is_some_and(|c|c.is_cancelled()){return Err(domain::error("CANCELLED","安装审查已取消"));}Ok(())}).map_err(fault)?;
            if self.settings.lock().unwrap().operations.values().any(|o|o.running){return Err(err("BUSY","设置仍在保存，已保留全部窗口"));}
            let mut manager=self.plugins.lock().unwrap();
            let fresh=self.updates.plan(&manager.selection_snapshot(),&plan.component_ids).map_err(fault)?;
            if fresh.plan_revision!=plan.plan_revision{return Err(err("CONFLICT","审查期间更新条件已变化，请重新核对"));}
            if !fresh.blocked_reasons.is_empty(){return Err(err("UPDATE_INCOMPATIBLE",fresh.blocked_reasons.join("；")));}
            for target in plan.components.iter().filter(|c|c.selected).filter_map(|c|c.target.as_ref()).filter(|t|matches!(t,Target::Plugin{..})) {
                let stage=self.updates.install_plugin(&mut manager,target,&plan.catalog_revision,id,&paths[target.id()]).map_err(fault)?;
                self.update_component(id,target,stage,target.asset().size,None);
            }
            if !restart_only {
                let target=plan.components.iter().find(|c|c.component_id=="core").and_then(|c|c.target.as_ref()).ok_or_else(||err("CONFLICT","缺少核心安装目标"))?;
                let Target::Core{release,installer}=target else {return Err(err("CONFLICT","无效核心目标"));};
                self.updates.with(|r|{let mut next=r.store.state.clone();next.handoff=Some(CoreHandoff{operation_id:id.into(),target_version:release.app_version.clone(),build_id:release.build_id.clone(),channel:self.updates.runtime.channel,asset_digest:installer.asset.archive_sha256.clone(),stage:Stage::HandoffPending});r.store.save_state(next)}).map_err(fault)?;
            }
            self.update_handoff.store(true,Ordering::SeqCst);
            self.stopping.store(true,Ordering::SeqCst);
        }
        self.shutdown_resources();
        if restart_only {self.updates.finish(id,Ok(Stage::Ready));self.app.restart();}
        let result=self.updates.with(|r|{let mut next=r.store.state.clone();next.handoff.as_mut().ok_or_else(||domain::error("CONFLICT","缺少安装交接记录"))?.stage=Stage::HandoffAttempted;r.store.save_state(next)?;r.store.update_operation(id,|o|o.stage=Stage::HandoffAttempted)?;Ok(())})
            .and_then(|_|{let (update,bytes)=native.ok_or_else(||domain::error("CONFLICT","缺少已验证安装器"))?;update.install(&bytes).map_err(|_|domain::error("UPDATE_INSTALL_FAILED","无法启动核心安装器"))});
        if let Err(e)=result {
            let _=self.updates.with(|r|{let mut next=r.store.state.clone();if let Some(h)=next.handoff.as_mut(){h.stage=Stage::Failed;}r.store.save_state(next) });
            self.updates.finish(id,Err(e));
            let answer=rfd::MessageDialog::new().set_title("SoDeck · 更新未完成").set_description("安装交接失败，当前会话已经关闭。可以重新打开当前版本，再检查和安装更新。")
                .set_buttons(rfd::MessageButtons::OkCancelCustom("重新打开".into(),"关闭".into())).show();
            if answer==rfd::MessageDialogResult::Custom("重新打开".into()){self.app.restart();}else{self.app.exit(1);}
        }
        Ok(json!({"accepted":true}))
    }
}
