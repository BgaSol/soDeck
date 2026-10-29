import {Button,IconButton,Notice,SearchSelect,SelectField,FormFields,Stack,Inline,Disclosure} from '@sodeck/ui';
import {ArrowUp,ArrowDown,X} from 'lucide-react';
import {useEffect,useRef} from 'react';
import type {RouteStep} from '@sodeck/sdk';
import type {Controller} from './controller';
import {locked,type Draft,type Catalog} from './model';
import {ProxyFields} from './proxy-fields';

export function RouteStepFields({step,index,draft:d,catalog,controller:c,source,change,move,remove}:{step:RouteStep;index:number;draft:Draft;catalog:Catalog;controller:Controller;source:string;change(step:RouteStep):void;move(delta:number):void;remove():void}){
  const candidate=d.inlineId?c.store.items.get(d.inlineId):undefined;
  const inline=candidate?.inlineTarget?.targetStepId===step.stepId?candidate:undefined;
  const addProxy=useRef<HTMLButtonElement>(null),hadInline=useRef(Boolean(inline));
  useEffect(()=>{if(hadInline.current&&!inline)addProxy.current?.focus({preventScroll:true});hadInline.current=Boolean(inline);},[inline?.id]);
  const owner=step.kind==='ssh'?catalog.items.find(i=>i.server.id===step.serverId)?.server:catalog.proxies.find(i=>i.proxy.id===step.proxyId)?.proxy;
  const loopback=owner&&(/^(localhost|127(?:\.\d+){3}|\[?::1\]?)$/i.test(owner.host));
  return <Stack>
    <Inline><strong id={`route-step-${step.stepId}`} tabIndex={-1}>第 {index+1} 站 · {step.kind==='ssh'?'SSH':'SOCKS5'}</strong>
      <IconButton id={`route-up-${step.stepId}`} label={`上移第 ${index+1} 站`} icon={ArrowUp} disabled={index===0} onClick={()=>move(-1)}/>
      <IconButton id={`route-down-${step.stepId}`} label={`下移第 ${index+1} 站`} icon={ArrowDown} disabled={index===d.fields.steps.length-1} onClick={()=>move(1)}/>
      <IconButton label={`移除第 ${index+1} 站`} icon={X} disabled={Boolean(inline&&locked(inline))} onClick={remove}/></Inline>
    <small>由{source}访问{loopback?`；${owner.host} 指${source}所在的网络环境`:''}</small>
    {step.kind==='ssh'?<SearchSelect label={`第 ${index+1} 站 SSH 服务器`} value={step.serverId} options={catalog.items.filter(i=>i.server.id!==d.server?.server.id&&!d.fields.steps.some(s=>s.stepId!==step.stepId&&s.kind==='ssh'&&s.serverId===i.server.id)).map(({server:s})=>({value:s.id,label:s.name,detail:`${s.username}@${s.host}:${s.port}`}))} onChange={serverId=>change({...step,serverId})} empty="没有可选服务器，请先新建并保存 SSH 连接"/>:<>
      {!inline&&<SearchSelect label={`第 ${index+1} 站 SOCKS5 代理`} value={step.proxyId} disabled={Boolean(inline)} options={catalog.proxies.map(({proxy:p})=>({value:p.id,label:p.name,detail:`${p.host}:${p.port}`}))} onChange={proxyId=>change({...step,proxyId})} empty="还没有代理，可以在这里新建"/>}
      {!inline?<Button ref={addProxy} variant="quiet" disabled={Boolean(candidate)} onClick={()=>c.act(()=>c.store.create('proxy',catalog,{inlineTarget:{parentDraftId:d.id,targetStepId:step.stepId}}))}>新建代理</Button>:<section className="sd-inline-editor" aria-label="就地新建代理" onKeyDown={e=>{if(e.key==='Escape'&&!e.nativeEvent.isComposing){e.preventDefault();e.stopPropagation();c.cancel(inline);}}}><Stack><strong>新建 SOCKS5 代理</strong><FormFields disabled={locked(inline)||c.store.frozen}><ProxyFields inline draft={inline} controller={c}/></FormFields>{inline.error&&<Notice kind="error">{inline.error}</Notice>}<Inline><Button disabled={locked(inline)} onClick={()=>c.cancel(inline)}>取消添加</Button><Button variant="primary" busy={locked(inline)&&inline.phase!=='reconciling'} onClick={()=>c.submit(inline)}>{inline.phase==='reconciling'?'核对保存结果':'保存并选用'}</Button></Inline><small>保存后即可复用，取消服务器配置也会保留此代理。</small></Stack></section>}
      <Disclosure title={`下一站域名解析 · ${step.resolve==='proxy'?'由代理解析':'本机解析'}`}><SelectField label="解析位置" value={step.resolve} onChange={e=>change({...step,resolve:e.target.value as 'proxy'|'local'})}><option value="proxy">由此代理解析</option><option value="local">由 SoDeck 本机解析</option></SelectField></Disclosure>
    </>}
  </Stack>;
}
