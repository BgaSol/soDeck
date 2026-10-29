use crate::{Core, CoreError, Result, secrets::Credential};
use super::{RoutePlan, ResolvedStep, digest};
use serde_json::json;

impl Core {
    pub(crate) fn runtime_credential_dispatch(&self,caller:&crate::Caller,method:&str,p:&serde_json::Value)->Result<serde_json::Value> {
        crate::route_diagnostics::check("x-methods",method,p)?;
        if let Some(result)=self.configuration_credentials.lock().unwrap().begin_runtime(caller,method,p)?{return Ok(result)}
        let result=self.runtime_credential_write(caller,method,p);
        self.configuration_credentials.lock().unwrap().finish_runtime(caller,p,&result);
        result
    }
    fn runtime_credential_write(&self,caller:&crate::Caller,method:&str,p:&serde_json::Value)->Result<serde_json::Value> {
        if method=="auth.ssh.confirm" {return self.confirm_ssh(caller,p);}
        let challenge_id=crate::model::field(p,"challengeId")?;
        let challenge={let _guard=self.configuration.lock().unwrap();self.checked_challenge(challenge_id)?};
        if caller.source!="shell" && challenge["instanceId"]!=caller.instance_id {
            return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证答复来源不匹配"));
        }
        let owner=crate::model::field(&challenge,"ownerId")?;
        let kind=if challenge["ownerKind"]=="socks5"{"socks5"}else{"ssh"};
        if (method=="auth.ssh.set"&&(kind!="ssh"||p["serverId"]!=owner)) || (method=="auth.socks5.set"&&(kind!="socks5"||p["proxyId"]!=owner)) {
            return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证对象不匹配"));
        }
        let context=crate::model::field(&challenge,"_contextKey").map_err(|_|CoreError::new("HOST_CHALLENGE_EXPIRED","请先确认主机身份并重新连接"))?;
        let credential=if method=="auth.credentials.usePreset" {
            if p["operationId"]!=challenge["operationId"] {return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证操作不匹配"));}
            self.secrets.get(&self.store.db.lock().unwrap(),kind,owner,"preset")?
                .filter(|c|c.context.as_deref()==challenge["_presetIdentity"].as_str())
                .ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","已保存凭据已变化，请重新连接"))?
        }else{
            let auth=if kind=="ssh"{self.store.server(owner)?.auth_type}else{"usernamePassword".into()};
            input_credential(p,&auth,kind=="socks5")?
        };
        let remember=p["remember"].as_bool().unwrap();
        let prepared=self.secrets.prepare(credential,remember)?;
        let _guard=self.configuration.lock().unwrap();
        if let Err(error)=self.checked_challenge(challenge_id) {self.secrets.discard(&prepared);return Err(error);}
        self.secrets.commit(&self.store.db.lock().unwrap(),kind,owner,context,prepared)?;
        *self.credential_revisions.lock().unwrap().entry(if kind=="ssh"{owner.to_owned()}else{format!("proxy:{owner}")}).or_default()+=1;
        self.host_challenges.lock().unwrap().remove(challenge_id);
        let pending=crate::server_catalog_store::bump_revision(&self.store.db.lock().unwrap()).is_err();
        if pending {self.fault("credentials.runtime","directory-refresh-pending");}
        Ok(json!({"stored":true,"remembered":remember}))
    }
    fn confirm_ssh(&self,caller:&crate::Caller,p:&serde_json::Value)->Result<serde_json::Value> {
        use crate::model::field;
        let challenge_id=field(p,"challengeId")?;
        let challenge={let _guard=self.configuration.lock().unwrap();self.checked_challenge(challenge_id)?};
        if challenge["ownerKind"]=="socks5"||caller.source!="shell"&&challenge["instanceId"]!=caller.instance_id||p["operationId"]!=challenge["operationId"] {
            return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","身份与登录答复不属于当前操作"));
        }
        let trust=challenge["trustKey"].as_str();
        let decision=if trust.is_some(){if challenge["previousFingerprint"].is_string(){"update"}else{"accept"}}else{"keep"};
        if p["trust"]!=decision {return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","请明确确认当前主机身份"));}
        let owner=field(&challenge,"ownerId")?;let context=field(&challenge,"_contextKey")?;
        let choice=field(p,"credential")?;
        if choice!="input"&&["password","privateKeyPath","passphrase"].iter().any(|k|p.get(k).is_some()) {return Err(CoreError::new("INVALID_PARAMS","已保存凭据不能混入本次输入"));}
        let credential=match choice {
            "preset"=>self.secrets.get(&self.store.db.lock().unwrap(),"ssh",owner,"preset")?.filter(|c|c.context.as_deref()==challenge["_presetIdentity"].as_str()).ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","已保存凭据已变化，请重新连接"))?,
            "bound" if trust.is_none()=>self.secrets.get(&self.store.db.lock().unwrap(),"ssh",owner,context)?.ok_or_else(||CoreError::new("AUTH_REQUIRED","此路径没有可用凭据"))?,
            "input"=>input_credential(p,&self.store.server(owner)?.auth_type,false)?,
            _=>return Err(CoreError::new("INVALID_PARAMS","请选择有效的登录凭据")),
        };
        let remember=p["remember"].as_bool().unwrap();let prepared=self.secrets.prepare(credential,remember)?;
        let _guard=self.configuration.lock().unwrap();
        if let Err(e)=self.checked_challenge(challenge_id){self.secrets.discard(&prepared);return Err(e);}
        self.secrets.commit_with(&self.store.db.lock().unwrap(),"ssh",owner,context,prepared,|tx|{
            if let Some(key)=trust {tx.execute("INSERT INTO host_keys(endpoint,fingerprint,algorithm) VALUES(?,?,?) ON CONFLICT(endpoint) DO UPDATE SET fingerprint=excluded.fingerprint,algorithm=excluded.algorithm",rusqlite::params![key,field(&challenge,"fingerprint")?,field(&challenge,"algorithm")?])?;}
            Ok(())
        })?;
        *self.credential_revisions.lock().unwrap().entry(owner.into()).or_default()+=1;
        self.host_challenges.lock().unwrap().remove(challenge_id);
        if crate::server_catalog_store::bump_revision(&self.store.db.lock().unwrap()).is_err(){self.fault("credentials.runtime","directory-refresh-pending");}
        Ok(json!({"stored":true,"remembered":remember}))
    }
    /// A known complete chain permits configuration prefill to bind to that
    /// context. Unknown upstream identities keep it as an explicit preset.
    pub(crate) fn trusted_context(&self,plan:&RoutePlan,index:usize)->Result<Option<String>> {
        trusted_context_db(&self.store.db.lock().unwrap(),plan,index)
    }
    pub(crate) fn has_server_credential(&self,db:&rusqlite::Connection,id:&str)->Result<bool> {
        let (path,steps)=crate::route_store::resolve(db,id,None)?;let plan=RoutePlan::from_steps(steps,path)?;let index=plan.steps.len()-1;
        let context=trusted_context_db(db,&plan,index)?;
        let bound=match context{Some(context)=>self.secrets.has_context(db,"ssh",id,&context)?,None=>false};
        Ok(bound||self.secrets.has_preset(db,"ssh",id,&super::identity::identity(&plan.steps[index]))?)
    }
    pub(crate) fn context_credential(&self,plan:&RoutePlan,index:usize,verified:&[String])->Result<Credential> {
        let step=&plan.steps[index];
        let context=plan.verified_context(index,verified);
        let db=self.store.db.lock().unwrap();
        if let Some(credential)=self.secrets.get(&db,step.owner_kind(),step.owner_id(),&context)? {return Ok(credential);}
        let preset=self.secrets.get(&db,step.owner_kind(),step.owner_id(),"preset")?;
        let owner_identity=digest(&super::identity::identity(step));
        let available=preset.as_ref().is_some_and(|c|c.context.as_deref()==Some(owner_identity.as_str()));
        Err(CoreError::new(if available{"CREDENTIAL_CONFIRM_REQUIRED"}else if step.owner_kind()=="ssh"{"AUTH_REQUIRED"}else{"PROXY_AUTH_REQUIRED"},
            if available{"此访问路径尚未授权使用已保存凭据，请确认"}else{"请输入此路径的认证信息"})
            .details(json!({"_contextKey":context,"_presetIdentity":owner_identity,"hasPreset":available,"rememberDefault":available&&self.secrets.remembered(&db,step.owner_kind(),step.owner_id(),"preset")?})))
    }
}

fn trusted_context_db(db:&rusqlite::Connection,plan:&RoutePlan,index:usize)->Result<Option<String>> {
    use rusqlite::OptionalExtension;
    let mut verified=Vec::new();
    for i in 0..=index {
        if matches!(plan.steps[i],ResolvedStep::Ssh(_)) {
            let known:Option<String>=db.query_row("SELECT fingerprint FROM host_keys WHERE endpoint=?",[plan.trust_key(i,&verified)],|r|r.get(0)).optional()?;
            let Some(key)=known else{return Ok(None)};verified.push(key);
        }
    }
    Ok(Some(plan.verified_context(index,&verified)))
}

#[cfg(test)]
mod confirmation_tests {
    use super::*;
    use crate::{Caller,model::id};
    fn setup(core:&Core)->(String,serde_json::Value){
        let server=core.call(&Caller::shell(),"servers.routes.create",json!({"name":"atomic","host":"fixture.invalid","username":"test","accessPath":{"steps":[]}})).unwrap();
        let owner=server["id"].as_str().unwrap().to_owned();let plan=core.route_plan(&owner).unwrap();let p=json!({"serverId":owner,"workspaceId":"atomic","operationId":null});
        let (attempt,_)=core.start_diagnostic(&Caller::shell(),&p,"session");core.begin_route_attempt(&Caller::shell(),&p,&plan,"atomic",&attempt);
        let mut error=core.verify_route_node(&plan,0,"SHA256:isolated-fixture","ssh-ed25519",&[]).err().unwrap();core.route_challenge(&Caller::shell(),&p,&plan,&attempt,&mut error).unwrap();
        (owner,json!({"challengeId":error.details.unwrap()["challengeId"],"operationId":null,"trust":"accept","credential":"input","password":"isolated-only","remember":true}))
    }
    #[test]
    fn identity_and_login_commit_once_and_rollback_together(){
        let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(owner,p)=setup(&core);
        core.store.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_binding BEFORE INSERT ON credential_bindings BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&p).is_err());
        assert_eq!(core.store.db.lock().unwrap().query_row("SELECT count(*) FROM host_keys",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        core.store.db.lock().unwrap().execute_batch("DROP TRIGGER fail_binding").unwrap();
        let result=core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&p).unwrap();let revision=core.credential_revisions.lock().unwrap()[&owner];
        assert_eq!(result,core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&p).unwrap());assert_eq!(revision,core.credential_revisions.lock().unwrap()[&owner]);
        let plan=core.route_plan(&owner).unwrap();assert!(core.verify_route_node(&plan,0,"SHA256:isolated-fixture","ssh-ed25519",&[]).is_ok());
        let mut changed=p.clone();changed["remember"]=json!(false);assert_eq!(core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&changed).unwrap_err().code,"OPERATION_CONFLICT");
    }
    #[test]
    fn stale_or_wrong_identity_does_not_write_trust(){
        let dir=tempfile::tempdir().unwrap();let core=Core::open(dir.path()).unwrap();let(owner,mut p)=setup(&core);
        p["trust"]=json!("keep");assert_eq!(core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&p).unwrap_err().code,"HOST_CHALLENGE_MISMATCH");p["trust"]=json!("accept");
        *core.credential_revisions.lock().unwrap().entry(owner).or_default()+=1;
        assert_eq!(core.runtime_credential_dispatch(&Caller::shell(),"auth.ssh.confirm",&p).unwrap_err().code,"HOST_CHALLENGE_EXPIRED");
        assert_eq!(core.store.db.lock().unwrap().query_row("SELECT count(*) FROM host_keys",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}

pub(crate) fn input_credential(p:&serde_json::Value,auth:&str,proxy:bool)->Result<Credential> {
    let password=p["password"].as_str().unwrap_or("");
    if matches!(auth,"password"|"usernamePassword") && (password.is_empty() || (proxy&&password.len()>255) || p.get("privateKeyPath").is_some() || p.get("passphrase").is_some()) {
        return Err(CoreError::new("INVALID_PARAMS","请填写有效登录密码"));
    }
    if auth=="key"&&p.get("password").is_some(){return Err(CoreError::new("INVALID_PARAMS","私钥认证不能同时提交密码"));}
    let private_key=if auth=="key" {
        use std::io::Read;
        let path=crate::model::field(p,"privateKeyPath")?;
        if !std::path::Path::new(path).is_absolute(){return Err(CoreError::new("INVALID_PARAMS","私钥文件需要绝对路径"));}
        let file=std::fs::File::open(path).map_err(|_|CoreError::new("PRIVATE_KEY_UNREADABLE","无法读取私钥文件"))?;
        let mut text=zeroize::Zeroizing::new(String::new());
        file.take(1024*1024+1).read_to_string(&mut text).map_err(|_|CoreError::new("PRIVATE_KEY_UNREADABLE","无法读取私钥文本"))?;
        if text.len()>1024*1024{return Err(CoreError::new("LIMIT_EXCEEDED","私钥文件超过 1 MiB"));}
        std::mem::take(&mut *text)
    }else{String::new()};
    Ok(Credential{context:None,password:password.into(),private_key,passphrase:p["passphrase"].as_str().unwrap_or("").into()})
}
