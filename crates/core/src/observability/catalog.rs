use super::{descriptor::{Descriptor,Definition},schema::Partition,Observability};
use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Catalog {pub sources:BTreeMap<String,(String,String,Option<Descriptor>)>,pub bytes:usize,pub dropped:u64,pub rejected:u64}
impl Core {
    /// Called only by the package host after integrity verification, never by plugin RPC.
    pub fn register_observation_source(&self,caller:&Caller,descriptor:Option<&[u8]>)->Result<()> {
        let plugin=caller.plugin_id.as_deref().ok_or_else(||CoreError::new("INVALID_SOURCE","需要插件身份"))?;
        let parsed=descriptor.map(|bytes|Descriptor::parse(bytes,plugin).map_err(|e|CoreError::new("INVALID_DESCRIPTOR",e))).transpose()?;
        let bytes=parsed.as_ref().map(|v|serde_json::to_vec(v).map(|v|v.len())).transpose()?.unwrap_or(0);let mut catalog=self.observability.catalog.lock().unwrap();
        if let Some(ref candidate)=parsed {
            for (category,definition) in candidate.definitions(){
                let data=serde_json::to_string(definition)?;
                for (other_plugin,_,other) in catalog.sources.values(){
                    if other_plugin==plugin {if let Some(other)=other {if other.definitions().any(|(c,d)|c==category&&d.name==definition.name&&d.type_version==definition.type_version&&serde_json::to_string(d).ok().as_ref()!=Some(&data)){return Err(CoreError::new("CONFLICT","同一类型版本的描述已改变"));}}}
                }
            }
            for partition in [Partition::Facts,Partition::Logs,Partition::Diagnostics] {
                let candidate=candidate.clone();let plugin=plugin.to_owned();
                self.observability.queries.query(partition,move|db|{
                    use rusqlite::OptionalExtension;
                    for(category,d)in candidate.definitions(){let previous:Option<String>=db.query_row("SELECT descriptor FROM type_catalog WHERE plugin_id=? AND category=? AND name=? AND version=?",rusqlite::params![plugin,category,d.name,d.type_version],|r|r.get(0)).optional()?;
                        // Compare the same canonical representation save_type writes.
                        // Parsing persisted f64 bounds can round large integers and
                        // falsely reject an unchanged descriptor on the next launch.
                        let serialized=serde_json::to_string(d)?;
                        if previous.is_some_and(|v|v!=serialized){return Err(CoreError::new("CONFLICT","历史类型版本的描述已改变"));}}
                    Ok(Value::Null)
                })?;
            }
        }
        let previous=catalog.sources.get(&caller.instance_id).and_then(|s|s.2.as_ref()).and_then(|s|serde_json::to_vec(s).ok()).map_or(0,|s|s.len());
        if catalog.bytes.saturating_sub(previous)+bytes>4*1024*1024||(!catalog.sources.contains_key(&caller.instance_id)&&catalog.sources.len()>=256){return Err(CoreError::new("CAPACITY","采集目录已满"));}
        catalog.bytes=catalog.bytes.saturating_sub(previous)+bytes;catalog.sources.insert(caller.instance_id.clone(),(plugin.into(),caller.plugin_version.clone().unwrap_or_default(),parsed));Ok(())
    }
}
impl Observability {
    pub(super) fn definition(&self,caller:&Caller,category:&str,name:&str,version:u32)->Result<Definition>{
        let catalog=self.catalog.lock().unwrap();
        let desc=catalog.sources.get(&caller.instance_id).and_then(|v|v.2.as_ref()).ok_or_else(||CoreError::new("TYPE_UNSUPPORTED","来源未声明采集目录"))?;
        let result=desc.definitions().find(|(c,d)|*c==category&&d.name==name&&d.type_version==version).map(|(_,d)|d.clone()).ok_or_else(||CoreError::new("TYPE_UNSUPPORTED","未声明的采集类型或版本"));result
    }
    pub(super) fn types(&self,p:&Value,one:bool)->Result<Value>{
        let _budget=super::query::ReadBudget::enter();
        let mut filter=p.clone();filter.as_object_mut().unwrap().remove("cursor");let fingerprint=super::model::hash(&filter);let now=crate::model::now_ms();
        let after=if let Some(token)=p["cursor"].as_str(){let value=super::model::unseal(&self.cursor_key,token)?;if value["filter"]!=fingerprint||value["expires"].as_u64().unwrap_or(0)<=now{return Err(CoreError::new("CURSOR_EXPIRED","类型目录游标失效"));}value["after"].as_str().unwrap_or("").to_owned()}else{String::new()};
        let limit=p["limit"].as_u64().unwrap_or(100).min(24)as usize;let mut all=BTreeMap::new();
        let key=|row:&Value|format!("{}:{}:{}:{:05}",row["pluginId"].as_str().unwrap(),row["category"].as_str().unwrap(),row["type"].as_str().unwrap(),row["typeVersion"].as_u64().unwrap());
        for partition in [Partition::Facts,Partition::Logs,Partition::Diagnostics]{let f=filter.clone();let after=after.clone();
            let rows=self.queries.query(partition,move|db|{
                let mut sql="(plugin_id||':'||category||':'||name||':'||printf('%05d',version))>?".to_owned();let mut args=vec![rusqlite::types::Value::Text(after)];
                for (field,column) in [("pluginId","plugin_id"),("category","category"),("type","name")]{if let Some(v)=f[field].as_str(){sql.push_str(&format!(" AND {column}=?"));args.push(v.to_owned().into());}}
                if let Some(version)=f["typeVersion"].as_u64(){sql.push_str(" AND version=?");args.push((version as i64).into());}
                args.push((limit as i64+1).into());let mut statement=db.prepare(&format!("SELECT plugin_id,category,name,version,descriptor FROM type_catalog WHERE {sql} ORDER BY plugin_id,category,name,version LIMIT ?"))?;
                let rows=statement.query_map(rusqlite::params_from_iter(args),|r|Ok(json!({"pluginId":r.get::<_,String>(0)?,"category":r.get::<_,String>(1)?,"type":r.get::<_,String>(2)?,"typeVersion":r.get::<_,u32>(3)?,"definition":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(Value::Null)})))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!(rows))
            })?;for row in rows.as_array().unwrap(){all.insert(key(row),row.clone());}
        }
        {let catalog=self.catalog.lock().unwrap();for(plugin,_,desc)in catalog.sources.values(){if let Some(desc)=desc{for(category,d)in desc.definitions(){let row=json!({"pluginId":plugin,"category":category,"type":d.name,"typeVersion":d.type_version,"definition":d});let k=key(&row);
            if k>after&&["pluginId","category","type","typeVersion"].iter().all(|field|filter.get(*field).is_none_or(|v|v==&row[*field])){all.entry(k).or_insert(row);if all.len()>limit+1{all.pop_last();}}
        }}}}
        let mut matching=all.into_iter();if one{return matching.next().map(|(_,v)|v).ok_or_else(||CoreError::new("NOT_FOUND","采集类型不存在"));}
        let mut last=after;let items:Vec<_>=matching.by_ref().take(limit).map(|(key,value)|{last=key;value}).collect();let next=matching.next().map(|_|super::model::seal(&self.cursor_key,&json!({"after":last,"filter":fingerprint,"expires":now+300000})));
        Ok(json!({"items":items,"nextCursor":next}))
    }
}
pub(super) fn save_type(tx:&rusqlite::Connection,plugin:&str,category:&str,d:&Definition)->Result<()> {
    let data=serde_json::to_string(d)?;
    use rusqlite::OptionalExtension;
    let previous:Option<String>=tx.query_row("SELECT descriptor FROM type_catalog WHERE plugin_id=? AND category=? AND name=? AND version=?",rusqlite::params![plugin,category,d.name,d.type_version],|r|r.get(0)).optional()?;
    if previous.as_ref().is_some_and(|s|s!=&data){return Err(CoreError::new("CONFLICT","同一类型版本的描述已改变"));}
    tx.execute("INSERT OR IGNORE INTO type_catalog VALUES(?,?,?,?,?)",rusqlite::params![plugin,category,d.name,d.type_version,data])?;Ok(())
}
