use super::*;
pub(crate) fn validate(method:&str,kind:&str,p:&Value)->Result<()> {
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../contracts/pty-observe-v1.schema.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||CoreError::new("UNSUPPORTED_API","未知 PTY 观察方法"))?;
    contract::check(schema,&schema["$defs"][name],p,method).map_err(|m|CoreError::new("INVALID_PARAMS",m))
}
impl Core {
    pub(crate) fn observe_pty(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        validate(method,"x-methods",p)?;
        if method=="pty.observe.close"&&caller.view_id.is_none()&&!self.registry.lock().unwrap().ptys.contains_key(field(p,"ptyId")?){return Ok(json!({"closed":true}));}
        let pty=self.pty(p)?;
        if pty.instance_id!=caller.instance_id || caller.view_id.is_some(){return Err(CoreError::new("SOURCE_INVALID","观察仅属于持有 PTY 的后台实例"));}
        let mut state=pty.state.lock().unwrap();
        if method=="pty.observe.open" {
            if state.state!="active"{return Err(CoreError::new("STALE_HANDLE","PTY 已结束"));}
            if state.observer.is_some(){return Err(CoreError::new("CAPACITY","此 PTY 已有观察器"));}
            let observer=ssh::observer::Observer::new(state.last_sequence);
            let result=json!({"observerId":observer.id,"cursor":state.last_sequence.to_string()});
            state.observer=Some(observer);return Ok(result);
        }
        if method=="pty.observe.close"&&state.observer.is_none(){return Ok(json!({"closed":true}));}
        let closed=state.state!="active";
        let observer=state.observer.as_mut().ok_or_else(||CoreError::new("STALE_HANDLE","观察器已结束"))?;
        if observer.id!=field(p,"observerId")?{return Err(CoreError::new("STALE_HANDLE","观察器身份已变化"));}
        if method=="pty.observe.close" {state.observer=None;return Ok(json!({"closed":true}));}
        observer.read(field(p,"after")?.parse().map_err(|_|CoreError::new("INVALID_CURSOR","观察游标无效"))?,closed)
    }
    /// Host transport hint; no terminal data or ownership handle is transferred.
    pub fn pty_observer_notices(&self)->Vec<(String,Value)>{
        let registry=self.registry.lock().unwrap();let mut notices=Vec::new();
        for pty in registry.ptys.values(){
            let mut state=pty.state.lock().unwrap();let closed=state.state!="active";
            if let Some(o)=&mut state.observer {
                if o.notify || (closed&&!o.end_notified) {
                    o.notify=false;o.end_notified=closed;
                    notices.push((pty.instance_id.clone(),json!({"ptyId":pty.id,"generation":pty.generation,"observerId":o.id})));
                }
            }
        }
        notices
    }
    pub fn retry_pty_observer_notice(&self,pty_id:&str){
        if let Some(pty)=self.registry.lock().unwrap().ptys.get(pty_id){if let Some(observer)=&mut pty.state.lock().unwrap().observer {observer.notify=true;}}
    }
    /// Host-owned context validation, not a plugin-to-plugin resource API.
    pub fn pty_context(&self,instance:&str,pty_id:&str,generation:u64,workspace:&str,view:&str)->Result<Value>{
        let pty=self.pty(&json!({"ptyId":pty_id,"generation":generation}))?;
        let state=pty.state.lock().unwrap();
        if pty.instance_id!=instance || pty.connection.workspace_id!=workspace || state.view_id.as_deref()!=Some(view) {
            return Err(CoreError::new("SOURCE_INVALID","目录来源与实际 PTY/视图归属不一致"));
        }
        Ok(json!({"connectionId":pty.connection.id,"connectionGeneration":pty.connection.generation,"serverId":pty.connection.server.id,
            "serverRevision":pty.connection.server.revision,"state":state.state}))
    }
}
