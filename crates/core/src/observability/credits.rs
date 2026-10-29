//! Host-issued buffer grants. Every JS endpoint receives a disjoint allowance.
use crate::Core;
use std::collections::{HashMap,HashSet};
#[derive(Default)]pub(super) struct Credits {pub grants:HashMap<String,(String,usize)>}
impl Core {
    pub fn grant_observation_credit(&self,instance:&str,endpoint:&str,requested:usize)->usize{
        let mut credits=self.observability.credits.lock().unwrap();
        if let Some((owner,bytes))=credits.grants.get(endpoint){return if owner==instance{*bytes}else{0};}
        let total:usize=credits.grants.values().map(|(_,n)|n).sum();let own:usize=credits.grants.values().filter(|(owner,_)|owner==instance).map(|(_,n)|n).sum();
        let grant=requested.min((256usize*1024).saturating_sub(own)).min((8usize*1024*1024).saturating_sub(total));
        if grant>=8192&&credits.grants.len()<1024{credits.grants.insert(endpoint.into(),(instance.into(),grant));grant}else{0}
    }
    pub fn retain_observation_owners(&self,live:&HashSet<String>){
        self.observability.credits.lock().unwrap().grants.retain(|owner,_|live.contains(owner));
        self.observability.retain_feeds(live);
        let mut contexts=self.observability.contexts.lock().unwrap();for context in contexts.active.values_mut(){if let Ok(source)=serde_json::from_value::<crate::Caller>(context["source"].clone()){if source.view_id.is_some()&&!live.contains(&super::feed::owner(&source)){context["surfaceEnded"]=serde_json::json!(true);}}}
    }
}
