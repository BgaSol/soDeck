//! The single writer reserves durable ranges before it starts event transactions.
use crate::{CoreError,Result};
use std::cell::Cell;
thread_local!{static RANGE:Cell<(i64,i64)>=const{Cell::new((0,0))};}
pub fn prepare(db:&rusqlite::Connection)->Result<()> {
    let needs=RANGE.with(|v|{let(n,end)=v.get();end-n<512});
    if needs {
        let previous:String=db.query_row("SELECT value FROM meta WHERE key='sequenceReserved'",[],|r|r.get(0))?;
        let start=previous.parse::<i64>().map_err(|_|CoreError::new("STORE_UNAVAILABLE","序号范围无效"))?;
        let end=start.checked_add(1024).ok_or_else(||CoreError::new("CAPACITY","序号已用尽"))?;
        db.execute("UPDATE meta SET value=? WHERE key='sequenceReserved'",[end.to_string()])?;
        RANGE.with(|v|v.set((start,end)));
    }Ok(())
}
pub fn next()->Result<i64>{RANGE.with(|v|{let(n,end)=v.get();if n>=end{return Err(CoreError::new("CAPACITY","事务事件数超过保留序号范围"));}v.set((n+1,end));Ok(n+1)})}
