//! Shared bounded configuration receipts. Business SQLite remains schema v6.
use crate::{model::now_ms,CoreError,Result};
use rusqlite::{Connection,OptionalExtension,params};
use serde_json::{json,Value};
pub(crate) fn receipt(db:&Connection,operation:&str,caller:&str,hash:Option<&str>)->Result<Option<Value>> {
    let row:Option<(String,String,String)>=db.query_row("SELECT caller_key,request_hash,result FROM server_catalog_receipts WHERE operation_id=?",[operation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    row.map(|(who,previous,result)|{
        if who!=caller || hash.is_some_and(|h|h!=previous) {return Err(CoreError::new("OPERATION_CONFLICT","该操作标识已用于其他请求"));}
        Ok(serde_json::from_str(&result)?)
    }).transpose()
}
pub(crate) fn insert_receipt(db:&Connection,operation:&str,caller:&str,hash:&str,value:&Value,pending:&[String])->Result<()> {
    let result=serde_json::to_string(&json!({"value":value,"pendingSecrets":pending}))?;
    if result.len()>8192 {return Err(CoreError::new("LIMIT_EXCEEDED","保存回执超出容量"));}
    reserve(db)?;
    db.execute("INSERT INTO server_catalog_receipts(operation_id,caller_key,request_hash,result,created_at) VALUES(?,?,?,?,?)",params![operation,caller,hash,result,now_ms() as i64])?;
    Ok(())
}
pub(crate) fn reserve(db:&Connection)->Result<()> {
    db.execute("DELETE FROM server_catalog_receipts WHERE created_at<? AND json_array_length(result,'$.pendingSecrets')=0",[now_ms().saturating_sub(86400000) as i64])?;
    let count:i64=db.query_row("SELECT count(*) FROM server_catalog_receipts",[],|r|r.get(0))?;
    if count>=1024 {return Err(CoreError::new("LIMIT_EXCEEDED","保存回执已满，请稍后重试"));}
    Ok(())
}
