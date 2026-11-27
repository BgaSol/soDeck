(() => {
  SoDeckMotion.observeMotion(document);
  const root=document.querySelector('#app'),error=document.querySelector('#error'),P=NetworkPresentation;
  let region=new SoDeckDOM.DomRegion(root),epoch=0,mounted=false,visible=false,readOnly=true,timer,polling=false,writes=0,requestOrder=0,acceptedOrder=0;
  let context,defaults={defaultType:'tcpForward',portMode:'auto',fixedPort:1080},settingsSnapshot;
  let draftRevision='0',navigationRevision='0',sync=Promise.resolve(),inputRevision=0,syncedInput=0,composing=false;
  const ui={catalog:{rules:[]},draft:null,selected:null,busy:new Set(),targets:new Map(),copied:new Map(),feedback:new Map(),saving:false,saveUncertain:false,cancelConfirm:false,deleteConfirm:null};
  const fail=err=>{error.textContent=String(err?.message||err);error.hidden=false;};error.onclick=()=>error.hidden=true;
  const valid=version=>mounted&&version===epoch;
  function settings(snapshot){if(!snapshot)return;if(settingsSnapshot?.definitionToken===snapshot.definitionToken&&BigInt(settingsSnapshot.revision)>BigInt(snapshot.revision))return;settingsSnapshot=snapshot;defaults={...defaults,...snapshot.values};void sodeck.request('settings',{op:'applied',pageId:snapshot.pageId,definitionToken:snapshot.definitionToken,revision:snapshot.revision}).catch(()=>{});}
  function tokens(values){for(const [name,value]of Object.entries(values||{})){if(name==='color-scheme')document.documentElement.style.colorScheme=value;else document.documentElement.style.setProperty('--'+name,value);}}
  function accept(data,order,{editor=false}={}){
    if(order<acceptedOrder)return;acceptedOrder=order;
    if(data.catalog){
      for(const row of data.catalog.rules){const old=ui.catalog.rules.find(r=>r.rule.id===row.rule.id);if(!P.ready(row)||old?.run?.snapshot?.listenerId!==row.run?.snapshot?.listenerId){ui.copied.delete(row.rule.id);ui.feedback.delete(row.rule.id);}}
      ui.catalog=data.catalog;
    }
    if(data.navigationRevision!==undefined&&BigInt(data.navigationRevision)>=BigInt(navigationRevision)){
      ui.selected=data.navigation?.selected??null;navigationRevision=data.navigationRevision;
    }
    if(data.draftRevision!==undefined)draftRevision=data.draftRevision;
    if(ui.saveUncertain&&data.writePending===false){editor=true;ui.saveUncertain=ui.saving=false;inputRevision=syncedInput=0;sync=sync.catch(()=>{});error.hidden=true;}
    if(editor||inputRevision===syncedInput){if(Object.hasOwn(data,'draft'))ui.draft=data.draft;}
  }
  async function request(data,options){const version=epoch,order=++requestOrder;try{const result=await sodeck.request('event',data);if(!valid(version))return null;accept(result,order,options);return result;}catch(err){if(valid(version))throw err;return null;}}
  function paint(){
    if(!mounted||composing)return;
    const active=document.activeElement,focused=root.contains(active),oldId=active.closest?.('[data-select]')?.dataset.select;
    const entity=active.closest?.('[data-select]'),neighbor=entity?.nextElementSibling?.dataset.select,previous=entity?.previousElementSibling?.dataset.select;
    region.update(root,P.manager(ui));
    if(focused&&(!active.isConnected||active.disabled)){
      ([oldId,neighbor,previous].filter(Boolean).map(id=>root.querySelector(`[data-action="expand"][data-id="${CSS.escape(id)}"]`)).find(Boolean)||root.querySelector('[data-action="create"]'))?.focus({preventScroll:true});
    }
  }
  async function refresh(){if(!mounted||!visible||readOnly||polling||writes||composing)return;polling=true;const version=epoch;try{const previous=navigationRevision;const data=await request({action:'snapshot'});if(data){paint();if(previous!==navigationRevision)reveal(ui.selected);}}catch(err){if(valid(version))fail(err);}finally{if(valid(version))polling=false;}}
  async function mutation(data,options){const version=epoch;writes++;try{return await request(data,options);}finally{if(valid(version))writes--;}}
  function enqueue(job){const version=epoch;const task=sync.catch(()=>{}).then(()=>valid(version)?job():null);sync=task;return task;}
  function collect(){const form=root.querySelector('form');if(!form||!ui.draft||ui.saving)return;const values=new FormData(form);ui.draft={...ui.draft,name:String(values.get('name')||''),bind:{host:String(values.get('bindHost')||'127.0.0.1'),port:values.has('auto')?0:Number(values.get('port')||1080)}};if(ui.draft.type==='tcpForward')ui.draft.target={host:String(values.get('host')||''),port:Number(values.get('targetPort')||0)};}
  function syncDraft(){
    if(ui.saving)return sync;
    collect();if(!ui.draft)return sync;
    const rule=structuredClone(ui.draft),revision=++inputRevision;
    return enqueue(async()=>{const result=await mutation({action:'draft',rule,draftRevision});if(result&&revision===inputRevision)syncedInput=revision;return result;});
  }
  function reveal(id){if(!id)return;root.querySelector(`[data-select="${CSS.escape(id)}"]`)?.scrollIntoView({block:'nearest'});}
  async function navigate(selected,offset=ui.catalog.offset||0){await enqueue(()=>mutation({action:'navigation',state:{selected,offset},navigationRevision}));paint();}
  async function begin(rule){
    const version=epoch;await sync;if(!valid(version))return;
    if(ui.draft&&inputRevision!==syncedInput){fail('草稿尚未同步，请重试');return;}
    if(ui.draft){const snapshot=await request({action:'snapshot'});if(!snapshot)return;if(snapshot.dirty){await resumeEdit();fail('请先保存或放弃当前草稿');return;}}
    const result=await enqueue(()=>mutation({action:'edit.begin',rule},{editor:true}));if(!result)return;
    inputRevision=syncedInput=0;ui.cancelConfirm=false;
    await navigate(rule.revision>0?rule.id:null);
    paint();root.querySelector('form input')?.focus({preventScroll:true});
  }
  async function resumeEdit(){if(!ui.draft)return;const id=ui.draft.id;if(ui.draft.revision>0){await enqueue(()=>mutation({action:'edit.reveal'}));paint();reveal(id);}else{paint();root.querySelector('form')?.scrollIntoView({block:'nearest'});}}
  async function settledComposition(){if(!composing)return;await new Promise((resolve,reject)=>{const version=epoch;const finish=()=>{clearTimeout(timeout);root.removeEventListener('compositionend',done);};const done=()=>{finish();setTimeout(()=>valid(version)?resolve():reject(new Error('页面已卸载')),0);};const timeout=setTimeout(()=>{finish();reject(new Error('输入法组合尚未结束，请完成输入后重试'));},8000);root.addEventListener('compositionend',done);});}
  async function save(){
    await settledComposition();const form=root.querySelector('form');if(!form?.reportValidity())return;
    const version=epoch;await syncDraft();if(!valid(version))return;
    ui.saving=true;paint();error.hidden=true;
    try{const data=await enqueue(()=>mutation({action:'save'},{editor:true}));if(data){inputRevision=syncedInput=0;ui.cancelConfirm=false;}}
    catch(err){if(valid(version)){ui.saveUncertain=true;await request({action:'snapshot'}).catch(()=>{});throw ui.saveUncertain?new Error('保存结果待确认，正在查询原提交，请稍候。'):err;}}
    finally{if(valid(version)){ui.saving=ui.saveUncertain;paint();}}
  }
  root.addEventListener('compositionstart',()=>composing=true);
  root.addEventListener('compositionend',()=>{composing=false;void syncDraft().then(paint).catch(fail);});
  root.addEventListener('input',ev=>{
    if(ui.saving||readOnly)return;
    if(ev.target.closest('form')){ui.cancelConfirm=false;if(!composing)void syncDraft().catch(fail);return;}
    if(ev.target.dataset.check){const id=ev.target.dataset.id,target=ui.targets.get(id)||{host:'',port:443};target[ev.target.dataset.check]=ev.target.value;ui.targets.set(id,target);}
  });
  root.addEventListener('change',ev=>{if(ev.target.name==='auto'){void syncDraft().then(paint).catch(fail);}});
  root.addEventListener('submit',ev=>{ev.preventDefault();if(!readOnly&&!ui.saving)void save().catch(fail);});
  root.addEventListener('click',ev=>{const version=epoch;void(async()=>{
    if(readOnly||ui.saving)return;
    const type=ev.target.closest('[data-type]')?.dataset.type;
    if(type&&ui.draft){collect();ui.draft.type=type;if(type==='socks5')delete ui.draft.target;else ui.draft.target={host:'',port:0};await syncDraft();paint();return;}
    const button=ev.target.closest('[data-action]');if(!button||button.disabled)return;
    const action=button.dataset.action,id=button.dataset.id,row=ui.catalog.rules.find(r=>r.rule.id===id);
    if(id&&!row)return;
    if(action!=='delete')ui.deleteConfirm=null;
    if(action==='expand'){await navigate(ui.selected===id?null:id);return;}
    if(action==='previous'||action==='next'){await navigate(null,Math.max(0,(ui.catalog.offset||0)+(action==='next'?20:-20)));return;}
    if(action==='resume-edit'){await resumeEdit();return;}
    if(action==='create'){
      const snapshot=await sodeck.request('settings',{op:'get'}).catch(()=>null);if(!valid(version))return;settings(snapshot);
      await begin({id:crypto.randomUUID(),serverId:context.serverId,revision:0,name:'',type:defaults.defaultType,bind:{host:'127.0.0.1',port:defaults.portMode==='fixed'?defaults.fixedPort:0},...(defaults.defaultType==='tcpForward'?{target:{host:'',port:0}}:{})});return;
    }
    if(action==='edit'){await begin(row.rule);return;}
    if(action==='keep-edit'){ui.cancelConfirm=false;paint();return;}
    if(action==='cancel'){
      await sync.catch(()=>{});if(!valid(version))return;const snapshot=await request({action:'snapshot'});if(!snapshot)return;
      if((snapshot.dirty||inputRevision!==syncedInput)&&!ui.cancelConfirm){ui.cancelConfirm=true;paint();return;}
      const result=await enqueue(()=>mutation({action:'edit.cancel',draftRevision},{editor:true}));if(!result)return;inputRevision=syncedInput=0;ui.cancelConfirm=false;paint();return;
    }
    if(action==='copy'){if(P.ready(row)){await sodeckClipboard.writeText(P.address(row.run.snapshot.address));if(valid(version)){ui.copied.set(id,Date.now()+2500);ui.feedback.set(id,'已复制本机入口');paint();}}return;}
    if(action==='delete'&&ui.deleteConfirm!==id){ui.deleteConfirm=id;paint();return;}
    if(ui.busy.has(id))return;
    const target=ui.targets.get(id)||{host:'',port:443};
    if(action==='check'&&row.rule.type==='socks5'&&(!target.host.trim()||!Number.isInteger(Number(target.port))||Number(target.port)<1||Number(target.port)>65535))throw new Error('请输入检查主机和 1–65535 的端口');
    ui.busy.add(id);paint();
    try{await mutation({action,id,revision:row.rule.revision,target:{host:target.host,port:Number(target.port)}});}
    finally{if(valid(version)){ui.busy.delete(id);ui.deleteConfirm=null;paint();}}
  })().catch(err=>{if(valid(version))fail(err);});});
  addEventListener('sodeck:mount',ev=>{
    epoch++;mounted=true;writes=0;polling=false;sync=Promise.resolve();requestOrder=acceptedOrder=0;inputRevision=syncedInput=0;composing=false;
    region.dispose();region=new SoDeckDOM.DomRegion(root);const m=ev.detail;context=m.context;visible=m.visible;readOnly=Boolean(m.readOnly);navigationRevision=draftRevision='0';
    ui.targets.clear();ui.copied.clear();ui.feedback.clear();ui.busy.clear();ui.saveUncertain=ui.saving=Boolean(m.data.writePending);ui.cancelConfirm=false;ui.deleteConfirm=null;
    accept(m.data,0,{editor:true});tokens(m.tokens);settings(m.settings);paint();clearInterval(timer);timer=setInterval(refresh,800);sodeck.prepared(true);
  });
  addEventListener('sodeck:surfaceReady',()=>{readOnly=false;void refresh();});
  addEventListener('sodeck:transferCancelled',()=>{readOnly=false;void refresh();});
  addEventListener('sodeck:settingsChanged',ev=>settings(ev.detail.settings));
  addEventListener('sodeck:textMenuError',ev=>fail(ev.detail));
  addEventListener('sodeck:themeChanged',ev=>tokens(ev.detail.tokens));
  addEventListener('sodeck:visibilityChanged',ev=>{visible=ev.detail.visible;if(visible)void refresh();});
  addEventListener('sodeck:flush',ev=>{const m=ev.detail,version=epoch;void(async()=>{await settledComposition();if(ui.saveUncertain){await request({action:'snapshot'});if(ui.saveUncertain)throw new Error('保存结果待确认，请稍候');}if(ui.draft)await syncDraft();await sync;if(valid(version))sodeck.flushed(m.id);})().catch(err=>{if(valid(version))sodeck.flushed(m.id,String(err));});});
  addEventListener('sodeck:unmount',()=>{mounted=false;epoch++;clearInterval(timer);region.dispose();});
})();
