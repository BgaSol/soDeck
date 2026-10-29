//! Communication history is a profile of the common store, never a second audit log.
use super::{schema::Partition,writer::{Databases,Observability}};
use crate::{CoreError,Result,traffic::Sample};
use rusqlite::{params,Connection,Row};
use serde_json::{json,Value};

#[derive(Clone)]
pub(crate) struct Bucket {pub owner:String,pub target:String,pub layer:String,pub from:u64,pub sample:Sample}
const TABLES:[&str;3]=["buckets_5s","buckets_1m","buckets_1h"];
const DAY:u64=24*60*60*1000;
fn bytes(row:&Row<'_>,index:usize)->rusqlite::Result<u64> {
    let value:Vec<u8>=row.get(index)?;
    Ok(u64::from_be_bytes(value.try_into().map_err(|_|rusqlite::Error::InvalidColumnType(index,"bytes".into(),rusqlite::types::Type::Blob))?))
}
fn decode(row:&Row<'_>)->rusqlite::Result<Bucket> {
    let valid=super::integer(row,9)?;let from=super::integer(row,3)?;let until=super::integer(row,4)?;let gap=super::integer(row,11)?;
    if until<from||valid>until-from{return Err(rusqlite::Error::InvalidQuery);}
    Ok(Bucket{owner:row.get(0)?,target:row.get(1)?,layer:row.get(2)?,from,sample:Sample {
        timestamp:until,duration_ms:(until.saturating_sub(from)).min(u32::MAX as u64)as u32,sent:bytes(row,5)?,received:bytes(row,6)?,
        valid_sent:bytes(row,7)?,valid_received:bytes(row,8)?,valid_ms:valid,samples:row.get(10)?,gap:gap>0,invalid:row.get(15)?,
        peak_send:row.get::<_,Option<f64>>(12)?.unwrap_or(0.0),peak_receive:row.get::<_,Option<f64>>(13)?.unwrap_or(0.0),peak_resolution_ms:row.get(14)?,
    }})
}
const COLUMNS:&str="owner,target,layer,from_utc,to_utc,sent,received,valid_sent,valid_received,valid_ms,samples,gap_ms,peak_send,peak_receive,peak_resolution_ms,invalid";
fn put(db:&Connection,table:&str,epoch:&str,boot:&str,b:&Bucket)->Result<()> {
    let s=b.sample;
    db.execute(&format!("INSERT INTO {table}(config_epoch,boot_id,{COLUMNS}) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"),
        params![epoch,boot,b.owner,b.target,b.layer,b.from as i64,s.timestamp as i64,s.sent.to_be_bytes().as_slice(),s.received.to_be_bytes().as_slice(),s.valid_sent.to_be_bytes().as_slice(),s.valid_received.to_be_bytes().as_slice(),s.valid_ms as i64,s.samples,
            if s.gap{s.timestamp.saturating_sub(b.from).saturating_sub(s.valid_ms).max(1)}else{0}as i64,
            (s.valid_ms>0).then_some(s.peak_send),(s.valid_ms>0).then_some(s.peak_receive),s.peak_resolution_ms,s.invalid])?;Ok(())
}
impl Observability {
    pub fn save_traffic(&self,buckets:Vec<Bucket>)->Result<()> {
        let charge=buckets.iter().map(|b|std::mem::size_of::<Bucket>()+b.owner.capacity()+b.target.capacity()+b.layer.capacity()+128).sum::<usize>();
        let epoch=self.config_epoch.clone();let boot=self.boot_id.clone();
        // Capture one bounded batch. Runtime sampling never waits for SQLite.
        self.submit_partition(Partition::Traffic,false,charge,move|db|{
            prepare_capacity(db)?;db.admit(Partition::Traffic,charge)?;
            let tx=db.traffic.transaction()?;
            for bucket in buckets {put(&tx,TABLES[0],&epoch,&boot,&bucket)?;}
            let old:String=tx.query_row("SELECT value FROM meta WHERE key='sequence'",[],|r|r.get(0))?;
            let next=old.parse::<i64>().ok().and_then(|n|n.checked_add(1)).ok_or_else(||CoreError::new("CAPACITY","历史水位已用尽"))?;
            tx.execute("UPDATE meta SET value=? WHERE key='sequence'",[next.to_string()])?;
            tx.commit()?;Ok(Value::Null)
        })?;Ok(())
    }
    pub fn history(&self,target:&Value,layer:&str,from:u64,to:u64)->Result<Value> {
        if to<=from||to-from>30*DAY{return Err(CoreError::new("INVALID_PARAMS","历史范围必须大于零且不超过30天"));}
        let epoch=target["configEpoch"].as_str().ok_or_else(||CoreError::new("INVALID_PARAMS","缺少配置代次"))?.to_owned();
        let owner=match target["kind"].as_str() {
            Some("all")=>format!("{layer}:all"),Some("server")=>format!("{layer}:server:{}",target["serverId"].as_str().unwrap()),
            Some("resource")=>return Err(CoreError::new("NOT_RECORDED","此资源没有可用的持久明细")),
            _=>return Err(CoreError::new("INVALID_PARAMS","无效历史目标")),
        };
        let target=target.clone();let layer=layer.to_owned();let store_epoch=self.epoch.clone();
        self.queries.query(Partition::Traffic,move|db|{
            let tx=db.unchecked_transaction()?;
            let watermark:String=tx.query_row("SELECT value FROM meta WHERE key='sequence'",[],|r|r.get(0))?;
            let mut base=5000u64;
            for (table,granularity) in TABLES.into_iter().zip([5000,60_000,3600_000]) {
                let present:bool=tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE config_epoch=? AND owner=? AND to_utc>? AND from_utc<?)"),params![epoch,owner,from as i64,to as i64],|r|r.get(0))?;
                if present{base=base.max(granularity);}
            }
            let resolution=(to-from).div_ceil(1200).div_ceil(base).max(1)*base;
            let mut points=vec![Aggregate::default();(to-from).div_ceil(resolution)as usize];
            let mut total=Aggregate::default();let mut earliest:Option<u64>=None;let mut boundary=false;
            for table in TABLES {
                let first=tx.query_row(&format!("SELECT MIN(from_utc) FROM {table} WHERE config_epoch=? AND owner=?"),params![epoch,owner],|r|super::optional_integer(r,0))?;
                if let Some(first)=first{earliest=Some(earliest.map_or(first,|old|old.min(first)));}
                let mut statement=tx.prepare(&format!("SELECT {COLUMNS} FROM {table} WHERE config_epoch=? AND owner=? AND to_utc>? AND from_utc<? ORDER BY from_utc"))?;
                let rows=statement.query_map(params![epoch,owner,from as i64,to as i64],decode)?;
                for row in rows {
                    let row=row?;
                    // Whole intervals are authoritative; never invent how bytes split at a boundary.
                    if row.from<from||row.sample.timestamp>to{boundary=true;continue;}
                    let index=((row.sample.timestamp.saturating_sub(from+1))/resolution)as usize;
                    points[index].add(&row);total.add(&row);
                }
            }
            let points:Vec<_>=points.into_iter().enumerate().map(|(i,p)|p.value(from+i as u64*resolution,(from+(i as u64+1)*resolution).min(to))).collect();
            let partial=boundary||total.sample.valid_ms<to-from||total.sample.gap||total.sample.invalid;
            let statistics=total.value(from,to);
            let result=json!({"kind":"stored","target":target,"layer":layer,"storeEpoch":store_epoch,"highWatermark":watermark,
                "fromUtc":from,"toUtc":to,"retainedFromUtc":earliest,"resolutionMs":resolution,"points":points,"statistics":statistics,
                "coverage":if partial{"partial"}else{"complete"},"boundaryOmitted":boundary});
            tx.commit()?;Ok(result)
        })
    }
}

#[derive(Clone,Default)]struct Aggregate {sample:Sample,from:Option<u64>,to:u64}
impl Aggregate {
    fn add(&mut self,b:&Bucket) {
        if self.from.is_none(){self.sample=b.sample;self.from=Some(b.from);self.to=b.sample.timestamp;return;}
        // SQL iterates each disjoint tier independently; gaps are derived from coverage.
        let gap=self.sample.gap||b.sample.gap;
        let mut newest=b.sample;newest.merge(self.sample);newest.gap=gap;self.sample=newest;
        self.from=Some(self.from.unwrap().min(b.from));self.to=self.to.max(b.sample.timestamp);
    }
    fn value(&self,from:u64,to:u64)->Value {
        let s=self.sample;let known=s.valid_ms>0&&!s.invalid;
        json!({"fromUtc":from,"toUtc":to,"sentBytes":s.sent.to_string(),"receivedBytes":s.received.to_string(),"validMs":s.valid_ms,
            "sendRate":known.then(||s.valid_sent as f64*1000.0/s.valid_ms as f64),"receiveRate":known.then(||s.valid_received as f64*1000.0/s.valid_ms as f64),
            "peakSendRate":known.then_some(s.peak_send),"peakReceiveRate":known.then_some(s.peak_receive),"peakResolutionMs":s.peak_resolution_ms.max(1),"sampleCount":s.samples,
            "gap":self.from.is_none()||s.gap||s.invalid||s.valid_ms<to-from,"invalid":s.invalid})
    }
}

fn prepare_capacity(db:&mut Databases)->Result<()> {
    let now=crate::model::now_ms();
    promote(&mut db.traffic,0,now.saturating_sub(DAY))?;
    promote(&mut db.traffic,1,now.saturating_sub(7*DAY))?;
    db.traffic.execute("DELETE FROM buckets_1h WHERE rowid IN(SELECT rowid FROM buckets_1h WHERE to_utc<? ORDER BY to_utc LIMIT 32)",[now.saturating_sub(30*DAY)as i64])?;
    if db.admit(Partition::Traffic,128*1024).is_err() {
        // Space pressure advances compaction before TTL. Coarse data replaces source atomically.
        promote(&mut db.traffic,0,now.saturating_sub(60_000))?;promote(&mut db.traffic,1,now.saturating_sub(3600_000))?;
        db.traffic.execute_batch("PRAGMA incremental_vacuum(64);PRAGMA wal_checkpoint(TRUNCATE)")?;
    }
    Ok(())
}
fn promote(db:&mut Connection,tier:usize,before:u64)->Result<()> {
    let source=TABLES[tier];let destination=TABLES[tier+1];let resolution=if tier==0{60_000}else{3600_000};
    // One owner/time group bounds work independently of database size.
    let group={let mut statement=db.prepare(&format!("SELECT config_epoch,boot_id,owner,from_utc/? FROM {source} WHERE to_utc<? ORDER BY from_utc LIMIT 1"))?;
        let mut rows=statement.query(params![resolution,before as i64])?;
        rows.next()?.map(|r|Ok::<_,rusqlite::Error>((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?))).transpose()?};
    let Some((epoch,boot,owner,group))=group else{return Ok(())};
    let tx=db.transaction()?;
    let mut rows={let mut statement=tx.prepare(&format!("SELECT {COLUMNS} FROM {source} WHERE config_epoch=? AND boot_id=? AND owner=? AND from_utc/?=? ORDER BY from_utc LIMIT 129"))?;
        let result=statement.query_map(params![epoch,boot,owner,resolution,group],decode)?.collect::<std::result::Result<Vec<_>,_>>()?;result};
    if rows.len()>128{return Err(CoreError::new("CAPACITY","历史汇总组超过单批预算"));}
    let Some(mut combined)=rows.pop()else{return Ok(())};
    for row in rows.iter().rev() {combined.from=combined.from.min(row.from);combined.sample.merge(row.sample);}
    // Each fine group is promoted once, and only after its full interval has ended.
    if (group as u64+1)*resolution as u64>before{return Ok(());}
    put(&tx,destination,&epoch,&boot,&combined)?;
    tx.execute(&format!("DELETE FROM {source} WHERE config_epoch=? AND boot_id=? AND owner=? AND from_utc/?=?"),params![epoch,boot,owner,resolution,group])?;
    tx.commit()?;Ok(())
}

#[cfg(test)]mod tests {
    use super::*;
    fn bucket(end:u64,sent:u64,receive:u64)->Bucket{Bucket{owner:"channel:all".into(),target:"@all".into(),layer:"channel".into(),from:end-5000,sample:Sample::new(end,5000,sent,receive,false,false)}}
    #[test]
    fn compacted_history_conserves_bytes_weighted_rates_and_fine_peaks() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        store.commit(64*1024,|db|{
            for tick in 1..=24 {put(&db.traffic,TABLES[0],"epoch","boot",&bucket(tick*5000,if tick==7{10000}else{100},50))?;}
            Ok(Value::Null)
        }).unwrap();
        let target=json!({"kind":"all","configEpoch":"epoch"});
        let first=store.history(&target,"channel",0,120000).unwrap();
        assert_eq!(first["statistics"]["sentBytes"],"12300");assert_eq!(first["statistics"]["validMs"],120000);
        assert_eq!(first["statistics"]["sendRate"],102.5);assert_eq!(first["statistics"]["peakSendRate"],2000.0);assert_eq!(first["coverage"],"complete");
        store.commit(64*1024,|db|{promote(&mut db.traffic,0,180000)?;promote(&mut db.traffic,0,180000)?;Ok(Value::Null)}).unwrap();
        let minute=store.history(&target,"channel",0,120000).unwrap();assert_eq!(minute["statistics"],first["statistics"]);assert_eq!(minute["resolutionMs"],60000);
        store.commit(64*1024,|db|{promote(&mut db.traffic,1,3600000)?;assert_eq!(db.traffic.query_row("SELECT count(*) FROM buckets_5s",[],|r|r.get::<_,u32>(0))?,0);assert_eq!(db.traffic.query_row("SELECT count(*) FROM buckets_1m",[],|r|r.get::<_,u32>(0))?,0);Ok(Value::Null)}).unwrap();
        let hour=store.history(&target,"channel",0,120000).unwrap();assert_eq!(hour["statistics"],first["statistics"]);assert_eq!(hour["resolutionMs"],3600000);
        let clipped=store.history(&target,"channel",1,120000).unwrap();assert_eq!(clipped["boundaryOmitted"],true);assert_eq!(clipped["statistics"]["sentBytes"],"0");
        assert!(store.shutdown());
    }
    #[test]
    fn promotion_failure_retains_sources_and_offline_time_is_not_zero() {
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();
        let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        store.commit(64*1024,|db|{
            put(&db.traffic,TABLES[0],"epoch","boot-a",&bucket(5000,10,20))?;put(&db.traffic,TABLES[0],"epoch","boot-b",&bucket(65000,30,40))?;
            db.traffic.execute_batch("CREATE TRIGGER reject_promotion BEFORE INSERT ON buckets_1m BEGIN SELECT RAISE(ABORT,'injected');END;")?;
            assert!(promote(&mut db.traffic,0,180000).is_err());assert_eq!(db.traffic.query_row("SELECT count(*) FROM buckets_5s",[],|r|r.get::<_,u32>(0))?,2);Ok(Value::Null)
        }).unwrap();
        let result=store.history(&json!({"kind":"all","configEpoch":"epoch"}),"channel",0,65000).unwrap();
        assert_eq!(result["statistics"]["sentBytes"],"40");assert_eq!(result["statistics"]["validMs"],10000);assert_eq!(result["statistics"]["sendRate"],4.0);assert_eq!(result["coverage"],"partial");
        assert_eq!(result["points"][1]["sendRate"],Value::Null);assert!(store.shutdown());
    }
    #[test]fn corrupt_negative_history_is_rejected_instead_of_becoming_zero(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"epoch".into()).unwrap();
        store.commit(4096,|db|{put(&db.traffic,TABLES[0],"epoch","boot",&bucket(5000,10,20))?;db.traffic.execute("UPDATE buckets_5s SET valid_ms=-1",[])?;Ok(Value::Null)}).unwrap();
        assert!(store.history(&json!({"kind":"all","configEpoch":"epoch"}),"channel",0,10000).is_err());assert!(store.shutdown());
    }
}
