//! The physical dataset is versioned and replaced by the startup coordinator.
use crate::{CoreError,Result};
use rusqlite::Connection;
use std::path::Path;

pub const VERSION:u32=2;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Partition {Facts,Logs,Traffic,Diagnostics}
pub const PARTITIONS:[Partition;4]=[Partition::Facts,Partition::Logs,Partition::Traffic,Partition::Diagnostics];
impl Partition {
    pub fn index(self)->usize {match self{Self::Facts=>0,Self::Logs=>1,Self::Traffic=>2,Self::Diagnostics=>3}}
    pub fn file(self)->&'static str {match self {Self::Facts=>"facts.sqlite",Self::Logs=>"logs.sqlite",Self::Traffic=>"traffic.sqlite",Self::Diagnostics=>"diagnostics.sqlite"}}
    pub fn budget(self)->u64 {(match self {Self::Facts=>256,Self::Logs=>128,Self::Traffic=>96,Self::Diagnostics=>32})*1024*1024}
}
pub fn initialize(path:&Path,partition:Partition,epoch:&str)->Result<()> {
    if path.exists(){return Err(CoreError::new("DATABASE_RECOVERY_REQUIRED","观测新库目标已经存在"));}
    let db=Connection::open(path)?;
    db.execute_batch("PRAGMA page_size=4096; PRAGMA auto_vacuum=INCREMENTAL; PRAGMA synchronous=FULL;
        CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        INSERT INTO meta VALUES('sequence','0'),('pruneRevision','0'),('sequenceReserved','0'); PRAGMA user_version=2;")?;
    db.execute("INSERT INTO meta VALUES('storeEpoch',?)",[epoch])?;
    let pages=partition.budget()*4/5/4096;
    db.pragma_update(None,"max_page_count",pages as u32)?;
    match partition {
        Partition::Facts=>db.execute_batch("CREATE TABLE events(
            event_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL UNIQUE,time INTEGER NOT NULL,kind TEXT NOT NULL,event_type TEXT NOT NULL,
            source TEXT NOT NULL,boot_id TEXT NOT NULL,config_epoch TEXT NOT NULL,target TEXT,resource TEXT,trace_id TEXT,call_id TEXT,outcome TEXT,
            payload TEXT NOT NULL CHECK(length(payload)<=16384));
            CREATE INDEX events_time ON events(time,sequence);CREATE INDEX events_target ON events(target,time);CREATE INDEX events_trace ON events(trace_id,sequence);CREATE INDEX events_call ON events(call_id,sequence);
            CREATE INDEX events_type_sequence ON events(event_type,sequence);
            CREATE TRIGGER events_pruned AFTER DELETE ON events BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='pruneRevision'; END;
            CREATE TABLE calls(id TEXT PRIMARY KEY,started INTEGER NOT NULL,ended INTEGER,caller TEXT NOT NULL,method TEXT NOT NULL,server_id TEXT,summary TEXT NOT NULL,status TEXT NOT NULL,error_code TEXT);
            CREATE INDEX calls_started ON calls(started,id);
            CREATE TABLE operation_receipts(operation_id TEXT PRIMARY KEY,caller_key TEXT NOT NULL,request_hash TEXT NOT NULL,result TEXT NOT NULL,created_at INTEGER NOT NULL);")?,
        Partition::Logs=>db.execute_batch("CREATE TABLE logs(event_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL UNIQUE,time INTEGER NOT NULL,source TEXT NOT NULL,level TEXT NOT NULL,payload TEXT NOT NULL CHECK(length(payload)<=16384)); CREATE INDEX logs_time ON logs(time);")?,
        Partition::Traffic=>{
            db.execute_batch("CREATE TABLE identities(identity_id TEXT PRIMARY KEY,config_epoch TEXT NOT NULL,boot_id TEXT NOT NULL,target TEXT NOT NULL,owner TEXT NOT NULL,kind TEXT NOT NULL,layer TEXT NOT NULL,ended INTEGER);
                CREATE TABLE path_segments(route_run_id TEXT PRIMARY KEY,config_epoch TEXT NOT NULL,boot_id TEXT NOT NULL,target TEXT NOT NULL,path TEXT NOT NULL);
                CREATE TABLE resource_totals(identity_id TEXT PRIMARY KEY,sent BLOB NOT NULL CHECK(length(sent)=8),received BLOB NOT NULL CHECK(length(received)=8),updated INTEGER NOT NULL);
                CREATE TABLE quality_buckets(target TEXT NOT NULL,boot_id TEXT NOT NULL,from_utc INTEGER NOT NULL,samples INTEGER NOT NULL,timeouts INTEGER NOT NULL,total_ms REAL NOT NULL,maximum_ms REAL,PRIMARY KEY(target,boot_id,from_utc));
                CREATE TABLE speedtest_results(task_id TEXT PRIMARY KEY,config_epoch TEXT NOT NULL,target TEXT NOT NULL,created_at INTEGER NOT NULL,result TEXT NOT NULL);
                CREATE TABLE gaps(gap_id TEXT PRIMARY KEY,config_epoch TEXT NOT NULL,boot_id TEXT NOT NULL,target TEXT,from_utc INTEGER NOT NULL,to_utc INTEGER,reason TEXT NOT NULL);
                CREATE INDEX gaps_time ON gaps(from_utc);")?;
            for name in ["buckets_5s","buckets_1m","buckets_1h"] {
                db.execute_batch(&format!("CREATE TABLE {name}(config_epoch TEXT NOT NULL,boot_id TEXT NOT NULL,owner TEXT NOT NULL,target TEXT NOT NULL,layer TEXT NOT NULL,from_utc INTEGER NOT NULL,to_utc INTEGER NOT NULL,sent BLOB NOT NULL CHECK(length(sent)=8),received BLOB NOT NULL CHECK(length(received)=8),valid_sent BLOB NOT NULL CHECK(length(valid_sent)=8),valid_received BLOB NOT NULL CHECK(length(valid_received)=8),valid_ms INTEGER NOT NULL,samples INTEGER NOT NULL,gap_ms INTEGER NOT NULL,peak_send REAL,peak_receive REAL,peak_resolution_ms INTEGER NOT NULL,invalid INTEGER NOT NULL,PRIMARY KEY(config_epoch,boot_id,owner,layer,from_utc));CREATE INDEX {name}_target_time ON {name}(config_epoch,target,layer,from_utc);"))?;
            }
        },
        Partition::Diagnostics=>db.execute_batch("CREATE TABLE issues(issue_id TEXT PRIMARY KEY,aggregation_key TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL,state TEXT NOT NULL,first_seen INTEGER NOT NULL,last_seen INTEGER NOT NULL,data TEXT NOT NULL);
            CREATE TABLE episodes(episode_id TEXT PRIMARY KEY,issue_id TEXT NOT NULL,started INTEGER NOT NULL,ended INTEGER,data TEXT NOT NULL);
            CREATE TABLE issue_changes(event_id TEXT NOT NULL,issue_id TEXT NOT NULL,sequence INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(event_id,issue_id));
            CREATE INDEX issue_changes_scope ON issue_changes(issue_id,sequence);
            CREATE TRIGGER issue_changes_pruned AFTER DELETE ON issue_changes BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='pruneRevision'; END;
            CREATE TABLE projection_checkpoint(source_epoch TEXT PRIMARY KEY,sequence INTEGER NOT NULL);
            CREATE TABLE snapshots(snapshot_id TEXT PRIMARY KEY,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,data TEXT NOT NULL);
            CREATE TABLE evidence_jobs(job_id TEXT PRIMARY KEY,created_at INTEGER NOT NULL,state TEXT NOT NULL,manifest TEXT NOT NULL);
            CREATE TABLE notification_receipts(receipt_id TEXT PRIMARY KEY,issue_id TEXT NOT NULL,created_at INTEGER NOT NULL);")?,
    }
    if matches!(partition,Partition::Facts|Partition::Logs) {
        db.execute_batch("CREATE TABLE records(event_id TEXT PRIMARY KEY,sequence INTEGER NOT NULL UNIQUE,received INTEGER NOT NULL,occurred INTEGER NOT NULL,config_epoch TEXT NOT NULL,boot_id TEXT NOT NULL,plugin_id TEXT,instance_id TEXT NOT NULL,kind TEXT NOT NULL,event_type TEXT NOT NULL,level TEXT,outcome TEXT,error_code TEXT,trace_id TEXT,call_id TEXT,operation_id TEXT,client_id TEXT,content_hash TEXT,envelope TEXT NOT NULL CHECK(length(CAST(envelope AS BLOB))<=16384));
            CREATE UNIQUE INDEX records_client ON records(instance_id,client_id) WHERE client_id IS NOT NULL;
            CREATE INDEX records_time ON records(received,sequence);CREATE INDEX records_type ON records(plugin_id,event_type,sequence);CREATE INDEX records_trace ON records(trace_id,sequence);CREATE INDEX records_call ON records(call_id,sequence);CREATE INDEX records_operation ON records(operation_id,sequence);
            CREATE TABLE record_targets(event_id TEXT NOT NULL REFERENCES records(event_id) ON DELETE CASCADE,kind TEXT NOT NULL,reference TEXT NOT NULL,generation TEXT,PRIMARY KEY(event_id,kind,reference));CREATE INDEX targets_ref ON record_targets(reference,event_id);
            CREATE TABLE type_catalog(plugin_id TEXT NOT NULL,category TEXT NOT NULL,name TEXT NOT NULL,version INTEGER NOT NULL,descriptor TEXT NOT NULL,PRIMARY KEY(plugin_id,category,name,version));
            CREATE TRIGGER records_pruned AFTER DELETE ON records BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='pruneRevision'; END;")?;
    }
    if partition==Partition::Facts {
        db.execute("INSERT INTO meta VALUES('signingKey',?)",[format!("{}{}",crate::model::id(),crate::model::id())])?;
        db.execute_batch("CREATE TABLE diagnostic_targets(reference TEXT PRIMARY KEY,kind TEXT NOT NULL,object_id TEXT NOT NULL,generation TEXT,created INTEGER NOT NULL);
            CREATE INDEX diagnostic_targets_object ON diagnostic_targets(kind,object_id);
            CREATE TABLE observation_policy(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL);
            CREATE TABLE observation_spans(span_id TEXT PRIMARY KEY,instance_id TEXT NOT NULL,client_id TEXT NOT NULL,content_hash TEXT NOT NULL,trace_id TEXT NOT NULL,parent_span_id TEXT,action_id TEXT NOT NULL,created INTEGER NOT NULL,touched INTEGER NOT NULL,outcome TEXT,context TEXT NOT NULL,definition TEXT NOT NULL,UNIQUE(instance_id,client_id));
            CREATE INDEX spans_trace ON observation_spans(trace_id,created);
            CREATE TABLE call_context(call_id TEXT PRIMARY KEY REFERENCES calls(id) ON DELETE CASCADE,data TEXT NOT NULL);
            CREATE TRIGGER records_prune_legacy AFTER DELETE ON records BEGIN DELETE FROM events WHERE event_id=OLD.event_id; END;
            CREATE TRIGGER legacy_prune_record AFTER DELETE ON events BEGIN DELETE FROM records WHERE event_id=OLD.event_id; END;")?;
    }
    if partition==Partition::Diagnostics {
        db.execute_batch("CREATE TABLE type_catalog(plugin_id TEXT NOT NULL,category TEXT NOT NULL,name TEXT NOT NULL,version INTEGER NOT NULL,descriptor TEXT NOT NULL,PRIMARY KEY(plugin_id,category,name,version));")?;
        db.execute_batch("CREATE TABLE observation_snapshots(id TEXT PRIMARY KEY,instance_id TEXT NOT NULL,plugin_id TEXT NOT NULL,scope TEXT NOT NULL,type TEXT NOT NULL,revision TEXT NOT NULL,received INTEGER NOT NULL,expires INTEGER NOT NULL,content_hash TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(instance_id,scope,type));
            CREATE TABLE observation_metrics(series TEXT NOT NULL,window INTEGER NOT NULL,sample_id TEXT NOT NULL,content_hash TEXT NOT NULL,data TEXT NOT NULL,instance_id TEXT NOT NULL,PRIMARY KEY(series,window),UNIQUE(instance_id,sample_id));
            CREATE TABLE metric_buckets(series TEXT NOT NULL,minute INTEGER NOT NULL,plugin_id TEXT NOT NULL,scope TEXT NOT NULL,type TEXT NOT NULL,data TEXT NOT NULL,PRIMARY KEY(series,minute));
            CREATE TABLE metric_closed(series TEXT PRIMARY KEY,window INTEGER NOT NULL);
            CREATE TABLE observation_jobs(id TEXT PRIMARY KEY,created INTEGER NOT NULL,updated INTEGER NOT NULL,state TEXT NOT NULL,data TEXT NOT NULL);
            CREATE TRIGGER snapshot_insert_revision AFTER INSERT ON observation_snapshots BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='sequence'; END;
            CREATE TRIGGER snapshot_delete_revision AFTER DELETE ON observation_snapshots BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='sequence'; END;
            CREATE TRIGGER snapshot_update_revision AFTER UPDATE ON observation_snapshots BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='sequence'; END;
            CREATE TRIGGER metric_insert_revision AFTER INSERT ON metric_buckets BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='sequence'; END;
            CREATE TRIGGER metric_delete_revision AFTER DELETE ON metric_buckets BEGIN UPDATE meta SET value=CAST(value AS INTEGER)+1 WHERE key='sequence'; END;
            CREATE TABLE evidence_chunks(job_id TEXT NOT NULL REFERENCES observation_jobs(id) ON DELETE CASCADE,position INTEGER NOT NULL,data BLOB NOT NULL,PRIMARY KEY(job_id,position));")?;
    }
    drop(db);std::fs::OpenOptions::new().write(true).open(path)?.sync_all()?;Ok(())
}
