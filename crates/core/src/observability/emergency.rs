//! Bounded failure facts. No request, result, exception text or file paths enter this channel.
use crate::model::{id,now_ms};
use serde::{Serialize,Deserialize};
use std::{collections::VecDeque,fs,io::{Write,BufRead,Seek,SeekFrom},path::{Path,PathBuf},sync::Mutex};

// Startup append/rotation and the archive read-to-delete handoff share one lock.
// The data-root process lock excludes writers from another application process.
static STARTUP_IO:Mutex<()>=Mutex::new(());
pub fn with_startup_diagnostics_io<T>(write:impl FnOnce()->T)->T{let _guard=STARTUP_IO.lock().unwrap_or_else(|e|e.into_inner());write()}

const MEMORY:usize=1024*1024;
// The existing startup channel occupies at most 6 MiB; runtime uses the remaining 2 MiB.
const SEGMENT:u64=512*1024;
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Failure {event_id:String,time:u64,code:String}
impl Failure {fn bytes(&self)->usize{std::mem::size_of::<Self>()+self.event_id.capacity()+self.code.capacity()+64}}
#[derive(Default)]
struct State {pending:VecDeque<Failure>,bytes:usize,dropped:u64,persisted:u64,write_failed:bool,archived:u64,archive_offsets:std::collections::HashMap<usize,(String,u64)>}
pub(super) struct Emergency {root:PathBuf,state:Mutex<State>}
impl Emergency {
    pub fn new(root:&Path)->Self {Self{root:root.join("emergency"),state:Mutex::new(State::default())}}
    pub fn record(&self,code:&str) {
        let code=if code.len()<=64&&code.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'.'){code}else{"STORE_ERROR"};
        let failure=Failure{event_id:id(),time:now_ms(),code:code.into()};let bytes=failure.bytes();
        let mut state=self.state.lock().unwrap();
        while state.bytes+bytes>MEMORY {let Some(old)=state.pending.pop_front()else{break};state.bytes-=old.bytes();state.dropped=state.dropped.saturating_add(1);}
        state.bytes+=bytes;state.pending.push_back(failure);
    }
    pub fn flush(&self) {
        // Only the common writer performs disk I/O. At most 32 failures per pass.
        for _ in 0..32 {
            let line={let state=self.state.lock().unwrap();let Some(record)=state.pending.front()else{return};serde_json::to_vec(record).unwrap()};
            let result=self.append(&line);
            let mut state=self.state.lock().unwrap();state.write_failed=result.is_err();
            if result.is_err(){return;}
            // Admission can evict the oldest while disk I/O is running. Remove by id.
            let saved:serde_json::Value=serde_json::from_slice(&line).unwrap();
            if state.pending.front().is_some_and(|r|saved["eventId"]==r.event_id) {
                let record=state.pending.pop_front().unwrap();state.bytes-=record.bytes();
            }
            state.persisted=state.persisted.saturating_add(1);
        }
    }
    pub fn archive(&self,db:&mut super::writer::Databases,boot:&str,config:&str)->crate::Result<()> {
        use rusqlite::OptionalExtension;use serde_json::json;
        for index in (0..7).rev(){let _startup_guard=(index>=4).then(||STARTUP_IO.lock().unwrap_or_else(|e|e.into_inner()));let path=if index<4{self.root.join(format!("failure-{index}.jsonl"))}else{let base=self.root.parent().and_then(Path::parent).unwrap_or(&self.root).join("logs/startup.log");if index==4{base}else{base.with_extension(format!("log.{}",index-4))}};let file=match fs::File::open(&path){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>continue,Err(e)=>return Err(e.into())};
            if file.metadata()?.len()>if index<4{SEGMENT}else{2*1024*1024}{return Err(crate::CoreError::new("CAPACITY","紧急日志段超限"));}
            let mut reader=std::io::BufReader::new(file);let mut first=String::new();reader.read_line(&mut first)?;
            let identity:String=super::model::hash(&json!(first));
            let offset=self.state.lock().unwrap().archive_offsets.get(&index).filter(|(id,_)|id==&identity).map_or(0,|(_,offset)|*offset);reader.seek(SeekFrom::Start(offset))?;
            db.admit_audit(128*1024)?;let tx=db.facts.transaction()?;let key:String=tx.query_row("SELECT value FROM meta WHERE key='signingKey'",[],|r|r.get(0))?;let epoch:String=tx.query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get(0))?;let mut copied=0;let mut complete=false;
            for _ in 0..32{let mut line=String::new();if reader.read_line(&mut line)?==0{complete=true;break;}
                if line.len()>1024{continue;}let value:Failure=match serde_json::from_str(&line){Ok(v)=>v,Err(_)=>{let parts=line.split_whitespace().collect::<Vec<_>>();if parts.len()!=3||!["DATA_LOCK_ACQUIRED","STARTUP_FAILED","STARTUP_WARNING","HOST_DIAGNOSTIC"].contains(&parts[2]){continue;}let Some(time)=parts[0].parse::<u64>().ok().and_then(|n|n.checked_mul(1000))else{continue};Failure{event_id:super::model::hash(&json!([identity,line])),time,code:parts[2].into()}}};
                if !super::descriptor::identifier(&value.event_id)||!super::descriptor::identifier(&value.code){continue;}
                let existing:Option<String>=tx.query_row("SELECT event_id FROM records WHERE instance_id='observation-emergency' AND client_id=?",[&value.event_id],|r|r.get(0)).optional()?;if existing.is_some(){continue;}
                let caller=crate::Caller{source:"core".into(),instance_id:"observation-emergency".into(),plugin_id:None,plugin_version:None,view_id:None,mount_generation:None};
                let mut event=super::model::envelope(&caller,boot,&epoch,"health","core.observation.recovered",json!({"reasonCode":value.code,"originalTime":value.time}));event["configEpoch"]=json!(config);event["provenance"]=json!("imported-emergency");event["clientEventId"]=json!(value.event_id);event["level"]=json!("warn");event["completeness"]=json!(["emergency-summary-only"]);
                super::catalog::save_type(&tx,"","events",&super::builtin::definition("core.observation.recovered")?)?;
                super::model::insert(&tx,&key,"f",&mut event,Some(&super::model::hash(&json!([value.time,value.code]))))?;copied+=1;
            }tx.commit()?;let offset=reader.stream_position()?;drop(reader);let mut state=self.state.lock().unwrap();state.archived+=copied;state.archive_offsets.insert(index,(identity,offset));drop(state);
            if complete{fs::remove_file(path)?;self.state.lock().unwrap().archive_offsets.remove(&index);}return Ok(());
        }Ok(())
    }
    fn append(&self,line:&[u8])->std::io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let path=|index|self.root.join(format!("failure-{index}.jsonl"));
        if fs::metadata(path(0)).map_or(0,|m|m.len())+line.len()as u64+1>SEGMENT {
            if path(3).exists(){fs::remove_file(path(3))?;}
            for index in (0..3).rev(){if path(index).exists(){fs::rename(path(index),path(index+1))?;}}
        }
        let mut file=fs::OpenOptions::new().create(true).append(true).open(path(0))?;
        file.write_all(line)?;file.write_all(b"\n")?;file.sync_data()
    }
    pub fn archived(&self)->u64{self.state.lock().unwrap().archived}
    pub fn health(&self)->serde_json::Value {
        let state=self.state.lock().unwrap();
        serde_json::json!({"memoryBytes":state.bytes,"diskBudgetBytes":2*1024*1024,"persisted":state.persisted.to_string(),"dropped":state.dropped.to_string(),"writeFailed":state.write_failed})
    }
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn memory_and_rotation_are_bounded_and_errors_are_codes_only(){
        let dir=tempfile::tempdir().unwrap();let channel=Emergency::new(dir.path());
        for _ in 0..10000{channel.record("secret error /private/path");}
        assert!(channel.state.lock().unwrap().bytes<=MEMORY);assert!(channel.state.lock().unwrap().dropped>0);
        fs::create_dir(&channel.root).unwrap();
        for _ in 0..5 {fs::write(channel.root.join("failure-0.jsonl"),vec![b'x';SEGMENT as usize]).unwrap();channel.flush();}
        assert!(fs::read_dir(&channel.root).unwrap().map(|e|e.unwrap().metadata().unwrap().len()).sum::<u64>()<=2*1024*1024);
        let data=fs::read_to_string(channel.root.join("failure-0.jsonl")).unwrap();assert!(!data.contains("secret"));assert!(data.contains("STORE_ERROR"));
    }
}


#[cfg(test)]mod recovery_tests {
    use super::*;use crate::observability::{Observability,schema::Partition};use serde_json::json;
    #[test]fn archived_failure_is_deduplicated_across_replay_and_never_copies_raw_text(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();
        fs::create_dir_all(root.path().join("emergency")).unwrap();let path=root.path().join("emergency/failure-0.jsonl");let bytes=b"secret-path-and-password\n{\"eventId\":\"fixed-failure\",\"time\":1,\"code\":\"STORE_UNAVAILABLE\"}\n";fs::write(&path,bytes).unwrap();
        let read=||store.queries.query(Partition::Facts,|db|{let mut s=db.prepare("SELECT envelope FROM records WHERE instance_id='observation-emergency'")?;let values=s.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(json!(values))}).unwrap();
        for _ in 0..2{let start=std::time::Instant::now();loop{let values=read();if values.as_array().unwrap().len()==1&&!path.exists(){assert!(!values.to_string().contains("secret-path"));assert!(values.to_string().contains("imported-emergency"));break;}assert!(start.elapsed()<std::time::Duration::from_secs(5));std::thread::sleep(std::time::Duration::from_millis(50));}fs::write(&path,bytes).unwrap();}
        assert_eq!(read().as_array().unwrap().len(),1);store.shutdown();
    }
}

#[cfg(test)]mod startup_concurrency_tests{
    use super::*;use crate::observability::{Observability,schema::Partition};use serde_json::json;
    #[test]fn startup_appends_during_archive_are_never_deleted_before_commit(){
        let temp=tempfile::tempdir().unwrap();let root=temp.path().join("observability");Observability::initialize_fixture(&root).unwrap();let store=Observability::open(&root,"boot".into(),"config".into()).unwrap();
        let path=temp.path().join("logs/startup.log");fs::create_dir_all(path.parent().unwrap()).unwrap();
        let writer=std::thread::spawn(move||{for n in 0..100{with_startup_diagnostics_io(||{let mut file=fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();writeln!(file,"{}",json!({"eventId":format!("startup-{n}"),"time":1,"code":"HOST_DIAGNOSTIC"})).unwrap();});std::thread::sleep(std::time::Duration::from_millis(2));}});
        writer.join().unwrap();let began=std::time::Instant::now();loop{let count=store.queries.query(Partition::Facts,|db|Ok(json!(db.query_row("SELECT count(*) FROM records WHERE instance_id='observation-emergency'",[],|r|r.get::<_,i64>(0))?))).unwrap();if count==100{break;}assert!(began.elapsed()<std::time::Duration::from_secs(8),"{count}");std::thread::sleep(std::time::Duration::from_millis(25));}store.shutdown();
    }
}
