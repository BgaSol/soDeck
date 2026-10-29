//! Only explicit transport identities participate, never a jump's own route.
use crate::{route::{AccessPath,RouteStep},route_store,CoreError,Result};
use rusqlite::Connection;
use serde_json::{json,Value};

pub(crate) fn identities(db:&Connection,path:&AccessPath)->Result<Value>{
    Ok(Value::Array(path.steps.iter().map(|step|match step {
        RouteStep::Ssh{step_id,server_id}=>{
            let s=route_store::server(db,server_id)?;
            Ok(json!({"stepId":step_id,"kind":"ssh","id":s.id,"host":s.host,"port":s.port,"username":s.username,"authType":s.auth_type}))
        },
        RouteStep::Socks5{step_id,proxy_id,..}=>{
            let p=route_store::proxy(db,proxy_id)?;
            Ok(json!({"stepId":step_id,"kind":"socks5","id":p.id,"host":p.host,"port":p.port,"username":p.username,"authType":p.auth_type}))
        }
    }).collect::<Result<Vec<_>>>()?))
}
pub(crate) fn check(db:&Connection,path:&AccessPath,expected:&Value)->Result<()> {
    if identities(db,path)?!=*expected {return Err(CoreError::new("REVISION_CONFLICT","所选跳板或代理的连接身份已变化，请重新选择"));}
    Ok(())
}
