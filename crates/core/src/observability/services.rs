use crate::{Caller,Core,Result};
use serde_json::{json,Value};
#[path="observability.generated.rs"]mod contract;
pub(super) fn schema()->&'static Value{static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../../contracts/observability-v2.schema.json")).expect("observation contract"))}
pub(super) fn validate_item(item:&Value)->Result<()>{crate::contract::check(schema(),&schema()["$defs"]["Item"],item,"event").map_err(|_|crate::CoreError::new("INVALID_EVENT","事件格式无效"))}
pub(super) fn validate(kind:&str,method:&str,value:&Value)->Result<()>{let schema=schema();let name=schema[kind][method].as_str().ok_or_else(||crate::CoreError::new("UNSUPPORTED_API","未知观测接口"))?;crate::contract::check(schema,&schema["$defs"][name],value,method).map_err(|_|crate::CoreError::new(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},"观测契约校验失败"))}
impl Core {
    pub(crate) fn observation_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        if schema()["x-methods"].get(method).is_some(){return self.observation_v2_dispatch(caller,method,p);}
        contract::validate("x-methods",method,p)?;
        let value=match method {
            "events.query"=>self.observability.events(&caller.instance_id,p)?,
            "events.get"=>self.observability.event(p)?,
            "events.aggregate"=>self.observability.aggregate_events(p)?,
            "observability.health.get"=>self.observability.health(),
            "observability.policy.get"=>json!({"revision":"1","profile":"communication-v1","auditRetentionDays":30,"logRetentionDays":7,"trafficRetentionDays":30,"diagnosticRetentionDays":7,"onlineBudgetBytes":512*1024*1024,"emergencyBudgetBytes":8*1024*1024,"auditReservedBytes":64*1024*1024}),
            _=>return Err(crate::CoreError::new("UNSUPPORTED_API","未知公共观测接口")),
        };
        contract::validate("x-results",method,&value)?;Ok(value)
    }
    fn observation_v2_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        if method=="events.v2.appendBatch" {
            if p.as_object().is_none_or(|o|o.len()!=2)||p["batchId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128)||p["items"].as_array().is_none_or(|a|a.len()>32){return Err(crate::CoreError::new("INVALID_PARAMS","事件批次格式无效"));}
        }else{validate("x-methods",method,p)?;}
        let store=&self.observability;
        let result=match method{
            "observability.describe"=>store.describe()?,
            "observability.targets.resolve"=>self.resolve_observation_target(caller,p)?,
            "events.types.list"=>store.types(p,false)?,"events.types.get"=>store.types(p,true)?,
            "events.v2.appendBatch"=>store.append(caller,p)?,
            "traces.begin"=>store.trace_begin(caller,p)?,"traces.stage"=>store.trace_stage(caller,p)?,"traces.end"=>store.trace_end(caller,p)?,
            "traces.get"=>store.query_v2(&caller.instance_id,&json!({"filter":{"traceId":p["traceId"]},"limit":p["limit"].as_u64().unwrap_or(100),"ascending":true}).as_object().map(|m|{let mut v=Value::Object(m.clone());if let Some(c)=p.get("cursor"){v["cursor"]=c.clone();}v}).unwrap())?,
            "events.v2.query"=>store.query_v2(&caller.instance_id,p)?,"events.v2.get"=>store.get_v2(p)?,"events.v2.aggregate"=>store.aggregate_v2(&caller.instance_id,p)?,
            "events.v2.open"=>store.feed_open(caller,p,None)?,"events.v2.ack"=>store.feed_ack(caller,p,None)?,"events.v2.close"=>store.feed_close(caller,p,None)?,
            "observability.v2.policy.get"=>store.policy()?,"observability.v2.policy.set"=>store.set_policy(caller,p)?,"observability.v2.health.get"=>store.health_v2(),
            "diagnostics.snapshots.report"=>store.snapshot_report(caller,p)?,"diagnostics.snapshots.query"=>store.data_query(p,false)?,"diagnostics.snapshots.get"=>store.snapshot_get(p)?,
            "metrics.report"=>store.metric_report(caller,p)?,"metrics.query"=>store.metric_query(p)?,
            "diagnostics.capture"=>store.create_evidence(p,true)?,"evidence.create"=>store.create_evidence(p,false)?,
            "events.v2.cleanup.start"=>store.cleanup_start(p)?,
            "diagnostics.capture.get"|"evidence.get"|"events.v2.cleanup.get"=>store.job_get(p)?,
            "diagnostics.capture.cancel"|"evidence.cancel"|"events.v2.cleanup.cancel"=>store.job_cancel(p)?,
            "evidence.export"=>return Err(crate::CoreError::new("UI_REQUIRED","导出需要原生来源窗口")),
            _=>return Err(crate::CoreError::new("UNSUPPORTED_API","观测能力尚未启用")),
        };validate("x-results",method,&result)?;Ok(result)
    }
}
