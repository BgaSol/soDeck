use super::{calls::event,schema::Partition,Observability};
use crate::{quality::probe::Outcome,Result};
use serde_json::{json,Value};
impl Observability {
    pub fn quality_ended(&self,server:&str,connection:&str,generation:u64)->Result<()> {
        let server=server.to_owned();let payload=json!({"connectionId":connection,"generation":generation.to_string()});let boot=self.boot_id.clone();let epoch=self.config_epoch.clone();
        self.submit(false,4096,move|db|{db.admit(Partition::Facts,4096)?;let tx=db.facts.transaction()?;
            event(&tx,"quality.segment.ended","",crate::model::now_ms(),"{\"source\":\"core\",\"instanceId\":\"connection-diagnostics\"}",&boot,&epoch,Some(&server),None,&payload)?;tx.commit()?;Ok(Value::Null)
        })?;Ok(())
    }
    pub fn quality_sample(&self,server:&str,route:&str,connection:&str,generation:u64,segment:u64,sample:&Outcome)->Result<()> {
        let payload=json!({"method":crate::quality::METHOD,"routeRevision":route,"connectionId":connection,"generation":generation.to_string(),"segmentId":segment.to_string(),"sample":sample});
        let target=server.to_owned();let boot=self.boot_id.clone();let epoch=self.config_epoch.clone();let time=sample.observed_at;
        let outcome=sample.outcome;let charge=payload.to_string().len()+4096;
        self.submit(false,charge,move|db|{
            db.admit(Partition::Facts,charge)?;let tx=db.facts.transaction()?;
            event(&tx,"quality.response.sample","",time,"{\"source\":\"core\",\"instanceId\":\"connection-diagnostics\"}",&boot,&epoch,Some(&target),Some(outcome),&payload)?;
            tx.commit()?;Ok(Value::Null)
        })?;Ok(())
    }
}
