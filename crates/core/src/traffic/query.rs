use crate::{CoreError,Result};
use crate::model::{id,now_ms};
use serde_json::{json,Value};
use std::{collections::HashMap,sync::Mutex,time::{Duration,Instant}};

struct Snapshot { owner:String, query:Value, summary:Value, rows:Vec<Value>, other:Value, deadline:Instant, expires_at:u64 }
#[derive(Default)]
pub(crate) struct Queries { snapshots:Mutex<HashMap<String,Snapshot>> }
impl Queries {
    pub fn remove_owner(&self,owner:&str){self.snapshots.lock().unwrap().retain(|_,s|s.owner!=owner);}
    pub fn page(&self,owner:&str,method:&str,p:&Value,make:impl FnOnce()->Result<(Value,Vec<Value>)>)->Result<Value> {
        let mut query=p.clone();query.as_object_mut().unwrap().remove("cursor");query.as_object_mut().unwrap().remove("limit");
        query["method"]=json!(method);
        let limit=p["limit"].as_u64().unwrap_or(50) as usize;
        let mut snapshots=self.snapshots.lock().unwrap();
        snapshots.retain(|_,s|s.deadline>Instant::now());
        let (snapshot_id,offset)=if let Some(cursor)=p["cursor"].as_str() {
            let (snapshot_id,offset)=cursor.split_once(':').ok_or_else(expired)?;
            (snapshot_id.to_owned(),offset.parse::<usize>().map_err(|_|expired())?)
        } else {
            if snapshots.values().filter(|s|s.owner!=owner||s.query!=query).count()>=8 {return Err(CoreError::new("CAPACITY","已有 8 份来源快照，请稍后刷新"));}
            let (summary,mut rows)=make()?;
            let sort=p["sort"].as_str().unwrap_or("totalRate");
            rows.sort_by(|a,b| {
                let order=if sort.ends_with("Bytes") {
                    let value=|v:&Value|v[sort].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(0);
                    value(b).cmp(&value(a))
                } else {let value=|v:&Value|if sort=="totalRate"{v["sendRate"].as_f64().unwrap_or(0.0)+v["receiveRate"].as_f64().unwrap_or(0.0)}else{v[sort].as_f64().unwrap_or(0.0)};value(b).total_cmp(&value(a))};
                order.then_with(||a["id"].as_str().cmp(&b["id"].as_str()))
            });
            let total_rows=rows.len();
            let mut bytes=serde_json::to_vec(&summary)?.len()+4096;
            rows.truncate(1024);
            let keep=rows.iter().take_while(|row| {bytes+=serde_json::to_vec(row).map_or(512*1024,|v|v.len());bytes<=512*1024}).count();
            rows.truncate(keep);
            let omitted=|field:&str| {
                let sum=rows.iter().map(|v|v[field].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(0) as u128).sum::<u128>();
                summary[field].as_str().and_then(|s|s.parse::<u64>().ok()).unwrap_or(0).saturating_sub(sum.min(u64::MAX as u128) as u64).to_string()
            };
            let other=json!({"sentBytes":omitted("sentBytes"),"receivedBytes":omitted("receivedBytes"),"resources":total_rows-rows.len()});
            let snapshot_id=id();
            // Refresh replaces this owner's previous copy, leaving other views intact.
            snapshots.retain(|_,s|s.owner!=owner||s.query!=query);
            snapshots.insert(snapshot_id.clone(),Snapshot {owner:owner.into(),query:query.clone(),summary,rows,other,
                deadline:Instant::now()+Duration::from_secs(300),expires_at:now_ms()+300_000});
            (snapshot_id,0)
        };
        let snapshot=snapshots.get(&snapshot_id).filter(|s|s.owner==owner&&s.query==query).ok_or_else(expired)?;
        if offset>snapshot.rows.len(){return Err(expired());}
        let end=offset.saturating_add(limit).min(snapshot.rows.len());
        Ok(json!({"summary":snapshot.summary,"items":snapshot.rows[offset..end],"snapshotId":snapshot_id,"expiresAt":snapshot.expires_at,
            "nextCursor":(end<snapshot.rows.len()).then(||format!("{snapshot_id}:{end}")),"other":snapshot.other}))
    }
}
fn expired()->CoreError{CoreError::new("CURSOR_EXPIRED","来源快照已过期或查询条件已变化，请刷新")}

#[cfg(test)]
mod tests {
    use super::*;
    fn data()->Result<(Value,Vec<Value>)> {
        Ok((json!({"sentBytes":"205","receivedBytes":"0"}),
            (0..205).map(|n|json!({"id":format!("r-{n:04}"),"sentBytes":"1","receivedBytes":"0","receiveRate":0.0})).collect()))
    }
    #[test]
    fn pages_are_frozen_scoped_and_replaced_on_refresh() {
        let queries=Queries::default();let p=json!({"limit":200});
        let first=queries.page("window-a","communication.sources",&p,data).unwrap();
        let next=json!({"limit":200,"cursor":first["nextCursor"]});
        let second=queries.page("window-a","communication.sources",&next,||panic!("must read the frozen sample")).unwrap();
        assert_eq!(second["items"].as_array().unwrap().len(),5);
        assert_eq!(first["summary"],second["summary"]);
        assert_eq!(queries.page("window-b","communication.sources",&next,data).unwrap_err().code,"CURSOR_EXPIRED");
        for _ in 0..12 {queries.page("window-a","communication.sources",&p,data).unwrap();}
        assert_eq!(queries.snapshots.lock().unwrap().len(),1);
        assert_eq!(queries.page("window-a","communication.sources",&next,data).unwrap_err().code,"CURSOR_EXPIRED");
    }
    #[test]
    fn detail_truncation_preserves_other_bytes() {
        let queries=Queries::default();
        let page=queries.page("window","communication.connections",&json!({}),||Ok((json!({"sentBytes":"2048","receivedBytes":"0"}),
            (0..2048).map(|n|json!({"id":n.to_string(),"sentBytes":"1","receivedBytes":"0"})).collect()))).unwrap();
        assert_eq!(page["other"]["sentBytes"],"1024");
        assert_eq!(page["other"]["resources"],1024);
    }
}
