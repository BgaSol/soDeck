
const key=value=>value?.state==='known'?JSON.stringify([value.pathStyle,value.path,value.filesystemView,value.witness]):'';
const canonical=value=>JSON.stringify(value&&typeof value==='object'?Object.fromEntries(Object.entries(value).sort(([a],[b])=>a.localeCompare(b)).map(([k,v])=>[k,v&&typeof v==='object'?JSON.parse(canonical(v)):v])):value);
const directory=value=>({path:value.path,pathStyle:value.pathStyle,filesystemView:value.filesystemView,...(value.witness?{witness:value.witness}:{})});

// One observation feeds the current protocol and the legacy terminal projection.
export function createLocationPublication(mount,{client,restore=()=>{},error=()=>{}}={}){
  let disposed=false,frozen=false,ready=false,initialized=false,initializing,epoch=0,current,lastKey='',activated=false;
  const slots=new Map(),jobs=new Map();
  const valid=()=>!disposed&&!frozen&&ready&&JSON.stringify(mount.context)===JSON.stringify(mount.data.openedContext);
  function enqueue(topic,value){slots.set(topic,value);pump(topic);}
  function publishDirectory(){
    if(current?.state!=='known')return;
    const next=key(current);
    if(next===lastKey&&!activated)return;
    lastKey=next;activated=false;
    enqueue('workspace.location.v1',{version:1,publicationId:crypto.randomUUID(),...directory(current)});
  }
  function pump(topic){
    if(!initialized||!valid()||jobs.has(topic)||!slots.has(topic))return;
    const generation=epoch;
    const job=(async()=>{
      while(valid()&&generation===epoch&&slots.has(topic)){
        const target=slots.get(topic);let baseline,done=false;
        for(let attempt=0;attempt<3&&valid()&&generation===epoch&&slots.get(topic)===target;attempt++){
          try{
            const record=(await client.request('get',{topic,key:'current'})).record;
            if(!valid()||generation!==epoch||slots.get(topic)!==target)break;
            if(canonical(record.value)===canonical(target)){done=true;break;}
            if(topic==='workspace.location.v1'&&record.value?.publicationId===target.publicationId)throw new Error('相同目录发布编号的内容不一致');
            if(baseline!==undefined&&record.revision!==baseline){done=true;break;}
            if(topic==='terminal.location.v1'&&record.value?.terminalId===target.terminalId&&BigInt(record.value.outputOffset||'0')>BigInt(target.outputOffset||'0')){done=true;break;}
            baseline=record.revision;
            await client.request('put',{topic,key:'current',expectedRevision:baseline,value:target});
            done=true;break;
          }catch(e){if(attempt===2&&valid()&&generation===epoch)error('目录位置发布失败：'+e.message);}
        }
        if(!valid()||generation!==epoch)break;
        if(slots.get(topic)===target)slots.delete(topic);
        if(!done)break;
      }
    })().finally(()=>{jobs.delete(topic);if(valid()&&slots.has(topic))queueMicrotask(()=>pump(topic));});
    jobs.set(topic,job);
  }
  return {
    observe(value,{restored=false}={}){current=value;if(restored)return;if(value.terminalId)enqueue('terminal.location.v1',value);if(initialized)publishDirectory();},
    async ready(){
      ready=true;if(initialized){for(const topic of slots.keys())pump(topic);return;}
      if(initializing)return initializing;
      initializing=(async()=>{
        try{
          const [legacy,location]=await Promise.all(['terminal.location.v1','workspace.location.v1'].map(topic=>client.request('get',{topic,key:'current'})));
          if(disposed)return;
          const previous=location.record?.value;
          if(previous?.version===1)lastKey=key({state:'known',...previous});
          if(!current?.terminalId)restore(legacy.record?.value);
        }catch(e){if(!disposed)error('目录位置恢复失败：'+e.message);}
        if(disposed)return;
        initialized=true;publishDirectory();for(const topic of slots.keys())pump(topic);
      })();return initializing;
    },
    activate(){activated=true;if(initialized)publishDirectory();},
    async freeze(value=true){frozen=value;++epoch;if(value)await Promise.all([...jobs.values()]);else for(const topic of slots.keys())pump(topic);},
    invalidate(){ready=false;++epoch;slots.clear();current=undefined;activated=false;},
    dispose(){disposed=true;++epoch;slots.clear();client.dispose();},
  };
}
