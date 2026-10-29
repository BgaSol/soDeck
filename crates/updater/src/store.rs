use crate::{atomic,catalog::VerifiedCatalog,digest,error,model::*,now_ms,plan::Target,strict,Error,Result};
use serde::{Deserialize,Serialize};
use std::{collections::{BTreeMap,BTreeSet,VecDeque},fs,path::{Path,PathBuf}};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CatalogPointer {pub sequence:u64,pub digest:String,pub channel:Channel}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub enum Stage { Downloading, Verifying, Downloaded, Preparing, Ready, WaitingForCore, Reviewing, HandoffPending, HandoffAttempted, Installed, Failed, Cancelled, RequiresUserAction }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ComponentResult {pub stage:Stage,pub received:u64,pub total:u64,pub error:Option<Error>,#[serde(default)]pub target_digest:Option<String>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Operation {
    pub operation_id:String,pub caller:String,pub request_digest:String,pub kind:String,pub stage:Stage,pub running:bool,
    pub components:BTreeMap<String,ComponentResult>,pub error:Option<Error>,pub updated_at:u64,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct InstallIntent {pub operation_id:String,pub target:Target,pub catalog_digest:String,pub stage:Stage,pub activation_attempt:Option<String>,pub previous_digest:Option<String>,#[serde(default)]pub required_core:Option<CoreHandoff>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CoreHandoff {pub operation_id:String,pub target_version:String,pub build_id:String,pub channel:Channel,pub asset_digest:String,pub stage:Stage}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PersistentState {
    pub schema_version:u32,pub generation:String,pub revision:u64,pub catalog:Option<CatalogPointer>,
    pub last_check:u64,pub last_success:u64,pub retry_after:u64,pub check_error:Option<Error>,
    pub operations:VecDeque<Operation>,pub installs:BTreeMap<String,InstallIntent>,pub handoff:Option<CoreHandoff>,
    #[serde(default)] pub notified_catalog_digest:Option<String>,
    #[serde(default)] pub recovery_required:bool,
    #[serde(default)] pub recovery_prepared:BTreeMap<String,String>,
}
impl Default for PersistentState{fn default()->Self{Self{schema_version:1,generation:uuid::Uuid::new_v4().to_string(),revision:0,catalog:None,last_check:0,last_success:0,retry_after:0,check_error:None,operations:VecDeque::new(),installs:BTreeMap::new(),handoff:None,notified_catalog_digest:None,recovery_required:false,recovery_prepared:BTreeMap::new()}}}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PolicyReceipt {pub operation_id:String,pub request_digest:String,pub revision:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Policy {pub schema_version:u32,pub generation:String,pub revision:u64,pub auto_check:bool,pub receipts:VecDeque<PolicyReceipt>}
impl Default for Policy{fn default()->Self{Self{schema_version:1,generation:uuid::Uuid::new_v4().to_string(),revision:0,auto_check:true,receipts:VecDeque::new()}}}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SourceIntent {pub operation_id:String,pub previous_digest:Option<String>,pub candidate_digest:String,pub origin:Origin}
#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PluginSources {pub follow_official:bool,pub packages:BTreeMap<String,Origin>,pub intent:Option<SourceIntent>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Sources {pub schema_version:u32,pub generation:String,pub revision:u64,pub plugins:BTreeMap<String,PluginSources>}
impl Default for Sources{fn default()->Self{Self{schema_version:1,generation:uuid::Uuid::new_v4().to_string(),revision:0,plugins:BTreeMap::new()}}}

pub struct Store {root:PathBuf,pub state:PersistentState,pub policy:Policy,pub sources:Sources,pub warnings:Vec<String>}
fn load<T:serde::de::DeserializeOwned+Default>(root:&Path,name:&str,max:u64,warnings:&mut Vec<String>)->Result<T>{
    let path=root.join(name);if !path.exists(){return Ok(T::default());}
    let bytes=atomic::read(&path,max)?;
    if let Ok(value)=strict::parse::<serde_json::Value>(&bytes,max as usize) {
        if value["schemaVersion"].as_u64().is_some_and(|v|v!=1){return Err(error("UPDATE_STATE_UNSUPPORTED","更新状态版本不受当前程序支持，原件已保留"));}
    }
    match strict::parse::<T>(&bytes,max as usize){Ok(v)=>Ok(v),Err(_)=>{
        let backup=atomic::directory(&root.join("diagnostics"))?.join(format!("{}-{name}",uuid::Uuid::new_v4()));
        atomic::write(&backup,&bytes)?;warnings.push(format!("{name}损坏，原件已保留；更新安装意图已暂停，请重新检查"));Ok(T::default())
    }}
}
impl Store {
    pub fn open(root:impl AsRef<Path>)->Result<Self>{
        let root=atomic::directory(root.as_ref())?;let mut warnings=vec![];
        let mut state:PersistentState=load(&root,"state.json",8*1024*1024,&mut warnings)?;
        let mut policy:Policy=load(&root,"preferences.json",128*1024,&mut warnings)?;
        let sources:Sources=load(&root,"sources.json",2*1024*1024,&mut warnings)?;
        if state.schema_version!=1||policy.schema_version!=1||sources.schema_version!=1{return Err(error("UPDATE_STATE_UNSUPPORTED","更新状态版本不受当前程序支持，已暂停更新"));}
        if !warnings.is_empty(){policy.auto_check=false;}
        if warnings.iter().any(|w|w.starts_with("state.json")){state.recovery_required=true;}
        let mut changed=false;
        for op in &mut state.operations {if op.running{op.running=false;op.stage=Stage::Failed;op.error=Some(error("RESULT_UNKNOWN","上次操作中断，请核对当前版本"));changed=true;}}
        let mut store=Self{root,state,policy,sources,warnings};
        if changed{let next=store.state.clone();store.save_state(next)?;}
        // Incomplete downloads are never resumed as a new user action.
        let cache=atomic::directory(&store.root.join("cache"))?;
        for e in fs::read_dir(cache).map_err(|_|error("STORAGE_ERROR","无法检查更新缓存"))?.flatten(){if e.file_name().to_string_lossy().ends_with(".part")&&e.file_type().is_ok_and(|t|t.is_file()){let _=fs::remove_file(e.path());}}
        Ok(store)
    }
    pub fn root(&self)->&Path{&self.root}
    pub fn reconcile_handoff(&mut self,runtime:&RuntimeIdentity)->Result<()> {
        let mut next=self.state.clone();
        let Some(h)=next.handoff.as_mut().filter(|h|!matches!(h.stage,Stage::Installed|Stage::Failed)) else {return Ok(())};
        let success=h.target_version==runtime.app_version&&h.build_id==runtime.build_id&&h.channel==runtime.channel;
        h.stage=if success{Stage::Installed}else{Stage::Failed};
        let error=(!success).then(||error("RESULT_UNKNOWN","上次核心安装未完成，请核对当前版本后重新操作"));
        if let Some(op)=next.operations.iter_mut().find(|o|o.operation_id==h.operation_id){
            op.running=false;op.stage=h.stage.clone();op.error=error.clone();op.updated_at=now_ms();
            if let Some(component)=op.components.get_mut("core"){component.stage=h.stage.clone();component.error=error.clone();}
        }
        if error.is_some(){next.check_error=error;}
        self.save_state(next)
    }
    pub fn save_state(&mut self,mut next:PersistentState)->Result<()> {
        next.revision=self.state.revision+1;
        while next.operations.len()>128{let Some(i)=next.operations.iter().position(|o|!o.running)else{return Err(error("CAPACITY","更新操作数量达到上限"));};next.operations.remove(i);}
        let bytes=serde_json::to_vec(&next).unwrap();if bytes.len()>8*1024*1024{return Err(error("CAPACITY","更新状态超出容量"));}
        atomic::write(&self.root.join("state.json"),&bytes)?;self.state=next;Ok(())
    }
    pub fn accept_catalog(&mut self,catalog:&VerifiedCatalog)->Result<()> {
        let c=catalog.catalog();
        if let Some(old)=&self.state.catalog{if old.channel!=c.channel||c.sequence<old.sequence||(c.sequence==old.sequence&&catalog.digest()!=old.digest){return Err(error("UPDATE_SOURCE_INVALID","拒绝退回旧目录或替换同批发布"));}}
        let dir=atomic::directory(&self.root.join("catalogs"))?;let dir=atomic::directory(&dir.join(c.channel.name()))?;let dir=atomic::directory(&dir.join(format!("{:06}",c.sequence)))?;
        atomic::write(&dir.join("catalog-v1.json"),catalog.bytes())?;atomic::write(&dir.join("catalog-v1.sig"),catalog.signature())?;
        let mut next=self.state.clone();next.catalog=Some(CatalogPointer{sequence:c.sequence,digest:catalog.digest().into(),channel:c.channel});next.last_success=now_ms();next.check_error=None;next.retry_after=0;self.save_state(next)
    }
    pub fn catalog(&self,keys:&BTreeMap<String,Vec<u8>>,channel:Channel)->Result<Option<VerifiedCatalog>> {
        let Some(pointer)=&self.state.catalog else{return Ok(None)};if pointer.channel!=channel{return Err(error("UPDATE_SOURCE_INVALID","缓存渠道与当前应用不一致"));}
        let dir=self.root.join("catalogs").join(channel.name()).join(format!("{:06}",pointer.sequence));
        let c=VerifiedCatalog::verify(&atomic::read(&dir.join("catalog-v1.json"),MAX_CATALOG as u64)?,&atomic::read(&dir.join("catalog-v1.sig"),16*1024)?,keys,channel)?;
        if c.digest()!=pointer.digest{return Err(error("UPDATE_SIGNATURE_INVALID","发布缓存与接收记录不一致"));}Ok(Some(c))
    }
    pub fn begin(&mut self,id:&str,caller:&str,kind:&str,request:&serde_json::Value)->Result<(Operation,bool)> {
        if uuid::Uuid::parse_str(id).is_err(){return Err(error("INVALID_PARAMS","更新操作标识无效"));}let request_digest=digest(request);
        if let Some(o)=self.state.operations.iter().find(|o|o.operation_id==id){if o.caller!=caller||o.request_digest!=request_digest||o.kind!=kind{return Err(error("CONFLICT","该更新操作标识已用于不同请求"));}return Ok((o.clone(),false));}
        if self.state.operations.iter().filter(|o|o.running).count()>=64{return Err(error("CAPACITY","更新操作队列已满"));}
        let op=Operation{operation_id:id.into(),caller:caller.into(),request_digest,kind:kind.into(),stage:Stage::Preparing,running:true,components:BTreeMap::new(),error:None,updated_at:now_ms()};
        let mut next=self.state.clone();next.operations.push_back(op.clone());self.save_state(next)?;Ok((op,true))
    }
    pub fn operation(&self,id:&str)->Option<&Operation>{self.state.operations.iter().find(|o|o.operation_id==id)}
    pub fn update_operation(&mut self,id:&str,change:impl FnOnce(&mut Operation))->Result<Operation>{
        let mut next=self.state.clone();let op=next.operations.iter_mut().find(|o|o.operation_id==id).ok_or_else(||error("RESULT_UNKNOWN","更新操作记录不可用"))?;change(op);op.updated_at=now_ms();let op=op.clone();self.save_state(next)?;Ok(op)
    }
    pub fn policy_validate(&self,generation:&str,revision:&str)->Result<()> {
        if generation!=self.policy.generation||revision!=self.policy.revision.to_string(){return Err(error("CONFLICT","更新设置已变化，请重新加载"));}Ok(())
    }
    pub fn policy_commit(&mut self,generation:&str,revision:&str,auto_check:bool,operation_id:&str)->Result<Policy>{
        if uuid::Uuid::parse_str(operation_id).is_err(){return Err(error("INVALID_PARAMS","设置操作标识无效"));}
        let request=digest(&(generation,revision,auto_check));
        if let Some(old)=self.policy.receipts.iter().find(|r|r.operation_id==operation_id){if old.request_digest!=request{return Err(error("CONFLICT","保存标识已用于不同设置"));}return Ok(self.policy.clone());}
        self.policy_validate(generation,revision)?;let mut next=self.policy.clone();next.revision+=1;next.auto_check=auto_check;
        next.receipts.push_back(PolicyReceipt{operation_id:operation_id.into(),request_digest:request,revision:next.revision});while next.receipts.len()>128{next.receipts.pop_front();}
        atomic::write(&self.root.join("preferences.json"),&serde_json::to_vec(&next).unwrap())?;self.policy=next;Ok(self.policy.clone())
    }
    pub fn source_for(&self,id:&str,digest:&str)->(Origin,bool){
        self.sources.plugins.get(id).map(|s|(s.packages.get(digest).cloned().unwrap_or(Origin::Local),s.follow_official&&s.intent.is_none())).unwrap_or((Origin::Local,false))
    }
    pub fn source_intent(&mut self,id:&str,previous:Option<&str>,candidate:&str,origin:Origin,operation_id:&str)->Result<()> {
        if !crate::catalog::id(id)||!crate::catalog::sha(candidate){return Err(error("INVALID_PARAMS","插件来源身份无效"));}
        let mut next=self.sources.clone();let source=next.plugins.entry(id.into()).or_default();
        if source.intent.as_ref().is_some_and(|i|i.operation_id!=operation_id){return Err(error("CONFLICT","该插件仍有待核对的导入"));}
        if origin==Origin::Local{source.follow_official=false;}
        source.intent=Some(SourceIntent{operation_id:operation_id.into(),previous_digest:previous.map(str::to_owned),candidate_digest:candidate.into(),origin});self.save_sources(next)
    }
    pub fn source_complete(&mut self,id:&str,actual_digest:Option<&str>)->Result<()> {
        let mut next=self.sources.clone();let Some(source)=next.plugins.get_mut(id)else{return Ok(())};let Some(intent)=source.intent.take()else{return Ok(())};
        if actual_digest==Some(intent.candidate_digest.as_str()){
            source.follow_official=intent.origin!=Origin::Local;
            source.packages.insert(intent.candidate_digest,intent.origin);
        }else{source.follow_official=false;}
        // Keep bounded historical provenance; unknown old versions remain local.
        while source.packages.len()>16{let key=source.packages.keys().find(|k|Some(k.as_str())!=actual_digest).cloned().unwrap();source.packages.remove(&key);}
        self.save_sources(next)
    }
    pub fn follow_official(&mut self,id:&str,actual_digest:&str,official_digest:Option<&str>)->Result<()> {
        if !crate::catalog::id(id)||!crate::catalog::sha(actual_digest){return Err(error("INVALID_PARAMS","插件来源身份无效"));}
        let mut next=self.sources.clone();let s=next.plugins.entry(id.into()).or_default();if s.intent.is_some(){return Err(error("CONFLICT","请先核对进行中的插件导入"));}
        s.follow_official=true;s.packages.insert(actual_digest.into(),if official_digest==Some(actual_digest){Origin::Official}else{Origin::Local});self.save_sources(next)
    }
    fn save_sources(&mut self,mut next:Sources)->Result<()> {
        next.revision=self.sources.revision+1;if next.plugins.len()>1024{return Err(error("CAPACITY","插件来源数量超出上限"));}
        let bytes=serde_json::to_vec(&next).unwrap();if bytes.len()>2*1024*1024{return Err(error("CAPACITY","插件来源记录超出容量"));}
        atomic::write(&self.root.join("sources.json"),&bytes)?;self.sources=next;Ok(())
    }
    pub fn pinned(&self)->BTreeSet<String>{self.state.installs.values().filter(|i|!matches!(i.stage,Stage::Installed|Stage::Failed|Stage::Cancelled)).map(|i|i.target.asset().archive_sha256.clone()).chain(self.state.handoff.iter().filter(|h|!matches!(h.stage,Stage::Installed|Stage::Failed)).map(|h|h.asset_digest.clone())).collect()}
    pub fn reserve_space(&self,bytes:u64,also_pinned:&BTreeSet<String>)->Result<()> {
        let mut used=tree_size(&self.root)?;if bytes>MAX_CACHE{return Err(error("CAPACITY","更新文件超过缓存预算"));}
        if used.saturating_add(bytes)<=MAX_CACHE{return Ok(())}
        let pinned=self.pinned().union(also_pinned).cloned().collect::<BTreeSet<_>>();
        let cache=atomic::directory(&self.root.join("cache"))?;
        let mut candidates=fs::read_dir(cache).map_err(|_|error("STORAGE_ERROR","无法读取更新缓存"))?.flatten().filter_map(|e|{let name=e.file_name().to_string_lossy().into_owned();let m=fs::symlink_metadata(e.path()).ok()?;(!atomic::is_link(&m)&&m.is_file()&&crate::catalog::sha(&name)&&!pinned.contains(&name)).then_some((m.modified().ok(),e.path(),m.len()))}).collect::<Vec<_>>();candidates.sort_by_key(|(time,_,_)|*time);
        for (_,path,len) in candidates{if used.saturating_add(bytes)<=MAX_CACHE{break;}fs::remove_file(path).map_err(|_|error("STORAGE_ERROR","无法清理未使用的更新缓存"))?;used=used.saturating_sub(len);}
        if used.saturating_add(bytes)>MAX_CACHE{return Err(error("CAPACITY","更新缓存空间不足，已准备的安装文件会保留"));}Ok(())
    }
}
fn tree_size(root:&Path)->Result<u64>{let mut size=0u64;for e in fs::read_dir(root).map_err(|_|error("STORAGE_ERROR","无法核对缓存容量"))?.flatten(){let m=fs::symlink_metadata(e.path()).map_err(|_|error("STORAGE_ERROR","无法核对缓存文件"))?;if atomic::is_link(&m){return Err(error("STORAGE_ERROR","更新目录包含链接"));}size=size.saturating_add(if m.is_dir(){tree_size(&e.path())?}else{m.len()});}Ok(size)}
