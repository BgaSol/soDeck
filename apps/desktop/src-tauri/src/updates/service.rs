use super::*;

impl State {
    pub(in crate::app) fn updates_key(&self)->sodeck_communication::Key {
        sodeck_communication::Key{contract:"updates.state.v1".into(),scope:self.app_generation.clone(),object:"updates".into()}
    }
    pub(in crate::app) fn publish_updates(&self) {
        let value=self.updates.with(|r|Ok(json!({"revision":format!("{}:{}",r.store.state.generation,r.store.state.revision),"checking":r.store.state.operations.iter().any(|o|o.kind=="updates.check"&&o.running),"activeOperations":r.store.state.operations.iter().filter(|o|o.running).count()})));
        if let Ok(value)=value {let _=self.communication.lock().unwrap().put(self.updates_key(),json!({"appGeneration":self.app_generation}),value);}
    }
    pub(in crate::app) fn start_updates(self:&Arc<Self>) {
        self.publish_updates();let weak=Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let jitter=u64::from_le_bytes(*Uuid::new_v4().as_bytes().first_chunk::<8>().unwrap())%31;
            let mut interval=tokio::time::interval(Duration::from_secs(60+jitter));
            loop {interval.tick().await;let Some(state)=weak.upgrade()else{return};
                if state.updates.cancel.is_cancelled(){return;}
                let due=state.updates.configured()&&state.updates.with(|r|Ok(r.store.policy.auto_check&&domain::now_ms()>=r.store.state.retry_after&&domain::now_ms().saturating_sub(r.store.state.last_check)>=6*60*60*1000)).unwrap_or(false);
                if due {let _=state.updates_call(&Caller::shell(),None,"updates.check",&json!({"operationId":Uuid::new_v4().to_string()}));}
            }
        });
    }
    pub(in crate::app) fn updates_call(self:&Arc<Self>,caller:&Caller,source:Option<&str>,method:&str,p:&Value)->Result<Value> {
        check("x-methods",method,p)?;
        let result=self.updates_dispatch(caller,source,method,p)?;
        check("x-results",method,&result)?;Ok(result)
    }
    fn updates_dispatch(self:&Arc<Self>,caller:&Caller,source:Option<&str>,method:&str,p:&Value)->Result<Value> {
        let updater=&self.updates;
        if method=="updates.status.get" {
            let result=updater.with(|r|Ok(json!({"revision":format!("{}:{}",r.store.state.generation,r.store.state.revision),"channel":updater.build.channel,"appVersion":updater.runtime.app_version,"buildId":updater.runtime.build_id,"distribution":updater.runtime.distribution,"configured":updater.configured(),"checking":r.store.state.operations.iter().any(|o|o.kind=="updates.check"&&o.running),"lastCheck":r.store.state.last_check,"lastSuccess":r.store.state.last_success,"retryAfter":r.store.state.retry_after,"error":r.store.state.check_error,"warnings":r.store.warnings.iter().take(16).map(|s|s.chars().take(512).collect::<String>()).collect::<Vec<_>>(),"policy":Updates::policy(r),"restartRequired":r.store.state.installs.values().any(|i|i.stage==Stage::Ready)})));
            return Ok(result.unwrap_or_else(|e|json!({"revision":"unavailable","channel":updater.build.channel,"appVersion":updater.runtime.app_version,"buildId":updater.runtime.build_id,"distribution":updater.runtime.distribution,"configured":false,"checking":false,"lastCheck":0,"lastSuccess":0,"retryAfter":0,"error":e,"warnings":[],"policy":{"generation":"unavailable","revision":"0","autoCheck":false},"restartRequired":false})));
        }
        if method=="updates.operation.get" {return updater.result(required(p,"operationId")?).map_err(fault);}
        if method.starts_with("updates.policy.") {
            let _gate=self.shutdown_lock.lock().unwrap();
            if method=="updates.policy.commit"&&(self.stopping.load(Ordering::SeqCst)||self.settings.lock().unwrap().exiting){return Err(err("SHUTTING_DOWN","应用正在退出"));}
            let result=updater.with(|r|match method {
                "updates.policy.validate"=>{r.store.policy_validate(p["generation"].as_str().unwrap(),p["expectedRevision"].as_str().unwrap())?;Ok(json!({"valid":true}))},
                "updates.policy.commit"=>{r.store.policy_commit(p["generation"].as_str().unwrap(),p["expectedRevision"].as_str().unwrap(),p["autoCheck"].as_bool().unwrap(),p["operationId"].as_str().unwrap())?;Ok(Updates::policy(r))},
                "updates.policy.operation.get"=>Ok(if p["generation"]==r.store.policy.generation&&r.store.policy.receipts.iter().any(|o|p["operationId"]==o.operation_id){json!({"state":"saved","result":Updates::policy(r)})}else{json!({"state":"unknown"})}),
                _=>Err(domain::error("UNSUPPORTED_API","未知更新设置方法")),
            }).map_err(fault)?;self.publish_updates();return Ok(result);
        }
        let manager=self.plugins.lock().unwrap().selection_snapshot();
        if matches!(method,"updates.plan.get"|"updates.components.list") {
            let ids=p["componentIds"].as_array().map(|a|a.iter().map(|v|v.as_str().unwrap().to_owned()).collect::<Vec<_>>()).unwrap_or_default();
            let snapshot=updater.snapshot(&manager).map_err(fault)?;
            let plan=updater.plan(&manager,&ids).map_err(fault)?;
            let revision=updater.plan(&manager,&[]).map_err(fault)?.plan_revision;
            if p["expectedRevision"].as_str().is_some_and(|r|r!=revision){return Err(err("CONFLICT","更新列表已变化，请重新核对"));}
            let rows=updater.rows(&plan,&snapshot).map_err(fault)?;
            if method=="updates.plan.get" {return Ok(json!({"planRevision":plan.plan_revision,"catalogRevision":plan.catalog_revision,"components":rows,"componentIds":plan.component_ids,"downloadBytes":plan.download_bytes,"blockedReasons":plan.blocked_reasons}));}
            let offset=if let Some(cursor)=p["cursor"].as_str(){let (r,n)=cursor.split_once(':').ok_or_else(||err("INVALID_PARAMS","更新游标无效"))?;if r!=revision{return Err(err("CONFLICT","更新列表已变化"));}n.parse::<usize>().map_err(|_|err("INVALID_PARAMS","更新游标无效"))?}else{0};
            if offset>rows.len(){return Err(err("INVALID_PARAMS","更新游标越界"));}
            let mut end=offset;let mut size=256;for row in rows.iter().skip(offset).take(p["limit"].as_u64().unwrap_or(50)as usize){size+=serde_json::to_vec(row).unwrap().len()+1;if size>60*1024{break;}end+=1;}
            return Ok(json!({"revision":revision,"items":rows[offset..end],"nextCursor":if end<rows.len(){Some(format!("{revision}:{end}"))}else{None}}));
        }
        let id=required(p,"operationId")?.to_owned();
        if method=="updates.cancel" {
            let owner=communication_services::caller_key(caller);
            let reviewing=updater.with(|r|{let op=r.store.operation(&id).ok_or_else(||domain::error("RESULT_UNKNOWN","更新操作已不可用"))?;
                if caller.plugin_id.is_some()&&op.caller!=owner{return Err(domain::error("SOURCE_INVALID","只能取消本调用者的更新操作"));}
                let reviewing=op.stage==Stage::Reviewing;
                if let Some(c)=r.cancellations.get(&id){c.cancel();}Ok(reviewing)}).map_err(fault)?;
            if reviewing{self.cancel_update_close_review(&id);}
            return updater.result(&id).map_err(fault);
        }
        let owner=communication_services::caller_key(caller);
        if let Some(old)=updater.with(|r|Ok(r.store.operation(&id).cloned())).map_err(fault)? {
            if old.caller!=owner||old.kind!=method||old.request_digest!=domain::digest(p){return Err(err("CONFLICT","更新操作标识已用于其他请求"));}
            return Ok(Updates::operation(&old));
        }
        if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
        if matches!(method,"updates.download"|"updates.install"|"app.restartForUpdates")&&caller.plugin_id.is_some() {
            self.settings_window_call(None,"ui.settings.open",&json!({"pageId":"core:updates","operationId":Uuid::new_v4().to_string()}))?;
            updater.with(|r|{r.store.begin(&id,&owner,method,p)?;r.store.update_operation(&id,|o|{o.stage=Stage::RequiresUserAction;o.running=false;})?;Ok(())}).map_err(fault)?;
            return updater.result(&id).map_err(fault);
        }
        if method=="updates.check" {
            if !updater.configured(){return Err(err("UPDATE_NOT_CONFIGURED","此开发构建尚未配置官方更新源"));}
            let work=updater.work.enter().map_err(fault)?;
            let cancellation=updater.cancel.child_token();
            updater.with(|r|{
                if r.store.state.retry_after>domain::now_ms(){return Err(domain::error("RATE_LIMITED","请稍后再检查更新"));}
                if domain::now_ms().saturating_sub(r.store.state.last_check)<30_000{return Err(domain::error("RATE_LIMITED","请间隔30秒再检查更新"));}
                if r.store.state.operations.iter().any(|o|o.kind==method&&o.running){return Err(domain::error("BUSY","正在检查更新"));}
                r.store.begin(&id,&owner,method,p)?;r.cancellations.insert(id.clone(),cancellation.clone());
                let mut next=r.store.state.clone();next.last_check=domain::now_ms();r.store.save_state(next)
            }).map_err(fault)?;
            let state=self.clone();let operation=id.clone();tauri::async_runtime::spawn(async move{let _work=work;state.check_updates(&operation,cancellation).await;});
        } else if method=="updates.source.followOfficial" {
            let _gate=self.shutdown_lock.lock().unwrap();let manager=self.plugins.lock().unwrap();let snapshot=manager.selection_snapshot();
            if self.stopping.load(Ordering::SeqCst){return Err(err("SHUTTING_DOWN","应用正在退出"));}
            let plugin=p["pluginId"].as_str().unwrap();let current=snapshot.plugins.iter().find(|m|m.plugin_id==plugin).and_then(|m|m.pending.as_ref().or(m.selected.as_ref())).ok_or_else(||err("CONFLICT","未找到当前插件包"))?;
            if p["expectedDigest"]!=current.digest{return Err(err("CONFLICT","当前插件包已变化"));}
            updater.with(|r|{let versions=r.catalog.as_ref().and_then(|c|c.catalog().plugins.get(plugin)).ok_or_else(||domain::error("UPDATE_SOURCE_INVALID","该插件不在官方签名目录中"))?;
                let official=std::iter::once(&versions.current).chain(&versions.history).find(|v|v.package_digest==current.digest).map(|v|v.package_digest.clone());
                r.store.begin(&id,&owner,method,p)?;r.store.follow_official(plugin,&current.digest,official.as_deref())?;Ok(())}).map_err(fault)?;
            updater.finish(&id,Ok(Stage::Installed));
        } else if method=="app.restartForUpdates" {
            self.request_update_restart(&id,&owner,p)?;
        } else if matches!(method,"updates.download"|"updates.install") {
            let ids=p["componentIds"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_owned()).collect::<Vec<_>>();
            if ids.is_empty(){return Err(err("INVALID_PARAMS","请选择要更新的组件"));}
            let plan=updater.plan(&manager,&ids).map_err(fault)?;
            if p["planRevision"]!=plan.plan_revision{return Err(err("CONFLICT","安装选择或兼容条件已变化，请重新规划"));}
            if !plan.blocked_reasons.is_empty(){return Err(err("UPDATE_INCOMPATIBLE",plan.blocked_reasons.join("；")));}
            if method=="updates.install" {self.ensure_review_mutable()?;}
            let work=updater.work.enter().map_err(fault)?;
            let cancellation=updater.cancel.child_token();
            updater.with(|r|{r.store.begin(&id,&owner,method,p)?;r.cancellations.insert(id.clone(),cancellation.clone());r.store.update_operation(&id,|o|{
                for c in plan.components.iter().filter(|c|c.selected){let target=c.target.as_ref().unwrap();o.components.insert(c.component_id.clone(),ComponentResult{stage:if method=="updates.download"{Stage::Downloading}else{Stage::Verifying},received:0,total:target.asset().size,error:None,target_digest:Some(target.asset().archive_sha256.clone())});}
            })?;Ok(())}).map_err(fault)?;
            let state=self.clone();let operation=id.clone();let install=method=="updates.install";
            if install {tauri::async_runtime::spawn_blocking(move||state.prepare_update_install(&operation,plan,cancellation,work));}
            else {tauri::async_runtime::spawn(async move{let _work=work;state.download_updates(&operation,plan,cancellation).await;});}
        } else {return Err(err("UNSUPPORTED_API","未知更新操作"));}
        let _=source;self.publish_updates();updater.result(&id).map_err(fault)
    }
    async fn check_updates(self:Arc<Self>,id:&str,cancel:CancellationToken) {
        let result=async {
            let _check=tokio::select!{_=cancel.cancelled()=>return Err(domain::error("CANCELLED","检查已取消")),guard=self.updates.check_lock.lock()=>guard};
            let root=self.updates.with(|r|{r.store.reserve_space(96*1024*1024+r.reservations.values().sum::<u64>(),&r.reservations.keys().cloned().collect())?;r.reservations.insert("metadata".into(),96*1024*1024);Ok(r.store.root().to_path_buf())})?;
            let mut session=domain::github::Session::new(&root)?;
            let result=session.discover(self.updates.runtime.channel,&self.updates.build.keys(),cancel).await;
            self.updates.with(|r|match result {Ok(catalog)=>{r.store.accept_catalog(&catalog)?;r.catalog=Some(catalog);Ok(Stage::Installed)},Err(e)=>{let mut next=r.store.state.clone();next.check_error=Some(e.clone());next.retry_after=session.retry_after_ms.max(domain::now_ms()+60_000);r.store.save_state(next)?;Err(e)}})
        }.await;
        let _=self.updates.with(|r|{r.reservations.remove("metadata");Ok(())});
        if result.is_ok(){
            let snapshot=self.plugins.lock().unwrap().selection_snapshot();
            if let Ok(plan)=self.updates.plan(&snapshot,&[]) {
                let notify=plan.components.iter().any(|c|c.available_version.is_some())&&self.updates.with(|r|{
                    if r.store.state.notified_catalog_digest.as_deref()==Some(&plan.catalog_revision){return Ok(false);}
                    let mut next=r.store.state.clone();next.notified_catalog_digest=Some(plan.catalog_revision.clone());r.store.save_state(next)?;Ok(true)
                }).unwrap_or(false);
                if notify {self.warning(&json!({"code":"UPDATE_AVAILABLE","message":"有可用更新，可在设置的更新页选择下载和安装"}));}
            }
        }
        self.updates.finish(id,result);self.publish_updates();
    }
    pub(super) fn update_component(&self,id:&str,target:&Target,stage:Stage,received:u64,error:Option<domain::Error>) {
        let _=self.updates.with(|r|{r.store.update_operation(id,|o|{if let Some(c)=o.components.get_mut(target.id()){c.stage=stage;c.received=received;c.error=error;}})?;Ok(())});self.publish_updates();
    }
    async fn download_updates(self:Arc<Self>,id:&str,plan:UpdatePlan,cancel:CancellationToken) {
        let mut error=None;
        for target in plan.components.iter().filter(|c|c.selected).filter_map(|c|c.target.as_ref()) {
            let result=self.download_target(id,target,&plan.catalog_revision,cancel.clone()).await;
            if let Err(e)=result {self.update_component(id,target,if e.code=="CANCELLED"{Stage::Cancelled}else{Stage::Failed},0,Some(e.clone()));error=Some(e);}
        }
        self.updates.finish(id,error.map_or(Ok(Stage::Downloaded),Err));self.publish_updates();
    }
    async fn download_target(self:&Arc<Self>,id:&str,target:&Target,catalog:&str,cancel:CancellationToken)->domain::Result<()> {
        let hash=target.asset().archive_sha256.clone();
        let lock=self.updates.with(|r|Ok(r.asset_locks.entry(hash.clone()).or_insert_with(||Arc::new(tokio::sync::Mutex::new(()))).clone()))?;
        let _asset=tokio::select!{_=cancel.cancelled()=>return Err(domain::error("CANCELLED","下载已取消")),v=lock.lock()=>v};
        let _permit=tokio::select!{_=cancel.cancelled()=>return Err(domain::error("CANCELLED","下载已取消")),v=self.updates.downloads.acquire()=>v.map_err(|_|domain::error("CANCELLED","更新服务已退出"))?};
        let root=self.updates.with(|r|{let additional=if domain::download::verify_cached(r.store.root(),target.asset()).is_ok(){0}else{target.asset().size};let bytes=r.reservations.values().sum::<u64>().saturating_add(additional);let mut pinned= r.reservations.keys().cloned().collect::<std::collections::BTreeSet<_>>();pinned.insert(hash.clone());r.store.reserve_space(bytes,&pinned)?;r.reservations.insert(hash.clone(),additional);Ok(r.store.root().to_path_buf())})?;
        let result=async {
            let client=domain::download::client()?;let mut last=Instant::now()-Duration::from_secs(1);
            domain::download::fetch(&client,&root,target.asset(),cancel.clone(),|received,_|{if last.elapsed()>=Duration::from_millis(250){last=Instant::now();self.update_component(id,target,Stage::Downloading,received,None);}}).await?;
            self.update_component(id,target,Stage::Verifying,target.asset().size,None);
            let state=self.clone();let owned=target.clone();let catalog=catalog.to_owned();
            tauri::async_runtime::spawn_blocking(move||{
                let path=state.updates.verify_target(&owned,&catalog)?;
                if matches!(owned,Target::Core{..}) {state.verify_installer(&owned,&path)?;}
                Ok::<_,domain::Error>(())
            }).await.map_err(|_|domain::error("RESULT_UNKNOWN","更新验证任务中断"))??;
            if cancel.is_cancelled(){return Err(domain::error("CANCELLED","下载已取消"));}
            self.update_component(id,target,Stage::Downloaded,target.asset().size,None);Ok(())
        }.await;
        let _=self.updates.with(|r|{r.reservations.remove(&hash);Ok(())});result
    }
}
