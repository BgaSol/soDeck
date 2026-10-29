//! Core-owned restart preferences. A corrupt file never means "enable everything".
use crate::{atomic_file, Error, Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeSet,VecDeque},fs,io::{Read,Write},path::{Path,PathBuf}};
const LIMIT:usize=1024*1024;
#[derive(Clone,Debug)]
pub struct BootEnablement { disabled:BTreeSet<String>, available:bool }
impl BootEnablement {
    pub fn enabled(&self,id:&str)->bool { self.available && !self.disabled.contains(id) }
}
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Change { pub plugin_id:String, pub enabled:bool }
#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Receipt { operation_id:String,digest:String,result:Value }
#[derive(Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Data {
    format_version:u32,storage_generation:String,revision:String,
    disabled_plugin_ids:Vec<String>,receipts:VecDeque<Receipt>,
}
impl Data {
    fn fresh(disabled:Vec<String>)->Self { Self {format_version:1,storage_generation:uuid::Uuid::new_v4().to_string(),revision:"0".into(),disabled_plugin_ids:disabled,receipts:VecDeque::new()} }
}
pub struct EnablementStore {
    path:PathBuf,data:Option<Data>,status:&'static str,reason:Option<String>,token:Option<String>,
}
fn fault(code:&'static str,message:&str)->Error {Error::Fault(code,message.into())}
pub fn valid_id(id:&str)->bool { !id.is_empty()&&id.len()<=128&&!id.starts_with('.')&&!id.ends_with('.')&&id.bytes().all(|b| b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'.'|b'-')) }
fn hash(bytes:&[u8])->String {format!("{:x}",Sha256::digest(bytes))}
fn read(path:&Path)->Result<Vec<u8>> {
    let m=fs::symlink_metadata(path)?;
    if crate::package::is_link(&m)||!m.is_file()||m.len()>LIMIT as u64 {return Err(fault("ENABLEMENT_UNAVAILABLE","启用配置文件类型或容量无效"));}
    let mut bytes=vec![];fs::File::open(path)?.take(LIMIT as u64+1).read_to_end(&mut bytes)?;
    if bytes.len()>LIMIT {return Err(fault("CAPACITY","启用配置超出容量"));} Ok(bytes)
}
impl EnablementStore {
    pub fn open(root:&Path)->Self {
        let path=root.join("enablement.json");
        let mut store=Self {path,data:None,status:"unavailable",reason:None,token:None};
        match read(&store.path) {
            Err(Error::Io(e)) if e.kind()==std::io::ErrorKind::NotFound=>{
                let data=Data::fresh(vec![]);
                if let Err(e)=store.persist(data){store.reason=Some(e.to_string());}
            }
            Err(_)=>store.reason=Some("无法读取启用配置，所有插件暂不启动".into()),
            Ok(bytes)=>{
                let parsed=serde_json::from_slice::<Value>(&bytes).ok();
                if parsed.as_ref().and_then(|v|v["formatVersion"].as_u64()).is_some_and(|v|v>1) {
                    store.status="newer";store.reason=Some("启用配置格式较新，请使用相应版本的程序".into());
                } else {
                    let data=serde_json::from_slice::<Data>(&bytes).ok().filter(Self::valid);
                    if let Some(data)=data {store.data=Some(data);store.status="ready";}
                    else {store.status="corrupt";store.token=Some(hash(&bytes));store.reason=Some("启用配置损坏，原文件已保留；所有插件暂不启动".into());}
                }
            }
        } store
    }
    fn valid(d:&Data)->bool {
        d.format_version==1&&uuid::Uuid::parse_str(&d.storage_generation).is_ok()&&d.revision.parse::<u64>().is_ok_and(|r|r.to_string()==d.revision)
        &&d.disabled_plugin_ids.len()<=1024&&d.disabled_plugin_ids.iter().all(|id|valid_id(id))
        &&d.disabled_plugin_ids.windows(2).all(|w|w[0]<w[1])&&d.receipts.len()<=128
        &&d.receipts.iter().all(|r|valid_operation(&r.operation_id)&&r.digest.len()==64&&r.digest.bytes().all(|b|b.is_ascii_hexdigit())
            &&r.result["operationId"]==r.operation_id&&r.result["generation"]==d.storage_generation
            &&r.result["persistedRevision"].as_str().is_some_and(|s|s.parse::<u64>().is_ok_and(|v|v.to_string()==s&&v<=d.revision.parse::<u64>().unwrap_or(0)))
            &&r.result["restartRequired"].is_boolean()&&r.result.as_object().is_some_and(|o|o.keys().all(|k|matches!(k.as_str(),"operationId"|"generation"|"persistedRevision"|"restartRequired"|"backupPath")))
            &&r.result.get("backupPath").is_none_or(|v|v.as_str().is_some_and(|s|s.len()<=4096)))
        &&d.receipts.iter().map(|r|&r.operation_id).collect::<BTreeSet<_>>().len()==d.receipts.len()
        &&serde_json::to_vec(&d.receipts).is_ok_and(|b|b.len()<=128*1024)
    }
    fn persist(&mut self,data:Data)->Result<()> {
        let bytes=serde_json::to_vec(&data)?;
        if bytes.len()>LIMIT {return Err(fault("CAPACITY","启用配置超出容量"));}
        atomic_file::write(&self.path,&bytes).map_err(|_|fault("STORAGE_ERROR","无法保存启用配置，原配置保留"))?;
        self.data=Some(data);self.status="ready";self.reason=None;self.token=None;Ok(())
    }
    pub fn boot(&self)->BootEnablement {BootEnablement {available:self.data.is_some(),disabled:self.data.as_ref().map(|d|d.disabled_plugin_ids.iter().cloned().collect()).unwrap_or_default()}}
    pub fn enabled(&self,id:&str)->bool {self.data.as_ref().is_some_and(|d|!d.disabled_plugin_ids.iter().any(|p|p==id))}
    pub fn revision(&self)->String {self.data.as_ref().map(|d|d.revision.clone()).unwrap_or_else(||"0".into())}
    pub fn snapshot(&self)->Value {json!({"storageStatus":self.status,"generation":self.data.as_ref().map(|d|&d.storage_generation),"revision":self.revision(),"reason":self.reason,"diagnosticToken":self.token})}
    pub fn validate(&self,generation:&str,revision:&str,changes:&[Change],known:&BTreeSet<String>)->Result<()> {
        let data=self.data.as_ref().ok_or_else(||fault("ENABLEMENT_UNAVAILABLE","启用配置不可用"))?;
        if generation!=data.storage_generation||revision!=data.revision {return Err(fault("CONFLICT","插件启用设置已改变，请载入最新值"));}
        if changes.is_empty()||changes.len()>1024||serde_json::to_vec(changes)?.len()>60*1024{return Err(fault("CAPACITY","启用更改数量或容量无效"));}
        let mut seen=BTreeSet::new();
        for change in changes {
            if !valid_id(&change.plugin_id)||!known.contains(&change.plugin_id)||!seen.insert(&change.plugin_id) {return Err(fault("INVALID_PARAMS","插件标识无效、未登记或重复"));}
        } Ok(())
    }
    pub fn commit(&mut self,generation:&str,revision:&str,changes:&[Change],known:&BTreeSet<String>,operation:&str,boot:&BootEnablement)->Result<Value> {
        if !valid_operation(operation){return Err(fault("INVALID_PARAMS","操作标识无效"));}
        let mut normalized=changes.to_vec();normalized.sort_by(|a,b|a.plugin_id.cmp(&b.plugin_id));
        let digest=hash(&serde_json::to_vec(&json!({"generation":generation,"expectedRevision":revision,"changes":normalized}))?);
        if let Some(data)=&self.data {
            if let Some(receipt)=data.receipts.iter().find(|r|r.operation_id==operation) {
                return if receipt.digest==digest {Ok(receipt.result.clone())}else{Err(fault("CONFLICT","同一操作标识不能提交不同更改"))};
            }
        }
        self.validate(generation,revision,changes,known)?;
        let mut next=self.data.clone().unwrap();
        let mut disabled=next.disabled_plugin_ids.iter().cloned().collect::<BTreeSet<_>>();
        for c in changes {if c.enabled{disabled.remove(&c.plugin_id);}else{disabled.insert(c.plugin_id.clone());}}
        if disabled.len()>1024 {return Err(fault("CAPACITY","停用插件数量超出容量"));}
        next.disabled_plugin_ids=disabled.into_iter().collect();
        next.revision=next.revision.parse::<u64>().unwrap().checked_add(1).ok_or_else(||fault("CAPACITY","设置修订已用尽"))?.to_string();
        let restart=known.iter().any(|id|boot.enabled(id)!=!next.disabled_plugin_ids.contains(id));
        let result=json!({"operationId":operation,"generation":next.storage_generation,"persistedRevision":next.revision,"restartRequired":restart});
        Self::receipt(&mut next,operation,digest,result.clone())?;
        self.persist(next)?;Ok(result)
    }
    fn receipt(data:&mut Data,operation:&str,digest:String,result:Value)->Result<()> {
        data.receipts.push_back(Receipt {operation_id:operation.into(),digest,result});
        while data.receipts.len()>128||serde_json::to_vec(&data.receipts)?.len()>128*1024 {data.receipts.pop_front();}
        Ok(())
    }
    pub fn operation(&self,generation:&str,operation:&str)->Value {
        if let Some(data)=&self.data {
            if data.storage_generation==generation {
                if let Some(receipt)=data.receipts.iter().find(|r|r.operation_id==operation){return json!({"state":"saved","result":receipt.result});}
            }
        }
        json!({"state":"unknown"})
    }
    pub fn repair(&mut self,token:&str,operation:&str,known:&BTreeSet<String>)->Result<Value> {
        if !valid_operation(operation){return Err(fault("INVALID_PARAMS","操作标识无效"));}
        let digest=hash(format!("repair:{token}").as_bytes());
        if let Some(data)=&self.data {if let Some(r)=data.receipts.iter().find(|r|r.operation_id==operation){
            return if r.digest==digest {Ok(r.result.clone())}else{Err(fault("CONFLICT","恢复操作已改变"))};
        }}
        if self.status!="corrupt"||self.token.as_deref()!=Some(token){return Err(fault("CONFLICT","启用配置状态已改变或不支持重建"));}
        let bytes=read(&self.path)?;
        if hash(&bytes)!=token{return Err(fault("CONFLICT","原启用文件已改变，请重新打开程序核对"));}
        let timestamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let backup=self.path.with_file_name(format!("enablement-backup-{timestamp}-{}.json",uuid::Uuid::new_v4()));
        let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&backup)?;
        file.write_all(&bytes)?;file.sync_all()?;drop(file);
        if hash(&read(&self.path)?)!=token{return Err(fault("CONFLICT","备份期间原文件已改变"));}
        let mut next=Data::fresh(known.iter().cloned().collect());
        let result=json!({"operationId":operation,"generation":next.storage_generation,"persistedRevision":"0","restartRequired":true,"backupPath":backup});
        Self::receipt(&mut next,operation,digest,result.clone())?;
        self.persist(next)?;Ok(result)
    }
}
pub fn valid_operation(id:&str)->bool {uuid::Uuid::parse_str(id).is_ok()}

#[cfg(test)]
mod tests {
    use super::*;
    fn change(enabled:bool)->Vec<Change>{vec![Change{plugin_id:"test.plugin".into(),enabled}]}
    fn known()->BTreeSet<String>{BTreeSet::from(["test.plugin".into()])}
    #[test] fn restart_snapshot_and_receipt_are_independent_of_saved_preference(){
        let dir=tempfile::tempdir().unwrap();let mut store=EnablementStore::open(dir.path());let boot=store.boot();let s=store.snapshot();let op=uuid::Uuid::new_v4().to_string();
        let saved=store.commit(s["generation"].as_str().unwrap(),"0",&change(false),&known(),&op,&boot).unwrap();
        assert!(boot.enabled("test.plugin"));assert!(!store.enabled("test.plugin"));
        assert_eq!(saved,store.commit(s["generation"].as_str().unwrap(),"0",&change(false),&known(),&op,&boot).unwrap());
        assert!(store.commit(s["generation"].as_str().unwrap(),"0",&change(true),&known(),&op,&boot).is_err());
        assert!(!EnablementStore::open(dir.path()).boot().enabled("test.plugin"));
    }
    #[test] fn corrupt_backup_repair_and_newer_file_do_not_default_enable(){
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("enablement.json");fs::write(&path,b"{bad").unwrap();
        let mut s=EnablementStore::open(dir.path());assert!(!s.boot().enabled("test.plugin"));let token=s.snapshot()["diagnosticToken"].as_str().unwrap().to_owned();
        let result=s.repair(&token,&uuid::Uuid::new_v4().to_string(),&known()).unwrap();
        assert_eq!(fs::read(result["backupPath"].as_str().unwrap()).unwrap(),b"{bad");assert!(!s.enabled("test.plugin"));
        fs::write(path,br#"{"formatVersion":2}"#).unwrap();let s=EnablementStore::open(dir.path());assert_eq!(s.snapshot()["storageStatus"],"newer");assert!(!s.boot().enabled("test.plugin"));
    }
    #[test] fn duplicate_ids_and_stale_revision_reject(){
        let dir=tempfile::tempdir().unwrap();let s=EnablementStore::open(dir.path());let g=s.snapshot()["generation"].as_str().unwrap().to_owned();
        assert!(s.validate(&g,"1",&change(false),&known()).is_err());
        assert!(s.validate(&g,"0",&[change(false)[0].clone(),change(true)[0].clone()],&known()).is_err());
        let mut bad=s.data.unwrap();bad.disabled_plugin_ids=vec!["test.plugin".into(),"test.plugin".into()];fs::write(dir.path().join("enablement.json"),serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(!EnablementStore::open(dir.path()).boot().enabled("test.plugin"));
    }
    #[test] fn bounded_receipts_expire_as_unknown_and_failed_writes_keep_revision(){
        let dir=tempfile::tempdir().unwrap();let mut s=EnablementStore::open(dir.path());let boot=s.boot();let g=s.snapshot()["generation"].as_str().unwrap().to_owned();let first=uuid::Uuid::new_v4().to_string();
        for i in 0..130{let op=if i==0{first.clone()}else{uuid::Uuid::new_v4().to_string()};s.commit(&g,&i.to_string(),&change(i%2==0),&known(),&op,&boot).unwrap();}
        assert_eq!(s.data.as_ref().unwrap().receipts.len(),128);assert_eq!(s.operation(&g,&first)["state"],"unknown");
        let disk=fs::read(&s.path).unwrap();fs::remove_file(&s.path).unwrap();fs::create_dir(&s.path).unwrap();
        assert!(s.commit(&g,"130",&change(false),&known(),&uuid::Uuid::new_v4().to_string(),&boot).is_err());assert_eq!(s.revision(),"130");
        fs::remove_dir(&s.path).unwrap();fs::write(&s.path,disk).unwrap();assert_eq!(EnablementStore::open(dir.path()).revision(),"130");
    }
    #[test] fn tampered_receipt_fails_closed_and_repair_checks_original_fingerprint(){
        let dir=tempfile::tempdir().unwrap();let mut s=EnablementStore::open(dir.path());let boot=s.boot();let g=s.snapshot()["generation"].as_str().unwrap().to_owned();
        s.commit(&g,"0",&change(false),&known(),&uuid::Uuid::new_v4().to_string(),&boot).unwrap();let mut d=s.data.unwrap();d.receipts[0].result["restartRequired"]=json!("false");fs::write(&s.path,serde_json::to_vec(&d).unwrap()).unwrap();
        let mut s=EnablementStore::open(dir.path());assert!(!s.boot().enabled("test.plugin"));let token=s.snapshot()["diagnosticToken"].as_str().unwrap().to_owned();
        fs::write(&s.path,b"changed again").unwrap();assert!(s.repair(&token,&uuid::Uuid::new_v4().to_string(),&known()).is_err());assert_eq!(fs::read(&s.path).unwrap(),b"changed again");
    }
    #[cfg(windows)]
    #[test] fn unreadable_file_and_failed_backup_preserve_original_and_execute_nothing(){
        use std::{os::windows::fs::OpenOptionsExt,process::Command};
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("enablement.json");fs::write(&path,b"{corrupt").unwrap();
        let exclusive=fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();let blocked=EnablementStore::open(dir.path());assert_eq!(blocked.snapshot()["storageStatus"],"unavailable");assert!(!blocked.boot().enabled("test.plugin"));drop(exclusive);
        let mut store=EnablementStore::open(dir.path());let token=store.snapshot()["diagnosticToken"].as_str().unwrap().to_owned();
        let user=format!("{}\\{}",std::env::var("USERDOMAIN").unwrap(),std::env::var("USERNAME").unwrap());
        let deny=Command::new("icacls.exe").arg(dir.path()).args(["/deny",&format!("{user}:(WD)")]).output().unwrap();assert!(deny.status.success());
        let result=store.repair(&token,&uuid::Uuid::new_v4().to_string(),&known());
        let restore=Command::new("icacls.exe").arg(dir.path()).args(["/remove:d",&user]).output().unwrap();assert!(restore.status.success());
        assert!(result.is_err());assert!(!store.boot().enabled("test.plugin"));assert_eq!(fs::read(path).unwrap(),b"{corrupt");assert_eq!(fs::read_dir(dir.path()).unwrap().count(),1);
    }
}
