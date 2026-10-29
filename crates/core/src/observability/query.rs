//! Read-only workers keep history scans away from the audit writer and SSH owners.
use super::schema::{Partition,PARTITIONS};
use crate::{CoreError,Result};
use rusqlite::{Connection,OpenFlags};
use serde_json::Value;
use std::{path::Path,sync::{Arc,Mutex,mpsc},time::{Duration,Instant}};
thread_local!{static DEADLINE:std::cell::Cell<Option<Instant>>=const{std::cell::Cell::new(None)};}
pub(super) struct ReadBudget(Option<Instant>);
impl ReadBudget{pub fn enter()->Self{let prior=DEADLINE.with(|v|{let prior=v.get();v.set(Some(prior.unwrap_or_else(||Instant::now()+Duration::from_secs(2))));prior});Self(prior)}}
impl Drop for ReadBudget{fn drop(&mut self){DEADLINE.with(|v|v.set(self.0));}}
type Read=Box<dyn FnOnce(&Connection)->Result<Value>+Send>;
struct Job {partition:Partition,deadline:Instant,read:Read,reply:mpsc::SyncSender<Result<Value>>}
pub(crate) struct Queries {sender:mpsc::SyncSender<Job>}
impl Queries {
    pub fn open(root:&Path)->Result<Self> {
        let (sender,receiver)=mpsc::sync_channel::<Job>(16);let receiver=Arc::new(Mutex::new(receiver));
        for index in 0..2 {
            let databases=PARTITIONS.into_iter().map(|partition|{
                let db=Connection::open_with_flags(root.join(partition.file()),OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                db.busy_timeout(Duration::from_millis(50))?;db.execute_batch("PRAGMA query_only=ON;PRAGMA cache_size=-128;")?;Ok((partition,db))
            }).collect::<Result<Vec<_>>>()?;
            let receiver=receiver.clone();
            std::thread::Builder::new().name(format!("sodeck-observation-query-{index}")).spawn(move||loop {
                let job=receiver.lock().unwrap().recv();let Ok(job)=job else{break};
                if Instant::now()>=job.deadline {let _=job.reply.send(Err(timeout()));continue;}
                let db=&databases.iter().find(|(p,_)|*p==job.partition).unwrap().1;
                if let Err(error)=db.progress_handler(1000,Some(move||Instant::now()>=job.deadline)){let _=job.reply.send(Err(error.into()));continue;}
                let result=(job.read)(db);
                if let Err(error)=db.progress_handler(0,None::<fn()->bool>){let _=job.reply.send(Err(error.into()));continue;}
                let result=if Instant::now()>=job.deadline{Err(timeout())}else{result.and_then(|value|{
                    if serde_json::to_vec(&value)?.len()>512*1024{return Err(CoreError::new("CAPACITY","查询结果超过 512 KiB，请缩小范围"));}Ok(value)
                })};let _=job.reply.send(result);
            })?;
        }
        Ok(Self{sender})
    }
    pub fn query(&self,partition:Partition,read:impl FnOnce(&Connection)->Result<Value>+Send+'static)->Result<Value> {
        let (reply,receiver)=mpsc::sync_channel(1);let deadline=DEADLINE.with(|v|v.get()).unwrap_or_else(||Instant::now()+Duration::from_secs(2));
        self.sender.try_send(Job{partition,deadline,read:Box::new(read),reply}).map_err(|_|CoreError::new("CAPACITY","观测查询队列已满，请稍后重试"))?;
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|timeout())?
    }
}
fn timeout()->CoreError{CoreError::new("CAPACITY","查询超过 2 秒，请缩小范围后重试")}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::Observability;
    use serde_json::json;
    #[test]
    fn long_read_is_interrupted_without_holding_the_audit_writer() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        let worker=store.clone();let (tx,rx)=mpsc::sync_channel(1);
        let thread=std::thread::spawn(move||worker.queries.query(Partition::Facts,move|db|{
            tx.send(()).unwrap();let _:i64=db.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n",[],|r|r.get(0))?;Ok(json!({}))
        }));rx.recv().unwrap();
        let start=Instant::now();let call=store.call_start(&crate::Caller::shell(),"unblocked",&json!({})).unwrap();store.call_finish(&call,&Ok(json!({}))).unwrap();
        assert!(start.elapsed()<Duration::from_millis(500));assert!(thread.join().unwrap().is_err());
        assert!(store.queries.query(Partition::Facts,|db|{db.execute("DELETE FROM calls",[])?;Ok(json!({}))}).is_err());
        assert!(store.shutdown());
    }
}
