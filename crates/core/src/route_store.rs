use crate::{model::Server, route::ConnectionRoute, store::Store, CoreError, Result};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashSet, VecDeque};

impl Store {
    pub fn connection_route(&self, id: &str) -> Result<ConnectionRoute> {
        let db = self.db.lock().unwrap();
        route(&db, id)
    }
    pub fn route_servers(&self, id: &str) -> Result<Vec<Server>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        resolve(&tx, id, None)
    }
    pub fn dependents(&self, id: &str) -> Result<Vec<Server>> {
        let db = self.db.lock().unwrap();
        dependents(&db, id)
    }
    pub fn preview_route(&self, value: &Server, config: &ConnectionRoute) -> Result<Vec<Server>> {
        let db = self.db.lock().unwrap();
        resolve(&db, &value.id, Some((value, config)))
    }
    pub fn save_server_route(&self, value: &Server, expected: Option<u64>, config: Option<&ConnectionRoute>, multihop: bool) -> Result<()> {
        value.validate()?;
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        save_server_route_tx(&tx,value,expected,config,multihop)?;
        crate::server_catalog_store::bump_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
pub(crate) fn save_server_route_tx(tx:&rusqlite::Connection,value:&Server,expected:Option<u64>,config:Option<&ConnectionRoute>,multihop:bool)->Result<()> {
        value.validate()?;
        let previous: Option<String> = tx.query_row("SELECT data FROM servers WHERE id=?", [&value.id], |r| r.get(0)).optional()?;
        match (previous, expected) {
            (None, None) => {},
            (Some(raw), Some(rev)) if serde_json::from_str::<Server>(&raw)?.revision == rev => {},
            _ => return Err(CoreError::new("REVISION_CONFLICT", "服务器配置已变化，请重新加载")),
        }
        let config = config.cloned().unwrap_or(route(&tx, &value.id)?);
        if !multihop && expected.is_some() && resolve(&tx, &value.id, None)?.len() > 2 {
            return Err(CoreError::new("ROUTE_VERSION_REQUIRED", "旧路由接口不能修改多级路径"));
        }
        let candidate = Some((value, &config));
        let nodes = resolve(&tx, &value.id, candidate)?;
        if !multihop {
            // The old route contract cannot create a multihop path indirectly
            // by changing a server already used by another target.
            if nodes.len() > 2 || dependents(&tx, &value.id)?.iter().any(|s| resolve(&tx, &s.id, candidate).map_or(true, |p| p.len() > 2)) {
                return Err(CoreError::new("ROUTE_VERSION_REQUIRED", "此路径需要多级跳板接口"));
            }
        }
        tx.execute("INSERT INTO servers(id,data) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![value.id,serde_json::to_string(value)?])?;
        tx.execute("INSERT INTO server_routes(server_id,data,jump_server_id) VALUES(?,?,?) ON CONFLICT(server_id) DO UPDATE SET data=excluded.data,jump_server_id=excluded.jump_server_id", params![value.id,serde_json::to_string(&config)?,config.jump_id()])?;
        Ok(())
}
pub(crate) fn server(db: &rusqlite::Connection, id: &str) -> Result<Server> {
    let raw: Option<String> = db.query_row("SELECT data FROM servers WHERE id=?", [id], |r| r.get(0)).optional()?;
    raw.map(|v|serde_json::from_str(&v).map_err(CoreError::from)).transpose()?.ok_or_else(||CoreError::new("NOT_FOUND","服务器不存在"))
}
pub(crate) fn route(db: &rusqlite::Connection, id: &str) -> Result<ConnectionRoute> {
    let raw: Option<String> = db.query_row("SELECT data FROM server_routes WHERE server_id=?", [id], |r|r.get(0)).optional()?;
    raw.map(|v|serde_json::from_str(&v).map_err(|_|CoreError::new("INVALID_ROUTE","不支持的连接路径格式"))).transpose().map(|v|v.unwrap_or_default())
}
pub(crate) fn dependents(db: &rusqlite::Connection, id: &str) -> Result<Vec<Server>> {
    let mut pending = VecDeque::from([id.to_owned()]);
    let mut visited = HashSet::from([id.to_owned()]);
    let mut result = Vec::new();
    while let Some(parent) = pending.pop_front() {
        for child in direct_dependents(db, &parent)? {
            if visited.insert(child.id.clone()) {
                pending.push_back(child.id.clone());
                result.push(child);
            }
        }
    }
    Ok(result)
}
fn direct_dependents(db: &rusqlite::Connection, id: &str) -> Result<Vec<Server>> {
    let mut query = db.prepare("SELECT s.data FROM servers s JOIN server_routes r ON s.id=r.server_id WHERE r.jump_server_id=?")?;
    let rows = query.query_map([id], |r|r.get::<_,String>(0))?;
    rows.map(|r|Ok(serde_json::from_str(&r?)?)).collect()
}

fn resolve(db: &rusqlite::Connection, id: &str, candidate: Option<(&Server, &ConnectionRoute)>) -> Result<Vec<Server>> {
    let mut current = id.to_owned();
    let mut visited = HashSet::new();
    let mut nodes = Vec::new();
    loop {
        if !visited.insert(current.clone()) {
            return Err(CoreError::new("INVALID_ROUTE", "跳板路径存在循环引用").details(serde_json::json!({"serverId":current})));
        }
        let (node, config) = match candidate.filter(|(s,_)| s.id == current) {
            Some((s,r)) => (s.clone(), r.clone()),
            None => (server(db, &current).map_err(|error| {
                if !nodes.is_empty() && error.code == "NOT_FOUND" {
                    CoreError::new("INVALID_ROUTE", "跳板路径引用的服务器不存在").details(serde_json::json!({"serverId":current}))
                } else { error }
            })?, route(db, &current)?),
        };
        nodes.push(node);
        match config.jump_id() {
            Some(next) => current = next.to_owned(),
            None => break,
        }
    }
    nodes.reverse();
    Ok(nodes)
}
