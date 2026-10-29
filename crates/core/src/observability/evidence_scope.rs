use super::{Observability,schema::Partition};
use crate::{CoreError,Result};
use serde_json::{json,Value};
use std::collections::BTreeSet;

impl Observability {
    /// Resolve historical diagnostic targets rather than consulting live plugin state.
    /// An unresolved narrow scope yields an explicit gap, never an unrelated snapshot.
    pub(super) fn evidence_scopes(&self,filter:&Value)->Result<Option<Vec<String>>>{
        if !["traceId","workspaceId","serverId","diagnosticRef","callId","operationId","clientEventId"].iter().any(|key|filter.get(*key).is_some()){return Ok(None);}
        let mut scopes=BTreeSet::new();let mut references=BTreeSet::new();
        if let Some(id)=filter["workspaceId"].as_str(){scopes.insert(id.to_owned());}
        if let Some(id)=filter["diagnosticRef"].as_str(){references.insert(id.to_owned());}
        let mut query=json!({"filter":filter,"limit":100});let mut count=0;
        loop {let page=self.query_v2("evidence-scope",&query)?;
            for event in page["items"].as_array().unwrap(){count+=1;
                for target in event["targets"].as_array().into_iter().flatten(){if let Some(id)=target["ref"].as_str(){references.insert(id.to_owned());}}
                if references.len()>256||count>10000{return Err(CoreError::new("CAPACITY","证据目标过多，请缩小范围"));}
            }
            if page["nextCursor"].is_null(){break;}query["cursor"]=page["nextCursor"].clone();
        }
        for reference in references {scopes.insert(reference.clone());
            let mapped=self.queries.query(Partition::Facts,move|db|{use rusqlite::OptionalExtension;Ok(db.query_row("SELECT object_id FROM diagnostic_targets WHERE reference=?",[reference],|r|r.get::<_,String>(0)).optional()?.map(Value::String).unwrap_or(Value::Null))})?;
            if let Some(id)=mapped.as_str(){scopes.insert(id.to_owned());}
        }
        Ok(Some(scopes.into_iter().collect()))
    }
}
