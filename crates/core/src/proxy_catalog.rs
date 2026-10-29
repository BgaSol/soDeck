use super::*;
use crate::{configuration_receipts as receipts,server_catalog_store as catalog,route::SocksProxy};
use rusqlite::params;
#[path="proxy_catalog_contract.generated.rs"] mod contract;

impl Core {
    pub(crate) fn proxy_catalog_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        contract::validate("x-methods",method,p)?;
        let _guard=self.configuration.lock().unwrap();
        let key=format!("{}:{}",caller.source,caller.plugin_id.as_deref().unwrap_or("@shell"));
        let mut db=self.store.db.lock().unwrap();
        let result=match method {
            "proxies.catalog.list"=>{
                let revision=catalog::revision(&db)?;
                let mut query=p.clone();query.as_object_mut().unwrap().remove("cursor");
                let binding=crate::route::digest(&json!([method,key,query]));
                let offset=if let Some(cursor)=p["cursor"].as_str(){
                    let cursor:Value=serde_json::from_slice(&STANDARD.decode(cursor).map_err(|_|CoreError::new("INVALID_PARAMS","目录游标无效"))?)?;
                    if cursor["revision"]!=revision||cursor["binding"]!=binding{return Err(CoreError::new("STALE_CURSOR","目录已变化，请重新获取"));}
                    cursor["offset"].as_u64().ok_or_else(||CoreError::new("INVALID_PARAMS","目录游标无效"))? as usize
                }else{0};
                let search=p["query"].as_str().unwrap_or("").trim().to_lowercase();
                let mut q=db.prepare("SELECT id,(SELECT count(DISTINCT target_server_id) FROM server_route_steps r WHERE r.proxy_id=p.id) FROM socks_proxies p ORDER BY json_extract(data,'$.name') COLLATE NOCASE,id")?;
                let mut rows=vec![];
                for row in q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?{
                    let (id,count)=row?;let mut proxy=crate::route_store::proxy(&db,&id)?;
                    if !format!("{} {} {}",proxy.name,proxy.host,proxy.username).to_lowercase().contains(&search){continue;}
                    proxy.has_preset=self.secrets.has_preset(&db,"socks5",&id,&crate::route::identity::proxy_identity(&proxy))?;
                    rows.push(json!({"proxy":proxy,"referenceCount":count}));
                }
                if offset>rows.len(){return Err(CoreError::new("STALE_CURSOR","目录游标已失效"));}
                let total=rows.len();let mut items=vec![];let mut bytes=1024;
                for item in rows.into_iter().skip(offset).take(p["limit"].as_u64().unwrap_or(100) as usize){let size=serde_json::to_vec(&item)?.len();if bytes+size>32768{break;}bytes+=size;items.push(item);}
                let next=offset+items.len();
                json!({"items":items,"catalogRevision":revision,"nextCursor":(next<total).then(||STANDARD.encode(json!({"revision":revision,"binding":binding,"offset":next}).to_string()))})
            },
            "proxies.catalog.operation"=>{
                let operation=format!("@proxy:{}",field(p,"operationId")?);
                match receipts::receipt(&db,&operation,&key,None)?{
                    None=>json!({"status":"unknown"}),
                    Some(r)=>{if r["value"]["type"]!="proxyCatalog"{return Err(CoreError::new("OPERATION_CONFLICT","此操作不是代理目录写入"));}json!({"status":"committed","result":r["value"]["result"]})},
                }
            },
            "proxies.catalog.save"|"proxies.catalog.remove"=>{
                let operation=format!("@proxy:{}",field(p,"operationId")?);
                let hash=crate::route::digest(&json!([method,p]));
                if let Some(r)=receipts::receipt(&db,&operation,&key,Some(&hash))?{r["value"]["result"].clone()}else{
                    let mut cleared=None;
                    let write=(|| {
                    let tx=db.transaction()?;
                    // Reserve bounded receipt capacity before any credential side effect.
                    receipts::reserve(&tx)?;
                    let old=p["proxyId"].as_str().map(|id|crate::route_store::proxy(&tx,id)).transpose()?;
                    if old.as_ref().is_some_and(|v|p["expectedRevision"].as_u64()!=Some(v.revision)){return Err(CoreError::new("REVISION_CONFLICT","代理已修改，请重新加载"));}
                    let result=if method.ends_with("remove"){
                        let proxy=old.ok_or_else(||CoreError::new("INVALID_PARAMS","缺少代理"))?;
                        self.check_proxy_removable(&tx,&proxy.id)?;
                        cleared=Some(proxy.id.clone());
                        tx.execute("DELETE FROM socks_proxies WHERE id=?",[&proxy.id])?;
                        json!({"deleted":true,"catalogRevision":catalog::bump_revision(&tx)?})
                    }else{
                        let c=&p["configuration"];
                        let mut proxy=SocksProxy{id:old.as_ref().map(|v|v.id.clone()).unwrap_or_else(id),name:field(c,"name")?.trim().into(),host:field(c,"host")?.trim().into(),port:c["port"].as_u64().unwrap() as u16,auth_type:field(c,"authType")?.into(),username:field(c,"username").unwrap_or("").into(),revision:old.as_ref().map_or(1,|v|v.revision+1),has_preset:false};
                        proxy.validate()?;
                        let count:i64=tx.query_row("SELECT count(*) FROM socks_proxies",[],|r|r.get(0))?;
                        if old.is_none()&&count>=10000{return Err(CoreError::new("LIMIT_EXCEEDED","网络代理配置已达上限"));}

                        proxy.has_preset=self.secrets.has_preset(&tx,"socks5",&proxy.id,&crate::route::identity::proxy_identity(&proxy))?;
                        tx.execute("INSERT INTO socks_proxies(id,data,revision) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data,revision=excluded.revision",params![proxy.id,serde_json::to_string(&proxy)?,proxy.revision as i64])?;
                        json!({"proxy":proxy,"catalogRevision":catalog::bump_revision(&tx)?})
                    };
                    receipts::insert_receipt(&tx,&operation,&key,&hash,&json!({"type":"proxyCatalog","result":result}),&[])?;
                    tx.commit()?;if let Some(owner)=&cleared{self.secrets.forget_owner("socks5",owner);}Ok(result)
                    })();
                    write?
                }
            },
            _=>return Err(CoreError::new("UNSUPPORTED_API","未知代理目录接口")),
        };
        contract::validate("x-results",method,&result)?;Ok(result)
    }
    pub(crate) fn check_proxy_removable(&self,db:&rusqlite::Connection,id:&str)->Result<()>{
        let saved={let mut q=db.prepare("SELECT DISTINCT target_server_id FROM server_route_steps WHERE proxy_id=?")?;let rows=q.query_map([id],|r|r.get::<_,String>(0))?;rows.collect::<std::result::Result<Vec<_>,_>>()?};
        let running=self.registry.lock().unwrap().connections.values().filter(|c|c.route.contains_proxy(id)).map(|c|c.id.clone()).collect::<Vec<_>>();
        if !saved.is_empty()||!running.is_empty(){return Err(CoreError::new("PROXY_DEPENDENTS","网络代理仍被服务器或运行连接引用").details(json!({"serverIds":saved,"connectionIds":running})));}Ok(())
    }
}
