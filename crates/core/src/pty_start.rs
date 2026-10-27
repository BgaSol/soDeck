//! Queryable startup receipts prevent replay of remote shell creation.
use super::*;
pub(crate) struct Receipt {
    params: Value,
    result: Option<Result<Value>>,
    completed: Option<Instant>,
    expired: bool,
}
pub(crate) fn sweep(starts:&mut HashMap<(String,String),Receipt>,registry:&mut Registry){
    let now=Instant::now();
    starts.retain(|(instance,_),receipt|{
        if registry.revoked.contains(instance){return false;}
        let Some(completed)=receipt.completed else{return true;};
        if now.duration_since(completed)>=Duration::from_secs(60)&&!receipt.expired {
            if let Some(Ok(result))=&receipt.result {
                if let Some(id)=result["ptyId"].as_str(){
                    if registry.ptys.get(id).is_some_and(|pty|pty.state.lock().unwrap().view_id.is_none()) {
                        if let Some(pty)=registry.ptys.remove(id){pty.connection.close_pty(&pty.state);}
                    }
                }
            }
            receipt.params=Value::Null;receipt.result=None;receipt.expired=true;
        }
        now.duration_since(completed)<Duration::from_secs(660)
    });
    Core::reap(registry);
}
impl Core {
    pub(crate) fn start_pty(&self,caller:&Caller,p:&Value)->Result<Value>{
        let key=(caller.instance_id.clone(),field(p,"operationId")?.to_owned());
        {
            let mut starts=self.pty_starts.lock().unwrap();
            if let Some(receipt)=starts.get(&key){
                if receipt.expired{return Err(CoreError::new("OPERATION_EXPIRED","启动回执已过期，不能重放"));}
                if receipt.params!=*p{return Err(CoreError::new("OPERATION_CONFLICT","启动操作标识不能用于不同参数"));}
                return receipt.result.clone().unwrap_or_else(||Err(CoreError::new("RESULT_UNKNOWN","启动仍在进行，请查询回执")));
            }
            if starts.len()>=512||starts.keys().filter(|(id,_)|id==&caller.instance_id).count()>=64{return Err(CoreError::new("CAPACITY","PTY 启动回执已满"));}
            starts.insert(key.clone(),Receipt{params:p.clone(),result:None,completed:None,expired:false});
        }
        let result=self.with_operation(caller,p,|token|self.open_pty(caller,p,token)).and_then(|result|{
            pty_observe::validate("pty.openCommand","x-results",&result)?;Ok(result)
        });
        if let Some(receipt)=self.pty_starts.lock().unwrap().get_mut(&key){receipt.result=Some(result.clone());receipt.completed=Some(Instant::now());}
        result
    }
    pub(crate) fn pty_start_result(&self,caller:&Caller,p:&Value)->Result<Value>{
        pty_observe::validate("pty.start.get","x-methods",p)?;
        let starts=self.pty_starts.lock().unwrap();
        let receipt=starts.get(&(caller.instance_id.clone(),field(p,"operationId")?.to_owned())).ok_or_else(||CoreError::new("OPERATION_NOT_FOUND","启动操作不存在"))?;
        if receipt.expired{return Err(CoreError::new("OPERATION_EXPIRED","启动回执已过期"));}
        let value=match &receipt.result {
            None=>json!({"state":"running","result":null,"error":null}),
            Some(Ok(result))=>json!({"state":"completed","result":result,"error":null}),
            Some(Err(error))=>json!({"state":"failed","result":null,"error":{"code":error.code,"message":error.message}}),
        };
        pty_observe::validate("pty.start.get","x-results",&value)?;Ok(value)
    }
}
