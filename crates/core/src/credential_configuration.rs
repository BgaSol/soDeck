use super::*;
use hmac::{Hmac,Mac};
use sha2::Sha256;
use zeroize::Zeroizing;
#[path="credential_configuration_contract.generated.rs"] mod contract;

struct Context { caller:String, request:Value, credential_revision:u64, created:Instant }
struct Receipt { digest:Vec<u8>, value:Value, created:Instant, pending_invalidation:bool }
pub(crate) struct ConfigurationCredentials {
    key:Zeroizing<[u8;32]>, contexts:HashMap<String,Context>, receipts:HashMap<(String,String),Receipt>,
}
impl ConfigurationCredentials {
    pub fn new()->Result<Self>{
        let mut key=Zeroizing::new([0;32]);
        getrandom::fill(key.as_mut()).map_err(|_|CoreError::new("INTERNAL_ERROR","无法创建凭据提交上下文"))?;
        Ok(Self{key,contexts:HashMap::new(),receipts:HashMap::new()})
    }
    pub(crate) fn begin_runtime(&mut self,caller:&Caller,method:&str,p:&Value)->Result<Option<Value>> {
        self.prune();
        let operation=(caller_key(caller),format!("runtime:{}",field(p,"challengeId")?));
        let serialized=Zeroizing::new(serde_json::to_vec(&json!([method,p]))?);
        let mut mac=Hmac::<Sha256>::new_from_slice(self.key.as_ref()).expect("HMAC key");mac.update(&serialized);
        if let Some(receipt)=self.receipts.get(&operation){
            if mac.verify_slice(&receipt.digest).is_err(){return Err(CoreError::new("OPERATION_CONFLICT","此挑战已提交不同的答复"));}
            return receipt_result(&receipt.value).map(Some);
        }
        if self.receipts.len()>=1024{return Err(CoreError::new("LIMIT_EXCEEDED","凭据保存回执已满"));}
        self.receipts.insert(operation,Receipt{digest:mac.finalize().into_bytes().to_vec(),value:json!({"status":"running"}),created:Instant::now(),pending_invalidation:false});
        Ok(None)
    }
    pub(crate) fn finish_runtime(&mut self,caller:&Caller,p:&Value,result:&Result<Value>) {
        let operation=(caller_key(caller),format!("runtime:{}",p["challengeId"].as_str().unwrap()));
        if let Ok(value)=result{if let Some(receipt)=self.receipts.get_mut(&operation){receipt.value=json!({"status":"stored","result":value});}}
        else{self.receipts.remove(&operation);}
    }
    fn prune(&mut self){self.contexts.retain(|_,v|v.created.elapsed()<Duration::from_secs(300));self.receipts.retain(|_,v|v.created.elapsed()<Duration::from_secs(86400)||v.value["status"]=="running"||v.pending_invalidation);}
}
fn conflict()->CoreError{CoreError::new("REVISION_CONFLICT","连接配置或凭据已变化，请重新加载后保存凭据")}
fn caller_key(c:&Caller)->String{format!("{}:{}:{}",c.source,c.plugin_id.as_deref().unwrap_or("@shell"),c.instance_id)}
fn owner_key(p:&Value)->String{if p["target"]["kind"]=="ssh"{p["target"]["id"].as_str().unwrap().into()}else{format!("proxy:{}",p["target"]["id"].as_str().unwrap())}}

impl Core {
    pub(crate) fn retry_credential_invalidation(&self){
        let Ok(_configuration)=self.configuration.try_lock()else{return};
        let mut state=self.configuration_credentials.lock().unwrap();
        if !state.receipts.values().any(|r|r.pending_invalidation){return;}
        if crate::server_catalog_store::bump_revision(&self.store.db.lock().unwrap()).is_ok(){
            for receipt in state.receipts.values_mut().filter(|r|r.pending_invalidation){receipt.pending_invalidation=false;receipt.value["result"].as_object_mut().unwrap().remove("postCommitPending");}
        }
    }
    fn check_credential_owner(&self,p:&Value)->Result<(bool,String)>{
        let id=p["target"]["id"].as_str().unwrap();let db=self.store.db.lock().unwrap();
        if p["target"]["kind"]=="ssh"{
            let s=crate::route_store::server(&db,id)?;
            if p["expectedOwnerRevision"].as_u64()!=Some(s.revision){return Err(conflict());}
            crate::configuration_dependencies::check(&db,&crate::route_store::route(&db,id)?,&p["expectedDependencies"])?;
            Ok((self.secrets.remembered(&db,"ssh",id,"preset")?,s.auth_type))
        }else{
            let proxy=crate::route_store::proxy(&db,id)?;
            if p["expectedOwnerRevision"].as_u64()!=Some(proxy.revision)||p["expectedDependencies"]!=json!([]){return Err(conflict());}
            Ok((self.secrets.remembered(&db,"socks5",id,"preset")?,proxy.auth_type))
        }
    }
    pub(crate) fn credential_configuration_dispatch(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        contract::validate("x-methods",method,p)?;
        self.retry_credential_invalidation();
        let caller=caller_key(caller);
        if method=="credentials.configuration.operation"{
            let mut state=self.configuration_credentials.lock().unwrap();state.prune();
            return Ok(state.receipts.get(&(caller,field(p,"operationId")?.into())).map(|v|v.value.clone()).unwrap_or(json!({"status":"unknown"})));
        }
        let _configuration=self.configuration.lock().unwrap();
        let mut state=self.configuration_credentials.lock().unwrap();state.prune();
        if method=="credentials.configuration.prepare"{
            let (remembered,_)=self.check_credential_owner(p)?;
            if state.contexts.len()>=256{return Err(CoreError::new("LIMIT_EXCEEDED","凭据准备请求已达上限，请稍后重试"));}
            let token=id();let revision=*self.credential_revisions.lock().unwrap().get(&owner_key(p)).unwrap_or(&0);
            state.contexts.insert(token.clone(),Context{caller,request:p.clone(),credential_revision:revision,created:Instant::now()});
            return Ok(json!({"contextToken":token,"remembered":remembered,"expiresInMs":300000}));
        }
        if method!="credentials.configuration.set"{return Err(CoreError::new("UNSUPPORTED_API","未知凭据配置接口"));}
        let operation=(caller.clone(),field(p,"operationId")?.into());
        let serialized=Zeroizing::new(serde_json::to_vec(p)?);
        let mut mac=Hmac::<Sha256>::new_from_slice(state.key.as_ref()).expect("HMAC key");mac.update(&serialized);
        if let Some(receipt)=state.receipts.get(&operation){
            if mac.verify_slice(&receipt.digest).is_err(){return Err(CoreError::new("OPERATION_CONFLICT","该操作标识已用于其他凭据请求"));}
            return receipt_result(&receipt.value);
        }
        let digest=mac.finalize().into_bytes().to_vec();
        if state.receipts.len()>=1024{return Err(CoreError::new("LIMIT_EXCEEDED","凭据保存回执已满，请稍后重试"));}
        let token=field(p,"contextToken")?;
        let context=state.contexts.get(token).ok_or_else(||CoreError::new("CONTEXT_EXPIRED","凭据提交上下文已过期，请重新准备"))?;
        if context.caller!=caller{return Err(CoreError::new("CONTEXT_EXPIRED","凭据提交来源不匹配"));}
        let request=context.request.clone();let credential_revision=context.credential_revision;
        let (_,auth_type)=self.check_credential_owner(&request)?;
        let revision_key=owner_key(&request);
        if *self.credential_revisions.lock().unwrap().get(&revision_key).unwrap_or(&0)!=credential_revision{return Err(conflict());}
        state.contexts.remove(token);
        state.receipts.insert(operation.clone(),Receipt{digest,value:json!({"status":"running"}),created:Instant::now(),pending_invalidation:false});
        drop(state);
        let result=(||{
            let owner=request["target"]["id"].as_str().unwrap();let remember=p["remember"].as_bool().unwrap();
            let kind=request["target"]["kind"].as_str().unwrap();
            let mut credential=crate::route::credentials::input_credential(p,&auth_type,kind=="socks5")?;
            let plan=if kind=="ssh"{Some(self.route_plan(owner)?)}else{None};
            let context=if let Some(plan)=&plan{self.trusted_context(plan,plan.steps.len()-1)?}else{None};
            let context=context.unwrap_or_else(||"preset".into());
            let identity=if let Some(plan)=&plan{crate::route::identity::identity(plan.steps.last().unwrap())}else{
                let proxy=crate::route_store::proxy(&self.store.db.lock().unwrap(),owner)?;
                crate::route::identity::proxy_identity(&proxy)
            };
            credential.context=Some(crate::route::digest(&identity));
            drop(_configuration);
            let prepared=self.secrets.prepare(credential,remember)?;
            let _configuration=self.configuration.lock().unwrap();
            let recheck=(||{
                self.check_credential_owner(&request)?;
                if *self.credential_revisions.lock().unwrap().get(&revision_key).unwrap_or(&0)!=credential_revision{return Err(conflict());}
                if let Some(plan)=&plan {if self.route_plan(owner)?.revision!=plan.revision{return Err(conflict());}}
                Ok(())
            })();
            if let Err(error)=recheck{self.secrets.discard(&prepared);return Err(error);}
            self.secrets.commit(&self.store.db.lock().unwrap(),kind,owner,&context,prepared)?;
            *self.credential_revisions.lock().unwrap().entry(revision_key).or_default()+=1;
            // The secret is committed. A directory refresh failure must not rotate it again.
            let stored=json!({"stored":true,"remembered":remember});
            let pending=crate::server_catalog_store::bump_revision(&self.store.db.lock().unwrap()).is_err();
            let mut state=self.configuration_credentials.lock().unwrap();let receipt=state.receipts.get_mut(&operation).unwrap();
            receipt.value=json!({"status":"stored","result":stored});receipt.pending_invalidation=pending;
            if pending{receipt.value["result"]["postCommitPending"]=json!(true);}
            Ok(stored)
        })();
        let mut state=self.configuration_credentials.lock().unwrap();let receipt=state.receipts.get_mut(&operation).unwrap();
        if receipt.value["status"]!="stored"{receipt.value=match &result{Ok(v)=>json!({"status":"stored","result":v}),Err(e)=>json!({"status":"failed","error":{"code":e.code,"message":e.message}})};}
        // Stored secrets are never rotated again to repair a directory notification.
        receipt_result(&receipt.value)
    }
}
fn receipt_result(v:&Value)->Result<Value>{
    match v["status"].as_str(){Some("stored")=>Ok(v["result"].clone()),Some("failed")=>Err(CoreError::new(v["error"]["code"].as_str().unwrap(),v["error"]["message"].as_str().unwrap())),_=>Err(CoreError::new("RESULT_UNKNOWN","凭据保存仍在执行，请核对原操作"))}
}
