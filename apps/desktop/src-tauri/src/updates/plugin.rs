use super::*;
use domain::store::{CoreHandoff, InstallIntent};
use sodeck_plugin_manager::{CompatibilityContext, InspectedPackage, StagedPackage};

impl Updates {
    pub(super) fn package_keys(&self)->TrustedKeys {self.build.keys().into_iter().map(|(k,v)|(k,v.into())).collect()}
    fn context(c:&Compatibility)->CompatibilityContext {
        CompatibilityContext{core_version:c.core_compatibility_version.clone(),api_version:c.plugin_api_version.clone(),capabilities:c.capabilities.clone(),platform:c.platform.split('-').next().unwrap_or(&c.platform).into()}
    }
    pub fn accept_package(&self,manager:&mut PluginManager,staged:StagedPackage,origin:Origin,operation:&str,only_missing:bool,expected_id:Option<&str>)->domain::Result<sodeck_plugin_manager::Manifest> {
        let manifest=staged.snapshot().manifest().clone();
        if expected_id.is_some_and(|id|id!=manifest.plugin_id){return Err(domain::error("PACKAGE_INVALID","随附插件身份与资源索引不符"));}
        let snapshot=manager.selection_snapshot();
        let old=snapshot.plugins.iter().find(|p|p.plugin_id==manifest.plugin_id);
        if only_missing&&manager.known_ids().contains(&manifest.plugin_id){return Ok(manifest);}
        let previous=old.and_then(|p|p.pending.as_ref().or(p.selected.as_ref())).map(|m|m.digest.as_str());
        let digest=staged.snapshot().digest().to_owned();
        // Sources must reach disk before the manager commit point.
        self.with(|r|r.store.source_intent(&manifest.plugin_id,previous,&digest,origin,operation))?;
        let result=manager.accept_staged(staged,only_missing,expected_id).map_err(manager_fault);
        let actual=manager.selection_snapshot();let actual=actual.plugins.iter().find(|p|p.plugin_id==manifest.plugin_id).and_then(|p|p.pending.as_ref().or(p.selected.as_ref())).map(|m|m.digest.as_str());
        self.with(|r|r.store.source_complete(&manifest.plugin_id,actual))?;
        if result.is_ok()&&actual==Some(digest.as_str()) {self.with(|r|{if r.store.state.recovery_required{let mut next=r.store.state.clone();next.recovery_prepared.insert(manifest.plugin_id.clone(),digest);r.store.save_state(next)?;}Ok(())})?;}
        result
    }
    pub(super) fn verified_catalog(&self,digest:&str)->domain::Result<VerifiedCatalog> {
        let current=self.with(|r|Ok(r.catalog.as_ref().filter(|c|c.digest()==digest).cloned()))?;
        if let Some(c)=current{return Ok(c);}
        let root=self.with(|r|Ok(r.store.root().join("catalogs").join(self.runtime.channel.name())))?;
        let entries=fs::read_dir(root).map_err(|_|domain::error("UPDATE_SOURCE_INVALID","安装所用的签名目录不可用"))?;
        for entry in entries.take(128).flatten() {
            if !entry.file_type().is_ok_and(|t|t.is_dir()) {continue;}
            let bytes=domain::atomic::read(&entry.path().join("catalog-v1.json"),MAX_CATALOG as u64)?;
            if domain::hash(&bytes)!=digest {continue;}
            let sig=domain::atomic::read(&entry.path().join("catalog-v1.sig"),16*1024)?;
            return VerifiedCatalog::verify(&bytes,&sig,&self.build.keys(),self.runtime.channel);
        }
        Err(domain::error("UPDATE_SOURCE_INVALID","安装所用的签名目录已不可用，请重新检查"))
    }
    pub(super) fn verify_target(&self,target:&Target,catalog_digest:&str)->domain::Result<PathBuf> {
        let verified=self.verified_catalog(catalog_digest)?;let catalog=verified.catalog();
        let known=match target {
            Target::Core{release,installer}=>std::iter::once(&catalog.core.current).chain(&catalog.core.history).any(|r|r==release&&r.platforms.get(&self.runtime.compatibility.platform)==Some(installer)),
            Target::Plugin{plugin_id,release,..}=>catalog.plugins.get(plugin_id).is_some_and(|v|std::iter::once(&v.current).chain(&v.history).any(|p|p==release)),
        };
        if !known{return Err(domain::error("UPDATE_SOURCE_INVALID","缓存目标与签名发布目录不一致"));}
        let root=self.with(|r|Ok(r.store.root().to_path_buf()))?;
        let path=domain::download::verify_cached(&root,target.asset())?;
        if let Target::Plugin{plugin_id,release,requires_core}=target {
            let context=if let Some(version)=requires_core {
                let core=std::iter::once(&catalog.core.current).chain(&catalog.core.history).find(|c|&c.app_version==version).ok_or_else(||domain::error("UPDATE_INCOMPATIBLE","缺少目标核心兼容信息"))?;
                core.context(&self.runtime.compatibility.platform)
            } else {self.runtime.compatibility.clone()};
            let package=InspectedPackage::inspect_archive(&path,&root.join("inspection"),&self.package_keys(),&Self::context(&context)).map_err(manager_fault)?;
            let m=package.manifest();
            if &m.plugin_id!=plugin_id||m.version!=release.version||m.data_schema_version!=release.data_schema_version
                ||serde_json::to_value(&m.requires).unwrap()!=serde_json::to_value(&release.requires).unwrap()
                ||package.digest()!=release.package_digest||package.signature().status!="verified"||package.signature().key_id.as_deref()!=Some(&release.signing_key_id) {
                return Err(domain::error("UPDATE_SIGNATURE_INVALID","官方插件包内容、兼容要求或签名与目录不一致"));
            }
        }
        Ok(path)
    }
    pub(super) fn install_plugin(&self,manager:&mut PluginManager,target:&Target,catalog_digest:&str,operation:&str,path:&Path)->domain::Result<Stage> {
        let Target::Plugin{plugin_id,release,requires_core}=target else {return Err(domain::error("INVALID_PARAMS","目标不是插件"));};
        let snapshot=manager.selection_snapshot();
        let old=snapshot.plugins.iter().find(|p|&p.plugin_id==plugin_id).ok_or_else(||domain::error("CONFLICT","插件已不在当前安装中"))?;
        if old.pending.is_some(){return Err(domain::error("CONFLICT","插件已有待重启候选"));}
        if old.selected.as_ref().is_some_and(|p|p.manifest.data_schema_version!=release.data_schema_version){return Err(domain::error("UPDATE_INCOMPATIBLE","插件数据格式不兼容"));}
        let previous_digest=old.selected.as_ref().map(|p|p.digest.clone());
        let required_core=if requires_core.is_some() {
            let catalog=self.verified_catalog(catalog_digest)?;let core=&catalog.catalog().core.current;
            if requires_core.as_deref()!=Some(&core.app_version){return Err(domain::error("UPDATE_INCOMPATIBLE","目标核心已不匹配"));}
            let installer=core.platforms.get(&self.runtime.compatibility.platform).ok_or_else(||domain::error("UPDATE_INCOMPATIBLE","目标核心缺少本平台资产"))?;
            Some(CoreHandoff{operation_id:operation.into(),target_version:core.app_version.clone(),build_id:core.build_id.clone(),channel:self.runtime.channel,asset_digest:installer.asset.archive_sha256.clone(),stage:Stage::WaitingForCore})
        }else{None};
        let stage=if required_core.is_some(){Stage::WaitingForCore}else{Stage::Preparing};
        let intent=InstallIntent{operation_id:operation.into(),target:target.clone(),catalog_digest:catalog_digest.into(),stage:stage.clone(),activation_attempt:None,previous_digest,required_core};
        self.with(|r|{let mut next=r.store.state.clone();next.installs.insert(plugin_id.clone(),intent);r.store.save_state(next)})?;
        if requires_core.is_some(){return Ok(stage);}
        let staged=manager.stager().stage(path).map_err(manager_fault)?;
        self.accept_package(manager,staged,Origin::Official,operation,false,Some(plugin_id))?;
        self.with(|r|{let mut next=r.store.state.clone();next.installs.get_mut(plugin_id).unwrap().stage=Stage::Ready;r.store.save_state(next)})?;
        Ok(Stage::Ready)
    }
    pub fn before_activation(&self,manager:&mut PluginManager,boot:&str)->domain::Result<()> {
        if self.with(|r|Ok(r.store.state.recovery_required))? {
            // Preserve every candidate after journal damage; only a fresh explicit
            // preparation can authorize it again. Never erase manual candidates.
            let snapshot=manager.selection_snapshot();
            self.with(|r|{if snapshot.plugins.iter().any(|p|p.pending.as_ref().is_some_and(|m|r.store.state.recovery_prepared.get(&p.plugin_id)!=Some(&m.digest))){return Err(domain::error("RESULT_UNKNOWN","更新恢复记录损坏，候选已保留但暂不激活，请重新导入待启用包以核对安装意图"));}let mut next=r.store.state.clone();next.recovery_required=false;next.recovery_prepared.clear();r.store.save_state(next)})?;
        }
        let sources=self.with(|r|Ok(r.store.sources.clone()))?;
        let snapshot=manager.selection_snapshot();
        for (id,source) in sources.plugins.iter().filter(|(_,s)|s.intent.is_some()) {
            let actual=snapshot.plugins.iter().find(|p|&p.plugin_id==id).and_then(|p|p.pending.as_ref().or(p.selected.as_ref())).map(|m|m.digest.as_str());
            let _=source;self.with(|r|r.store.source_complete(id,actual))?;
        }
        let intents=self.with(|r|Ok(r.store.state.installs.clone()))?;
        for (id,mut intent) in intents {
            if !matches!(intent.stage,Stage::Ready|Stage::Preparing|Stage::WaitingForCore){continue;}
            let Target::Plugin{release,..}=&intent.target else {continue;};
            let snapshot=manager.selection_snapshot();let selected=snapshot.plugins.iter().find(|p|p.plugin_id==id);
            let actual=selected.and_then(|p|p.selected.as_ref());
            if actual.is_some_and(|m|m.digest==release.package_digest) {intent.stage=Stage::Installed;}
            else if intent.activation_attempt.is_some() {
                manager.cancel_pending(&id,&release.version,&release.package_digest).map_err(manager_fault)?;
                intent.stage=Stage::Failed;
            } else {
                let result=(||->domain::Result<bool> {
                    if let Some(core)=&intent.required_core {
                        let authorized=self.with(|r|Ok(r.store.state.handoff.as_ref().is_some_and(|h|h.stage==Stage::Installed&&h.asset_digest==core.asset_digest&&h.target_version==core.target_version&&h.build_id==core.build_id)))?;
                        if !authorized||self.runtime.app_version!=core.target_version||self.runtime.build_id!=core.build_id||self.runtime.channel!=core.channel {return Ok(false);}
                        if actual.map(|m|&m.digest)!=intent.previous_digest.as_ref()||selected.and_then(|p|p.pending.as_ref()).is_some(){return Err(domain::error("CONFLICT","等待核心期间插件选择已变化"));}
                        let path=self.verify_target(&intent.target,&intent.catalog_digest)?;
                        let staged=manager.stager().stage(&path).map_err(manager_fault)?;
                        self.accept_package(manager,staged,Origin::Official,&intent.operation_id,false,Some(&id))?;
                        intent.stage=Stage::Ready;
                    }
                    let current=manager.selection_snapshot();
                    let candidate=current.plugins.iter().find(|p|p.plugin_id==id).and_then(|p|p.pending.as_ref());
                    if candidate.is_none_or(|m|m.digest!=release.package_digest){return Err(domain::error("CONFLICT","已准备的插件候选与安装意图不一致"));}
                    Ok(manager.boot_enabled(&id))
                })();
                match result {Ok(true)=>intent.activation_attempt=Some(boot.into()),Ok(false)=>{},Err(_)=>intent.stage=Stage::Failed}
            }
            self.with(|r|{let mut next=r.store.state.clone();next.installs.insert(id.clone(),intent);r.store.save_state(next)})?;
        }
        Ok(())
    }
    pub fn after_activation(&self,manager:&PluginManager)->domain::Result<()> {
        let snapshot=manager.selection_snapshot();
        self.with(|r|{let mut next=r.store.state.clone();for (id,intent) in &mut next.installs {
            if intent.activation_attempt.is_none()||!matches!(intent.stage,Stage::Ready|Stage::Preparing){continue;}
            let Target::Plugin{release,..}=&intent.target else {continue;};
            let selected=snapshot.plugins.iter().find(|p|p.plugin_id==*id).and_then(|p|p.selected.as_ref());
            intent.stage=if selected.is_some_and(|m|m.digest==release.package_digest){Stage::Installed}else{Stage::Failed};
        }
        for (id,intent) in &next.installs {
            if !matches!(intent.stage,Stage::Installed|Stage::Failed){continue;}
            if let Some(op)=next.operations.iter_mut().find(|o|o.operation_id==intent.operation_id){if let Some(c)=op.components.get_mut(id){c.stage=intent.stage.clone();c.error=(intent.stage==Stage::Failed).then(||domain::error("UPDATE_ACTIVATION_FAILED","插件未能激活，请在插件管理中核对原因"));}}
        }
        for op in &mut next.operations {
            if op.kind=="updates.install"&&!op.components.is_empty()&&op.components.values().all(|c|matches!(c.stage,Stage::Installed|Stage::Failed|Stage::Cancelled)) {
                op.running=false;op.updated_at=domain::now_ms();op.error=op.components.values().find_map(|c|c.error.clone());op.stage=if op.error.is_some(){Stage::Failed}else{Stage::Installed};
            }
        }
        r.store.save_state(next)})
    }
}
