use super::schema::{self,Partition,PARTITIONS};
use crate::{CoreError,Result};
use rusqlite::Connection;
use serde_json::{json,Value};
use std::{collections::VecDeque,path::{Path,PathBuf},sync::{Arc,Condvar,Mutex,mpsc},time::{Duration,Instant}};

const QUEUE_BYTES:usize=8*1024*1024;
const RESERVED_BYTES:usize=1024*1024;
type Work=Box<dyn FnOnce(&mut Databases)->Result<Value>+Send>;
type AuditWork=Box<dyn FnOnce(&Connection)->Result<Value>+Send>;
enum WorkItem{Ordinary(Work),Audit(AuditWork)}
struct Job {work:WorkItem,partition:Partition,bytes:usize,deadline:Instant,reply:mpsc::SyncSender<Result<Value>>}
#[derive(Default)]
struct Queue {control:VecDeque<Job>,ordinary:VecDeque<Job>,bytes:usize,stopping:bool,failed:u64,discarded:u64,last_error:Option<String>,partition_errors:[Option<String>;4],last_success:Option<u64>,last_failure:Option<u64>}
struct Shared {queue:Mutex<Queue>,changed:Condvar,finished:Condvar,done:Mutex<bool>,emergency:super::emergency::Emergency}
pub(crate) struct Databases {pub facts:Connection,pub traffic:Connection,pub diagnostics:Connection,pub logs:Connection,root:PathBuf,evidence_reservations:std::collections::HashMap<String,usize>}
pub(crate) struct Observability {pub epoch:String,pub root:PathBuf,pub boot_id:String,pub config_epoch:String,pub queries:super::query::Queries,pub(super) cursor_key:String,pub(super) signing_key:String,pub(super) catalog:Mutex<super::catalog::Catalog>,pub(super) contexts:Mutex<super::context::Contexts>,pub(super) metric_ring:Mutex<super::metrics::MetricRing>,pub(super) credits:Mutex<super::credits::Credits>,pub(super) feeds:Mutex<super::feed::Feeds>,pub(super) jobs:Mutex<super::evidence::Jobs>,pub(super) automatic:Mutex<super::automatic::Automatic>,shared:Arc<Shared>}
fn unavailable()->CoreError {CoreError::new("STORE_UNAVAILABLE","公共观测存储暂不可写")}
impl Databases {
    fn audit_batch(&mut self,batch:Vec<Job>)->Vec<(mpsc::SyncSender<Result<Value>>,Result<Value>,usize)>{
        let bytes=batch.iter().map(|job|job.bytes).sum::<usize>();
        if let Err(error)=self.admit_audit(bytes).and_then(|_|super::sequence::prepare(&self.facts)){
            return batch.into_iter().map(|job|(job.reply,Err(CoreError::new(&error.code,&error.message)),job.bytes)).collect();
        }
        let tx=match self.facts.transaction(){Ok(tx)=>tx,Err(error)=>return batch.into_iter().map(|job|(job.reply,Err(CoreError::new("STORAGE_ERROR",error.to_string())),job.bytes)).collect()};
        let mut results=Vec::new();
        for job in batch{
            let result=if Instant::now()>=job.deadline{Err(unavailable())}else{match job.work{WorkItem::Audit(work)=>{
                let result=tx.execute_batch("SAVEPOINT audit_item").map_err(Into::into).and_then(|_|work(&tx));
                if result.is_ok(){tx.execute_batch("RELEASE audit_item").map_err(Into::into).and(result)}else{let _=tx.execute_batch("ROLLBACK TO audit_item; RELEASE audit_item");result}
            },WorkItem::Ordinary(_)=>unreachable!()}};
            results.push((job.reply,result,job.bytes));
        }
        if let Err(error)=tx.commit(){for (_,result,_)in &mut results{*result=Err(CoreError::new("STORAGE_ERROR",error.to_string()));}}
        results
    }
    fn db(&self,partition:Partition)->&Connection {match partition{Partition::Facts=>&self.facts,Partition::Logs=>&self.logs,Partition::Traffic=>&self.traffic,Partition::Diagnostics=>&self.diagnostics}}
    fn open(root:&Path)->Result<Self> {
        let open=|partition:Partition|->Result<Connection>{
            let path=root.join(partition.file());
            let db=Connection::open_with_flags(path,rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            db.busy_timeout(Duration::from_millis(100))?;
            let version:u32=db.pragma_query_value(None,"user_version",|r|r.get(0))?;
            if version!=schema::VERSION{return Err(CoreError::new("UNSUPPORTED_DATA_VERSION","观测数据库需要由启动协调器备份重建"));}
            db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=64; PRAGMA journal_size_limit=262144; PRAGMA cache_size=-128;")?;
            db.pragma_update(None,"max_page_count",(partition.budget()*4/5/4096)as u32)?;Ok(db)
        };
        Ok(Self{facts:open(Partition::Facts)?,traffic:open(Partition::Traffic)?,diagnostics:open(Partition::Diagnostics)?,logs:open(Partition::Logs)?,root:root.into(),evidence_reservations:Default::default()})
    }
    fn physical_bytes(&self,partition:Partition)->Result<u64>{let mut total=0;for suffix in ["","-wal","-shm"]{let path=self.root.join(format!("{}{suffix}",partition.file()));match std::fs::metadata(path){Ok(m)=>total+=m.len(),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.into())}}Ok(total)}
    pub(super) fn reserve_evidence(&mut self,id:&str)->Result<usize>{
        let reserved=self.evidence_reservations.values().sum::<usize>()as u64*4;
        let available=(Partition::Diagnostics.budget()*9/10).saturating_sub(self.physical_bytes(Partition::Diagnostics)?+reserved+1024*1024);
        // Leave room for another admitted task and ordinary snapshots/metrics.
        let allowance=(available/8).min(50*1024*1024)as usize;
        if allowance<256*1024{return Err(CoreError::new("CAPACITY","诊断分区不足以预留证据空间"));}
        self.evidence_reservations.insert(id.to_owned(),allowance);Ok(allowance)
    }
    pub(super) fn release_evidence(&mut self,id:&str){self.evidence_reservations.remove(id);}
    pub(super) fn admit_evidence_body(&mut self,id:&str,bytes:usize)->Result<()> {
        if self.evidence_reservations.get(id).copied().unwrap_or(0)<bytes+16*1024{return Err(CoreError::new("CAPACITY","证据达到预留上限，保留终态清单空间"));}
        self.admit_evidence(id,bytes)
    }
    pub(super) fn admit_evidence(&mut self,id:&str,bytes:usize)->Result<()> {
        let remaining=self.evidence_reservations.get_mut(id).ok_or_else(||CoreError::new("CAPACITY","证据预留已释放"))?;
        if bytes>*remaining{return Err(CoreError::new("CAPACITY","证据达到已预留空间，请缩小范围"));}
        *remaining-=bytes;
        if let Err(error)=self.admit(Partition::Diagnostics,bytes){*self.evidence_reservations.get_mut(id).unwrap()+=bytes;return Err(error);}Ok(())
    }
    pub fn admit(&self,partition:Partition,bytes:usize)->Result<()> {
        self.admit_with_priority(partition,bytes,false)
    }
    pub fn admit_audit(&self,bytes:usize)->Result<()> {self.admit_with_priority(Partition::Facts,bytes,true)}
    fn admit_with_priority(&self,partition:Partition,bytes:usize,audit:bool)->Result<()> {
        let db=match partition {Partition::Facts=>&self.facts,Partition::Traffic=>&self.traffic,Partition::Diagnostics=>&self.diagnostics,Partition::Logs=>&self.logs};
        if bytes>256*1024{return Err(CoreError::new("CAPACITY","单次观测写入超过 256 KiB"));}
        // This owner has no long-lived read transaction. A busy checkpoint must
        // reject new writes before a reader can pin an unbounded WAL.
        let size=||->Result<u64>{Ok(self.physical_bytes(partition)?+if partition==Partition::Diagnostics{self.evidence_reservations.values().sum::<usize>()as u64*4}else{0})};
        let budget=partition.budget()-if partition==Partition::Facts&&!audit{64*1024*1024}else{0};
        // SQLite's page ceiling is intentionally below the physical partition
        // budget. Reclaim before that ceiling, including reusable freelist pages.
        let fits=||->Result<bool>{
            let pages:u64=db.pragma_query_value(None,"page_count",|r|r.get::<_,i64>(0))? as u64;
            let free:u64=db.pragma_query_value(None,"freelist_count",|r|r.get::<_,i64>(0))? as u64;
            let maximum:u64=db.pragma_query_value(None,"max_page_count",|r|r.get::<_,i64>(0))? as u64;
            let page_size:u64=db.pragma_query_value(None,"page_size",|r|r.get::<_,i64>(0))? as u64;
            let reserve=bytes as u64*4+262144;
            Ok(size()?.saturating_add(reserve)<=budget*9/10&&pages.saturating_sub(free).saturating_mul(page_size).saturating_add(reserve)<=maximum*page_size*19/20)
        };
        for pass in 0..5 {
            if fits()?{return Ok(());}
            let (busy,_,_):(u32,u32,u32)=db.query_row("PRAGMA wal_checkpoint(TRUNCATE)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            if busy!=0||pass==4{break;}
            if partition==Partition::Facts {
                let removed=db.execute("DELETE FROM records WHERE event_id IN(SELECT event_id FROM records WHERE call_id IS NULL AND NOT EXISTS(SELECT 1 FROM observation_spans s WHERE s.trace_id=records.trace_id AND s.outcome IS NULL) ORDER BY received,sequence LIMIT 32)",[])?;
                if removed==0&&!audit{return Err(CoreError::new("CAPACITY","普通事实空间已用尽，保留审计预留"));}
                if removed==0&&audit {
                    // Active calls stay pinned. A completed call and all its facts retire together.
                    db.execute_batch("BEGIN IMMEDIATE;")?;
                    let result=(||->Result<()>{
                        db.execute("DELETE FROM events WHERE call_id IN(SELECT id FROM calls WHERE ended IS NOT NULL AND NOT EXISTS(SELECT 1 FROM records r JOIN observation_spans s ON s.trace_id=r.trace_id WHERE r.call_id=calls.id AND s.outcome IS NULL) ORDER BY started LIMIT 32)",[])?;
                        db.execute("DELETE FROM calls WHERE id IN(SELECT id FROM calls WHERE ended IS NOT NULL AND NOT EXISTS(SELECT 1 FROM records r JOIN observation_spans s ON s.trace_id=r.trace_id WHERE r.call_id=calls.id AND s.outcome IS NULL) ORDER BY started LIMIT 32)",[])?;Ok(())
                    })();
                    if let Err(error)=result{let _=db.execute_batch("ROLLBACK");return Err(error);}
                    db.execute_batch("COMMIT")?;
                }
                db.execute_batch("PRAGMA incremental_vacuum(64)")?;
            }else if partition==Partition::Logs{
                db.execute("DELETE FROM records WHERE event_id IN(SELECT event_id FROM records ORDER BY CASE level WHEN 'trace' THEN 0 WHEN 'debug' THEN 1 WHEN 'info' THEN 2 ELSE 3 END,received LIMIT 32)",[])?;
                db.execute_batch("PRAGMA incremental_vacuum(64)")?;
            }else if partition==Partition::Diagnostics{
                db.execute("DELETE FROM observation_jobs WHERE id IN(SELECT id FROM observation_jobs WHERE state NOT IN('queued','running') AND id NOT IN(SELECT json_extract(data,'$.manifest.packageId') FROM observation_jobs WHERE state IN('queued','running') AND json_extract(data,'$.kind')='export') ORDER BY updated LIMIT 8)",[])?;
                db.execute("DELETE FROM observation_snapshots WHERE id IN(SELECT id FROM observation_snapshots WHERE expires<? OR json_extract(data,'$.ended')=1 ORDER BY received LIMIT 16)",[crate::model::now_ms()as i64])?;
                db.execute("DELETE FROM metric_buckets WHERE rowid IN(SELECT rowid FROM metric_buckets WHERE minute<? ORDER BY minute LIMIT 32)",[crate::model::now_ms().saturating_sub(180000)as i64])?;
                db.execute_batch("PRAGMA incremental_vacuum(64)")?;
            }else{break;}
        }
        if !fits()?{return Err(CoreError::new("CAPACITY","观测分区需要清理，已停止本批写入"));}Ok(())
    }
}
impl Observability {
    // Compatibility for embedders/tests supplying the core directory itself.
    // Desktop uses the versioned dataset coordinator instead. Never use a parent.
    pub fn ensure_new(root:&Path)->Result<()> {
        if root.try_exists()? {return Ok(());}
        let staged=root.with_file_name(format!("observability-new-{}",crate::model::id()));
        std::fs::create_dir(&staged)?;
        let epoch=crate::model::id();
        for partition in PARTITIONS {schema::initialize(&staged.join(partition.file()),partition,&epoch)?;}
        std::fs::rename(staged,root)?;
        Ok(())
    }
    /// Startup must initialize or recover the whole dataset before this point.
    pub fn open(root:&Path,boot_id:String,config_epoch:String)->Result<Arc<Self>> {
        let mut databases=Databases::open(root)?;
        let epoch:String=databases.facts.query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get(0))?;
        for db in [&databases.traffic,&databases.diagnostics,&databases.logs] {
            let actual:String=db.query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get(0))?;
            if actual!=epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","观测数据集成员代次不一致"));}
        }
        let signing_key:String=databases.facts.query_row("SELECT value FROM meta WHERE key='signingKey'",[],|r|r.get(0))?;
        let queries=super::query::Queries::open(root)?;
        let shared=Arc::new(Shared{queue:Mutex::new(Queue::default()),changed:Condvar::new(),finished:Condvar::new(),done:Mutex::new(false),emergency:super::emergency::Emergency::new(root)});
        let worker=shared.clone();let archive_boot=boot_id.clone();let archive_config=config_epoch.clone();
        std::thread::Builder::new().name("sodeck-observation-writer".into()).spawn(move||{
            let mut control_run=0;
            let mut maintenance=Instant::now();
            loop {
                let job={let mut queue=worker.queue.lock().unwrap();if queue.control.is_empty()&&queue.ordinary.is_empty()&&!queue.stopping{queue=worker.changed.wait_timeout(queue,Duration::from_secs(1)).unwrap().0;}
                    let next=if control_run>=8&&!queue.ordinary.is_empty(){control_run=0;queue.ordinary.pop_front()}else{queue.control.pop_front().inspect(|_|control_run+=1).or_else(||{control_run=0;queue.ordinary.pop_front()})};
                    if next.is_none()&&queue.stopping{break;}next};
                let Some(job)=job else{if maintenance.elapsed()>=Duration::from_secs(60){if let Err(error)=super::retention::maintain(&mut databases){worker.emergency.record(&error.code);}maintenance=Instant::now();}let _=super::sequence::prepare(&databases.facts).and_then(|_|worker.emergency.archive(&mut databases,&archive_boot,&archive_config));worker.emergency.flush();continue};
                if matches!(job.work,WorkItem::Audit(_)){
                    let mut batch=vec![job];let mut bytes=batch[0].bytes;
                    {let mut queue=worker.queue.lock().unwrap();if queue.control.is_empty()&&!queue.stopping{queue=worker.changed.wait_timeout(queue,Duration::from_millis(2)).unwrap().0;}
                        while batch.len()<32&&queue.control.front().is_some_and(|j|matches!(j.work,WorkItem::Audit(_))&&bytes+j.bytes<=192*1024){let next=queue.control.pop_front().unwrap();bytes+=next.bytes;batch.push(next);}}
                    let results=databases.audit_batch(batch);
                    for (reply,result,bytes) in results{
                        let mut queue=worker.queue.lock().unwrap();queue.bytes=queue.bytes.saturating_sub(bytes);
                        if let Err(error)=&result{queue.failed+=1;queue.last_error=Some(error.code.clone());queue.partition_errors[Partition::Facts.index()]=Some(error.code.clone());queue.last_failure=Some(crate::model::now_ms());worker.emergency.record(&error.code);}
                        else{queue.partition_errors[Partition::Facts.index()]=None;queue.last_success=Some(crate::model::now_ms());}drop(queue);let _=reply.send(result);
                    }
                    if maintenance.elapsed()>=Duration::from_secs(60){if let Err(error)=super::retention::maintain(&mut databases){worker.emergency.record(&error.code);}maintenance=Instant::now();}worker.emergency.flush();continue;
                }
                let before=databases.db(job.partition).total_changes();
                let result=if Instant::now()>=job.deadline{Err(unavailable())}else{super::sequence::prepare(&databases.facts).and_then(|_|match job.work{WorkItem::Ordinary(work)=>work(&mut databases),WorkItem::Audit(_)=>unreachable!()})};
                let wrote=databases.db(job.partition).total_changes()>before;
                let mut queue=worker.queue.lock().unwrap();queue.bytes=queue.bytes.saturating_sub(job.bytes);
                if let Err(error)=&result{if matches!(error.code.as_str(),"STORAGE_ERROR"|"IO_ERROR"|"STORE_UNAVAILABLE"|"CAPACITY"){queue.failed=queue.failed.saturating_add(1);queue.last_error=Some(error.code.clone());queue.partition_errors[job.partition.index()]=Some(error.code.clone());queue.last_failure=Some(crate::model::now_ms());worker.emergency.record(&error.code);}}
                else if wrote{queue.partition_errors[job.partition.index()]=None;queue.last_success=Some(crate::model::now_ms());}
                drop(queue);let _=job.reply.send(result);
                if maintenance.elapsed()>=Duration::from_secs(60){if let Err(error)=super::retention::maintain(&mut databases){worker.emergency.record(&error.code);}maintenance=Instant::now();}
                worker.emergency.flush();
            }
            for db in [&databases.facts,&databases.traffic,&databases.diagnostics,&databases.logs]{let _=db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");}
            worker.emergency.flush();
            *worker.done.lock().unwrap()=true;worker.finished.notify_all();
        })?;
        let store=Arc::new(Self{epoch,root:root.into(),boot_id,config_epoch,queries,shared,cursor_key:crate::model::id(),signing_key,catalog:Mutex::new(super::catalog::Catalog::default()),contexts:Mutex::new(super::context::Contexts::default()),metric_ring:Mutex::new(super::metrics::MetricRing::default()),credits:Mutex::new(super::credits::Credits::default()),feeds:Mutex::new(super::feed::Feeds::default()),jobs:Mutex::new(super::evidence::Jobs::default()),automatic:Mutex::new(super::automatic::Automatic::default())});
        Self::start_feed_pump(&store);Ok(store)
    }
    pub fn submit(&self,control:bool,bytes:usize,work:impl FnOnce(&mut Databases)->Result<Value>+Send+'static)->Result<mpsc::Receiver<Result<Value>>> {
        self.submit_partition(Partition::Facts,control,bytes,work)
    }
    pub fn submit_partition(&self,partition:Partition,control:bool,bytes:usize,work:impl FnOnce(&mut Databases)->Result<Value>+Send+'static)->Result<mpsc::Receiver<Result<Value>>> {
        let (tx,rx)=mpsc::sync_channel(1);let mut queue=self.shared.queue.lock().unwrap();
        if queue.stopping{return Err(unavailable());}
        // Charged while queued AND executing; reserved capacity cannot be consumed by metrics.
        let charge=bytes.saturating_add(256);
        if charge>256*1024||queue.bytes.saturating_add(charge)>if control{QUEUE_BYTES}else{QUEUE_BYTES-RESERVED_BYTES} {
            queue.discarded=queue.discarded.saturating_add(1);self.shared.emergency.record("QUEUE_CAPACITY");self.shared.changed.notify_one();return Err(CoreError::new("CAPACITY","观测写入队列已满"));
        }
        queue.bytes+=charge;let job=Job{work:WorkItem::Ordinary(Box::new(work)),partition,bytes:charge,deadline:Instant::now()+Duration::from_secs(2),reply:tx};
        if control{queue.control.push_back(job);}else{queue.ordinary.push_back(job);}
        self.shared.changed.notify_one();Ok(rx)
    }
    pub(super) fn audit(&self,bytes:usize,work:impl FnOnce(&Connection)->Result<Value>+Send+'static)->Result<Value>{
        let (reply,received)=mpsc::sync_channel(1);let charge=bytes.saturating_add(256);let mut queue=self.shared.queue.lock().unwrap();
        if queue.stopping{return Err(unavailable());}if charge>192*1024||queue.bytes+charge>QUEUE_BYTES{return Err(CoreError::new("CAPACITY","审计队列已满"));}
        queue.bytes+=charge;queue.control.push_back(Job{work:WorkItem::Audit(Box::new(work)),partition:Partition::Facts,bytes:charge,deadline:Instant::now()+Duration::from_secs(2),reply});self.shared.changed.notify_one();drop(queue);
        received.recv_timeout(Duration::from_secs(2)).map_err(|_|unavailable())?
    }
    pub fn commit(&self,bytes:usize,work:impl FnOnce(&mut Databases)->Result<Value>+Send+'static)->Result<Value> {
        // An admitted audit must finish before its caller executes side effects.
        self.submit(true,bytes,work)?.recv_timeout(Duration::from_secs(2)).map_err(|_|{self.shared.emergency.record("WRITE_TIMEOUT");unavailable()})?
    }
    pub(super) fn write(&self,partition:Partition,bytes:usize,work:impl FnOnce(&mut Databases)->Result<Value>+Send+'static)->Result<Value>{
        self.submit_partition(partition,false,bytes,work)?.recv_timeout(Duration::from_secs(2)).map_err(|_|unavailable())?
    }
    pub(super) fn management_tail_failed(&self){self.shared.emergency.record("MANAGEMENT_TAIL_GAP");self.automatic.lock().unwrap().tail_gaps+=1;}
    pub(super) fn emergency_archived(&self)->u64{self.shared.emergency.archived()}
    pub(super) fn stopping(&self)->bool{self.shared.queue.lock().unwrap().stopping}
    pub fn health(&self)->Value {
        let queue=self.shared.queue.lock().unwrap();
        let partitions:Vec<_>=PARTITIONS.into_iter().map(|partition|{
            let size=|suffix:&str|std::fs::metadata(self.root.join(format!("{}{suffix}",partition.file()))).map(|m|m.len()).ok();
            let main=size("");let wal=size("-wal").unwrap_or(0);let shared=size("-shm").unwrap_or(0);
            json!({"partition":partition.file().trim_end_matches(".sqlite"),"mainBytes":main,"walBytes":wal,"sharedBytes":shared,"budgetBytes":partition.budget(),"error":queue.partition_errors[partition.index()]})
        }).collect();
        json!({"storeEpoch":self.epoch,"state":if queue.stopping{"unavailable"}else if queue.partition_errors.iter().any(Option::is_some)||queue.bytes>=QUEUE_BYTES-RESERVED_BYTES{"degraded"}else{"active"},"queueBytes":queue.bytes,"queueBudgetBytes":QUEUE_BYTES,"failedWrites":queue.failed.to_string(),"droppedWrites":queue.discarded.to_string(),"lastError":queue.last_error,"lastFailureAt":queue.last_failure,"lastSuccessAt":queue.last_success,"budgetBytes":512*1024*1024,"partitions":partitions,"emergency":self.shared.emergency.health()})
    }
    pub fn shutdown(&self)->bool {
        let deadline=Instant::now()+Duration::from_secs(2);
        {let mut queue=self.shared.queue.lock().unwrap();queue.stopping=true;self.shared.changed.notify_all();}
        let done=self.shared.done.lock().unwrap();let (done,_)=self.shared.finished.wait_timeout_while(done,deadline.saturating_duration_since(Instant::now()),|done|!*done).unwrap();
        if *done{return true;}
        let mut queue=self.shared.queue.lock().unwrap();
        // Canceled jobs were never executed. Synchronous callers receive an explicit failure.
        while let Some(job)=queue.control.pop_front().or_else(||queue.ordinary.pop_front()){queue.bytes=queue.bytes.saturating_sub(job.bytes);queue.discarded=queue.discarded.saturating_add(1);let _=job.reply.send(Err(unavailable()));}
        false
    }
    #[cfg(test)]
    pub fn fail_writes(&self,fail:bool) {
        self.commit(1024,move|db|{db.facts.execute_batch(if fail{"PRAGMA query_only=ON"}else{"PRAGMA query_only=OFF"})?;Ok(Value::Null)}).unwrap();
    }
    #[cfg(test)]
    pub fn initialize_fixture(root:&Path)->Result<()> {
        std::fs::create_dir_all(root)?;let epoch=crate::model::id();for p in PARTITIONS {schema::initialize(&root.join(p.file()),p,&epoch)?;}Ok(())
    }
}
impl Drop for Observability {fn drop(&mut self){self.shutdown();}}

#[cfg(test)]mod tests {
    use super::*;
    #[test]
    fn control_has_reserved_capacity_and_ordinary_work_is_not_starved() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        let (entered,ready)=mpsc::sync_channel(1);let (release,held)=mpsc::sync_channel(1);
        let blocker=store.submit(true,1024,move|_|{entered.send(()).unwrap();held.recv().unwrap();Ok(Value::Null)}).unwrap();ready.recv().unwrap();
        let order=Arc::new(Mutex::new(Vec::new()));let mut ordinary=Vec::new();
        loop {let log=order.clone();match store.submit(false,250*1024,move|_|{log.lock().unwrap().push("ordinary");Ok(Value::Null)}){Ok(reply)=>ordinary.push(reply),Err(error)=>{assert_eq!(error.code,"CAPACITY");break;}}}
        assert!(!ordinary.is_empty());let mut controls=Vec::new();
        for _ in 0..16{let log=order.clone();controls.push(store.submit(true,1024,move|_|{log.lock().unwrap().push("control");Ok(Value::Null)}).unwrap());}
        assert!(store.health()["queueBytes"].as_u64().unwrap()<=QUEUE_BYTES as u64);
        release.send(()).unwrap();blocker.recv().unwrap().unwrap();for reply in controls.into_iter().chain(ordinary){reply.recv().unwrap().unwrap();}
        let log=order.lock().unwrap();assert_eq!(log[0],"control");assert!(log.iter().position(|s|*s=="ordinary").unwrap()<=8);assert!(store.shutdown());
    }
    #[test]
    fn expired_queued_audit_never_executes_after_the_caller_times_out() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        let (entered,ready)=mpsc::sync_channel(1);let (release,held)=mpsc::sync_channel(1);
        let blocker=store.submit(true,1024,move|_|{entered.send(()).unwrap();held.recv().unwrap();Ok(Value::Null)}).unwrap();ready.recv().unwrap();
        let executed=Arc::new(std::sync::atomic::AtomicBool::new(false));let state=executed.clone();
        let started=Instant::now();assert!(store.commit(1024,move|_|{state.store(true,std::sync::atomic::Ordering::SeqCst);Ok(Value::Null)}).is_err());
        assert!(started.elapsed()<Duration::from_secs(3));release.send(()).unwrap();blocker.recv().unwrap().unwrap();assert!(store.shutdown());assert!(!executed.load(std::sync::atomic::Ordering::SeqCst));
    }
}

#[cfg(test)]mod capacity_tests{
    use super::*;
    #[test]fn audit_reclaims_before_sqlite_page_ceiling_and_preserves_active_calls(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();
        store.commit(1024,|db|{db.facts.pragma_update(None,"max_page_count",1024)?;Ok(Value::Null)}).unwrap();
        let active=store.call_start(&crate::Caller::shell(),"pinned",&json!({})).unwrap();
        for n in 0..1200 {let id=store.call_start(&crate::Caller::shell(),"loop",&json!({})).unwrap();store.call_finish(&id,&Ok(Value::Null)).unwrap();if n%20==0 {let _=store.write(Partition::Facts,4096,|db|{db.admit(Partition::Facts,4096)?;Ok(Value::Null)});}}
        store.call_finish(&active,&Ok(Value::Null)).unwrap();
        store.commit(1024,|db|{let calls:i64=db.facts.query_row("SELECT count(*) FROM calls",[],|r|r.get(0))?;assert!(calls<1201&&calls>0);let pending:i64=db.facts.query_row("SELECT count(*) FROM calls WHERE ended IS NULL",[],|r|r.get(0))?;assert_eq!(pending,0);Ok(Value::Null)}).unwrap();
        assert!(store.shutdown());
    }
    #[test]fn evidence_reservations_compete_with_actual_partition_usage_and_release(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let mut db=Databases::open(root.path()).unwrap();
        let first=db.reserve_evidence("one").unwrap();let second=db.reserve_evidence("two").unwrap();assert!(first>second&&second>=256*1024);
        assert_eq!(db.admit_evidence("one",first+1).unwrap_err().code,"CAPACITY");
        db.admit_evidence("one",64*1024).unwrap();assert_eq!(db.evidence_reservations["one"],first-64*1024);
        db.release_evidence("one");db.release_evidence("two");assert!(db.evidence_reservations.is_empty());
        assert!(db.reserve_evidence("three").unwrap()>=first);
    }
    #[test]
    #[ignore="writes all four real partition budgets including a pinned WAL; run explicitly for capacity acceptance"]
    fn real_four_partition_capacity_and_pinned_wal_stay_bounded(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let db=Databases::open(root.path()).unwrap();
        for partition in PARTITIONS{
            let connection=db.db(partition);connection.execute_batch("CREATE TABLE capacity_fixture(value BLOB); INSERT INTO capacity_fixture VALUES(zeroblob(65536)); PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
            let reader=Connection::open(root.path().join(partition.file())).unwrap();reader.execute_batch("BEGIN;").unwrap();let _:i64=reader.query_row("SELECT count(*) FROM capacity_fixture",[],|r|r.get(0)).unwrap();
            let mut rows=1;let mut rejected=false;
            for _ in 0..5000{
                let admitted=if partition==Partition::Facts{db.admit_audit(65536)}else{db.admit(partition,65536)};
                if admitted.is_err()||connection.execute("INSERT INTO capacity_fixture VALUES(zeroblob(65536))",[]).is_err(){rejected=true;break;}
                rows+=1;assert!(db.physical_bytes(partition).unwrap()<=partition.budget());
            }
            assert!(rejected&&rows>100);let pinned=db.physical_bytes(partition).unwrap();assert!(pinned<=partition.budget());
            reader.execute_batch("ROLLBACK;").unwrap();connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();assert!(db.physical_bytes(partition).unwrap()<=partition.budget());
            println!("{} rows={} pinnedBytes={} budget={}",partition.file(),rows,pinned,partition.budget());
        }
    }
}

#[cfg(test)]mod grouped_audit_tests{
    use super::*;
    #[test]fn individual_failure_rolls_back_and_shared_commit_failure_never_reports_success(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let mut db=Databases::open(root.path()).unwrap();
        db.facts.execute_batch("CREATE TABLE test_audit(id INTEGER PRIMARY KEY);CREATE TABLE test_parent(id INTEGER PRIMARY KEY);CREATE TABLE test_child(id INTEGER REFERENCES test_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
        let build=|work:AuditWork|{let(reply,received)=mpsc::sync_channel(1);(Job{work:WorkItem::Audit(work),partition:Partition::Facts,bytes:4096,deadline:Instant::now()+Duration::from_secs(2),reply},received)};
        let(good,_)=build(Box::new(|tx|{tx.execute("INSERT INTO test_audit VALUES(1)",[])?;Ok(json!("good"))}));
        let(bad,_)=build(Box::new(|tx|{tx.execute("INSERT INTO test_audit VALUES(2)",[])?;Err(CoreError::new("INJECTED","item failed"))}));
        let results=db.audit_batch(vec![good,bad]);assert_eq!(results[0].1.as_ref().unwrap(),&json!("good"));assert!(results[1].1.is_err());assert_eq!(db.facts.query_row("SELECT count(*) FROM test_audit",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        let(a,_)=build(Box::new(|tx|{tx.execute("INSERT INTO test_audit VALUES(3)",[])?;Ok(Value::Null)}));
        let(b,_)=build(Box::new(|tx|{tx.execute("INSERT INTO test_child VALUES(999)",[])?;Ok(Value::Null)}));
        assert!(db.audit_batch(vec![a,b]).iter().all(|(_,result,_)|result.is_err()));assert_eq!(db.facts.query_row("SELECT count(*) FROM test_audit",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
}
