const identity=record=>JSON.stringify([record.producer,record.value?.publicationId,record.revision]);
const cancelled=()=>Object.assign(new Error('定位已取消'),{code:'cancelled'});
export function latestLocation(records){return records.filter(r=>r.topic==='workspace.location.v1'&&r.key==='current'&&/^\d{1,20}$/.test(r.revision)).reduce((latest,r)=>!latest||BigInt(r.revision)>BigInt(latest.revision)?r:latest,null);}
export function validateLocation(value){
  const path=p=>typeof p==='string'&&new TextEncoder().encode(p).length<=4096&&!/[\u0000-\u001f\u007f]/.test(p);
  if(value?.version!==1||typeof value.publicationId!=='string'||!/^[\da-f]{8}(-[\da-f]{4}){3}-[\da-f]{12}$/i.test(value.publicationId))throw new Error('目录发布格式或版本不可识别');
  if(!path(value.path)||!(value.pathStyle==='posix'&&value.path.startsWith('/')||value.pathStyle==='windows'&&/^[a-z]:[\\/]/i.test(value.path)))throw new Error('目录路径无效或不支持');
  if(!['session-default','shell-reported'].includes(value.filesystemView))throw new Error('目录的文件系统视角无法对应');
  if(value.filesystemView==='shell-reported'&&(!path(value.witness?.path)||!value.witness.path.startsWith('/')||typeof value.witness.value!=='string'||!value.witness.value||new TextEncoder().encode(value.witness.value).length>128))throw new Error('目录缺少文件系统对应标记');
  return value;
}

// Projection revision is a watermark, not a history or a preferred producer.
export class DirectoryTarget {
  constructor(context,saved){this.context=structuredClone(context);this.seen=null;this.target=null;this.handled=false;this.manual=false;this.valid=false;this.error='';if(saved?.version===1&&JSON.stringify(saved.context)===JSON.stringify(context)&&/^\d{1,20}$/.test(saved.seenThrough)){this.seen=saved.seenThrough;this.savedTarget=saved.target;this.handled=saved.handled===true;this.manual=saved.manual===true;}}
  receive(projection){
    this.valid=!!projection&&projection.scope.state==='active'&&projection.scope.connected!==false&&!projection.scope.frozen;
    if(!projection)return;
    const latest=latestLocation(projection.records),previous=this.seen;
    if(latest&&(previous===null||BigInt(latest.revision)>BigInt(previous))){this.target=latest;this.savedTarget=null;this.handled=false;this.manual=false;this.error='';}
    else {
      const targetId=this.target?identity(this.target):this.savedTarget;
      this.target=targetId?projection.records.find(r=>r.topic==='workspace.location.v1'&&r.key==='current'&&identity(r)===targetId)||null:null;
      this.savedTarget=null;
    }
    this.seen=projection.revision;
    if(this.target)try{validateLocation(this.target.value);}catch(error){this.target=null;this.handled=true;this.error=error.message;}
  }
  snapshot(){return {version:1,context:this.context,seenThrough:this.seen||'0',target:this.target?identity(this.target):null,handled:this.handled,manual:this.manual};}
}

export function connectDirectoryFollow(tree,mount,{client,data,save,changed=()=>{}}){
  const state=new DirectoryTarget(mount.context,mount.data?.navigation?.follow);
  let enabled=mount.settings?.values?.followTerminal!==false,visible=mount.visible!==false,active=false,frozen=false,disposed=false,job,serial=0;
  const pending=new Set();
  const toggle=document.createElement('button');toggle.className='sd-control follow-directory';toggle.textContent='跟随目录';toggle.title='跟随组件发布的目录';tree.toolbar.append(toggle);
  const status=document.createElement('details');status.className='follow-status';const summary=document.createElement('summary');summary.setAttribute('aria-live','polite');const detail=document.createElement('p'),locate=document.createElement('button');locate.className='sd-control';locate.textContent='定位到最近目录';status.append(summary,detail,locate);tree.toolbar.parentElement?.append(status);
  const allowed=()=>!disposed&&active&&!frozen&&visible&&enabled&&state.valid;
  function cancel(){if(job){++serial;tree.cancelNavigation();job=null;}}
  function render(){
    toggle.setAttribute('aria-pressed',String(enabled));toggle.disabled=!active||frozen;
    summary.textContent=!enabled?'跟随已关闭':!state.valid?'已暂停：目录位置暂不可用':state.error?'无法浏览目录':job?'正在定位…':state.manual?'已手动浏览，下次目录发布时跟随':state.target?'已定位：'+state.target.value.path:'等待目录位置';
    detail.textContent=state.error||state.target?.value.path||'跟随当前服务器的组件目录发布。手动浏览不会向其他组件发送命令。';locate.hidden=!enabled||!state.target;locate.disabled=!!job||!allowed();
  }
  function update(){
    if(!allowed()||!state.target){cancel();render();return;}
    if(!state.handled&&!job){
      const target=state.target,ticket=++serial;tree.cancelNavigation();const navigation=tree.navigation();
      const check=()=>{navigation();if(!allowed()||ticket!==serial||state.target!==target)throw cancelled();};
      const work=(async()=>{try{const info=await data('resolveLocation',target.value);check();if(info.attrs?.kind!=='directory')throw new Error('发布的位置不是目录');await tree.locateResolved(info,{check,focus:false});check();state.handled=true;state.manual=false;state.error='';}catch(error){if(ticket===serial&&error.code!=='cancelled'){state.handled=true;state.error=error.message;}}finally{pending.delete(work);if(ticket===serial){job=null;changed();render();}}})();
      job=work;pending.add(work);
    }
    render();
  }
  tree.onManualNavigation=()=>{cancel();state.handled=true;state.manual=true;state.error='';changed();render();};
  locate.onclick=()=>{state.handled=false;state.manual=false;state.error='';update();};
  toggle.onclick=async()=>{toggle.disabled=true;try{await save(!enabled);}finally{render();}};
  const watch=client.watch({topics:['workspace.location.v1']},projection=>{const old=state.target;state.receive(projection);if(old!==state.target&&job)cancel();changed();update();});
  return {
    settings(snapshot){const next=snapshot?.values?.followTerminal!==false;if(next!==enabled){enabled=next;cancel();if(next){state.handled=false;state.manual=false;}}update();},
    lifecycle(value){if(value.active!==undefined)active=value.active;if(value.visible!==undefined)visible=value.visible;if(value.frozen!==undefined)frozen=value.frozen;update();},
    snapshot:()=>state.snapshot(),
    async flush(){cancel();await Promise.all([...pending]);},
    dispose(){disposed=true;cancel();tree.onManualNavigation=null;watch.dispose();client.dispose();toggle.remove();status.remove();},
  };
}
