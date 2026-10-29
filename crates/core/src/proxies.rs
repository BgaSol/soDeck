use super::*;
use crate::route::{RoutePlan,SocksProxy};
use rusqlite::params;

impl Core {
    pub(crate) fn proxy_value(&self,id:&str)->Result<Value>{
        let mut proxy=crate::route_store::proxy(&self.store.db.lock().unwrap(),id)?;
        proxy.has_preset=self.secrets.has_preset(&self.store.db.lock().unwrap(),"socks5",id,&crate::route::identity::proxy_identity(&proxy))?;
        Ok(serde_json::to_value(proxy)?)
    }
    pub(crate) fn proxy_credential_for(&self,plan:&RoutePlan,index:usize,verified:&[String])->Result<Credential>{
        let _guard=self.configuration.lock().unwrap();
        if self.route_plan(&plan.target_server_id)?.revision!=plan.revision{return Err(CoreError::new("REVISION_CONFLICT","访问路径已变化"));}
        let proxy=plan.steps[index].proxy()?;
        if proxy.auth_type=="none"{return Ok(Credential::default());}
        self.context_credential(plan,index,verified)
    }
    pub(crate) fn proxy_dispatch(&self,method:&str,p:&Value)->Result<Value>{
        crate::route_diagnostics::check("x-methods",method,p)?;
        let _guard=self.configuration.lock().unwrap();
        let value=match method {
            "proxies.socks5.list"=>{
                let ids={let db=self.store.db.lock().unwrap();let mut q=db.prepare("SELECT id FROM socks_proxies ORDER BY rowid")?;let rows=q.query_map([],|r|r.get::<_,String>(0))?;rows.collect::<std::result::Result<Vec<_>,_>>()?};
                json!({"proxies":ids.iter().map(|id|self.proxy_value(id)).collect::<Result<Vec<_>>>()?})
            },
            "proxies.socks5.get"=>self.proxy_value(field(p,"proxyId")?)?,
            "proxies.socks5.create"|"proxies.socks5.update"=>{
                let old=if method.ends_with("update"){Some(crate::route_store::proxy(&self.store.db.lock().unwrap(),field(p,"proxyId")?)?)}else{None};
                if old.as_ref().is_some_and(|v|p["expectedRevision"].as_u64()!=Some(v.revision)){return Err(CoreError::new("REVISION_CONFLICT","代理已修改，请重新加载"));}
                let proxy=SocksProxy{id:old.as_ref().map(|v|v.id.clone()).unwrap_or_else(id),name:field(p,"name")?.trim().into(),host:field(p,"host")?.trim().into(),port:u16::try_from(number(p,"port")?).map_err(|_|CoreError::new("INVALID_PARAMS","代理端口无效"))?,auth_type:field(p,"authType")?.into(),username:p["username"].as_str().unwrap_or("").into(),revision:old.as_ref().map_or(1,|v|v.revision+1),has_preset:false};
                proxy.validate()?;
                // Clear before publishing an endpoint change. A storage failure cannot
                // cause the old credential to be sent to a newly configured endpoint.

                let mut db=self.store.db.lock().unwrap();let tx=db.transaction()?;
                let count:i64=tx.query_row("SELECT count(*) FROM socks_proxies",[],|r|r.get(0))?;
                if old.is_none()&&count>=10000{return Err(CoreError::new("LIMIT_EXCEEDED","网络代理配置已达上限"));}
                tx.execute("INSERT INTO socks_proxies(id,data,revision) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data,revision=excluded.revision",params![proxy.id,serde_json::to_string(&proxy)?,proxy.revision as i64])?;
                crate::server_catalog_store::bump_revision(&tx)?;tx.commit()?;drop(db);self.proxy_value(&proxy.id)?
            },
            "proxies.socks5.delete"=>{
                let id=field(p,"proxyId")?;
                let mut db=self.store.db.lock().unwrap();let tx=db.transaction()?;crate::route_store::proxy(&tx,id)?;
                let saved={let mut q=tx.prepare("SELECT DISTINCT target_server_id FROM server_route_steps WHERE proxy_id=?")?;let rows=q.query_map([id],|r|r.get::<_,String>(0))?;rows.collect::<std::result::Result<Vec<_>,_>>()?};
                let running=self.registry.lock().unwrap().connections.values().filter(|c|c.route.contains_proxy(id)).map(|c|c.id.clone()).collect::<Vec<_>>();
                if !saved.is_empty()||!running.is_empty(){return Err(CoreError::new("PROXY_DEPENDENTS","网络代理仍被服务器或运行连接引用").details(json!({"serverIds":saved,"connectionIds":running})));}
                tx.execute("DELETE FROM socks_proxies WHERE id=?",[id])?;crate::server_catalog_store::bump_revision(&tx)?;tx.commit()?;self.secrets.forget_owner("socks5",id);json!({"deleted":true})
            },

            _=>return Err(CoreError::new("UNSUPPORTED_API","未知网络代理接口")),
        };
        crate::route_diagnostics::check("x-results",method,&value)?;Ok(value)
    }
}
