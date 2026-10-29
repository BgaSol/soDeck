use crate::{model::Server, route::{AccessPath, RouteStep, ResolvedStep, ProxyResolve, plan::{SshStep, ProxyStep}}, store::Store, CoreError, Result};
use rusqlite::{params, OptionalExtension};

impl Store {
    pub fn connection_route(&self, id: &str) -> Result<AccessPath> {
        let db = self.db.lock().unwrap();
        route(&db, id)
    }
    pub fn resolved_route(&self, id: &str) -> Result<(AccessPath, Vec<ResolvedStep>)> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        resolve(&tx, id, None)
    }
    pub fn dependents(&self, id: &str) -> Result<Vec<Server>> {
        let db = self.db.lock().unwrap();
        dependents(&db, id)
    }
    pub fn preview_route(&self, value: &Server, config: &AccessPath) -> Result<(AccessPath, Vec<ResolvedStep>)> {
        let db = self.db.lock().unwrap();
        resolve(&db, &value.id, Some((value, config)))
    }
    pub fn save_server_route(&self, value: &Server, expected: Option<u64>, config: Option<&AccessPath>) -> Result<()> {
        value.validate()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        save_server_route_tx(&tx,value,expected,config)?;
        crate::server_catalog_store::bump_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
pub(crate) fn save_server_route_tx(tx:&rusqlite::Connection,value:&Server,expected:Option<u64>,config:Option<&AccessPath>)->Result<()> {
        value.validate()?;
        let previous: Option<String> = tx.query_row("SELECT data FROM servers WHERE id=?", [&value.id], |r| r.get(0)).optional()?;
        match (previous, expected) {
            (None, None) => {},
            (Some(raw), Some(rev)) if serde_json::from_str::<Server>(&raw)?.revision == rev => {},
            _ => return Err(CoreError::new("REVISION_CONFLICT", "服务器配置已变化，请重新加载")),
        }
        let config = config.cloned().unwrap_or(route(&tx, &value.id)?);
        resolve(&tx, &value.id, Some((value, &config)))?;
        tx.execute("INSERT INTO servers(id,data) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![value.id,serde_json::to_string(value)?])?;
        tx.execute("DELETE FROM server_route_steps WHERE target_server_id=?",[&value.id])?;
        for (index,step) in config.steps.iter().enumerate() {
            match step {
                RouteStep::Ssh { step_id, server_id } => {
                    tx.execute("INSERT INTO server_route_steps(target_server_id,ordinal,step_id,kind,ssh_server_id) VALUES(?,?,?,'ssh',?)",params![value.id,index as i64,step_id,server_id])?;
                }
                RouteStep::Socks5 { step_id, proxy_id, resolve } => {
                    tx.execute("INSERT INTO server_route_steps(target_server_id,ordinal,step_id,kind,proxy_id,resolve) VALUES(?,?,?,'socks5',?,?)",params![value.id,index as i64,step_id,proxy_id,if *resolve==ProxyResolve::Proxy {"proxy"} else {"local"}])?;
                }
            }
        }
        Ok(())
}
pub(crate) fn server(db: &rusqlite::Connection, id: &str) -> Result<Server> {
    let raw: Option<String> = db.query_row("SELECT data FROM servers WHERE id=?", [id], |r| r.get(0)).optional()?;
    raw.map(|v|serde_json::from_str(&v).map_err(CoreError::from)).transpose()?.ok_or_else(||CoreError::new("NOT_FOUND","服务器不存在"))
}
pub(crate) fn route(db: &rusqlite::Connection, id: &str) -> Result<AccessPath> {
    let mut query=db.prepare("SELECT ordinal,step_id,kind,ssh_server_id,proxy_id,resolve FROM server_route_steps WHERE target_server_id=? ORDER BY ordinal")?;
    let rows=query.query_map([id],|r|Ok((r.get::<_,i64>(0)? as usize,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?)))?;
    let mut steps=Vec::new();
    for row in rows {
        let (ordinal,step_id,kind,server,proxy,resolve)=row?;
        if ordinal!=steps.len(){return Err(CoreError::new("INVALID_ROUTE","连接步骤顺序不连续"));}
        steps.push(match (kind.as_str(),server,proxy,resolve.as_deref()) {
            ("ssh",Some(server_id),None,None)=>RouteStep::Ssh{step_id,server_id},
            ("socks5",None,Some(proxy_id),Some("proxy"))=>RouteStep::Socks5{step_id,proxy_id,resolve:ProxyResolve::Proxy},
            ("socks5",None,Some(proxy_id),Some("local"))=>RouteStep::Socks5{step_id,proxy_id,resolve:ProxyResolve::Local},
            _=>return Err(CoreError::new("INVALID_ROUTE","连接步骤数据无效")),
        });
    }
    let path=AccessPath{steps};path.validate(id)?;Ok(path)
}
pub(crate) fn dependents(db:&rusqlite::Connection,id:&str)->Result<Vec<Server>> {
    let mut query=db.prepare("SELECT DISTINCT s.data FROM servers s JOIN server_route_steps r ON s.id=r.target_server_id WHERE r.ssh_server_id=?")?;
    let rows=query.query_map([id],|r|r.get::<_,String>(0))?;
    rows.map(|r|Ok(serde_json::from_str(&r?)?)).collect()
}
pub(crate) fn proxy(db:&rusqlite::Connection,id:&str)->Result<crate::route::SocksProxy>{
    let raw:Option<String>=db.query_row("SELECT data FROM socks_proxies WHERE id=?",[id],|r|r.get(0)).optional()?;
    raw.map(|r|serde_json::from_str(&r).map_err(CoreError::from)).transpose()?.ok_or_else(||CoreError::new("PROXY_NOT_FOUND","网络代理不存在"))
}
pub(crate) fn resolve(db:&rusqlite::Connection,id:&str,candidate:Option<(&Server,&AccessPath)>)->Result<(AccessPath,Vec<ResolvedStep>)> {
    let (target,path)=match candidate{Some((s,p))=>(s.clone(),p.clone()),None=>(server(db,id)?,route(db,id)?)};
    path.validate(id)?;target.validate()?;let mut steps=Vec::new();
    for step in &path.steps {
        steps.push(match step {
            RouteStep::Ssh{step_id,server_id}=>{let server=server(db,server_id)?;server.validate()?;ResolvedStep::Ssh(SshStep{step_id:step_id.clone(),server,credential_revision:0})},
            RouteStep::Socks5{step_id,proxy_id,resolve}=>{let proxy=proxy(db,proxy_id)?;proxy.validate()?;ResolvedStep::Socks5(ProxyStep{step_id:step_id.clone(),proxy,resolve:*resolve,credential_revision:0})},
        });
    }
    steps.push(ResolvedStep::Ssh(SshStep{step_id:"target".into(),server:target,credential_revision:0}));Ok((path,steps))
}
