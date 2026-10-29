use super::*;
use crate::{Caller,Core};

#[path="quality.generated.rs"]mod contract;
use contract::validate;
impl Core {
    pub fn quality_revision(&self)->u64 {self.quality.revision()}
    pub fn quality_object_id(p:&Value)->Result<String> {
        validate("x-methods","quality.summary",p)?;
        Ok(crate::route::digest(&json!([p["configEpoch"],p["serverId"].as_str().unwrap_or("")])))
    }
    pub fn quality_state(&self,p:&Value)->Result<Value> {
        validate("x-methods","quality.summary",p)?;
        if p["configEpoch"]!=self.traffic.config_epoch{return Err(CoreError::new("STORE_EPOCH_CHANGED","诊断目标属于另一配置"));}
        Ok(json!({"configEpoch":self.traffic.config_epoch,"serverId":p["serverId"],"revision":self.quality.revision().to_string()}))
    }
    pub(crate) fn restore_quality_policies(&self)->Result<()> {
        for server in self.store.servers()? {
            let value=self.store.get_setting("@core",&format!("quality:{}",server.id))?;
            if !value.is_null(){validate("x-results","quality.policy.get",&value)?;self.quality.load_policy(&server.id,serde_json::from_value(value)?)?;}
        }
        Ok(())
    }
    fn quality_connections(&self)->Vec<Arc<Connection>> {
        self.registry.lock().unwrap().connections.values().filter(|c|*c.status.lock().unwrap()=="active"&&!c.purpose.starts_with("speedtest")).cloned().collect()
    }
    pub(crate) fn quality_tick(&self){self.quality.tick(&self.quality_connections(),&self.observability);}
    pub(crate) fn quality_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        validate("x-methods",method,p)?;
        if p.get("configEpoch").is_some_and(|epoch|epoch!=&self.traffic.config_epoch){return Err(CoreError::new("STORE_EPOCH_CHANGED","诊断目标属于另一配置"));}
        let result=match method {
            "quality.summary"=>{self.quality_tick();self.quality.summary(p["serverId"].as_str())},
            "quality.policy.get"=>serde_json::to_value(self.quality.policy(p["serverId"].as_str().unwrap()))?,
            "quality.policy.set"=>{
                let server=p["serverId"].as_str().unwrap();let _configuration=self.configuration.lock().unwrap();
                self.store.server(server)?;
                let mut policy=serde_json::to_value(self.quality.policy(server))?;
                if policy["revision"]!=p["expectedRevision"]{return Err(CoreError::new("CONFLICT","探测设置已变化，请刷新"));}
                for (key,value) in p["patch"].as_object().unwrap(){policy[key]=value.clone();}
                if policy["recoveryMs"].as_u64().unwrap()>=policy["slowMs"].as_u64().unwrap(){return Err(CoreError::new("INVALID_PARAMS","恢复阈值必须小于提醒阈值"));}
                let revision=policy["revision"].as_str().unwrap().parse::<u64>().ok().and_then(|n|n.checked_add(1)).ok_or_else(||CoreError::new("CAPACITY","探测设置修订已用尽"))?;
                policy["revision"]=json!(revision.to_string());
                // Admission is checked before persistence; no successful write with an unapplied policy.
                {let state=self.quality.state.lock().unwrap();if !state.policies.contains_key(server)&&state.policies.len()>=MAX_TARGETS{return Err(CoreError::new("CAPACITY","探测设置容量已满"));}}
                self.store.set_setting("@core",&format!("quality:{server}"),&policy)?;
                self.quality.load_policy(server,serde_json::from_value(policy.clone())?)?;policy
            },
            "quality.check.start"=>{
                self.quality_tick();self.quality.start(&caller.instance_id,p["operationId"].as_str().unwrap(),p["serverId"].as_str().unwrap(),p["routeRevision"].as_str(),&crate::route::digest(p),&self.quality_connections())?
            },
            "quality.check.get"|"quality.check.cancel"=>self.quality.receipt(&caller.instance_id,p["taskId"].as_str().unwrap(),method.ends_with("cancel"))?,
            _=>return Err(CoreError::new("UNSUPPORTED_API","未知连接诊断接口")),
        };
        validate("x-results",method,&result)?;Ok(result)
    }
}
