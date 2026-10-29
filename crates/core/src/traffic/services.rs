use crate::{Caller,Core,CoreError,Result};
use serde_json::{json,Value};
impl Core {
    pub fn release_communication_owner(&self,owner:&str){self.traffic_queries.remove_owner(owner);self.quality.cancel_owner(owner);}
    pub(crate) fn communication_history(&self,method:&str,p:&Value)->Result<Value> {
        super::history_contract::validate("x-methods",method,p)?;
        let layer=p["layer"].as_str().unwrap_or("channel");
        let target=p.get("target").cloned().unwrap_or_else(||json!({"kind":"all","configEpoch":self.traffic.config_epoch}));
        let stored=|range:&Value|self.observability.history(&target,layer,range["fromUtc"].as_u64().unwrap(),range["toUtc"].as_u64().unwrap());
        let value=match method {
            "communication.history.query"=>if p["range"]["kind"]=="live" {
                self.traffic.live_history(&self.communication_target(p)?,layer,p["range"]["minutes"].as_u64().unwrap())?
            }else{stored(&p["range"])?},
            "communication.history.compare"=>{
                let first=stored(&p["first"])?;let second=stored(&p["second"])?;
                let comparable=first["coverage"]=="complete"&&second["coverage"]=="complete"
                    &&p["first"]["toUtc"].as_u64().unwrap()-p["first"]["fromUtc"].as_u64().unwrap()==p["second"]["toUtc"].as_u64().unwrap()-p["second"]["fromUtc"].as_u64().unwrap();
                json!({"first":first,"second":second,"comparable":comparable})
            },
            "communication.policy.get"=>self.communication_policy()?,
            "communication.policy.set"=>{
                let _guard=self.configuration.lock().unwrap();let mut policy=self.communication_policy()?;
                if p["expectedRevision"]!=policy["revision"]{return Err(CoreError::new("CONFLICT","通信设置已变化，请刷新"));}
                let revision=policy["revision"].as_str().unwrap().parse::<u64>().ok().and_then(|r|r.checked_add(1)).ok_or_else(||CoreError::new("CAPACITY","设置修订已用尽"))?;
                for (key,value) in p["patch"].as_object().unwrap(){policy[key]=value.clone();}
                policy["revision"]=json!(revision.to_string());
                self.store.set_setting("@core","communicationPolicy",&policy)?;
                self.traffic.set_history_enabled(policy["saveHistory"].as_bool().unwrap());policy
            },
            _=>return Err(CoreError::new("UNSUPPORTED_API","未知通信历史接口")),
        };
        super::history_contract::validate("x-results",method,&value)?;Ok(value)
    }
    pub(crate) fn communication_policy(&self)->Result<Value> {
        let value=self.store.get_setting("@core","communicationPolicy")?;
        if value.is_null(){Ok(json!({"revision":"0","saveHistory":true,"rateUnit":"bytes","notifications":true}))}
        else{super::history_contract::validate("x-results","communication.policy.get",&value)?;Ok(value)}
    }
    pub fn validate_communication(kind:&str,method:&str,value:&Value)->Result<()> {super::contract::validate(kind,method,value)}
    /// Internal latest-state handoff; public subscription controls are audited by the host.
    pub fn communication_revision(&self)->u64 {self.traffic.revision()}
    pub fn communication_snapshot(&self,p:&Value)->Result<Value> {
        super::contract::validate("x-methods","communication.summary",p)?;
        let target=self.communication_target(p)?;
        self.traffic.target_summary(&target,p["layer"].as_str().unwrap_or("channel"))
    }
    pub fn communication_object_id(p:&Value)->Result<String> {
        use sha2::{Digest,Sha256};
        super::contract::validate("x-methods","communication.summary",p)?;
        let t=&p["target"];
        let normalized=json!([p["layer"].as_str().unwrap_or("channel"),t["configEpoch"].as_str().unwrap_or("current"),
            t["kind"].as_str().unwrap_or("all"),t["serverId"].as_str().unwrap_or(""),t["bootId"].as_str().unwrap_or(""),
            t["resourceId"].as_str().unwrap_or(""),t["generation"].as_str().unwrap_or("")]);
        Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(&normalized)?)))
    }
    fn communication_target(&self,p:&Value)->Result<Value> {
        let target=p.get("target").cloned().unwrap_or_else(||json!({"kind":"all","configEpoch":self.traffic.config_epoch}));
        if target["configEpoch"]!=self.traffic.config_epoch {return Err(CoreError::new("STORE_EPOCH_CHANGED","通信目标属于不同配置，请刷新"));}
        if target["kind"]=="resource"&&target["bootId"]!=self.traffic.boot_id {return Err(CoreError::new("STALE_GENERATION","资源属于上次运行"));}
        Ok(target)
    }
    pub(crate) fn traffic_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        super::contract::validate("x-methods",method,p)?;
        let target=self.communication_target(p)?;
        let layer=p["layer"].as_str().unwrap_or("channel");
        let result=if method=="communication.summary" {self.traffic.target_summary(&target,layer)?}
        else {self.traffic_queries.page(&caller.instance_id,method,p,||self.traffic.view(&target,layer,method=="communication.connections",p["groupBy"]=="server"))?};
        super::contract::validate("x-results",method,&result)?;
        Ok(result)
    }
}
