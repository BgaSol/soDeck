//! Sequence-based pages keep a stable high watermark without pinning a WAL reader.
use super::{schema::Partition,Observability};
use crate::{CoreError,Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD,Engine};
use rusqlite::{params_from_iter,types::Value as Sql,OptionalExtension,Row};
use serde_json::{json,Value};

#[derive(serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Cursor {store_epoch:String,fingerprint:String,before:i64,watermark:i64,prune:u64,expires:u64}
fn expired()->CoreError {CoreError::new("CURSOR_EXPIRED","事件查询已过期或记录已清理，请重新查询")}
fn number(db:&rusqlite::Connection,key:&str)->Result<u64> {
    let value:String=db.query_row("SELECT value FROM meta WHERE key=?",[key],|r|r.get(0))?;
    value.parse().map_err(|_|CoreError::new("STORE_UNAVAILABLE","观测元数据损坏"))
}
const COLUMNS:&str="event_id,sequence,time,kind,event_type,source,boot_id,config_epoch,target,call_id,trace_id,outcome,payload";
fn event(row:&Row<'_>)->rusqlite::Result<Value> {
    let source:String=row.get(5)?;let payload:String=row.get(12)?;
    let parse=|s:&str|serde_json::from_str::<Value>(s).map_err(|e|rusqlite::Error::FromSqlConversionFailure(12,rusqlite::types::Type::Text,Box::new(e)));
    Ok(json!({"eventId":row.get::<_,String>(0)?,"sequence":super::integer(row,1)?.to_string(),"time":super::integer(row,2)?,"kind":row.get::<_,String>(3)?,"type":row.get::<_,String>(4)?,"source":parse(&source)?,
        "bootId":row.get::<_,String>(6)?,"configEpoch":row.get::<_,String>(7)?,"serverId":row.get::<_,Option<String>>(8)?,"callId":row.get::<_,Option<String>>(9)?,"traceId":row.get::<_,Option<String>>(10)?,"outcome":row.get::<_,Option<String>>(11)?,"attributes":parse(&payload)?}))
}
fn filters(p:&Value)->Result<(String,Vec<Sql>)> {
    let from=p["fromUtc"].as_u64().unwrap();let to=p["toUtc"].as_u64().unwrap();
    if to<=from||to-from>30*24*60*60*1000{return Err(CoreError::new("INVALID_FILTER","事件查询范围必须大于零且不超过30天"));}
    let mut sql="config_epoch=? AND time>=? AND time<?".to_owned();let mut args=vec![Sql::Text(p["configEpoch"].as_str().unwrap().into()),Sql::Integer(from as i64),Sql::Integer(to as i64)];
    for (field,column) in [("serverId","target"),("bootId","boot_id"),("kind","kind"),("type","event_type"),("outcome","outcome"),("callId","call_id"),("traceId","trace_id")] {
        if let Some(value)=p[field].as_str(){sql.push_str(&format!(" AND {column}=?"));args.push(Sql::Text(value.into()));}
    }
    Ok((sql,args))
}
impl Observability {
    fn cursor(&self,cursor:&Cursor)->Result<String> {
        let encoded=URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor)?);
        let seal=crate::route::digest(&json!([self.cursor_key,encoded]));Ok(format!("{encoded}.{seal}"))
    }
    fn decode_cursor(&self,value:&str)->Result<Cursor> {
        let (encoded,seal)=value.split_once('.').ok_or_else(expired)?;
        // The checksum binds the canonical envelope to this store process. It
        // carries no authorization and can never be used as a resource handle.
        if crate::route::digest(&json!([self.cursor_key,encoded]))!=seal{return Err(expired());}
        let cursor:Cursor=serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).map_err(|_|expired())?).map_err(|_|expired())?;
        if cursor.store_epoch!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测数据集已重建"));}
        if cursor.expires<crate::model::now_ms()||cursor.before<0||cursor.before>cursor.watermark{return Err(expired());}Ok(cursor)
    }
    pub fn events(&self,owner:&str,p:&Value)->Result<Value> {
        let (filter,args)=filters(p)?;
        let mut query=p.clone();query.as_object_mut().unwrap().remove("cursor");query.as_object_mut().unwrap().remove("limit");
        let fingerprint=crate::route::digest(&json!([owner,query]));
        let cursor=p["cursor"].as_str().map(|value|self.decode_cursor(value)).transpose()?;
        if cursor.as_ref().is_some_and(|c|c.fingerprint!=fingerprint){return Err(expired());}
        let epoch=self.epoch.clone();let config=p["configEpoch"].as_str().unwrap().to_owned();let limit=p["limit"].as_u64().unwrap_or(100)as usize;
        let mut result=self.queries.query(Partition::Facts,move|db|{
            let tx=db.unchecked_transaction()?;let watermark=i64::try_from(number(&tx,"sequence")?).map_err(|_|CoreError::new("STORE_UNAVAILABLE","观测提交序号损坏"))?;let prune=number(&tx,"pruneRevision")?;
            let cursor=cursor.unwrap_or(Cursor{store_epoch:epoch.clone(),fingerprint,before:watermark,watermark,prune,expires:crate::model::now_ms()+300000});
            if cursor.prune!=prune||cursor.watermark>watermark{return Err(expired());}
            let mut arguments=args;arguments.push(Sql::Integer(cursor.before));arguments.push(Sql::Integer((limit+1)as i64));
            let sql=format!("SELECT {COLUMNS} FROM events WHERE {filter} AND sequence<=? ORDER BY sequence DESC LIMIT ?");
            let mut statement=tx.prepare(&sql)?;let rows=statement.query_map(params_from_iter(arguments),event)?;
            let mut items=Vec::new();let mut bytes=2048;let mut more=false;
            for row in rows {let row=row?;let size=serde_json::to_vec(&row)?.len()+64;
                if items.len()==limit||bytes+size>256*1024{more=true;break;}bytes+=size;items.push(row);
            }
            let retained=tx.query_row("SELECT MIN(time),MAX(time) FROM events WHERE config_epoch=?",[config],|r|Ok((super::optional_integer(r,0)?,super::optional_integer(r,1)?)))?;
            let next=if more {items.last().map(|v|Cursor{before:v["sequence"].as_str().unwrap().parse::<i64>().unwrap()-1,..cursor})}else{None};
            Ok(json!({"storeEpoch":epoch,"highWatermark":cursor.watermark.to_string(),"retainedFromUtc":retained.0,"retainedToUtc":retained.1,"coverage":if prune>0{"partial"}else{"complete"},"items":items,"nextCursor":next.map(|c|serde_json::to_value(c).unwrap())}))
        })?;
        if !result["nextCursor"].is_null(){result["nextCursor"]=json!(self.cursor(&serde_json::from_value(result["nextCursor"].take())?)?);}
        Ok(result)
    }
    pub fn event(&self,p:&Value)->Result<Value> {
        if p["storeEpoch"]!=self.epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测数据集已重建"));}
        let id=p["eventId"].as_str().unwrap().to_owned();let expected=p["sequence"].as_str().and_then(|s|s.parse::<u64>().ok());
        self.queries.query(Partition::Facts,move|db|{
            let tx=db.unchecked_transaction()?;
            let value=tx.query_row(&format!("SELECT {COLUMNS} FROM events WHERE event_id=?"),[id],event).optional()?;
            match value {Some(value)=>Ok(value),None=>{
                let watermark=number(&tx,"sequence")?;
                let known=expected.is_some_and(|s|s<=watermark);
                Err(CoreError::new(if known{"GONE"}else{"NOT_FOUND"},"事件不存在或已超出留存范围"))
            }}
        })
    }
    pub fn aggregate_events(&self,p:&Value)->Result<Value> {
        let (filter,args)=filters(p)?;
        let group=match p["groupBy"].as_str().unwrap_or("type") {"type"=>"event_type","outcome"=>"COALESCE(outcome,'unknown')","server"=>"COALESCE(target,'@all')","minute"=>"CAST(time/60000 AS TEXT)","hour"=>"CAST(time/3600000 AS TEXT)",_=>return Err(CoreError::new("INVALID_FILTER","不支持的事件分组"))};
        let epoch=self.epoch.clone();
        self.queries.query(Partition::Facts,move|db|{
            let tx=db.unchecked_transaction()?;let watermark=number(&tx,"sequence")?;let prune=number(&tx,"pruneRevision")?;
            let mut statement=tx.prepare(&format!("SELECT {group},COUNT(*) FROM events WHERE {filter} GROUP BY {group} ORDER BY {group} LIMIT 201"))?;
            let items=statement.query_map(params_from_iter(args),|r|Ok(json!({"key":r.get::<_,String>(0)?,"count":super::integer(r,1)?.to_string()})))?.collect::<std::result::Result<Vec<_>,_>>()?;
            if items.len()>200{return Err(CoreError::new("CAPACITY","事件分组超过200项，请缩小范围"));}
            Ok(json!({"storeEpoch":epoch,"highWatermark":watermark.to_string(),"coverage":if prune>0{"partial"}else{"complete"},"groups":items}))
        })
    }
}

#[cfg(test)]mod tests {
    use super::*;
    use crate::{Caller,Core};
    fn insert(store:&Observability,count:usize,time:u64) {
        store.commit(128*1024,move|db|{let tx=db.facts.transaction()?;
            for _ in 0..count {super::super::calls::event(&tx,"quality.fixture","",time,"{}","boot","epoch",Some("server"),Some("succeeded"),&json!({"count":1}))?;}
            tx.commit()?;Ok(Value::Null)
        }).unwrap();
    }
    #[test]fn pages_keep_their_watermark_and_reject_modified_foreign_or_pruned_cursors(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        insert(&store,205,1000);let p=json!({"configEpoch":"epoch","kind":"domain","serverId":"server","fromUtc":0,"toUtc":10000,"limit":100});
        let first=store.events("owner",&p).unwrap();assert_eq!(first["items"].as_array().unwrap().len(),100);
        let mut next=p.clone();next["cursor"]=first["nextCursor"].clone();
        assert_eq!(store.events("another",&next).unwrap_err().code,"CURSOR_EXPIRED");
        let mut tampered=next.clone();tampered["cursor"]=json!(format!("{}x",next["cursor"].as_str().unwrap()));assert_eq!(store.events("owner",&tampered).unwrap_err().code,"CURSOR_EXPIRED");
        insert(&store,5,500);let second=store.events("owner",&next).unwrap();assert_eq!(first["highWatermark"],second["highWatermark"]);
        next["cursor"]=second["nextCursor"].clone();let third=store.events("owner",&next).unwrap();assert_eq!(third["items"].as_array().unwrap().len(),5);assert!(third["nextCursor"].is_null());
        let ids:std::collections::HashSet<_>=[&first,&second,&third].iter().flat_map(|p|p["items"].as_array().unwrap().iter().map(|i|i["eventId"].as_str().unwrap())).collect();assert_eq!(ids.len(),205);
        let aggregate=store.aggregate_events(&p).unwrap();assert_eq!(aggregate["groups"][0]["count"],"210");
        let item=third["items"][4].clone();
        assert_eq!(store.event(&json!({"storeEpoch":store.epoch,"eventId":item["eventId"]})).unwrap(),item);
        let removed=item["eventId"].as_str().unwrap().to_owned();store.commit(1024,move|db|{db.facts.execute("DELETE FROM events WHERE event_id=?",[removed])?;Ok(Value::Null)}).unwrap();
        assert_eq!(store.event(&json!({"storeEpoch":store.epoch,"eventId":item["eventId"],"sequence":item["sequence"]})).unwrap_err().code,"GONE");
        assert_eq!(store.events("owner",&next).unwrap_err().code,"CURSOR_EXPIRED");assert_eq!(store.events("owner",&p).unwrap()["coverage"],"partial");assert!(store.shutdown());
    }
    #[test]fn health_survives_audit_failure_and_recovers_only_after_a_successful_write(){
        let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let caller=Caller::shell();
        assert_eq!(core.call(&caller,"observability.health.get",json!({})).unwrap()["state"],"active");
        core.observability.fail_writes(true);
        let health=core.call(&caller,"observability.health.get",json!({})).unwrap();assert_eq!(health["state"],"degraded");assert!(health["failedWrites"].as_str().unwrap().parse::<u64>().unwrap()>0);
        core.observability.fail_writes(false);assert_eq!(core.observability.health()["state"],"degraded");
        let recovered=core.call(&caller,"observability.health.get",json!({})).unwrap();assert_eq!(recovered["state"],"active");assert!(!recovered["lastFailureAt"].is_null());assert!(!recovered["lastSuccessAt"].is_null());
        assert_eq!(recovered["partitions"].as_array().unwrap().iter().map(|p|p["budgetBytes"].as_u64().unwrap()).sum::<u64>(),512*1024*1024);
    }
}
