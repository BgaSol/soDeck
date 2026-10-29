use crate::{Caller,CoreError,Result};
use serde_json::{json,Value};
use rusqlite::params;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD,Engine};
use sha2::{Digest,Sha256};
pub fn hash(value:&Value)->String {format!("{:x}",Sha256::digest(serde_json::to_vec(value).expect("JSON value")))}
pub fn seal(key:&str,value:&Value)->String {
    let data=URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("JSON value"));
    format!("{data}.{}",hash(&json!([key,data])))
}
pub fn unseal(key:&str,value:&str)->Result<Value>{
    let(data,tag)=value.split_once('.').ok_or_else(||CoreError::new("INVALID_REFERENCE","引用无效"))?;
    if hash(&json!([key,data]))!=tag{return Err(CoreError::new("INVALID_REFERENCE","引用校验失败"));}
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(data).map_err(|_|CoreError::new("INVALID_REFERENCE","引用编码无效"))?).map_err(Into::into)
}
pub fn coverage(partial:bool,reasons:Vec<&str>)->Value{json!({"state":if partial{"partial"}else{"full"},"reasons":reasons})}
pub fn envelope(caller:&Caller,boot:&str,epoch:&str,kind:&str,name:&str,fields:Value)->Value{
    let now=crate::model::now_ms();
    json!({"eventId":"pending","storeEpoch":epoch,"sequence":"0","schemaVersion":2,"type":name,"typeVersion":1,"kind":kind,"level":"info","occurredAt":now,"receivedAt":now,"committedAt":now,"bootId":boot,"configEpoch":"pending","source":caller,"initiator":{"kind":"unknown","assurance":"unknown"},"provenance":if caller.plugin_id.is_some(){"plugin-reported"}else{"core-observed"},"targets":[],"traceId":null,"spanId":null,"parentSpanId":null,"actionId":null,"callId":null,"operationId":null,"causeEventId":null,"clientEventId":null,"outcome":null,"errorCode":null,"attributes":fields,"completeness":[]})
}
pub fn insert(tx:&rusqlite::Connection,key:&str,partition:&str,event:&mut Value,content_hash:Option<&str>)->Result<()> {
    let seq=super::sequence::next()?;let now=crate::model::now_ms();
    // Short authenticated IDs fit the legacy 128-character eventId contract.
    let nonce=crate::model::id();let unsigned=format!("v2-{partition}-{:x}-{}",seq,&nonce[..8]);
    let tag=hash(&json!([key,event["storeEpoch"],unsigned]));let id=format!("{unsigned}-{}",&tag[..32]);
    event["eventId"]=json!(id);event["sequence"]=json!(seq.to_string());event["committedAt"]=json!(now);
    let s=|name:&str|event[name].as_str().map(str::to_owned);
    tx.execute("INSERT INTO records(event_id,sequence,received,occurred,config_epoch,boot_id,plugin_id,instance_id,kind,event_type,level,outcome,error_code,trace_id,call_id,operation_id,client_id,content_hash,envelope) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",params![id,seq,event["receivedAt"].as_u64().unwrap_or(now)as i64,event["occurredAt"].as_u64().unwrap_or(now)as i64,s("configEpoch"),s("bootId"),event["source"]["pluginId"].as_str(),event["source"]["instanceId"].as_str().unwrap_or("core"),s("kind"),s("type"),s("level"),s("outcome"),s("errorCode"),s("traceId"),s("callId"),s("operationId"),s("clientEventId"),content_hash,event.to_string()])?;
    for target in event["targets"].as_array().into_iter().flatten(){tx.execute("INSERT OR IGNORE INTO record_targets VALUES(?,?,?,?)",params![id,target["kind"].as_str(),target["ref"].as_str(),target["generation"].as_str()])?;}
    tx.execute("UPDATE meta SET value=? WHERE key='sequence'",[seq.to_string()])?;Ok(())
}
pub fn valid_id(key:&str,epoch:&str,id:&str)->bool{
    let Some((unsigned,tag))=id.rsplit_once('-')else{return false};
    unsigned.starts_with("v2-")&&tag==&hash(&json!([key,epoch,unsigned]))[..32]
}

/// Conservative allocation accounting for retained JSON, including collection nodes.
pub(super) fn memory_bytes(value:&Value)->usize{
    std::mem::size_of::<Value>()+match value{
        Value::String(s)=>s.capacity(),
        Value::Array(a)=>a.capacity()*std::mem::size_of::<Value>()+a.iter().map(memory_bytes).sum::<usize>(),
        Value::Object(o)=>o.iter().map(|(key,v)|128+key.capacity()+memory_bytes(v)).sum(),
        _=>0,
    }
}
