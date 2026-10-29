//! Automatic copies consume the same task and storage budgets as explicit captures.
//! Only fact owners request them; failures in this module never trigger another copy.
use super::Observability;
use crate::Caller;
use serde_json::{json,Value};
use std::{collections::{HashMap,VecDeque},sync::Arc};

#[derive(Default)]
pub(super) struct Automatic {
    pending:VecDeque<Value>,
    recent:HashMap<String,u64>,
    admitted:VecDeque<u64>,
    merged:u64,
    throttled:u64,
    failed:u64,
    started:u64,
    pub tail_gaps:u64,
}
impl Observability {
    pub(super) fn request_automatic(&self,caller:&Caller,targets:&Value,event:Option<Value>){
        let now=crate::model::now_ms();
        // View/workspace scope remains stable when a resource is replaced.
        let target=targets.as_array().and_then(|a|a.iter().find(|t|t["kind"]=="workspace").or_else(||a.iter().find(|t|t["kind"]=="resource")));
        let key=super::model::hash(&json!([caller.instance_id,target.map(|v|&v["ref"])]));
        let mut state=self.automatic.lock().unwrap();
        state.recent.retain(|_,time|now.saturating_sub(*time)<300000);
        while state.admitted.front().is_some_and(|time|now.saturating_sub(*time)>=60000){state.admitted.pop_front();}
        if state.recent.contains_key(&key){state.merged+=1;return;}
        if state.admitted.len()>=10||state.pending.len()>=10 {state.throttled+=1;return;}
        let mut filter=json!({"instanceId":caller.instance_id});
        if let Some(plugin)=&caller.plugin_id{filter["pluginId"]=json!(plugin);}
        if let Some(target)=target{filter["diagnosticRef"]=target["ref"].clone();}
        let mut request=json!({"filter":filter,"allowPartial":true});
        if let Some(event)=event{request["eventId"]=event;}
        state.pending.push_back(request);state.recent.insert(key,now);state.admitted.push_back(now);
    }
    pub(super) fn pump_automatic(self:&Arc<Self>){
        if self.jobs.lock().unwrap().active.values().filter(|(kind,_)|kind!="cleanup").count()>=2{return;}
        let request=self.automatic.lock().unwrap().pending.pop_front();
        if let Some(request)=request{let result=self.create_evidence(&request,true);let mut state=self.automatic.lock().unwrap();if result.is_ok(){state.started+=1;}else{state.failed+=1;}}
    }
    pub(super) fn automatic_health(&self)->Value {
        let state=self.automatic.lock().unwrap();
        json!({"pending":state.pending.len(),"mergeScopes":state.recent.len(),"started":state.started.to_string(),"merged":state.merged.to_string(),"throttled":state.throttled.to_string(),"failed":state.failed.to_string(),"managementTailGaps":state.tail_gaps.to_string()})
    }
}
