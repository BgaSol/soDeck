use super::*;
use crate::server_catalog_store as catalog;
use rusqlite::params;
use sha2::{Digest,Sha256};
#[path="server_catalog_contract.generated.rs"]
mod contract;

fn digest(value:&Value)->String {format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap()))}
fn caller_key(caller:&Caller)->String {format!("{}:{}",caller.source,caller.plugin_id.as_deref().unwrap_or("@shell"))}

impl Core {
    pub fn catalog_revision(&self)->Result<String> {catalog::revision(&self.store.db.lock().unwrap())}
    pub(crate) fn catalog_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        contract::validate("x-methods",method,p)?;
        let _guard=self.configuration.lock().unwrap();
        let mut db=self.store.db.lock().unwrap();
        let key=caller_key(caller);
        let result=match method {
            "servers.catalog.get"=>self.catalog_project(catalog::item(&db,field(p,"serverId")?)?),
            "serverGroups.list"|"servers.catalog.list"=>{
                let revision=catalog::revision(&db)?;
                let mut query=p.clone();query.as_object_mut().unwrap().remove("cursor");
                let binding=digest(&json!([method,key,query]));
                let offset=if let Some(cursor)=p["cursor"].as_str() {
                    let decoded:Value=serde_json::from_slice(&STANDARD.decode(cursor).map_err(|_|CoreError::new("INVALID_PARAMS","目录游标无效"))?)?;
                    if decoded["revision"]!=revision||decoded["binding"]!=binding {return Err(CoreError::new("STALE_CURSOR","目录已变化，请重新获取"));}
                    decoded["offset"].as_u64().ok_or_else(||CoreError::new("INVALID_PARAMS","目录游标无效"))? as usize
                } else {0};
                let mut rows=Vec::new();
                if method=="serverGroups.list" {
                    let mut q=db.prepare("SELECT id FROM server_groups ORDER BY name_key,id")?;
                    for id in q.query_map([],|r|r.get::<_,String>(0))? {rows.push(catalog::group(&db,&id?)?);}
                } else {
                    let search=p["query"].as_str().unwrap_or("").trim().to_lowercase();
                    let mut q=db.prepare("SELECT id FROM servers ORDER BY json_extract(data,'$.name') COLLATE NOCASE,id")?;
                    for id in q.query_map([],|r|r.get::<_,String>(0))? {
                        let item=self.catalog_project(catalog::item(&db,&id?)?);
                        if p.get("groupId").is_some_and(|g|*g!=item["groupId"]) {continue;}
                        let group=item["groupId"].as_str().map(|id|catalog::group(&db,id)).transpose()?;
                        let s=&item["server"];
                        let searchable=format!("{} {} {} {}",s["name"].as_str().unwrap(),s["host"].as_str().unwrap(),s["username"].as_str().unwrap(),group.as_ref().and_then(|g|g["name"].as_str()).unwrap_or("")).to_lowercase();
                        if searchable.contains(&search) {rows.push(item);}
                    }
                }
                if offset>rows.len() {return Err(CoreError::new("STALE_CURSOR","目录游标已失效"));}
                let total=rows.len();let limit=p["limit"].as_u64().unwrap_or(100) as usize;
                let mut page=Vec::new();let mut bytes=1024;
                for item in rows.into_iter().skip(offset).take(limit) {
                    let size=serde_json::to_vec(&item)?.len();
                    if bytes+size>32*1024 {break;}
                    bytes+=size;page.push(item);
                }
                let next=offset+page.len();
                let cursor=(next<total).then(||STANDARD.encode(serde_json::to_vec(&json!({"revision":revision,"binding":binding,"offset":next})).unwrap()));
                if method=="serverGroups.list" {json!({"groups":page,"catalogRevision":revision,"nextCursor":cursor})}else{json!({"items":page,"catalogRevision":revision,"nextCursor":cursor})}
            },
            "servers.catalog.operation"=>{
                match catalog::receipt(&db,field(p,"operationId")?,&key,None)? {
                    None=>json!({"status":"unknown"}),
                    Some(receipt)=>{
                        if receipt["value"].get("server").is_none() {return Err(CoreError::new("OPERATION_CONFLICT","此操作不是服务器保存"));}
                        json!({"status":"committed","result":self.catalog_project(receipt["value"].clone())})
                    }
                }
            },
            "serverGroups.create"|"serverGroups.update"|"serverGroups.delete"|"servers.catalog.save"=>{
                let operation=field(p,"operationId")?;
                let hash=digest(&json!([method,p]));
                let receipt=if let Some(receipt)=catalog::receipt(&db,operation,&key,Some(&hash))? {receipt} else {
                    let tx=db.transaction()?;
                    let (value,pending)=if method=="servers.catalog.save" {catalog::save(&tx,p)?}else{(catalog::group_write(&tx,method,p)?,vec![])};
                    catalog::insert_receipt(&tx,operation,&key,&hash,&value,&pending)?;
                    tx.commit()?;
                    json!({"value":value,"pendingSecrets":pending})
                };
                let mut value=receipt["value"].clone();
                let pending=receipt["pendingSecrets"].as_array().unwrap();
                for id in pending {
                    let server_id=id.as_str().unwrap();
                    self.secrets.remove(server_id).map_err(|e|e.details(json!({"committed":true,"serverId":value["server"]["id"],"operationId":operation,"stage":"credentialCleanup"})))?;
                    *self.credential_revisions.lock().unwrap().entry(server_id.into()).or_default()+=1;
                }
                if !pending.is_empty() {
                    value["postCommitPending"]=json!(false);
                    db.execute("UPDATE server_catalog_receipts SET result=? WHERE operation_id=?",params![serde_json::to_string(&json!({"value":value,"pendingSecrets":[]}))?,operation])?;
                }
                if method=="servers.catalog.save" {self.catalog_project(value)}else{value}
            },
            _=>return Err(CoreError::new("METHOD_NOT_FOUND","未知服务器目录方法")),
        };
        contract::validate("x-results",method,&result)?;
        Ok(result)
    }
    fn catalog_project(&self,mut value:Value)->Value {
        if let Some(id)=value["server"]["id"].as_str().map(str::to_owned) {value["server"]["hasSecret"]=json!(self.secrets.remembered(&id));}
        value
    }
}
