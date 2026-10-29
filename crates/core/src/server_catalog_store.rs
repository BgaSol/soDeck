use crate::{model::id,route::AccessPath,route_store,CoreError,Result,Server};
use rusqlite::{Connection,OptionalExtension,params};
use serde_json::{json,Value};
use unicode_normalization::UnicodeNormalization;

pub(crate) fn revision(db:&Connection)->Result<String> {
    let value:String=db.query_row("SELECT value FROM settings WHERE namespace='@core' AND key='serverCatalogRevision'",[],|r|r.get(0))?;
    Ok(serde_json::from_str(&value)?)
}
pub(crate) fn bump_revision(db:&Connection)->Result<String> {
    let next=revision(db)?.parse::<u64>().ok().and_then(|v|v.checked_add(1)).filter(|v|*v<=i64::MAX as u64).ok_or_else(||CoreError::new("LIMIT_EXCEEDED","目录修订已达上限"))?.to_string();
    db.execute("UPDATE settings SET value=? WHERE namespace='@core' AND key='serverCatalogRevision'",[serde_json::to_string(&next)?])?;
    Ok(next)
}
fn conflict()->CoreError {CoreError::new("REVISION_CONFLICT","数据已变化，请重新加载后保存")}
pub(crate) fn membership(db:&Connection,server_id:&str)->Result<(Option<String>,i64)> {
    Ok(db.query_row("SELECT group_id,revision FROM server_group_membership WHERE server_id=?",[server_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.unwrap_or((None,0)))
}
pub(crate) fn item(db:&Connection,server_id:&str)->Result<Value> {
    let (group,revision)=membership(db,server_id)?;
    Ok(json!({"server":route_store::server(db,server_id)?,"accessPath":route_store::route(db,server_id)?,"groupId":group,"membershipRevision":revision.to_string(),"postCommitPending":false}))
}
pub(crate) fn group(db:&Connection,group_id:&str)->Result<Value> {
    db.query_row("SELECT g.id,g.name,g.revision,(SELECT count(*) FROM server_group_membership m WHERE m.group_id=g.id) FROM server_groups g WHERE id=?",[group_id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"revision":r.get::<_,i64>(2)?.to_string(),"serverCount":r.get::<_,i64>(3)?}))).optional()?.ok_or_else(||CoreError::new("GROUP_NOT_FOUND","分组已删除，请重新选择"))
}
pub(crate) fn group_write(db:&Connection,method:&str,p:&Value)->Result<Value> {
    let group_id=p["groupId"].as_str().map(str::to_owned).unwrap_or_else(id);
    let previous=if method!="serverGroups.create" {
        let previous=group(db,&group_id)?;
        if previous["revision"]!=p["expectedRevision"] {return Err(conflict());}
        Some(previous)
    } else {None};
    if method=="serverGroups.delete" {
        if p["expectedCatalogRevision"]!=revision(db)? {return Err(conflict());}
        let count=db.execute("UPDATE server_group_membership SET group_id=NULL,revision=revision+1 WHERE group_id=?",[&group_id])?;
        db.execute("DELETE FROM server_groups WHERE id=?",[&group_id])?;
        return Ok(json!({"deleted":true,"movedCount":count,"catalogRevision":bump_revision(db)?}));
    }
    let name=p["name"].as_str().unwrap().trim();
    if name.is_empty() || name.chars().count()>64 || name.chars().any(char::is_control) {return Err(CoreError::new("INVALID_PARAMS","分组名称需为1～64个字符且不能含控制字符"));}
    let key:String=name.nfc().collect::<String>().to_lowercase().nfc().collect();
    let duplicate:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM server_groups WHERE name_key=? AND id<>?)",params![key,group_id],|r|r.get(0))?;
    if duplicate {return Err(CoreError::new("GROUP_NAME_CONFLICT","已有同名分组"));}
    let next=previous.as_ref().map(|v|v["revision"].as_str().unwrap().parse::<i64>().unwrap()).unwrap_or(0).checked_add(1).ok_or_else(||CoreError::new("LIMIT_EXCEEDED","分组修订已达上限"))?;
    db.execute("INSERT INTO server_groups(id,name,name_key,revision) VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,name_key=excluded.name_key,revision=excluded.revision",params![group_id,name,key,next])?;
    Ok(json!({"group":group(db,&group_id)?,"catalogRevision":bump_revision(db)?}))
}

pub(crate) fn save(db:&Connection,p:&Value)->Result<(Value,Vec<String>)> {
    let old=p["serverId"].as_str().map(|id|route_store::server(db,id)).transpose()?;
    let server_id=old.as_ref().map_or_else(id,|s|s.id.clone());
    if old.is_none() && (p.get("configuration").is_none() || p.get("groupId").is_none()) {return Err(CoreError::new("INVALID_PARAMS","新建服务器需提供配置和分组"));}
    let (previous_group,member_revision)=membership(db,&server_id)?;
    let group_changed=p.get("groupId").is_some_and(|v|v.as_str()!=previous_group.as_deref());
    if p.get("groupId").is_some() && old.is_some() && p["expectedMembershipRevision"]!=member_revision.to_string() {return Err(conflict());}
    if let Some(group_id)=p["groupId"].as_str() {group(db,group_id)?;}
    let pending=Vec::new();
    let mut changed=old.is_none()||group_changed;
    if let Some(config)=p.get("configuration") {
        let previous_route=old.as_ref().map(|s|route_store::route(db,&s.id)).transpose()?;
        let route:AccessPath=serde_json::from_value(config["accessPath"].clone())?;
        if let Some(expected)=p.get("expectedDependencies"){crate::configuration_dependencies::check(db,&route,expected)?;}
        if old.as_ref().is_some_and(|s|p["expectedServerRevision"].as_u64()!=Some(s.revision)) {return Err(conflict());}
        let mut server=Server{id:server_id.clone(),name:config["name"].as_str().unwrap().trim().into(),host:config["host"].as_str().unwrap().trim().into(),port:config["port"].as_u64().unwrap() as u16,username:config["username"].as_str().unwrap().trim().into(),auth_type:config["authType"].as_str().unwrap().into(),revision:old.as_ref().map_or(1,|s|s.revision),has_secret:false};
        server.validate()?;
        let auth_changed=old.as_ref().is_some_and(|s|s.host!=server.host||s.port!=server.port||s.username!=server.username||s.auth_type!=server.auth_type);
        let config_changed=old.as_ref().is_none_or(|s|s.name!=server.name)||auth_changed||Some(&route)!=previous_route.as_ref();
        if config_changed {
            if let Some(s)=&old {server.revision=s.revision.checked_add(1).ok_or_else(||CoreError::new("LIMIT_EXCEEDED","服务器修订已达上限"))?;}
            route_store::save_server_route_tx(db,&server,old.as_ref().map(|s|s.revision),Some(&route))?;
            changed=true;
        }
    }
    if p.get("groupId").is_some() && (group_changed || old.is_none()) {
        db.execute("INSERT INTO server_group_membership(server_id,group_id,revision) VALUES(?,?,?) ON CONFLICT(server_id) DO UPDATE SET group_id=excluded.group_id,revision=excluded.revision",params![server_id,p["groupId"].as_str(),member_revision+1])?;
    }
    if changed {bump_revision(db)?;}
    let mut value=item(db,&server_id)?;
    value["postCommitPending"]=json!(!pending.is_empty());
    Ok((value,pending))
}

pub(crate) use crate::configuration_receipts::{receipt,insert_receipt};
