use super::*;
use base64::{engine::general_purpose::STANDARD,Engine};
use sodeck_communication::Key;
use std::sync::OnceLock;

pub(super) struct DirectorySource {
    id:String,generation:String,instance:String,view:String,workspace:String,workspace_generation:String,
    pty:String,pty_generation:u64,sequence:Option<u64>,revision:u64,path_revision:u64,
    last_report:Value,status:String,logical:Value,physical:Value,evidence:Value,observed:u64,
    window:Instant,reports:u8,
}
fn validate(method:&str,kind:&str,p:&Value)->Result<()> {
    static SCHEMA:OnceLock<Value>=OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../../contracts/directory-context-v1.schema.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||err("UNSUPPORTED_API","未知目录上下文方法"))?;
    sodeck_core::contract::check(schema,&schema["$defs"][name],p,method).map_err(|message|err("INVALID_PARAMS",message))?;
    if serde_json::to_vec(p).unwrap().len()>16*1024 {return Err(err("CAPACITY","目录报告超过 16 KiB"));}
    for field in ["logicalPath","physicalPath"] {
        if let Some(encoded)=p[field]["bytesBase64"].as_str(){
            let bytes=STANDARD.decode(encoded).map_err(|_|err("INVALID_PARAMS","目录不是有效 base64"))?;
            if bytes.first()!=Some(&b'/')||bytes.len()>4096||bytes.contains(&0){return Err(err("INVALID_PARAMS","目录必须为有效绝对字节路径"));}
        }
    }
    if method=="directories.report"&&p["status"]=="ready"&&["logicalPath","physicalPath","evidence"].iter().any(|key|p[*key].is_null()) {
        return Err(err("INVALID_PARAMS","已确认目录缺少路径或进程身份"));
    }
    Ok(())
}
pub(super) fn key(workspace:&str,generation:&str)->Key {Key{contract:"workspace.directory.v1".into(),scope:generation.into(),object:workspace.into()}}

impl State {
    pub(super) fn select_directory_window(&self,label:&str){
        let Some(id)=label.strip_prefix("detached-")else{return;};
        if self.stopping.load(Ordering::SeqCst)||crate::presentation::owns_focus(){return;}
        let Some(binding)=self.view_binding(id)else{return;};
        if binding.phase.load(Ordering::SeqCst)!=1||!binding.geometry.lock().unwrap().1{return;}
        // A newly created or moving surface must not create a user selection.
        let busy=self.workspace_busy.lock().unwrap();
        let mut w=self.workbench.lock().unwrap();
        let Ok(view)=w.view(id)else{return;};
        if busy.contains(&view.workspace_id)||!w.provides_directory(&view){return;}
        if w.workspaces.get(&view.workspace_id).is_some_and(|ws|ws.directory_source_view.as_deref()==Some(id)){return;}
        let _=w.activate(id);drop(w);drop(busy);self.changed();
    }
    pub(super) fn directory_call(&self,caller:&Caller,method:&str,p:&Value)->Result<Value>{
        validate(method,"x-methods",p)?;
        self.state_caller(caller)?;
        if method=="directories.connection" {
            let w=self.workbench.lock().unwrap();
            let workspace=required(&p["scope"],"workspaceId")?;
            let generation=required(&p["scope"],"workspaceGeneration")?;
            if !w.workspaces.get(workspace).is_some_and(|ws|ws.generation==generation)
                || !w.views.iter().any(|v|v.workspace_id==workspace&&Some(&v.plugin_id)==caller.plugin_id.as_ref())
                || caller.view_id.as_deref().is_some_and(|id|w.view(id).map_or(true,|v|v.workspace_id!=workspace)) {
                return Err(err("SOURCE_INVALID","目录来源不属于此工作区"));
            }
            let sources=self.directories.lock().unwrap();
            let source=sources.values().find(|s|s.id==p["sourceId"]&&s.generation==p["sourceGeneration"]&&s.workspace==workspace&&s.workspace_generation==generation)
                .ok_or_else(||err("SOURCE_EXPIRED","目录来源已失效"))?;
            let context=self.core.pty_context(&source.instance,&source.pty,source.pty_generation,&source.workspace,&source.view)?;
            if context["state"]!="active"{return Err(err("SOURCE_EXPIRED","终端连接已结束"));}
            let result=json!({"connectionId":context["connectionId"],"generation":context["connectionGeneration"]});
            validate(method,"x-results",&result)?;return Ok(result);
        }
        if caller.view_id.is_some(){return Err(err("SOURCE_INVALID","目录报告必须由后台业务实例提交"));}
        let owner=caller.plugin_id.as_deref().ok_or_else(||err("SOURCE_INVALID","目录来源需要插件实例"))?;
        let w=self.workbench.lock().unwrap();
        let mut sources=self.directories.lock().unwrap();
        let result=match method {
            "directories.register"=>{
                let view=w.view(required(p,"viewId")?).map_err(native_error)?;
                let ws=w.workspaces.get(&view.workspace_id).ok_or_else(||err("STALE_WORKSPACE","工作区已结束"))?;
                if view.plugin_id!=owner||p["scope"]["workspaceId"]!=view.workspace_id||p["scope"]["workspaceGeneration"]!=ws.generation||!w.provides_directory(&view){return Err(err("SOURCE_INVALID","目录来源声明或工作区不匹配"));}
                let pty=required(&p["pty"],"id")?;let generation=p["pty"]["generation"].as_u64().unwrap();
                self.core.pty_context(&caller.instance_id,pty,generation,&view.workspace_id,&view.view_id)?;
                if let Some(source)=sources.get(&view.view_id).filter(|s|s.instance==caller.instance_id&&s.pty==pty&&s.pty_generation==generation){
                    json!({"sourceId":source.id,"sourceGeneration":source.generation})
                }else{
                    let source=DirectorySource{id:Uuid::new_v4().to_string(),generation:Uuid::new_v4().to_string(),instance:caller.instance_id.clone(),view:view.view_id.clone(),workspace:view.workspace_id.clone(),workspace_generation:ws.generation.clone(),pty:pty.into(),pty_generation:generation,sequence:None,revision:0,path_revision:0,last_report:Value::Null,status:"unknown".into(),logical:Value::Null,physical:Value::Null,evidence:Value::Null,observed:0,window:Instant::now(),reports:0};
                    let result=json!({"sourceId":source.id,"sourceGeneration":source.generation});sources.insert(view.view_id,source);result
                }
            }
            "directories.report"|"directories.unregister"=>{
                let id=required(p,"sourceId")?;
                let view_id=sources.iter().find(|(_,s)|s.id==id).map(|(view,_)|view.clone());
                if view_id.is_none()&&method=="directories.unregister"{return Ok(json!({"accepted":true}));}
                let view_id=view_id.ok_or_else(||err("SOURCE_EXPIRED","目录来源已失效"))?;
                let source=sources.get_mut(&view_id).unwrap();
                if source.instance!=caller.instance_id||source.generation!=required(p,"sourceGeneration")?{return Err(err("SOURCE_INVALID","目录来源不属于此会话实例"));}
                if method=="directories.unregister"{sources.remove(&view_id);json!({"accepted":true})}else{
                    if !w.views.iter().any(|v|v.view_id==source.view&&v.workspace_id==source.workspace&&v.plugin_id==owner)||!w.workspaces.get(&source.workspace).is_some_and(|ws|ws.generation==source.workspace_generation){return Err(err("SOURCE_EXPIRED","目录来源视图或工作区已结束"));}
                    let context=self.core.pty_context(&source.instance,&source.pty,source.pty_generation,&source.workspace,&source.view)?;
                    if context["state"]!="active"{return Err(err("SOURCE_EXPIRED","PTY 已结束，目录报告无效"));}
                    let sequence:u64=required(p,"reportSequence")?.parse().map_err(|_|err("INVALID_PARAMS","目录序号超限"))?;
                    if source.sequence.is_some_and(|n|sequence<=n){
                        if source.sequence!=Some(sequence)||source.last_report!=*p{return Err(err("STALE_REPORT","目录报告重复冲突或序号倒退"));}
                    }else{
                        if source.window.elapsed()>=Duration::from_secs(1){source.window=Instant::now();source.reports=0;}
                        if source.reports>=10{return Err(err("RATE_LIMITED","目录报告过于频繁"));}source.reports+=1;
                        let preserve=p["status"]=="lastConfirmed";
                        let logical=if preserve {source.logical.clone()}else{p["logicalPath"].clone()};
                        let physical=if preserve {source.physical.clone()}else{p["physicalPath"].clone()};
                        let evidence=if preserve {source.evidence.clone()}else{p["evidence"].clone()};
                        if source.logical!=logical||source.physical!=physical||source.evidence!=evidence{source.path_revision+=1;}
                        source.logical=logical;source.physical=physical;source.evidence=evidence;
                        source.status=required(p,"status")?.into();source.sequence=Some(sequence);source.revision+=1;source.last_report=p.clone();
                        source.observed=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    }
                    json!({"accepted":true,"sourceRevision":source.revision.to_string()})
                }
            }
            _=>return Err(err("UNSUPPORTED_API","未知目录上下文操作")),
        };
        drop(sources);drop(w);
        self.sync_directories();
        validate(method,"x-results",&result)?;Ok(result)
    }

    pub(super) fn sync_directories(&self){
        let w=self.workbench.lock().unwrap();let mut sources=self.directories.lock().unwrap();
        sources.retain(|view,s|w.views.iter().any(|v|&v.view_id==view)&&w.workspaces.get(&s.workspace).is_some_and(|ws|ws.generation==s.workspace_generation));
        for source in sources.values_mut(){
            if self.core.pty_context(&source.instance,&source.pty,source.pty_generation,&source.workspace,&source.view).map_or(true,|c|c["state"]!="active") {
                if !source.physical.is_null(){source.path_revision+=1;}
                source.status="disconnected".into();source.logical=Value::Null;source.physical=Value::Null;source.evidence=Value::Null;
            }
        }
        let mut hub=self.communication.lock().unwrap();
        for (workspace,ws) in &w.workspaces {
            let scope=json!({"workspaceId":workspace,"workspaceGeneration":ws.generation});
            let view=ws.directory_source_view.as_deref().and_then(|id|w.view(id).ok());
            let source=view.as_ref().and_then(|v|sources.get(&v.view_id));
            let descriptor=view.as_ref().map(|view|json!({"id":source.map_or(view.view_id.as_str(),|s|s.id.as_str()),"generation":source.map_or("pending",|s|s.generation.as_str()),"viewId":view.view_id,"title":view.title}));
            let value=json!({"scope":scope,"source":descriptor,"selectionRevision":ws.directory_selection_revision.to_string(),
                "pathRevision":source.map_or(0,|s|s.path_revision).to_string(),"status":source.map_or("unknown",|s|s.status.as_str()),
                "logicalPath":source.map(|s|s.logical.clone()),"physicalPath":source.map(|s|s.physical.clone()),"evidence":source.map(|s|s.evidence.clone()),"observedAt":source.map_or(0,|s|s.observed)});
            let k=key(workspace,&ws.generation);
            if hub.put(k.clone(),scope,value).is_err(){hub.remove(&k);}
        }
    }
}
