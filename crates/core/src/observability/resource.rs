//! A resource owner records creation and exactly one release, including implicit teardown.
use super::{Observability,builtin,catalog,context,model,schema::Partition,targets};
use crate::Caller;
use serde_json::{json,Value};
use std::sync::{Arc,Weak};

pub(crate) struct ResourceObservation {
    store:Weak<Observability>,caller:Caller,target:Value,server:Option<Value>,kind:&'static str,context:Option<Value>,active:bool,
}
impl ResourceObservation {
    pub(crate) fn new(store:&Arc<Observability>,caller:&Caller,id:&str,generation:Option<u64>,server:Option<&str>,kind:&'static str)->Self{
        let mut value=Self::pending(store,caller,id,generation,server,kind);value.activate();value
    }
    /// Carry the originating context until the native owner actually allocates a resource.
    pub(crate) fn pending(store:&Arc<Observability>,caller:&Caller,id:&str,generation:Option<u64>,server:Option<&str>,kind:&'static str)->Self{
        let target=targets::reference(&store.signing_key,&store.boot_id,"resource",id,generation.map(|v|v.to_string()).as_deref());
        let server=server.map(|id|targets::reference(&store.signing_key,&store.config_epoch,"server",id,Some(&store.config_epoch)));
        Self{store:Arc::downgrade(store),caller:caller.clone(),target,server,kind,context:context::DiagnosticScope::capture(),active:false}
    }
    pub(crate) fn activate(&mut self){
        if !self.active{self.active=true;self.record(true);}
    }
    fn record(&self,created:bool){
        let Some(store)=self.store.upgrade()else{return};
        let name=if created{"core.resource.created"}else{"core.resource.released"};
        let Ok(definition)=builtin::definition(name)else{return};
        let mut event=model::envelope(&self.caller,&store.boot_id,&store.epoch,"lifecycle",name,json!({"method":self.kind,"resourceRef":self.target["ref"]}));
        event["configEpoch"]=json!(store.config_epoch);event["provenance"]=json!("core-observed");event["targets"]=json!([self.target]);
        if let Some(server)=&self.server{event["targets"].as_array_mut().unwrap().push(server.clone());}
        if let Some(context)=context::DiagnosticScope::capture().or_else(||self.context.clone()){context::attach(&mut event,&context);}
        let key=store.signing_key.clone();let _=store.submit_partition(Partition::Facts,false,8192,move|db|{db.admit(Partition::Facts,8192)?;let tx=db.facts.transaction()?;catalog::save_type(&tx,"","events",&definition)?;model::insert(&tx,&key,"f",&mut event,None)?;tx.commit()?;Ok(Value::Null)});
    }
}
impl Drop for ResourceObservation{fn drop(&mut self){if self.active{self.record(false);}}}

impl Observability {
    pub(crate) fn resource_caller(&self,instance:&str)->Caller{
        let catalog=self.catalog.lock().unwrap();let source=catalog.sources.get(instance);
        Caller{source:if source.is_some(){"plugin"}else{"core"}.into(),instance_id:instance.into(),plugin_id:source.map(|v|v.0.clone()),plugin_version:source.map(|v|v.1.clone()),view_id:None,mount_generation:None}
    }
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn reservations_do_not_invent_resources_and_native_owner_releases_once(){
        let root=tempfile::tempdir().unwrap();let core=crate::Core::open(root.path()).unwrap();let caller=Caller::shell();
        drop(ResourceObservation::pending(&core.observability,&caller,"failed-open",Some(1),None,"remote.handle"));
        let mut owner=ResourceObservation::pending(&core.observability,&caller,"real-open",Some(1),None,"remote.handle");
        owner.activate();owner.activate();drop(owner);
        // Readers use a separate pool; wait for the asynchronous release to commit.
        let started=std::time::Instant::now();
        let events=loop{let page=core.call(&caller,"events.v2.query",json!({"filter":{"kind":"lifecycle"},"limit":100})).unwrap();
            if page["items"].as_array().unwrap().iter().any(|e|e["type"]=="core.resource.released"){break page;}
            assert!(started.elapsed()<std::time::Duration::from_secs(3));std::thread::sleep(std::time::Duration::from_millis(10));};
        let items=events["items"].as_array().unwrap();
        let created:Vec<_>=items.iter().filter(|e|e["type"]=="core.resource.created").collect();
        let released:Vec<_>=items.iter().filter(|e|e["type"]=="core.resource.released").collect();
        assert_eq!(created.len(),1);assert_eq!(released.len(),1);
        assert_eq!(created[0]["targets"],released[0]["targets"]);
    }
}
