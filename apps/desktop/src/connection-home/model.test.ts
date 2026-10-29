import {describe,it,expect,vi} from 'vitest';
import {Drafts} from './drafts';
import {emptyCatalog,dependencies} from './model';
import {Saves} from './save';
import {core,CoreError} from '@sodeck/sdk';
describe('connection drafts',()=>{
  it('retains independent drafts, clears secrets on auth changes, and rejects capacity without eviction',()=>{
    const store=new Drafts();let first='';
    for(let i=0;i<16;i++){const d=store.create('ssh',emptyCatalog);if(i===0)first=d.id;store.edit(d.id,{host:`host-${i}`});store.secret(d.id,{password:'isolated'});}
    expect(()=>store.create('ssh',emptyCatalog)).toThrow('16');expect(store.items.size).toBe(16);
    store.edit(first,{authType:'key'});expect(store.secrets.has(first)).toBe(false);
    store.select({kind:'proxies'});expect(store.items.get(first)?.fields.host).toBe('host-0');
    expect(JSON.stringify([...store.items.values()])).not.toContain('isolated');
  });
  it('never includes a jump node own access path or name in dependency identity',()=>{
    const store=new Drafts(),d=store.create('ssh',emptyCatalog);d.fields.pathEnabled=true;d.fields.steps=[{stepId:'step',kind:'ssh',serverId:'jump'}];
    const c={...emptyCatalog,items:[{server:{id:'jump',name:'Display',host:'jump.invalid',port:22,username:'user',authType:'password' as const,revision:8,hasSecret:false},accessPath:{steps:[{stepId:'other-step',kind:'socks5' as const,proxyId:'other',resolve:'local' as const}]},groupId:null,membershipRevision:'1',postCommitPending:false}]};
    expect(dependencies(d.fields,c)).toEqual([{stepId:'step',kind:'ssh',id:'jump',host:'jump.invalid',port:22,username:'user',authType:'password'}]);
  });
  it('counts group drafts before accepting a seventeenth edit, and rejects oversized fields without loss',()=>{
    const store=new Drafts();store.groupEdit={name:''};store.editGroup(store.groupEdit,'group');
    for(let i=0;i<15;i++){const d=store.create('ssh',emptyCatalog);store.edit(d.id,{host:`saved-${i}`});}
    expect(store.dirtyCount()).toBe(16);expect(()=>store.create('proxy',emptyCatalog)).toThrow('16');
    const first=[...store.items.values()][0];expect(()=>store.edit(first.id,{name:'x'.repeat(300000)})).toThrow('容量');expect(first.fields.name).toBe('');
    expect(()=>store.editGroup(store.groupEdit!,'x'.repeat(300000))).toThrow('容量');expect(store.groupEdit!.name).toBe('group');
    store.secret(first.id,{password:'retained'});expect(()=>store.secret(first.id,{password:'密'.repeat(90000)})).toThrow('256 KiB');expect(store.secrets.get(first.id).password).toBe('retained');
  });
});
describe('save phases',()=>{
  it('keeps the committed ID when credentials fail and never opens until the retry succeeds',async()=>{
    const store=new Drafts(),d=store.create('ssh',emptyCatalog);store.edit(d.id,{host:'test.invalid',username:'user'});store.secret(d.id,{password:'isolated'});
    let fail=true;const methods:string[]=[];
    const call=vi.spyOn(core,'call').mockImplementation(async(method,params:any)=>{methods.push(method);
      if(method==='servers.catalog.save')return {server:{...params.configuration,id:'saved',revision:1,hasSecret:false},accessPath:{steps:[]},groupId:null,membershipRevision:'1',postCommitPending:false};
      if(method==='credentials.configuration.prepare')return {contextToken:'context',remembered:false,expiresInMs:300000};
      if(method==='credentials.configuration.set'){if(fail)throw new CoreError({code:'SECRET_STORE_ERROR',message:'fixture'});return {stored:true,remembered:true};}
    });
    const open=vi.fn(async()=>{}),saves=new Saves(store,()=>emptyCatalog,async()=>{},open);
    await saves.save(d.id,true,()=>true);expect(d.phase).toBe('credentialError');expect(d.server?.server.id).toBe('saved');expect(open).not.toHaveBeenCalled();
    fail=false;await saves.save(d.id);expect(methods.filter(m=>m==='servers.catalog.save')).toHaveLength(1);expect(open).toHaveBeenCalledTimes(1);expect(store.secrets.has(d.id)).toBe(false);call.mockRestore();
  });
  it('queries an unknown configuration receipt, and only returns an inline proxy to its captured parent',async()=>{
    const store=new Drafts(),parent=store.create('ssh',emptyCatalog);store.edit(parent.id,{pathEnabled:true,steps:[{stepId:'p-step',kind:'socks5',proxyId:'',resolve:'proxy'}]});const d=store.create('proxy',emptyCatalog,{inlineTarget:{parentDraftId:parent.id,targetStepId:'p-step'}});store.edit(d.id,{host:'proxy.invalid'});
    const proxy={id:'p',name:'proxy.invalid:1080',host:'proxy.invalid',port:1080,username:'',authType:'none',revision:1,hasPreset:false};
    const methods:string[]=[];const call=vi.spyOn(core,'call').mockImplementation(async(method)=>{methods.push(method);if(method==='proxies.catalog.save')throw new CoreError({code:'RESULT_UNKNOWN',message:'fixture'});return {status:'committed',result:{proxy,catalogRevision:'2'}};});
    const saves=new Saves(store,()=>emptyCatalog,async()=>{},async()=>{});await saves.save(d.id);expect(d.phase).toBe('reconciling');
    store.edit(parent.id,{host:'parent.invalid'});const other=store.create('ssh',emptyCatalog);await saves.save(d.id);
    expect((parent.fields.steps[0] as {proxyId:string}).proxyId).toBe('p');expect(other.fields.steps).toEqual([]);expect(methods).toEqual(['proxies.catalog.save','proxies.catalog.operation']);call.mockRestore();
  });
});
