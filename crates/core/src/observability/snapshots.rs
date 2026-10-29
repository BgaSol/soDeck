use super::{descriptor,ingest,model,schema::Partition,Observability};
use crate::{Caller,CoreError,Result};
use serde_json::{json,Value};
use rusqlite::{params,OptionalExtension};
impl Observability {
    pub(super) fn snapshot_report(&self,caller:&Caller,p:&Value)->Result<Value>{
        let _admission=ingest::admission(self,caller,1)?;
        let d=self.definition(caller,"snapshots",p["snapshotType"].as_str().unwrap(),p["typeVersion"].as_u64().unwrap()as u32)?;
        descriptor::check_fields(&d.fields,&p["fields"]).map_err(|e|CoreError::new("INVALID_EVENT",e))?;
        let bytes=p.to_string().len();if bytes>32768{return Err(CoreError::new("CAPACITY","快照超过32KiB"));}
        let p=p.clone();let caller=caller.clone();let boot=self.boot_id.clone();
        self.write(Partition::Diagnostics,bytes+8192,move|db|{db.admit(Partition::Diagnostics,bytes+8192)?;let tx=db.diagnostics.transaction()?;let now=crate::model::now_ms();
            let prior:Option<(String,String,String,u64,usize)>=tx.query_row("SELECT id,revision,content_hash,received,length(data) FROM observation_snapshots WHERE instance_id=? AND scope=? AND type=?",params![caller.instance_id,p["scope"].as_str(),d.name],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,super::integer(r,3)?,r.get::<_,i64>(4)? as usize))).optional()?;
            let hash=model::hash(&p);let revision=p["producerRevision"].as_str().unwrap().parse::<u64>().map_err(|_|CoreError::new("INVALID_PARAMS","快照修订无效"))?;
            if let Some((id,rev,old,received,_))=&prior{let old_revision=rev.parse::<u64>().unwrap_or(0);
                if revision<old_revision{return Ok(json!({"storedSnapshotId":id,"receivedAt":received,"status":"ignored"}));}
                if revision==old_revision{if old!=&hash{return Err(CoreError::new("CONFLICT","快照修订内容冲突"));}return Ok(json!({"storedSnapshotId":id,"receivedAt":received,"status":"duplicate"}));}
                if now<received.saturating_add(5000){return Err(CoreError::new("CAPACITY","快照目标每5秒最多更新一次"));}
            }
            let targets:u32=tx.query_row("SELECT count(*) FROM observation_snapshots WHERE instance_id=?",[&caller.instance_id],|r|r.get(0))?;
            if prior.is_none()&&targets>=16{return Err(CoreError::new("CAPACITY","来源快照目标已满"));}
            let total:usize=tx.query_row("SELECT COALESCE(sum(length(data)),0) FROM observation_snapshots",[],|r|Ok(r.get::<_,i64>(0)? as usize))?;
            let id=prior.as_ref().map(|v|v.0.clone()).unwrap_or_else(crate::model::id);let expires=now+d.stale_after_ms.unwrap_or(300000);
            let data=json!({"snapshotId":id,"source":caller,"bootId":boot,"type":d.name,"typeVersion":d.type_version,"scope":p["scope"],"producerRevision":p["producerRevision"],"observedAt":p["observedAt"],"receivedAt":now,"expiresAt":expires,"fields":p["fields"],"definition":d});
            if total.saturating_sub(prior.as_ref().map_or(0,|v|v.4))+data.to_string().len()>8*1024*1024{return Err(CoreError::new("CAPACITY","最新快照总量已满"));}
            super::catalog::save_type(&tx,caller.plugin_id.as_deref().unwrap_or(""),"snapshots",&d)?;
            tx.execute("INSERT OR REPLACE INTO observation_snapshots VALUES(?,?,?,?,?,?,?,?,?,?)",params![id,caller.instance_id,caller.plugin_id.unwrap_or_default(),p["scope"].as_str(),d.name,revision.to_string(),now as i64,expires as i64,hash,data.to_string()])?;tx.commit()?;Ok(json!({"storedSnapshotId":id,"receivedAt":now,"status":"stored"}))
        })
    }
    pub(super) fn snapshot_get(&self,p:&Value)->Result<Value>{let id=p["snapshotId"].as_str().unwrap().to_owned();let expected=p.get("producerRevision").cloned();let boot=self.boot_id.clone();
        self.queries.query(Partition::Diagnostics,move|db|{let data:Option<String>=db.query_row("SELECT data FROM observation_snapshots WHERE id=?",[id],|r|r.get(0)).optional()?;let mut v:Value=serde_json::from_str(&data.ok_or_else(||CoreError::new("NOT_FOUND","快照不存在"))?)?;v["state"]=json!(if expected.as_ref().is_some_and(|r|r!=&v["producerRevision"]){"superseded"}else if v["bootId"]!=boot||v["ended"]==true{"ended"}else if v["expiresAt"].as_u64().unwrap_or(0)<=crate::model::now_ms(){"stale"}else{"current"});Ok(v)})
    }
    pub(super) fn data_query(&self,p:&Value,metrics:bool)->Result<Value>{
        let _budget=super::query::ReadBudget::enter();
        let p=p.clone();let boot=self.boot_id.clone();let key=self.cursor_key.clone();let filter_hash=model::hash(&{let mut p=p.clone();p.as_object_mut().unwrap().remove("cursor");p});
        let offset=if let Some(token)=p["cursor"].as_str(){let value=model::unseal(&key,token)?;if value["filter"]!=filter_hash||value["expires"].as_u64().unwrap_or(0)<crate::model::now_ms(){return Err(CoreError::new("CURSOR_EXPIRED","数据查询已过期"));}value["offset"].as_u64().unwrap_or(0)}else{0};
        let expected=p["cursor"].as_str().map(|token|model::unseal(&key,token)).transpose()?.map(|v|v["revision"].clone());
        self.queries.query(Partition::Diagnostics,move|db|{let tx=db.unchecked_transaction()?;let db=&tx;let revision=super::history::number(db,"sequence")?;if expected.is_some_and(|v|v!=revision){return Err(CoreError::new("CURSOR_EXPIRED","现场或指标版本在分页期间变化"));}let mut sql="1=1".to_string();let mut args:Vec<rusqlite::types::Value>=vec![];
            for(field,column)in [("pluginId","plugin_id"),("scope","scope"),("type","type")]{if let Some(s)=p[field].as_str(){sql.push_str(&format!(" AND {column}=?"));args.push(s.to_owned().into());}}
            let column=if metrics{"minute"}else{"received"};for(field,op)in [("fromUtc",">="),("toUtc","<")]{if let Some(n)=p[field].as_u64(){sql.push_str(&format!(" AND {column}{op}?"));args.push((n as i64).into());}}
            let limit=p["limit"].as_u64().unwrap_or(100).min(200);args.push((limit as i64+1).into());args.push((offset as i64).into());
            let mut stmt=db.prepare(&format!("SELECT data FROM {} WHERE {sql} ORDER BY {column},{} LIMIT ? OFFSET ?",if metrics{"metric_buckets"}else{"observation_snapshots"},if metrics{"series"}else{"id"}))?;
            let rows=stmt.query_map(rusqlite::params_from_iter(args),|r|r.get::<_,String>(0))?;let mut values=Vec::new();let mut bytes=2048;let mut more=false;
            for row in rows{let row=row?;if values.len()>=limit as usize||bytes+row.len()>256*1024{more=true;break;}bytes+=row.len();let mut v:Value=serde_json::from_str(&row)?;if !metrics{v["state"]=json!(if v["bootId"]!=boot||v["ended"]==true{"ended"}else if v["expiresAt"].as_u64().unwrap_or(0)<=crate::model::now_ms(){"stale"}else{"current"});}values.push(v);}
            let next=more.then(||model::seal(&key,&json!({"offset":offset+values.len()as u64,"revision":revision,"filter":filter_hash,"expires":crate::model::now_ms()+300000})));
            Ok(json!({"items":values,"nextCursor":next,"coverage":model::coverage(true,vec!["reported-samples-only"])}))
        })
    }
}
