use sodeck_core::{Caller,Core};
use serde_json::{json,Value};
use std::{thread,time::{Duration,Instant}};
fn descriptor()->Value{json!({"schemaVersion":1,"pluginId":"fixture.obs","events":[{"type":"plugin.fixture.obs.changed","typeVersion":1,"kind":"domain","fields":{"count":{"type":"integer","classification":"count","minimum":0}}},{"type":"plugin.fixture.obs.debug","typeVersion":1,"kind":"log","level":"debug","fields":{}}],"actions":[{"type":"plugin.fixture.obs.open","typeVersion":1,"kind":"action","fields":{}}],"snapshots":[{"type":"fixture.state","typeVersion":1,"fields":{"count":{"type":"integer","classification":"count"}}}],"metrics":[{"type":"fixture.count","typeVersion":1,"fields":{},"unit":"count","metricKind":"counter","labels":{}}]})}
fn fixture()->(tempfile::TempDir,Core,Caller){let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let caller=Caller{source:"plugin".into(),plugin_id:Some("fixture.obs".into()),plugin_version:Some("1.0.0".into()),instance_id:"fixture-instance".into(),view_id:None,mount_generation:None};core.register_observation_source(&caller,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();(root,core,caller)}
fn append(core:&Core,caller:&Caller,id:&str)->Value{core.call(caller,"events.v2.appendBatch",json!({"batchId":id,"items":[{"clientEventId":id,"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":2}}]})).unwrap()}
#[test]
fn catalog_large_numeric_bounds_survive_restart_and_still_reject_changes() {
    let root = tempfile::tempdir().unwrap();
    let caller = Caller { source: "plugin".into(), plugin_id: Some("fixture.obs".into()), plugin_version: Some("1.0.0".into()), instance_id: "large-bounds".into(), view_id: None, mount_generation: None };
    let mut definition = descriptor();
    definition["events"][0]["fields"]["count"]["maximum"] = json!(9007199254740991u64);
    let bytes = serde_json::to_vec(&definition).unwrap();
    {
        let core = Core::open(root.path()).unwrap();
        core.register_observation_source(&caller, Some(&bytes)).unwrap();
        assert_eq!(append(&core, &caller, "large-bound-before")["items"][0]["status"], "persisted");
    }
    let core = Core::open(root.path()).unwrap();
    core.register_observation_source(&caller, Some(&bytes)).unwrap();
    assert_eq!(append(&core, &caller, "large-bound-after")["items"][0]["status"], "persisted");
    core.close_instance(&caller.instance_id);
    definition["events"][0]["fields"]["count"]["maximum"] = json!(9007199254740990u64);
    assert_eq!(core.register_observation_source(&caller, Some(&serde_json::to_vec(&definition).unwrap())).unwrap_err().code, "CONFLICT");
}
#[test]
fn every_public_diagnostic_call_is_audited_once_including_rejections() {
    let (root,core,caller)=fixture();
    let open=core.call(&caller,"events.v2.open",json!({"filter":{"kind":"domain"},"from":"now"})).unwrap();
    let db=rusqlite::Connection::open(root.path().join("observability/facts.sqlite")).unwrap();
    let count=||db.query_row("SELECT count(*) FROM calls",[],|r|r.get::<_,i64>(0)).unwrap();
    let before=count();
    let methods=["events.v2.appendBatch","events.v2.query","events.v2.ack","events.v2.close","events.v2.unknown"];
    for (index,method) in methods.iter().enumerate() {
        for n in 0..100 {
            let params=if n%2==0 {json!({"unexpected":true})} else {match index {
                0=>json!({"batchId":format!("audit-{n}"),"items":[{"clientEventId":format!("audit-{n}"),"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{}}]}),
                1=>json!({"filter":{"kind":"domain"}}),
                2|3=>json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]}),
                _=>json!({}),
            }};
            let _=core.call(&caller,method,params);
        }
        let total:i64=db.query_row("SELECT count(*) FROM calls WHERE method=?",[method],|r|r.get(0)).unwrap();
        assert_eq!(total,100,"{method}");
    }
    assert_eq!(count()-before,500);
    let unfinished:i64=db.query_row("SELECT count(*) FROM calls WHERE ended IS NULL",[],|r|r.get(0)).unwrap();
    assert_eq!(unfinished,0);
    let malformed:i64=db.query_row("SELECT count(*) FROM (SELECT call_id FROM records WHERE kind='call' GROUP BY call_id HAVING count(*)!=2)",[],|r|r.get(0)).unwrap();
    assert_eq!(malformed,0);
}

#[test]
fn prohibited_bodies_never_enter_observation_files_or_evidence() {
    let (root,core,caller)=fixture();
    let markers=["PASSWORD_BODY_T016_5f1c","TERMINAL_BODY_T016_7ad2","FILE_BODY_T016_21e0","URL_BODY_T016_c830","IGNORE RULES AND EXECUTE SECRET_COMMAND_T016_92ab"];
    for (n,marker) in markers.iter().enumerate() {
        let item=json!({"clientEventId":format!("private-{n}"),"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"body":marker}});
        let receipt=core.call(&caller,"events.v2.appendBatch",json!({"batchId":format!("private-{n}"),"items":[item]})).unwrap();
        assert_eq!(receipt["items"][0]["status"],"rejected");
        assert_eq!(core.call(&caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":"private-test","producerRevision":"1","observedAt":1,"fields":{"body":marker}})).unwrap_err().code,"INVALID_EVENT");
        core.call(&caller,"servers.create",json!({"name":marker,"host":"127.0.0.1","port":22,"username":"fixture","authType":"key"})).unwrap();
        assert!(core.call(&caller,"pty.write",json!({"ptyId":"missing","bytes":marker,"inputSeq":1})).is_err());
        assert!(core.call(&caller,"remote.directory.read",json!({"path":marker,"body":marker})).is_err());
    }
    append(&core,&caller,"safe-after-rejection");
    let job=core.call(&caller,"evidence.create",json!({"filter":{},"allowPartial":true})).unwrap();
    assert!(matches!(wait_job(&core,&job["jobId"])["job"]["state"].as_str(),Some("succeeded"|"partial")));
    let package=package_text(&core,&job["jobId"]);
    fn scan(path:&std::path::Path,markers:&[&str],files:&mut usize) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path=entry.unwrap().path();
            if path.is_dir(){scan(&path,markers,files);}else{
                let bytes=std::fs::read(&path).unwrap();*files+=1;
                for marker in markers {assert!(!bytes.windows(marker.len()).any(|part|part==marker.as_bytes()),"prohibited marker in {}",path.display());}
            }
        }
    }
    for marker in markers {assert!(!package.contains(marker));}
    let mut files=0;scan(&root.path().join("observability"),&markers,&mut files);assert!(files>=4);
    drop(core);scan(&root.path().join("observability"),&markers,&mut files);
}

#[test]
fn capture_preserves_current_stale_and_unintegrated_source_coverage() {
    let (root,core,a)=fixture();let mut b=a.clone();b.instance_id="stale-source".into();
    core.register_observation_source(&b,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();
    let mut c=a.clone();c.instance_id="legacy-source".into();c.plugin_id=Some("fixture.legacy".into());core.register_observation_source(&c,None).unwrap();
    let report=|caller:&Caller|core.call(caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":"shared","producerRevision":"1","observedAt":1,"fields":{"count":1}})).unwrap();
    let current=report(&a);let stale=report(&b);
    let db=rusqlite::Connection::open(root.path().join("observability/diagnostics.sqlite")).unwrap();
    db.execute("UPDATE observation_snapshots SET expires=1,data=json_set(data,'$.expiresAt',1) WHERE id=?",[stale["storedSnapshotId"].as_str()]).unwrap();
    assert_eq!(core.call(&a,"diagnostics.snapshots.get",json!({"snapshotId":current["storedSnapshotId"]})).unwrap()["state"],"current");
    assert_eq!(core.call(&a,"diagnostics.snapshots.get",json!({"snapshotId":stale["storedSnapshotId"]})).unwrap()["state"],"stale");
    let job=core.call(&a,"diagnostics.capture",json!({"filter":{},"allowPartial":true})).unwrap();
    db.execute("UPDATE observation_snapshots SET data=json_set(data,'$.fields.count',2)",[]).unwrap();
    let finished=wait_job(&core,&job["jobId"]);assert_eq!(finished["job"]["state"],"partial");assert!(finished["job"]["manifest"]["gaps"].as_array().unwrap().contains(&json!("audit-only-source")));
    let text=package_text(&core,&job["jobId"]);let records:Vec<Value>=text.lines().map(|line|serde_json::from_str(line).unwrap()).collect();
    let snapshots:Vec<_>=records.iter().filter(|v|v["recordType"]=="snapshot").collect();assert_eq!(snapshots.len(),2);
    assert!(snapshots.iter().all(|v|v["value"]["fields"]["count"]==1));
    assert_eq!(snapshots.iter().filter(|v|v["value"]["expiresAt"]==1).count(),1);
    let sources=&records.iter().find(|v|v["recordType"]=="coverage").unwrap()["value"]["sources"];
    assert!(sources.as_array().unwrap().iter().any(|s|s["pluginId"]=="fixture.legacy"&&s["coverage"]=="audit-only"));
}
#[test]fn metrics_enforce_series_units_and_declared_labels(){
    let(_root,core,caller)=fixture();let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;let to=now/10000*10000;
    let sample=|index|json!({"metricType":"fixture.count","typeVersion":1,"scope":format!("scope-{index}"),"windowId":"window","fromUtc":to-10000,"toUtc":to,"sampleId":format!("sample-{index}"),"unit":"count","value":1,"labels":{}});
    for offset in (0..128).step_by(16){let items:Vec<_>=(offset..offset+16).map(sample).collect();let result=core.call(&caller,"metrics.report",json!({"items":items})).unwrap();assert!(result["items"].as_array().unwrap().iter().all(|v|v["status"]=="persisted"));}
    let result=core.call(&caller,"metrics.report",json!({"items":[sample(128)]})).unwrap();assert_eq!(result["items"][0]["error"]["code"],"CAPACITY");
    for (field,value) in [("unit",json!("seconds")),("labels",json!({"requestId":"arbitrary"}))]{let mut invalid=sample(129);invalid[field]=value;let result=core.call(&caller,"metrics.report",json!({"items":[invalid]})).unwrap();assert_eq!(result["items"][0]["error"]["code"],"INVALID_EVENT");}
}
#[test]fn feed_activation_repeats_unacked_events_and_requires_explicit_prune_gap(){
    let(root,core,caller)=fixture();let first=append(&core,&caller,"before-open");let open=core.call(&caller,"events.v2.open",json!({"filter":{"kind":"domain"},"from":"retained"})).unwrap();let second=append(&core,&caller,"before-ack");
    core.call(&caller,"events.v2.ack",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]})).unwrap();let third=append(&core,&caller,"after-ack");
    let next=||{let began=Instant::now();loop{if let Some(value)=core.observation_deliveries().into_iter().find(|v|v.owner.starts_with(&caller.instance_id)){return (value.event,serde_json::from_str::<Value>(&value.payload).unwrap());}assert!(began.elapsed()<Duration::from_secs(3));thread::sleep(Duration::from_millis(20));}};
    let mut seen=std::collections::HashSet::new();while seen.len()<3{let(event,batch)=next();assert_eq!(event,"events.v2.batch");let(_,repeat)=next();assert_eq!(batch["batchId"],repeat["batchId"]);for item in batch["items"].as_array().unwrap(){seen.insert(item["eventId"].clone().to_string());}core.call(&caller,"events.v2.ack",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"],"batchId":batch["batchId"]})).unwrap();}
    for receipt in [first,second,third]{assert!(seen.contains(&receipt["items"][0]["eventId"].to_string()));}
    let db=rusqlite::Connection::open(root.path().join("observability/facts.sqlite")).unwrap();db.execute("DELETE FROM records WHERE client_id='before-open'",[]).unwrap();drop(db);
    let(event,gap)=next();assert_eq!(event,"events.v2.gap");let mut ack=json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]});assert_eq!(core.call(&caller,"events.v2.ack",ack.clone()).unwrap_err().code,"GAP_REQUIRES_ACK");ack["acceptGap"]=json!(true);assert_eq!(core.call(&caller,"events.v2.ack",ack).unwrap()["checkpoint"],gap["checkpoint"]);
    core.call(&caller,"events.v2.close",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]})).unwrap();assert!(core.observation_deliveries().is_empty());
}
#[test]
#[ignore="child process for killed_writer_recovers_committed_history; launched by its parent"]
fn observation_crash_child(){
    let root=std::env::var("SODECK_OBSERVATION_CRASH_DIR").expect("isolated parent required");let core=Core::open(std::path::Path::new(&root)).unwrap();let caller=Caller{source:"plugin".into(),plugin_id:Some("fixture.obs".into()),instance_id:"crash-producer".into(),..Caller::shell()};
    core.register_observation_source(&caller,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();
    let checkpoint=core.call(&caller,"events.v2.open",json!({"filter":{"pluginId":"fixture.obs","kind":"domain"},"from":"now"})).unwrap();
    let mut ids=Vec::new();for i in 0..32{let name=format!("crash-{i}");let receipt=core.call(&caller,"events.v2.appendBatch",json!({"batchId":name,"items":[{"clientEventId":name,"type":if i%2==0{"plugin.fixture.obs.changed"}else{"plugin.fixture.obs.debug"},"typeVersion":1,"occurredAt":1,"level":"error","fields":{}}]})).unwrap();assert_eq!(receipt["items"][0]["status"],"persisted");ids.push(receipt["items"][0]["eventId"].clone());}
    let trace=core.call(&caller,"traces.begin",json!({"clientOperationId":"crash-active","actionType":"plugin.fixture.obs.open","trigger":"plugin","fields":{}})).unwrap();
    let evidence=core.call(&caller,"evidence.create",json!({"filter":{"pluginId":"fixture.obs","kind":"domain"}})).unwrap();assert_eq!(wait_job(&core,&evidence["jobId"])["job"]["state"],"succeeded");
    let export=core.begin_evidence_export(evidence["jobId"].as_str().unwrap()).unwrap();
    let ready=json!({"ids":ids,"trace":trace,"evidence":evidence,"export":export,"checkpoint":checkpoint,"water":core.call(&caller,"events.v2.query",json!({"filter":{},"limit":1})).unwrap()["highWatermark"]});
    std::fs::write(std::path::Path::new(&root).join("ready.json"),ready.to_string()).unwrap();thread::sleep(Duration::from_secs(60));panic!("parent failed to kill crash fixture");
}
#[test]fn killed_writer_recovers_committed_history_and_pending_jobs(){
    use std::process::{Command,Stdio};
    let root=tempfile::tempdir().unwrap();let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","observation_crash_child","--ignored","--nocapture"]).env("SODECK_OBSERVATION_CRASH_DIR",root.path()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let started=Instant::now();let ready=loop{if let Ok(bytes)=std::fs::read(root.path().join("ready.json")){if let Ok(value)=serde_json::from_slice::<Value>(&bytes){break value;}}if started.elapsed()>Duration::from_secs(15){let _=child.kill();let _=child.wait();panic!("crash fixture did not become ready");}assert!(child.try_wait().unwrap().is_none(),"child exited before committed marker");thread::sleep(Duration::from_millis(20));};
    child.kill().unwrap();let exit=child.wait().unwrap();assert!(!exit.success());
    let core=Core::open(root.path()).unwrap();let caller=Caller::shell();let epoch=core.call(&caller,"events.v2.query",json!({"filter":{},"limit":1})).unwrap()["storeEpoch"].clone();
    for id in ready["ids"].as_array().unwrap(){assert_eq!(core.call(&caller,"events.v2.get",json!({"storeEpoch":epoch,"eventId":id})).unwrap()["eventId"],*id);}
    let trace=core.call(&caller,"traces.get",json!({"traceId":ready["trace"]["traceId"]})).unwrap();assert!(trace["items"].as_array().unwrap().iter().any(|v|v["outcome"]=="interrupted"));
    assert_eq!(core.call(&caller,"evidence.get",json!({"jobId":ready["export"]["jobId"]})).unwrap()["job"]["state"],"interrupted");assert_eq!(core.call(&caller,"evidence.get",json!({"jobId":ready["evidence"]["jobId"]})).unwrap()["job"]["state"],"succeeded");
    let resumed=core.call(&caller,"events.v2.open",json!({"filter":{"pluginId":"fixture.obs","kind":"domain"},"checkpoint":ready["checkpoint"]["checkpoint"]})).unwrap();core.call(&caller,"events.v2.ack",json!({"subscriptionId":resumed["subscriptionId"],"feedEpoch":resumed["feedEpoch"]})).unwrap();
    let began=Instant::now();loop{let batches=core.observation_deliveries();if let Some(batch)=batches.iter().find(|v|v.event=="events.v2.batch"){assert_eq!(serde_json::from_str::<Value>(&batch.payload).unwrap()["items"].as_array().unwrap().len(),16);break;}assert!(began.elapsed()<Duration::from_secs(3));thread::sleep(Duration::from_millis(20));}
    let water=core.call(&caller,"events.v2.query",json!({"filter":{},"limit":1})).unwrap()["highWatermark"].as_str().unwrap().parse::<u64>().unwrap();assert!(water>ready["water"].as_str().unwrap().parse::<u64>().unwrap());
}
#[test]fn optional_log_partition_failure_does_not_block_or_replay_business(){
    let(root,core,caller)=fixture();let policy=core.call(&caller,"observability.v2.policy.get",json!({})).unwrap();
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;
    core.call(&caller,"observability.v2.policy.set",json!({"expectedRevision":policy["revision"],"patch":{"sourceOverrides":[{"sourceId":"fixture.obs","logLevel":"debug","debugUntil":now+60000}]}})).unwrap();
    let db=rusqlite::Connection::open(root.path().join("observability/logs.sqlite")).unwrap();db.execute_batch("CREATE TRIGGER deny_optional_log BEFORE INSERT ON records BEGIN SELECT RAISE(ABORT,'test log failure'); END;").unwrap();
    let rejected=core.call(&caller,"events.v2.appendBatch",json!({"batchId":"failure","items":[{"clientEventId":"failure","type":"plugin.fixture.obs.debug","typeVersion":1,"occurredAt":1,"fields":{}}]})).unwrap();assert_eq!(rejected["items"][0]["status"],"rejected");
    let server=core.call(&caller,"servers.create",json!({"name":"log failure isolation","host":"127.0.0.1","port":22,"username":"test","authType":"key"})).unwrap();
    let servers=core.call(&caller,"servers.list",json!({})).unwrap();assert_eq!(servers["servers"].as_array().unwrap().iter().filter(|s|s["id"]==server["id"]).count(),1);
    assert_eq!(append(&core,&caller,"domain-still-works")["items"][0]["status"],"persisted");
    db.execute_batch("DROP TRIGGER deny_optional_log;").unwrap();
}
#[test]fn v2_duration_distribution_and_pages_preserve_missing_samples_and_watermark(){
    let root=tempfile::tempdir().unwrap();let core=Core::open(root.path()).unwrap();let caller=Caller{source:"plugin".into(),plugin_id:Some("fixture.obs".into()),instance_id:"duration-fixture".into(),..Caller::shell()};
    let mut types=descriptor();types["events"][0]["fields"]["durationMs"]=json!({"type":"number","classification":"duration","minimum":0});core.register_observation_source(&caller,Some(&serde_json::to_vec(&types).unwrap())).unwrap();
    for start in (0..205).step_by(32){let items:Vec<_>=(start..(start+32).min(205)).map(|i|json!({"clientEventId":format!("sample-{i}"),"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1000,"fields":match i{0=>json!({"durationMs":10}),1=>json!({"durationMs":100}),_=>json!({})}})).collect();let receipts=core.call(&caller,"events.v2.appendBatch",json!({"batchId":format!("batch-{start}"),"items":items})).unwrap();assert!(receipts["items"].as_array().unwrap().iter().all(|i|i["status"]=="persisted"));}
    let filter=json!({"pluginId":"fixture.obs","kind":"domain","timeField":"occurredAt","fromUtc":0,"toUtc":2000});
    let first=core.call(&caller,"events.v2.query",json!({"filter":filter,"limit":100})).unwrap();append(&core,&caller,"backdated-late");
    let second=core.call(&caller,"events.v2.query",json!({"filter":filter,"limit":100,"cursor":first["nextCursor"]})).unwrap();
    let third=core.call(&caller,"events.v2.query",json!({"filter":filter,"limit":100,"cursor":second["nextCursor"]})).unwrap();
    assert_eq!(first["highWatermark"],third["highWatermark"]);assert_eq!(third["items"].as_array().unwrap().len(),5);assert!(third["nextCursor"].is_null());
    let ids:std::collections::HashSet<_>=[&first,&second,&third].iter().flat_map(|page|page["items"].as_array().unwrap().iter().map(|v|v["eventId"].as_str().unwrap())).collect();assert_eq!(ids.len(),205);
    let result=core.call(&caller,"events.v2.aggregate",json!({"filter":filter,"groupBy":"type"})).unwrap();let group=&result["groups"][0];assert_eq!(group["count"],"206");let duration=&group["duration"];
    assert_eq!(duration["sampleCount"],"2");assert_eq!(duration["min"],10.);assert_eq!(duration["max"],100.);assert_eq!(duration["mean"],55.);assert_eq!(duration["unit"],"ms");assert_eq!(duration["buckets"][2]["count"],"1");assert_eq!(duration["buckets"][4]["count"],"1");assert_eq!(result["fromUtc"],0);assert_eq!(result["toUtc"],2000);
    let missing=core.call(&caller,"events.v2.aggregate",json!({"filter":{"clientEventId":"backdated-late"},"groupBy":"type"})).unwrap();assert_eq!(missing["groups"][0]["duration"]["sampleCount"],"0");assert!(missing["groups"][0]["duration"]["mean"].is_null());
}
#[test]fn typed_submission_dedup_and_safe_fields(){let(_root,core,caller)=fixture();let first=append(&core,&caller,"one");assert_eq!(first["items"][0]["status"],"persisted");let duplicate=append(&core,&caller,"one");assert_eq!(duplicate["items"][0]["status"],"duplicate");assert_eq!(first["items"][0]["eventId"],duplicate["items"][0]["eventId"]);
    let mixed=core.call(&caller,"events.v2.appendBatch",json!({"batchId":"mixed","items":[{"clientEventId":"bad","type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"password":"secret-marker"}},{"clientEventId":"filtered","type":"plugin.fixture.obs.debug","typeVersion":1,"occurredAt":1,"fields":{}},{"clientEventId":"new","type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":3}}]})).unwrap();assert_eq!(mixed["items"][0]["status"],"rejected");assert_eq!(mixed["items"][1]["status"],"filtered");assert_eq!(mixed["items"][2]["status"],"persisted");
    let page=core.call(&caller,"events.v2.query",json!({"filter":{"kind":"domain","pluginId":"fixture.obs"}})).unwrap();assert_eq!(page["items"].as_array().unwrap().len(),2);assert!(!page.to_string().contains("secret-marker"));
    let got=core.call(&caller,"events.v2.get",json!({"storeEpoch":first["storeEpoch"],"eventId":first["items"][0]["eventId"]})).unwrap();assert_eq!(got["source"]["instanceId"],"fixture-instance");
}
#[test]fn context_is_explicit_and_call_finishes_on_same_trace(){let(_root,core,caller)=fixture();let context=core.call(&caller,"traces.begin",json!({"clientOperationId":"open-1","actionType":"plugin.fixture.obs.open","trigger":"user","fields":{}})).unwrap();let metadata=json!({"diagnostics":{"version":1,"contextRef":context["contextRef"]}});
    core.diagnostic_metadata(&caller,Some(&metadata),||core.call(&caller,"servers.list",json!({}))).unwrap();let page=core.call(&caller,"traces.get",json!({"traceId":context["traceId"]})).unwrap();assert_eq!(page["items"].as_array().unwrap().len(),3);
    let end=json!({"contextRef":context["contextRef"],"outcome":"succeeded"});assert_eq!(core.call(&caller,"traces.end",end.clone()).unwrap()["duplicate"],false);assert_eq!(core.call(&caller,"traces.end",end).unwrap()["duplicate"],true);
    let mut other=caller.clone();other.instance_id="another-instance".into();assert_eq!(core.diagnostic_metadata(&other,Some(&metadata),||Ok(Value::Null)).unwrap_err().code,"INVALID_CONTEXT");
}
#[test]fn snapshots_and_metric_delta_survive_producer_exit(){let(_root,core,caller)=fixture();let snapshot=json!({"snapshotType":"fixture.state","typeVersion":1,"scope":"workspace-1","producerRevision":"1","observedAt":1,"fields":{"count":3}});let value=core.call(&caller,"diagnostics.snapshots.report",snapshot.clone()).unwrap();assert_eq!(core.call(&caller,"diagnostics.snapshots.report",snapshot).unwrap()["status"],"duplicate");assert_eq!(core.call(&caller,"diagnostics.snapshots.get",json!({"snapshotId":value["storedSnapshotId"]})).unwrap()["fields"]["count"],3);
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;let to=now/10000*10000;let sample=json!({"metricType":"fixture.count","typeVersion":1,"scope":"workspace-1","windowId":(to-10000).to_string(),"fromUtc":to-10000,"toUtc":to,"sampleId":"sample-1","unit":"count","value":5,"labels":{}});
    assert_eq!(core.call(&caller,"metrics.report",json!({"items":[sample]})).unwrap()["items"][0]["status"],"persisted");assert_eq!(core.call(&caller,"metrics.report",json!({"items":[sample]})).unwrap()["items"][0]["status"],"duplicate");let metrics=core.call(&caller,"metrics.query",json!({"pluginId":"fixture.obs"})).unwrap();assert_eq!(metrics["items"][0]["value"],5.0);
    core.close_instance(&caller.instance_id);assert_eq!(core.call(&Caller::shell(),"diagnostics.snapshots.query",json!({})).unwrap()["items"].as_array().unwrap().len(),1);
}
#[test]fn reliable_feed_and_evidence_do_not_need_private_plugin_callbacks(){let(_root,core,caller)=fixture();let open=core.call(&caller,"events.v2.open",json!({"filter":{"kind":"domain"},"from":"now"})).unwrap();core.call(&caller,"events.v2.ack",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]})).unwrap();let persisted=append(&core,&caller,"feed-event");let start=Instant::now();let delivery=loop{if let Some(d)=core.observation_deliveries().into_iter().find(|d|d.event=="events.v2.batch"){break d;}assert!(start.elapsed()<Duration::from_secs(5));thread::sleep(Duration::from_millis(30));};let payload:Value=serde_json::from_str(&delivery.payload).unwrap();assert_eq!(payload["items"][0]["eventId"],persisted["items"][0]["eventId"]);
    core.call(&caller,"events.v2.ack",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"],"batchId":payload["batchId"]})).unwrap();
    let job=core.call(&caller,"evidence.create",json!({"filter":{"pluginId":"fixture.obs","kind":"domain"}})).unwrap();let start=Instant::now();let result=loop{let v=core.call(&caller,"evidence.get",json!({"jobId":job["jobId"]})).unwrap();if matches!(v["job"]["state"].as_str(),Some("succeeded"|"partial"|"failed")){break v;}assert!(start.elapsed()<Duration::from_secs(5));thread::sleep(Duration::from_millis(30));};assert_eq!(result["job"]["state"],"succeeded","{result}");assert!(!result["chunks"].as_array().unwrap().is_empty());
}

#[test]fn official_descriptors_validate_and_reject_free_text(){
    let(_root,core,mut caller)=fixture();
    for(plugin,bytes)in [("sodeck.terminal",include_bytes!("../../../plugins/terminal/diagnostics/observability.json").as_slice()),("sodeck.files",include_bytes!("../../../plugins/files/diagnostics/observability.json").as_slice()),("sodeck.network-proxy",include_bytes!("../../../plugins/network-proxy/diagnostics/observability.json").as_slice())]{
        caller.plugin_id=Some(plugin.into());caller.instance_id=plugin.into();core.register_observation_source(&caller,Some(bytes)).unwrap();
        let mut bad:Value=serde_json::from_slice(bytes).unwrap();bad["events"][0]["fields"]["path"]=json!({"type":"string","classification":"structure","maxLength":256});
        assert_eq!(core.register_observation_source(&caller,Some(&serde_json::to_vec(&bad).unwrap())).unwrap_err().code,"INVALID_DESCRIPTOR");
    }
}
#[test]fn host_transfers_context_without_forging_owner_or_mount(){
    let(_root,core,worker)=fixture();let mut view=worker.clone();view.view_id=Some("view-a".into());view.mount_generation=Some(1);
    let context=core.call(&view,"traces.begin",json!({"clientOperationId":"transfer","actionType":"plugin.fixture.obs.open","trigger":"user","fields":{}})).unwrap();let metadata=json!({"diagnostics":{"version":1,"contextRef":context["contextRef"]}});
    assert_eq!(core.diagnostic_metadata(&worker,Some(&metadata),||Ok(Value::Null)).unwrap_err().code,"INVALID_CONTEXT");
    let token=core.diagnostic_metadata(&view,Some(&metadata),||Ok(core.transfer_diagnostic_context(&worker)?.unwrap())).unwrap();
    core.diagnostic_metadata(&worker,Some(&token),||core.call(&worker,"servers.list",json!({}))).unwrap();
    view.mount_generation=Some(2);core.diagnostic_metadata(&view,Some(&metadata),||core.call(&view,"servers.list",json!({}))).unwrap();
    let page=core.call(&worker,"events.v2.query",json!({"filter":{"kind":"call"}})).unwrap();assert!(page["items"].as_array().unwrap().iter().any(|v|v["completeness"].as_array().unwrap().contains(&json!("associationMissing"))));
    let bad=json!({"diagnostics":{"version":1,"contextRef":"fake","source":"trusted"}});assert!(core.diagnostic_metadata(&worker,Some(&bad),||panic!("invalid metadata executed business")).is_err());
}
#[test]fn end_dedup_compares_complete_result_and_close_marks_snapshots(){
    let(_root,core,caller)=fixture();let context=core.call(&caller,"traces.begin",json!({"clientOperationId":"end-content","actionType":"plugin.fixture.obs.open","trigger":"user","fields":{}})).unwrap();
    core.call(&caller,"traces.end",json!({"contextRef":context["contextRef"],"outcome":"failed","errorCode":"ONE"})).unwrap();
    assert_eq!(core.call(&caller,"traces.end",json!({"contextRef":context["contextRef"],"outcome":"failed","errorCode":"TWO"})).unwrap_err().code,"CONFLICT");
    let snapshot=core.call(&caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":"workspace","producerRevision":"1","observedAt":1,"fields":{"count":1}})).unwrap();
    core.close_instance(&caller.instance_id);assert_eq!(core.call(&Caller::shell(),"diagnostics.snapshots.get",json!({"snapshotId":snapshot["storedSnapshotId"]})).unwrap()["state"],"ended");
}
#[test]fn cleanup_removes_selected_history_and_retained_id_reports_gone(){
    let(_root,core,caller)=fixture();let event=append(&core,&caller,"delete-me");
    let job=core.call(&caller,"events.v2.cleanup.start",json!({"storeEpoch":event["storeEpoch"],"filter":{"kind":"domain","clientEventId":"delete-me"}})).unwrap();
    let started=Instant::now();loop{let result=core.call(&caller,"events.v2.cleanup.get",json!({"jobId":job["jobId"]})).unwrap();if result["job"]["state"]=="succeeded"{assert_eq!(result["job"]["manifest"]["removed"],1);break;}assert!(started.elapsed()<Duration::from_secs(5),"{result}");thread::sleep(Duration::from_millis(20));}
    assert_eq!(core.call(&caller,"events.v2.get",json!({"storeEpoch":event["storeEpoch"],"eventId":event["items"][0]["eventId"]})).unwrap_err().code,"GONE");
}

#[test]fn catalog_conflicts_survive_restart_and_retire_instance_memory(){
    let(root,core,caller)=fixture();append(&core,&caller,"catalog");core.close_instance(&caller.instance_id);
    assert_eq!(core.call(&Caller::shell(),"observability.v2.health.get",json!({})).unwrap()["catalogBytes"],0);
    drop(core);let core=Core::open(root.path()).unwrap();let mut changed=descriptor();changed["events"][0]["fields"]["count"]["maximum"]=json!(10);
    assert_eq!(core.register_observation_source(&caller,Some(&serde_json::to_vec(&changed).unwrap())).unwrap_err().code,"CONFLICT");
    core.register_observation_source(&caller,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();
}
#[test]fn diagnostic_targets_are_authenticated_and_do_not_encode_private_identifiers(){
    let(_root,core,caller)=fixture();let target=core.observation_target("resource","private-resource-id",Some("1")).unwrap();assert!(!target.to_string().contains("private-resource-id"));
    let item=json!({"clientEventId":"target-1","type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":1},"targets":[target]});
    let result=core.call(&caller,"events.v2.appendBatch",json!({"batchId":"t","items":[item]})).unwrap();assert_eq!(result["items"][0]["status"],"persisted");
    let mut bad=item.clone();bad["clientEventId"]=json!("target-2");bad["targets"][0]["generation"]=json!("2");let result=core.call(&caller,"events.v2.appendBatch",json!({"batchId":"bad","items":[bad]})).unwrap();assert_eq!(result["items"][0]["error"]["code"],"INVALID_REFERENCE");
    let page=core.call(&caller,"events.v2.query",json!({"filter":{"diagnosticRef":target["ref"]}})).unwrap();assert_eq!(page["items"].as_array().unwrap().len(),1);
}
#[test]fn metric_details_are_bounded_by_boot_while_minute_deltas_remain_persisted(){
    let(root,core,caller)=fixture();let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;let to=now/10000*10000;
    let sample=json!({"metricType":"fixture.count","typeVersion":1,"scope":"w","windowId":(to-10000).to_string(),"fromUtc":to-10000,"toUtc":to,"sampleId":"detail","unit":"count","value":5,"labels":{}});
    core.call(&caller,"metrics.report",json!({"items":[sample]})).unwrap();let detail=core.call(&caller,"metrics.query",json!({"resolutionMs":10000})).unwrap();assert_eq!(detail["items"][0]["value"],5);assert_eq!(detail["items"][0]["coveredMs"],10000);
    drop(core);let core=Core::open(root.path()).unwrap();assert_eq!(core.call(&Caller::shell(),"metrics.query",json!({"resolutionMs":10000})).unwrap()["items"],json!([]));assert_eq!(core.call(&Caller::shell(),"metrics.query",json!({})).unwrap()["items"][0]["value"],5.0);
}
#[test]fn evidence_manifest_authenticates_all_prior_bytes_and_remains_readable_after_exit(){
    use sha2::{Digest,Sha256};let(_root,core,caller)=fixture();append(&core,&caller,"evidence-hash");core.close_instance(&caller.instance_id);
    let job=core.call(&Caller::shell(),"evidence.create",json!({"filter":{"pluginId":"fixture.obs","kind":"domain"}})).unwrap();let started=Instant::now();
    loop{let state=core.call(&Caller::shell(),"evidence.get",json!({"jobId":job["jobId"]})).unwrap();if matches!(state["job"]["state"].as_str(),Some("succeeded"|"partial")){break;}assert!(started.elapsed()<Duration::from_secs(5),"{state}");thread::sleep(Duration::from_millis(20));}
    let mut offset=0;let mut content=String::new();loop{let page=core.call(&Caller::shell(),"evidence.get",json!({"jobId":job["jobId"],"offset":offset})).unwrap();for chunk in page["chunks"].as_array().unwrap(){content.push_str(chunk.as_str().unwrap());}match page["nextOffset"].as_u64(){Some(next)=>offset=next,None=>break}}
    let manifest_start=content.rfind("{\"recordType\":\"manifest\"").unwrap();let manifest:Value=serde_json::from_str(content[manifest_start..].trim()).unwrap();assert_eq!(manifest["value"]["contentSha256"],format!("{:x}",Sha256::digest(&content.as_bytes()[..manifest_start])));assert!(content.contains("plugin.fixture.obs.changed"));assert!(content.contains("recordType\":\"type"));
}

#[test]fn checkpoint_survives_restart_and_diagnostic_reads_do_not_feed_themselves(){
    let(root,core,caller)=fixture();let filter=json!({"kind":"call"});let open=core.call(&caller,"events.v2.open",json!({"filter":filter,"from":"now"})).unwrap();
    core.call(&caller,"events.v2.ack",json!({"subscriptionId":open["subscriptionId"],"feedEpoch":open["feedEpoch"]})).unwrap();
    for _ in 0..8{core.call(&caller,"events.v2.query",json!({"filter":{}})).unwrap();}thread::sleep(Duration::from_millis(150));assert!(core.observation_deliveries().is_empty());
    drop(core);let core=Core::open(root.path()).unwrap();core.call(&caller,"servers.list",json!({})).unwrap();
    let resumed=core.call(&caller,"events.v2.open",json!({"filter":filter,"checkpoint":open["checkpoint"]})).unwrap();core.call(&caller,"events.v2.ack",json!({"subscriptionId":resumed["subscriptionId"],"feedEpoch":resumed["feedEpoch"]})).unwrap();
    let start=Instant::now();loop{let deliveries=core.observation_deliveries();if let Some(batch)=deliveries.iter().find(|d|d.event=="events.v2.batch"){let payload:Value=serde_json::from_str(&batch.payload).unwrap();assert!(payload["items"].as_array().unwrap().iter().all(|v|v["attributes"]["method"]=="servers.list"),"{}",batch.payload);break;}assert!(start.elapsed()<Duration::from_secs(3));thread::sleep(Duration::from_millis(20));}
}
#[test]fn stale_context_allows_duplicate_receipt_but_not_new_facts_and_restart_interrupts(){
    let(root,core,caller)=fixture();let context=core.call(&caller,"traces.begin",json!({"clientOperationId":"stale","actionType":"plugin.fixture.obs.open","trigger":"plugin","fields":{}})).unwrap();
    let item=json!({"clientEventId":"before-end","type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":1},"contextRef":context["contextRef"]});
    let submit=|i:Value|core.call(&caller,"events.v2.appendBatch",json!({"batchId":"retry","items":[i]})).unwrap();assert_eq!(submit(item.clone())["items"][0]["status"],"persisted");
    core.call(&caller,"traces.end",json!({"contextRef":context["contextRef"],"outcome":"succeeded"})).unwrap();assert_eq!(submit(item.clone())["items"][0]["status"],"duplicate");let mut fresh=item;fresh["clientEventId"]=json!("after-end");assert_eq!(submit(fresh)["items"][0]["error"]["code"],"STALE_CONTEXT");
    let active=core.call(&caller,"traces.begin",json!({"clientOperationId":"interrupted","actionType":"plugin.fixture.obs.open","trigger":"plugin","fields":{}})).unwrap();drop(core);
    let core=Core::open(root.path()).unwrap();let page=core.call(&Caller::shell(),"traces.get",json!({"traceId":active["traceId"]})).unwrap();assert!(page["items"].as_array().unwrap().iter().any(|e|e["outcome"]=="interrupted"));
}
#[test]fn endpoint_credits_are_disjoint_and_reclaimed_on_owner_or_instance_end(){
    let(_root,core,caller)=fixture();assert_eq!(core.grant_observation_credit(&caller.instance_id,"worker",128*1024),128*1024);
    for i in 0..4{assert_eq!(core.grant_observation_credit(&caller.instance_id,&format!("ui-{i}"),32*1024),32*1024);}assert_eq!(core.grant_observation_credit(&caller.instance_id,"extra",8192),0);
    core.close_observation_owner("ui-1");assert_eq!(core.grant_observation_credit(&caller.instance_id,"extra",32*1024),32*1024);core.close_instance(&caller.instance_id);
    assert_eq!(core.call(&Caller::shell(),"observability.v2.health.get",json!({})).unwrap()["producerCredits"]["grantedBytes"],0);
}
#[test]fn policy_cas_and_expiring_source_override_preserve_required_facts(){
    let(root,core,caller)=fixture();let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;
    let policy=core.call(&caller,"observability.v2.policy.set",json!({"expectedRevision":"1","patch":{"sourceOverrides":[{"sourceId":"fixture.obs","logLevel":"debug","debugUntil":now+60000}]}})).unwrap();assert_eq!(policy["revision"],"2");
    assert_eq!(core.call(&caller,"observability.v2.policy.set",json!({"expectedRevision":"1","patch":{"logLevel":"error"}})).unwrap_err().code,"CONFLICT");
    drop(core);let db=rusqlite::Connection::open(root.path().join("observability/facts.sqlite")).unwrap();db.execute("UPDATE observation_policy SET data=json_set(data,'$.sourceOverrides[0].debugUntil',0)",[]).unwrap();drop(db);
    let core=Core::open(root.path()).unwrap();assert_eq!(core.call(&Caller::shell(),"observability.v2.policy.get",json!({})).unwrap()["sourceOverrides"],json!([]));core.register_observation_source(&caller,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();assert_eq!(append(&core,&caller,"always-domain")["items"][0]["status"],"persisted");
}

#[test]fn metric_sample_identity_and_kind_fields_cannot_double_count(){
    let(_root,core,caller)=fixture();let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64;let to=now/10000*10000;
    let sample=json!({"metricType":"fixture.count","typeVersion":1,"scope":"one","windowId":"w","fromUtc":to-10000,"toUtc":to,"sampleId":"identity","unit":"count","value":5,"labels":{}});
    let report=|s:Value|core.call(&caller,"metrics.report",json!({"items":[s]})).unwrap()["items"][0].clone();assert_eq!(report(sample.clone())["status"],"persisted");let mut conflicting=sample.clone();conflicting["scope"]=json!("two");assert_eq!(report(conflicting)["error"]["code"],"CONFLICT");let mut invalid=sample;invalid["buckets"]=json!([1]);assert_eq!(report(invalid)["error"]["code"],"INVALID_EVENT");assert_eq!(core.call(&caller,"metrics.query",json!({})).unwrap()["items"][0]["value"],5.0);
}
#[test]fn snapshot_pagination_rejects_changed_dataset_and_workspace_id_resolves_generations(){
    let(_root,core,caller)=fixture();for n in 0..3{core.call(&caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":format!("scope-{n}"),"producerRevision":"1","observedAt":1,"fields":{"count":n}})).unwrap();}
    let page=core.call(&caller,"diagnostics.snapshots.query",json!({"limit":1})).unwrap();core.close_instance(&caller.instance_id);assert_eq!(core.call(&Caller::shell(),"diagnostics.snapshots.query",json!({"limit":1,"cursor":page["nextCursor"]})).unwrap_err().code,"CURSOR_EXPIRED");
    let mut next=caller;next.instance_id="next-instance".into();core.register_observation_source(&next,Some(&serde_json::to_vec(&descriptor()).unwrap())).unwrap();let target=core.observation_target("workspace","workspace-history",Some("generation-one")).unwrap();
    core.call(&next,"events.v2.appendBatch",json!({"batchId":"workspace","items":[{"clientEventId":"workspace-event","type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":1},"targets":[target]}]})).unwrap();
    let page=core.call(&next,"events.v2.query",json!({"filter":{"workspaceId":"workspace-history"}})).unwrap();assert_eq!(page["items"].as_array().unwrap().len(),1);
}


fn wait_job(core:&Core,id:&Value)->Value {let started=Instant::now();loop{let result=core.call(&Caller::shell(),"evidence.get",json!({"jobId":id})).unwrap();if !matches!(result["job"]["state"].as_str(),Some("queued"|"running")){return result;}assert!(started.elapsed()<Duration::from_secs(5),"{result}");thread::sleep(Duration::from_millis(10));}}
fn package_text(core:&Core,id:&Value)->String{let mut text=String::new();let mut offset=0;loop{let result=core.call(&Caller::shell(),"evidence.get",json!({"jobId":id,"offset":offset})).unwrap();for chunk in result["chunks"].as_array().unwrap(){text.push_str(chunk.as_str().unwrap());}match result["nextOffset"].as_u64(){Some(next)=>offset=next,None=>return text}}}
#[test]fn evidence_respects_trace_snapshot_scope_and_task_references(){
    let(root,core,caller)=fixture();let target=core.observation_target("workspace","one",Some("1")).unwrap();
    for scope in ["one","unrelated"]{core.call(&caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":scope,"producerRevision":"1","observedAt":1,"fields":{"count":2}})).unwrap();}
    let context=core.call(&caller,"traces.begin",json!({"clientOperationId":"scoped","actionType":"plugin.fixture.obs.open","trigger":"plugin","fields":{},"targets":[target]})).unwrap();
    let job=core.call(&caller,"evidence.create",json!({"traceId":context["traceId"]})).unwrap();assert_eq!(wait_job(&core,&job["jobId"])["job"]["state"],"succeeded");let text=package_text(&core,&job["jobId"]);let snapshots=text.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).filter(|v|v["recordType"]=="snapshot").collect::<Vec<_>>();assert_eq!(snapshots.len(),1,"{text}");assert_eq!(snapshots[0]["value"]["scope"],"one");
    assert_eq!(core.call(&caller,"evidence.get",json!({"jobId":"invented"})).unwrap_err().code,"INVALID_REFERENCE");
    let db=rusqlite::Connection::open(root.path().join("observability/diagnostics.sqlite")).unwrap();db.execute("DELETE FROM observation_jobs WHERE id=?",[job["jobId"].as_str()]).unwrap();
    assert_eq!(core.call(&caller,"evidence.get",json!({"jobId":job["jobId"]})).unwrap_err().code,"GONE");
}
#[test]fn one_hundred_faults_merge_without_recursive_capture(){
    let(_root,core,caller)=fixture();
    for batch in 0..4{let items=(0..25).map(|n|json!({"clientEventId":format!("fault-{batch}-{n}"),"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":1},"outcome":"failed"})).collect::<Vec<_>>();let result=core.call(&caller,"events.v2.appendBatch",json!({"batchId":format!("b{batch}"),"items":items})).unwrap();assert!(result["items"].as_array().unwrap().iter().all(|i|i["status"]=="persisted"));}
    let start=Instant::now();loop{let health=core.call(&caller,"observability.v2.health.get",json!({})).unwrap();let auto=&health["automaticCapture"];if auto["started"]=="1"{assert_eq!(auto["merged"],"99");assert_eq!(auto["failed"],"0");break;}assert!(start.elapsed()<Duration::from_secs(5),"{health}");thread::sleep(Duration::from_millis(20));}
}
#[test]fn action_versions_and_atomic_policy_facts_keep_their_contract(){
    let(_root,core,caller)=fixture();let mut desc=descriptor();desc["actions"][0]["typeVersion"]=json!(2);core.register_observation_source(&caller,Some(&serde_json::to_vec(&desc).unwrap())).unwrap();
    let action=core.call(&caller,"traces.begin",json!({"clientOperationId":"v2","actionType":"plugin.fixture.obs.open","typeVersion":2,"trigger":"plugin","fields":{}})).unwrap();core.call(&caller,"traces.end",json!({"contextRef":action["contextRef"],"outcome":"succeeded"})).unwrap();
    let trace=core.call(&caller,"traces.get",json!({"traceId":action["traceId"]})).unwrap();assert!(trace["items"].as_array().unwrap().iter().all(|e|e["typeVersion"]==2));
    core.call(&caller,"observability.v2.policy.set",json!({"expectedRevision":"1","patch":{"logLevel":"warn"}})).unwrap();
    let history=core.call(&caller,"events.v2.query",json!({"filter":{"type":"core.observation.policy.changed"}})).unwrap();assert_eq!(history["items"].as_array().unwrap().len(),1);assert_eq!(history["items"][0]["attributes"]["changedFields"],json!(["logLevel"]));assert_eq!(history["items"][0]["provenance"],"core-observed");
}

#[test]fn evidence_freezes_snapshot_before_producer_replaces_it(){
    let(_root,core,caller)=fixture();let report=|revision:&str,count:i64|core.call(&caller,"diagnostics.snapshots.report",json!({"snapshotType":"fixture.state","typeVersion":1,"scope":"one","producerRevision":revision,"observedAt":1,"fields":{"count":count}})).unwrap();
    report("1",1);thread::sleep(Duration::from_millis(5050));let job=core.call(&caller,"diagnostics.capture",json!({"filter":{"pluginId":"fixture.obs"}})).unwrap();report("2",2);
    assert_eq!(wait_job(&core,&job["jobId"])["job"]["state"],"succeeded");let text=package_text(&core,&job["jobId"]);let values=text.lines().map(|s|serde_json::from_str::<Value>(s).unwrap()).filter(|v|v["recordType"]=="snapshot").collect::<Vec<_>>();assert_eq!(values.len(),1);assert_eq!(values[0]["value"]["fields"]["count"],1);
}

#[test]fn batch_transactions_isolate_items_and_never_ack_a_failed_commit(){
    let(root,core,caller)=fixture();let db=rusqlite::Connection::open(root.path().join("observability/facts.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE report_parent(id INTEGER PRIMARY KEY); CREATE TABLE report_child(id INTEGER REFERENCES report_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_item BEFORE INSERT ON records WHEN NEW.client_id='bad-item' BEGIN SELECT RAISE(ABORT,'injected');END; CREATE TRIGGER fail_commit AFTER INSERT ON records WHEN NEW.client_id='bad-commit' BEGIN INSERT INTO report_child VALUES(99);END;").unwrap();
    let item=|id:&str,n:u32|json!({"clientEventId":id,"type":"plugin.fixture.obs.changed","typeVersion":1,"occurredAt":1,"fields":{"count":n}});
    let report=|items:Value|core.call(&caller,"events.v2.appendBatch",json!({"batchId":"batch","items":items})).unwrap();
    let mixed=report(json!([item("good-one",1),item("bad-item",2),item("good-two",3),item("good-one",1),item("good-one",4)]));
    assert_eq!(mixed["items"].as_array().unwrap().iter().map(|r|r["status"].as_str().unwrap()).collect::<Vec<_>>(),vec!["persisted","rejected","persisted","duplicate","rejected"]);
    let failed=report(json!([item("rolled-back",1),item("bad-commit",2)]));assert!(failed["items"].as_array().unwrap().iter().all(|r|r["status"]=="rejected"));
    assert_eq!(db.query_row("SELECT count(*) FROM records WHERE client_id IN('rolled-back','bad-commit')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    let log=json!({"clientEventId":"mixed-log","type":"plugin.fixture.obs.debug","typeVersion":1,"level":"info","occurredAt":1,"fields":{}});let mixed=report(json!([item("first-fact",1),log,item("last-fact",2)]));let seq=mixed["items"].as_array().unwrap().iter().map(|r|r["sequence"].as_str().unwrap().parse::<u64>().unwrap()).collect::<Vec<_>>();assert!(seq.windows(2).all(|s|s[0]<s[1]));
}
