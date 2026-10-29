import {describe,it,expect} from 'vitest';
import {Rules,requestHash} from './rules';
import type {ObservationClient} from './observation';
const draft={name:'代理',type:'network-proxy',bindMode:'fixed',port:10808,autoReconnect:false};
function storage(initial:unknown=null) {
  let value=initial;let fail=false;
  return {fail:(value=true)=>{fail=value;},get:()=>value,call:async<T>(method:string,params:unknown):Promise<T>=>{
    if(method==='storage.get')return value as T;
    if(fail)throw Error('disk full');value=(params as {value:unknown}).value;return {} as T;
  }};
}
describe('rule persistence and concurrency',()=>{
  it('emits saved/deleted only once after confirmed writes, never for conflicts or failed storage',async()=>{
    const store=storage(),rules=new Rules(store),events:{type:string;fields:unknown}[]=[];
    const diagnostic={call:store.call,async emit(type:string,fields:unknown){events.push({type,fields});}} as unknown as ObservationClient;
    await rules.load();const saved=await rules.save('server',draft,0,diagnostic);
    await expect(rules.save('server',saved,0,diagnostic)).rejects.toMatchObject({code:'REVISION_CONFLICT'});
    store.fail();await expect(rules.save('server',{...saved,name:'unsaved'},1,diagnostic)).rejects.toThrow('disk full');
    await expect(rules.remove('server',saved.id,1,diagnostic)).rejects.toThrow('disk full');expect(events).toHaveLength(1);
    store.fail(false);await rules.save('server',{...saved,name:'updated'},1,diagnostic);await rules.remove('server',saved.id,2,diagnostic);
    expect(events.map(e=>e.type)).toEqual(['plugin.sodeck.network-proxy.rule.saved','plugin.sodeck.network-proxy.rule.saved','plugin.sodeck.network-proxy.rule.deleted']);
    expect(events.map(e=>(e.fields as {revision:number}).revision)).toEqual([1,2,2]);expect(JSON.stringify(events)).not.toContain('updated');
    const input={...draft,name:''},hash=requestHash(input);await rules.saveDraft('server',input,0,'auto','same-submission',hash,diagnostic);await rules.saveDraft('server',input,0,'auto','same-submission',hash,diagnostic);
    expect(events.filter(e=>e.type.endsWith('rule.saved'))).toHaveLength(3);
  });
  it('atomically stores automatic names and durable bounded save receipts',async()=>{
    const store=storage(),rules=new Rules(store);await rules.load();
    const input={...draft,name:'',bindMode:'auto',port:0},hash=requestHash(input);
    const result=await rules.saveDraft('server',input,0,'auto','submission-1',hash);
    expect(rules.get('server',result.ruleId)).toMatchObject({name:'内网代理',port:0,bindMode:'auto'});
    expect(rules.automatic(result.ruleId)).toBe(true);
    const reopened=new Rules(store);await reopened.load();
    expect(await reopened.saveDraft('server',input,0,'auto','submission-1',hash)).toEqual(result);
    expect(reopened.all('server')).toHaveLength(1);
    await expect(reopened.saveDraft('server',input,0,'auto','submission-1',requestHash('changed'))).rejects.toMatchObject({code:'OPERATION_CONFLICT'});
    // Old workers keep the strict Rule and discard only outer optional metadata.
    const previous={schemaVersion:1,rules:reopened.all('server')};const upgraded=new Rules(storage(previous));await upgraded.load();
    expect(upgraded.automatic(result.ruleId)).toBe(false);expect(upgraded.get('server',result.ruleId).name).toBe('内网代理');
  });
  it('reconciles a lost save response and blocks new writes while its result is unknown',async()=>{
    let value:unknown=null,commit=false,readsFail=false,writes=0;
    const rpc={async call<T>(method:string,p:unknown):Promise<T>{if(method==='storage.get'){if(readsFail)throw Error('unreadable');return value as T;}writes++;if(commit)value=(p as {value:unknown}).value;throw Object.assign(Error('lost'),{code:'RESULT_UNKNOWN'});}};
    const rules=new Rules(rpc);await rules.load();const input={...draft,name:''},hash=requestHash(input);
    commit=true;const saved=await rules.saveDraft('server',input,0,'auto','lost-1',hash);expect(writes).toBe(1);expect(rules.all('server')).toHaveLength(1);
    readsFail=true;
    await expect(rules.saveDraft('server',input,0,'auto','lost-2',hash)).rejects.toMatchObject({code:'RESULT_UNKNOWN'});
    await expect(rules.save('server',draft,0)).rejects.toMatchObject({code:'RESULT_UNKNOWN'});
    readsFail=false;expect(await rules.result('lost-2')).toMatchObject({submissionId:'lost-2'});expect(writes).toBe(2);
    expect(rules.all('server')).toHaveLength(2);expect(saved.ruleId).not.toBe(rules.all('server')[1].id);
  });
  it('serializes saves and rejects stale revision without clobbering a different view',async()=>{
    const store=storage(),rules=new Rules(store);await rules.load();const saved=await rules.save('server',draft,0);
    const results=await Promise.allSettled([rules.save('server',{...saved,name:'first'},1),rules.save('server',{...saved,name:'second'},1)]);
    expect(results[0].status).toBe('fulfilled');expect(results[1].status).toBe('rejected');expect(rules.get('server',saved.id).name).toBe('first');
  });
  it('keeps the last saved value when storage fails and refuses unknown schemas',async()=>{
    const store=storage(),rules=new Rules(store);await rules.load();const saved=await rules.save('server',draft,0);store.fail();
    await expect(rules.save('server',{...saved,name:'not saved'},1)).rejects.toThrow('disk full');expect(rules.get('server',saved.id)).toEqual(saved);
    await expect(new Rules(storage({schemaVersion:2,rules:[]})).load()).rejects.toMatchObject({code:'SCHEMA_UNSUPPORTED'});
  });
  it('isolates server ownership, immutable mode and run snapshots',async()=>{
    const rules=new Rules(storage());await rules.load();const saved=await rules.save('server',draft,0),snapshot=rules.get('server',saved.id);
    await expect(rules.save('other',saved,1)).rejects.toMatchObject({code:'INVALID_PARAMS'});
    await expect(rules.save('server',{...saved,type:'port-forward',targetHost:'x',targetPort:80},1)).rejects.toMatchObject({code:'INVALID_PARAMS'});
    await rules.save('server',{...saved,port:10809},1);expect(snapshot.port).toBe(10808);
    await expect(rules.remove('server',saved.id,1)).rejects.toMatchObject({code:'REVISION_CONFLICT'});await rules.remove('server',saved.id,2);expect(rules.all('server')).toEqual([]);
  });
  it('enforces explicit automatic port selection and forbids dynamic-proxy fixed targets',async()=>{
    const rules=new Rules(storage());await rules.load();
    await expect(rules.save('server',{...draft,port:0},0)).rejects.toMatchObject({code:'INVALID_PARAMS'});
    await expect(rules.save('server',{...draft,targetHost:'x'},0)).rejects.toMatchObject({code:'INVALID_PARAMS'});
    const rule=await rules.save('server',{...draft,bindMode:'auto',port:0},0);expect(rule.port).toBe(0);
  });
});
