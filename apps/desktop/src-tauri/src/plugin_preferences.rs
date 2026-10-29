//! One lock per actual data generation, shared by legacy and grouped writes.
use super::{err, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use sodeck_plugin_manager::settings_descriptor::{Descriptor, Entry, Page};
use std::{collections::{BTreeMap,BTreeSet,VecDeque}, fs, path::{Path,PathBuf}, sync::{Arc,Mutex,OnceLock,Weak}};
const META:&str="__sodeckPreferences";
const LIMIT:usize=1024*1024;
#[derive(Clone,Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Group { schema_version:u32, keys:Vec<String>, revision:u64, receipts:VecDeque<Receipt> }
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Receipt {pub operation_id:String,pub digest:String,pub old_revision:String,pub persisted_revision:String,pub keys:Vec<String>,pub persistence:String}
impl Receipt {pub fn result(&self,current:bool)->Value {json!({"operationId":self.operation_id,"persistedRevision":self.persisted_revision,"persistence":self.persistence,"synchronization":if current{"current"}else{"pending"}})}}
#[derive(Default,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Metadata {format_version:u32,groups:BTreeMap<String,Group>}
#[derive(Clone)]
pub struct Ticket {
    pub id:String,pub epoch:String,pub group:Page,pub expected:String,pub operation:String,pub digest:String,pub keys:BTreeSet<String>,
    pub applied:Option<String>,pub saved:bool,
}
#[derive(Default)]
struct Session {epoch:String,plugin:String,descriptor:Option<Descriptor>,tickets:BTreeMap<String,Ticket>}
pub struct Store {path:PathBuf,session:Mutex<Session>}
impl Store {
    pub fn shared(data:&Path)->Arc<Self>{
        static STORES:OnceLock<Mutex<BTreeMap<PathBuf,Weak<Store>>>>=OnceLock::new();
        let path=data.to_path_buf();let mut stores=STORES.get_or_init(Default::default).lock().unwrap();
        stores.retain(|_,s|s.strong_count()>0);
        if let Some(store)=stores.get(&path).and_then(Weak::upgrade){return store;}
        let store=Arc::new(Self{path:path.clone(),session:Mutex::new(Session::default())});stores.insert(path,Arc::downgrade(&store));store
    }
    pub fn activate(&self,plugin:&str,epoch:&str,descriptor:Option<Descriptor>){
        let mut s=self.session.lock().unwrap();*s=Session{epoch:epoch.into(),plugin:plugin.into(),descriptor,tickets:BTreeMap::new()};
    }
    pub fn generation(&self)->String {self.path.file_name().unwrap_or_default().to_string_lossy().into_owned()}
    fn load(&self)->Result<(Value,Metadata)>{
        let file=self.path.join("settings.json");
        let value=match fs::symlink_metadata(&file){
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>json!({}),
            Err(e)=>return Err(err("STORAGE_ERROR",e.to_string())),
            Ok(m)=>{if !m.file_type().is_file()||m.len()>LIMIT as u64{return Err(err("STORAGE_ERROR","插件设置文件类型或容量无效"));}
                let mut bytes=Vec::new();fs::File::open(file).and_then(|f|f.take(LIMIT as u64+1).read_to_end(&mut bytes)).map_err(|e|err("STORAGE_ERROR",e.to_string()))?;if bytes.len()>LIMIT{return Err(err("CAPACITY","插件设置超出容量"));}
                serde_json::from_slice(&bytes).map_err(|_|err("STORAGE_ERROR","插件设置已损坏，原文件已保留"))?}
        };
        if !value.is_object(){return Err(err("STORAGE_ERROR","插件设置必须为对象"));}
        let meta=match value.get(META){None=>Metadata{format_version:1,..Default::default()},Some(v)=>{
            let m:Metadata=serde_json::from_value(v.clone()).map_err(|_|err("CONFLICT","插件设置保留元数据冲突"))?;
            if m.format_version!=1{return Err(err("READ_ONLY","插件设置格式较新"));}m
        }};Ok((value,meta))
    }
    fn write(&self,mut value:Value,meta:&Metadata)->Result<()>{
        if meta.groups.values().map(|g|serde_json::to_vec(&g.receipts).unwrap().len()).sum::<usize>()>128*1024{return Err(err("CAPACITY","插件设置回执超出容量"));}
        value[META]=serde_json::to_value(meta).unwrap();let bytes=serde_json::to_vec(&value).map_err(|e|err("STORAGE_ERROR",e.to_string()))?;
        if bytes.len()>LIMIT{return Err(err("CAPACITY","插件设置超出容量"));}
        crate::atomic_file::write(&self.path.join("settings.json"),&bytes).map_err(|e|err("STORAGE_ERROR",e))
    }
    fn page(s:&Session,group:&str)->Result<Page>{s.descriptor.as_ref().and_then(|d|d.pages.iter().find(|p|p.id==group)).cloned().ok_or_else(||err("SETTINGS_UNAVAILABLE","设置组未声明"))}
    fn check_epoch(s:&Session,epoch:&str)->Result<()>{if s.epoch!=epoch{return Err(err("STALE_PROVIDER","设置提供者已重建"));}Ok(())}
    pub fn read(&self,epoch:&str,group:&str)->Result<Value>{
        let s=self.session.lock().unwrap();Self::check_epoch(&s,epoch)?;let page=Self::page(&s,group)?;
        let (value,meta)=self.load()?;let g=meta.groups.get(group);
        if g.is_some_and(|g|g.schema_version>page.schema_version){return Err(err("READ_ONLY","设置数据版本较新"));}
        let mut warnings=vec![];
        let values=page.fields().map(|f|{let v=value.get(&f.key).filter(|v|f.accepts(v));if value.get(&f.key).is_some()&&v.is_none(){warnings.push(format!("{} 的旧值无效，应用后将修复",f.label));}json!({"key":f.key,"value":v.unwrap_or(&f.default)})}).collect::<Vec<_>>();
        if serde_json::to_vec(&values).unwrap().len()>16*1024{return Err(err("CAPACITY","设置组值超出容量"));}
        Ok(json!({"pageId":format!("plugin:{}:{group}",s.plugin),"providerEpoch":epoch,"storageGeneration":self.generation(),"schemaVersion":page.schema_version,"settingsRevision":g.map_or(0,|g|g.revision).to_string(),"values":values,"syncState":"current","warnings":warnings}))
    }
    pub fn raw(&self,epoch:&str,method:&str,p:&Value)->Result<Value>{
        let s=self.session.lock().unwrap();Self::check_epoch(&s,epoch)?;
        let key=p["key"].as_str().unwrap_or("settings");if key.len()>200||key==META{return Err(err("INVALID_PARAMS","设置键不可用"));}
        let (mut value,mut meta)=self.load()?;
        if method=="storage.get"{return Ok(value.get(key).cloned().unwrap_or(Value::Null));}
        if meta.groups.iter().any(|(id,g)|g.keys.iter().any(|k|k==key)&&g.schema_version>s.descriptor.as_ref().and_then(|d|d.pages.iter().find(|p|&p.id==id)).map_or(1,|p|p.schema_version)){return Err(err("READ_ONLY","设置数据版本较新"));}
        if s.descriptor.as_ref().is_some_and(|d|d.pages.iter().any(|p|p.fields().any(|f|f.key==key))){return Err(err("SETTINGS_MANAGED","请通过设置组提交此字段"));}
        value[key]=p["value"].clone();
        for g in meta.groups.values_mut(){if g.keys.iter().any(|k|k==key){g.revision=g.revision.checked_add(1).ok_or_else(||err("CAPACITY","设置修订溢出"))?;}}
        self.write(value,&meta)?;Ok(json!({"saved":true}))
    }
    pub fn issue(&self,ticket:Ticket)->Result<()>{
        let mut s=self.session.lock().unwrap();Self::check_epoch(&s,&ticket.epoch)?;
        s.tickets.retain(|_,t|!t.saved);
        if s.tickets.values().any(|t|t.group.id==ticket.group.id){return Err(err("BUSY","此设置组正在保存"));}
        if s.tickets.len()>=8{return Err(err("BUSY","设置操作过多"));}s.tickets.insert(ticket.id.clone(),ticket);Ok(())
    }
    pub fn commit(&self,epoch:&str,id:&str,changes:Vec<Entry>)->Result<Value>{
        let mut s=self.session.lock().unwrap();Self::check_epoch(&s,epoch)?;
        let ticket=s.tickets.get(id).cloned().ok_or_else(||err("STALE_OPERATION","设置操作凭据失效"))?;
        ticket.group.validate_changes(&changes).map_err(|e|err("INVALID_PARAMS",e))?;
        if changes.iter().map(|e|e.key.clone()).collect::<BTreeSet<_>>()!=ticket.keys{return Err(err("INVALID_PARAMS","提交字段与操作凭据不匹配"));}
        let (mut value,mut meta)=self.load()?;let group=meta.groups.entry(ticket.group.id.clone()).or_insert_with(||Group{schema_version:ticket.group.schema_version,..Default::default()});
        if let Some(r)=group.receipts.iter().find(|r|r.operation_id==ticket.operation){
            if r.digest!=ticket.digest{return Err(err("CONFLICT","操作编号已用于其他更改"));}return Ok(r.result(false));
        }
        if group.revision.to_string()!=ticket.expected||group.schema_version!=ticket.group.schema_version{return Err(err("CONFLICT","设置已在其他位置改变，请载入最新值"));}
        let changed=changes.iter().any(|e|value.get(&e.key)!=Some(&e.value));
        let revision=if changed{group.revision.checked_add(1).ok_or_else(||err("CAPACITY","设置修订溢出"))?}else{group.revision};
        for e in changes {value[e.key]=e.value;}
        group.keys=ticket.group.fields().map(|f|f.key.clone()).collect();group.revision=revision;
        let receipt=Receipt{operation_id:ticket.operation,digest:ticket.digest,old_revision:ticket.expected,persisted_revision:revision.to_string(),keys:ticket.keys.into_iter().collect(),persistence:if changed{"saved"}else{"unchanged"}.into()};
        let group_values=ticket.group.fields().map(|f|json!({"key":f.key,"value":value.get(&f.key).filter(|v|f.accepts(v)).unwrap_or(&f.default)})).collect::<Vec<_>>();
        if serde_json::to_vec(&group_values).unwrap().len()>16*1024{return Err(err("CAPACITY","设置组值超出容量"));}
        // A saved group must remain representable by the public page RPC as well as its feed.
        let warnings=ticket.group.fields().filter(|f|value.get(&f.key).is_some_and(|v|!f.accepts(v))).map(|f|format!("{} 的旧值无效，应用后将修复",f.label)).collect::<Vec<_>>();
        let snapshot_bytes=serde_json::to_vec(&group_values).unwrap().len()+serde_json::to_vec(&warnings).unwrap().len()+1024;
        if snapshot_bytes>24*1024||snapshot_bytes+serde_json::to_vec(&ticket.group).unwrap().len()+1024>64*1024{return Err(err("CAPACITY","设置页面超出传输容量"));}
        group.receipts.push_back(receipt.clone());while group.receipts.len()>64{group.receipts.pop_front();}
        self.write(value,&meta)?;s.tickets.get_mut(id).unwrap().saved=true;Ok(receipt.result(false))
    }
    pub fn receipt(&self,group:&str,operation:&str)->Result<Option<Receipt>>{let _s=self.session.lock().unwrap();let (_,meta)=self.load()?;Ok(meta.groups.get(group).and_then(|g|g.receipts.iter().find(|r|r.operation_id==operation)).cloned())}
    pub fn applied(&self,epoch:&str,id:&str,revision:&str)->Result<()> {
        let mut s=self.session.lock().unwrap();Self::check_epoch(&s,epoch)?;
        let group=s.tickets.get(id).ok_or_else(||err("STALE_OPERATION","设置操作已失效"))?.group.id.clone();
        let (_,meta)=self.load()?;
        if meta.groups.get(&group).map_or(0,|g|g.revision).to_string()!=revision{return Err(err("CONFLICT","不能确认旧设置修订"));}
        s.tickets.get_mut(id).unwrap().applied=Some(revision.into());Ok(())
    }
    pub fn finish(&self,id:&str)->Option<Ticket>{self.session.lock().unwrap().tickets.remove(id)}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup()->(tempfile::TempDir,Arc<Store>,Page){
        let dir=tempfile::tempdir().unwrap();let store=Store::shared(dir.path());
        let descriptor=Descriptor::parse(br#"{"descriptorVersion":1,"pluginId":"sample","pages":[{"id":"general","title":"Settings","order":0,"schemaVersion":1,"sections":[{"id":"display","title":"Display","fields":[{"key":"size","label":"Size","type":"integer","default":14,"minimum":10,"maximum":24,"effect":"immediate"}]}]}]}"#,"sample").unwrap();
        let page=descriptor.pages[0].clone();store.activate("sample","epoch",Some(descriptor));(dir,store,page)
    }
    fn ticket(page:&Page,id:&str,rev:&str)->Ticket{Ticket{id:id.into(),epoch:"epoch".into(),group:page.clone(),expected:rev.into(),operation:id.into(),digest:id.into(),keys:["size".into()].into(),applied:None,saved:false}}
    #[test] fn read_is_pure_atomic_commit_keeps_unknown_and_receipt(){
        let (dir,s,page)=setup();assert_eq!(s.read("epoch","general").unwrap()["settingsRevision"],"0");assert!(!dir.path().join("settings.json").exists());
        fs::write(dir.path().join("settings.json"),br#"{"size":12,"other":{"keep":true}}"#).unwrap();
        s.issue(ticket(&page,"op","0")).unwrap();s.commit("epoch","op",vec![Entry{key:"size".into(),value:json!(16)}]).unwrap();
        assert_eq!(s.read("epoch","general").unwrap()["values"][0]["value"],16);
        assert!(s.receipt("general","op").unwrap().is_some());assert_eq!(s.load().unwrap().0["other"]["keep"],true);
        assert!(s.raw("epoch","storage.set",&json!({"key":"size","value":18})).is_err());
        s.finish("op");s.issue(ticket(&page,"conflict","0")).unwrap();assert!(s.commit("epoch","conflict",vec![Entry{key:"size".into(),value:json!(18)}]).is_err());
    }
    #[test] fn corrupt_and_unknown_metadata_are_never_overwritten(){
        let (dir,s,_)=setup();for content in ["{broken",r#"{"__sodeckPreferences":{"formatVersion":9,"groups":{}}}"#]{fs::write(dir.path().join("settings.json"),content).unwrap();assert!(s.read("epoch","general").is_err());assert!(s.raw("epoch","storage.set",&json!({"key":"other","value":1})).is_err());assert_eq!(fs::read_to_string(dir.path().join("settings.json")).unwrap(),content);}
    }
    #[test] fn shared_store_and_epoch_protect_retired_workers(){let (dir,s,p)=setup();assert!(Arc::ptr_eq(&s,&Store::shared(dir.path())));s.issue(ticket(&p,"op","0")).unwrap();s.activate("sample","new",None);assert!(s.commit("epoch","op",vec![]).is_err());assert!(s.raw("epoch","storage.set",&json!({"key":"other","value":1})).is_err());}
    #[test] fn receipts_are_bounded_and_evicted_operation_cannot_replay(){
        let (_dir,s,p)=setup();for n in 0..65{let id=format!("op{n}");let rev=s.read("epoch","general").unwrap()["settingsRevision"].as_str().unwrap().to_string();s.issue(ticket(&p,&id,&rev)).unwrap();s.commit("epoch",&id,vec![Entry{key:"size".into(),value:json!(14+n%2)}]).unwrap();s.finish(&id);}
        assert!(s.receipt("general","op0").unwrap().is_none());assert!(s.receipt("general","op64").unwrap().is_some());assert_eq!(s.load().unwrap().1.groups["general"].receipts.len(),64);
        s.issue(ticket(&p,"op0","0")).unwrap();assert!(s.commit("epoch","op0",vec![Entry{key:"size".into(),value:json!(14)}]).is_err());
    }
    #[test] fn file_capacity_and_concurrent_legacy_writer_preserve_group(){
        let (dir,s,p)=setup();let legacy=s.clone();let thread=std::thread::spawn(move||{for n in 0..20{legacy.raw("epoch","storage.set",&json!({"key":"unknown","value":n})).unwrap();}});
        s.issue(ticket(&p,"op","0")).unwrap();s.commit("epoch","op",vec![Entry{key:"size".into(),value:json!(18)}]).unwrap();thread.join().unwrap();let before=fs::read(dir.path().join("settings.json")).unwrap();
        assert_eq!(s.load().unwrap().0["size"],18);assert_eq!(s.load().unwrap().0["unknown"],19);
        assert!(s.raw("epoch","storage.set",&json!({"key":"large","value":"x".repeat(LIMIT)})).is_err());assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(),before);
    }
    #[test] #[cfg(windows)] fn denied_replace_does_not_advance_revision_or_receipt(){
        use std::os::windows::fs::OpenOptionsExt;
        let (dir,s,p)=setup();let path=dir.path().join("settings.json");fs::write(&path,br#"{"size":14,"unknown":true}"#).unwrap();let before=fs::read(&path).unwrap();
        let held=fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();s.issue(ticket(&p,"op","0")).unwrap();assert!(s.commit("epoch","op",vec![Entry{key:"size".into(),value:json!(18)}]).is_err());drop(held);
        assert_eq!(fs::read(&path).unwrap(),before);assert_eq!(s.read("epoch","general").unwrap()["settingsRevision"],"0");assert!(s.receipt("general","op").unwrap().is_none());assert_eq!(fs::read_dir(dir.path()).unwrap().count(),1);
    }
}
