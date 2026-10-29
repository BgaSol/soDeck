//! Shared serialized history pages. Consumers at nearby checkpoints reuse one read.
use super::{Observability,model,schema::Partition};
use crate::{CoreError,Result};
use serde_json::{json,Value};
use std::{collections::VecDeque,sync::Arc,time::{Duration,Instant}};
#[derive(Clone)]pub(super) struct Page{pub items:Vec<(u64,Arc<str>)>,pub through:u64}
struct Cached{key:String,after:u64,page:Page,bytes:usize,time:Instant}
#[derive(Default)]pub(super) struct Cache{pages:VecDeque<Cached>,bytes:usize}
impl Cache{
    pub fn page(&mut self,store:&Observability,owner:&str,filter:&Value,after:u64,high:u64,water:&Value)->Result<Page>{
        while self.pages.front().is_some_and(|p|p.time.elapsed()>Duration::from_secs(1)){self.bytes-=self.pages.pop_front().unwrap().bytes;}
        let key=model::hash(&json!([filter,water["prune"]]));
        if let Some(cached)=self.pages.iter().rev().find(|p|p.key==key&&p.after<=after&&p.page.through>after&&p.page.through<=high){return Ok(Page{items:cached.page.items.iter().filter(|(seq,_)|*seq>after).cloned().collect(),through:cached.page.through});}
        let simple=filter.as_object().unwrap().keys().all(|k|matches!(k.as_str(),"kind"|"pluginId"|"type"|"excludeCall"|"excludeDiagnosticCalls"));
        let page=if simple{read_simple(store,filter,after,high,water)?}else{
            let mut f=filter.clone();let exclude=f.as_object_mut().unwrap().remove("excludeCall")==Some(json!(true));
            let cursor=model::seal(&store.cursor_key,&json!({"epoch":store.epoch,"fingerprint":model::hash(&json!([owner,f,true])),"expires":crate::model::now_ms()+300000,"high":high,"prune":water["prune"],"after":after}));
            let result=store.query_v2_at(owner,&json!({"filter":f,"limit":100,"ascending":true,"cursor":cursor}),Some(water))?;
            let mut items=Vec::new();let mut bytes=0;let mut through=after;let mut capped=false;
            for e in result["items"].as_array().unwrap(){let seq=e["sequence"].as_str().unwrap().parse().unwrap();if exclude&&e["kind"]=="call"{through=seq;continue;}let raw=e.to_string();if bytes+raw.len()>60*1024{capped=true;break;}bytes+=raw.len();through=seq;items.push((seq,Arc::from(raw)));}
            if !capped&&result["nextCursor"].is_null(){through=high;}Page{items,through}
        };
        let bytes=page.items.iter().map(|(_,s)|s.len()+64).sum::<usize>()+key.len()+256;
        while self.bytes+bytes>512*1024{let Some(old)=self.pages.pop_front()else{break};self.bytes-=old.bytes;}
        if bytes<=512*1024{self.bytes+=bytes;self.pages.push_back(Cached{key,after,page:page.clone(),bytes,time:Instant::now()});}Ok(page)
    }
}
fn read_simple(store:&Observability,filter:&Value,after:u64,high:u64,water:&Value)->Result<Page>{
    let mut rows=Vec::new();let mut capped=false;let mut frontier=high;
    for partition in [Partition::Facts,Partition::Logs]{
        if let Some(kind)=filter["kind"].as_str(){if (kind=="log")!=(partition==Partition::Logs){continue;}}
        let filter=filter.clone();let config=store.config_epoch.clone();let prune=water["prune"][if partition==Partition::Facts{0}else{1}].clone();
        let result=store.queries.query(partition,move|db|{
            let tx=db.unchecked_transaction()?;if json!(super::history::number(&tx,"pruneRevision")?)!=prune{return Err(CoreError::new("CURSOR_EXPIRED","历史已清理"));}
            let mut sql="sequence>? AND sequence<=? AND config_epoch=? AND received>=?".to_owned();let mut args:Vec<rusqlite::types::Value>=vec![(after as i64).into(),(high as i64).into(),config.into(),(crate::model::now_ms().saturating_sub(30*86400000)as i64).into()];
            for (key,column)in [("kind","kind"),("pluginId","plugin_id"),("type","event_type")]{if let Some(v)=filter[key].as_str(){sql.push_str(&format!(" AND {column}=?"));args.push(v.to_owned().into());}}
            if filter["excludeCall"]==true{sql.push_str(" AND kind!='call'");}else{sql.push_str(&super::history::exclude_diagnostic_sql());}
            let mut statement=tx.prepare(&format!("SELECT sequence,envelope FROM records WHERE {sql} ORDER BY sequence LIMIT 101"))?;
            let mut query=statement.query(rusqlite::params_from_iter(args))?;let mut items=Vec::new();let mut bytes=0;
            while let Some(row)=query.next()?{let seq:i64=row.get(0)?;let raw:String=row.get(1)?;bytes+=raw.len();items.push(json!([seq,raw]));if bytes>64*1024{break;}}Ok(json!(items))
        })?;
        if result.as_array().unwrap().len()>100||result.as_array().unwrap().iter().map(|v|v[1].as_str().unwrap().len()).sum::<usize>()>64*1024{capped=true;if let Some(last)=result.as_array().unwrap().last(){frontier=frontier.min(last[0].as_u64().unwrap());}}
        rows.extend(result.as_array().unwrap().iter().map(|v|(v[0].as_u64().unwrap(),Arc::<str>::from(v[1].as_str().unwrap()))));
    }
    rows.retain(|(seq,_)|*seq<=frontier);rows.sort_by_key(|(seq,_)|*seq);let mut bytes=0;let mut items=Vec::new();
    for (seq,raw)in rows{if items.len()>=100||bytes+raw.len()>60*1024{capped=true;break;}bytes+=raw.len();items.push((seq,raw));}
    let through=if capped{items.last().map_or(after,|(seq,_)|*seq)}else{high};Ok(Page{items,through})
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn serialized_pages_match_query_filters_across_partition_frontiers_and_pruning(){
        let root=tempfile::tempdir().unwrap();Observability::initialize_fixture(root.path()).unwrap();let store=Observability::open(root.path(),"boot".into(),"config".into()).unwrap();
        let key=store.signing_key.clone();let epoch=store.epoch.clone();store.write(Partition::Facts,200*1024,move|db|{
            for n in 0..160{let log=n%3==0;let mut e=model::envelope(&crate::Caller::shell(),"boot",&epoch,if log{"log"}else if n%5==0{"call"}else{"domain"},"fixture.event",json!({"count":n,"padding":"x".repeat(4000),"method":if n%2==0{"events.v2.query"}else{"servers.list"}}));e["configEpoch"]=json!("config");model::insert(if log{&db.logs}else{&db.facts},&key,if log{"l"}else{"f"},&mut e,None)?;}Ok(Value::Null)
        }).unwrap();
        for filter in [json!({"excludeCall":true,"excludeDiagnosticCalls":true}),json!({"kind":"log","excludeCall":false,"excludeDiagnosticCalls":true}),json!({"kind":"call","excludeCall":false,"excludeDiagnosticCalls":true}),json!({"level":"info","excludeCall":true,"excludeDiagnosticCalls":true})]{
            let water=store.watermarks().unwrap();let high=water["high"].as_u64().unwrap();let mut cache=Cache::default();let mut after=0;let mut actual=Vec::new();
            for _ in 0..100{let page=cache.page(&store,"owner",&filter,after,high,&water).unwrap();let repeat=cache.page(&store,"another-owner",&filter,after,high,&water).unwrap();assert_eq!(page.through,repeat.through);assert!(cache.bytes<=512*1024);actual.extend(page.items.iter().map(|(_,raw)|serde_json::from_str::<Value>(raw).unwrap()));assert!(page.through>after||after==high);after=page.through;if after==high{break;}}
            assert_eq!(after,high);let mut public_filter=filter.clone();let exclude=public_filter.as_object_mut().unwrap().remove("excludeCall")==Some(json!(true));let mut query=json!({"filter":public_filter,"ascending":true,"limit":100});let mut expected=Vec::new();loop{let page=store.query_v2("query",&query).unwrap();expected.extend(page["items"].as_array().unwrap().iter().filter(|e|!exclude||e["kind"]!="call").cloned());if page["nextCursor"].is_null(){break;}query["cursor"]=page["nextCursor"].clone();}assert_eq!(actual,expected);
        }
        let mut cache=Cache::default();let water=store.watermarks().unwrap();let filter=json!({"kind":"domain","excludeCall":false,"excludeDiagnosticCalls":true});cache.page(&store,"owner",&filter,0,water["high"].as_u64().unwrap(),&water).unwrap();
        store.write(Partition::Facts,1024,|db|{db.facts.execute("DELETE FROM records WHERE kind='domain'",[])?;Ok(Value::Null)}).unwrap();let next=store.watermarks().unwrap();assert_ne!(next["prune"],water["prune"]);assert!(cache.page(&store,"owner",&filter,0,next["high"].as_u64().unwrap(),&next).unwrap().items.is_empty());store.shutdown();
    }
}
