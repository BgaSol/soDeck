use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
pub(super) fn reference(key:&str,epoch:&str,kind:&str,id:&str,generation:Option<&str>)->Value {
    let opaque=super::model::hash(&json!([key,epoch,kind,id,generation]));
    let tag=super::model::hash(&json!([key,kind,generation,opaque]));
    let mut target=json!({"kind":kind,"ref":format!("d1-{opaque}-{}",&tag[..32])});
    if let Some(generation)=generation{target["generation"]=json!(generation);}target
}
pub(super) fn validate(key:&str,target:&Value)->Result<()> {
    let reference=target["ref"].as_str().unwrap_or_default();
    let Some((opaque,tag))=reference.strip_prefix("d1-").and_then(|v|v.split_once('-'))else{return Err(CoreError::new("INVALID_REFERENCE","诊断引用必须由核心签发"));};
    if opaque.len()!=64||tag!=&super::model::hash(&json!([key,target["kind"],target.get("generation"),opaque]))[..32]{return Err(CoreError::new("INVALID_REFERENCE","诊断引用校验失败"));}Ok(())
}
impl Core {
    /// Hosts validate workspace/view generations before issuing these references.
    /// The result has no authority in resource execution APIs.
    pub fn observation_target(&self,kind:&str,id:&str,generation:Option<&str>)->Result<Value>{
        if !matches!(kind,"server"|"workspace"|"view"|"resource")||id.is_empty()||id.len()>128||generation.is_some_and(|v|v.is_empty()||v.len()>128){return Err(CoreError::new("INVALID_REFERENCE","诊断目标无效"));}
        let epoch=if kind=="server"{&self.observability.config_epoch}else{&self.observability.boot_id};
        let target=reference(&self.observability.signing_key,epoch,kind,id,generation);
        let value=target.clone();let kind=kind.to_owned();let id=id.to_owned();let generation=generation.map(str::to_owned);
        self.observability.write(super::schema::Partition::Facts,4096,move|db|{db.admit(super::schema::Partition::Facts,4096)?;db.facts.execute("INSERT OR IGNORE INTO diagnostic_targets VALUES(?,?,?,?,?)",rusqlite::params![value["ref"].as_str(),kind,id,generation,crate::model::now_ms()as i64])?;Ok(Value::Null)})?;
        Ok(target)
    }
    pub(super) fn resolve_observation_target(&self,caller:&Caller,p:&Value)->Result<Value>{
        let kind=p["kind"].as_str().unwrap();let id=p["id"].as_str().unwrap();
        if kind=="server"{self.store.server(id)?;return self.observation_target(kind,id,Some(&self.observability.config_epoch));}
        if kind!="resource"{return Err(CoreError::new("HOST_REQUIRED","工作区和视图引用需要宿主校验"));}
        let resources=self.resources();let mut found=None;
        for category in ["connections","ptys","leases"]{if let Some(value)=resources[category].as_array().and_then(|a|a.iter().find(|v|v["id"]==id)){found=Some(value.get("generation").and_then(Value::as_u64).map(|v|v.to_string()));break;}}
        if found.is_none(){if let Some(value)=resources["remote"]["handles"].as_array().and_then(|a|a.iter().find(|v|v["id"]==id&&v["revoked"]!=true)){found=Some(resources["connections"].as_array().and_then(|a|a.iter().find(|c|c["id"]==value["connectionId"])).and_then(|v|v["generation"].as_u64()).map(|g|g.to_string()));}}
        if found.is_none(){let mut query=json!({});loop{let page=self.network_dispatch(caller,"net.resources.list",&query)?;if let Some(value)=page["items"].as_array().and_then(|a|a.iter().find(|v|v["handle"]["id"]==id)){found=Some(value["handle"]["generation"].as_u64().map(|v|v.to_string()));break;}let Some(cursor)=page["nextCursor"].as_str()else{break};query["cursor"]=json!(cursor);}}
        let actual=found.ok_or_else(||CoreError::new("NOT_FOUND","资源不存在或已经结束"))?;
        if p["generation"].as_str().is_some_and(|g|Some(g)!=actual.as_deref()){return Err(CoreError::new("STALE_HANDLE","资源代次已经改变"));}
        self.observation_target(kind,id,actual.as_deref())
    }
}
