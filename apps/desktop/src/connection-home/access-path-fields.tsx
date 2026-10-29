import {Button,Notice,RadioGroup,SelectField,FormSection,Stack,Inline} from '@sodeck/ui';
import {useState} from 'react';
import type {Controller} from './controller';
import {dependencies,type Draft,type Catalog} from './model';
import {insertStep,moveStep,arrivalSource} from './route-draft';
import {RouteStepFields} from './route-step-fields';
export function AccessPathFields({draft:d,catalog,controller:c}:{draft:Draft;catalog:Catalog;controller:Controller}){
  const f=d.fields,[position,setPosition]=useState('end');
  const patch=(p:Partial<Draft['fields']>)=>c.act(()=>{const next={...d.fields,...p};c.store.edit(d.id,{...p,dependencies:dependencies(next,catalog)});});
  const focus=(id:string)=>requestAnimationFrame(()=>document.getElementById('route-step-'+id)?.focus());
  const add=(kind:'ssh'|'socks5')=>{const at=position==='end'?f.steps.length:f.steps.findIndex(s=>s.stepId===position);const steps=insertStep(f.steps,Math.max(0,at),kind);patch({steps});focus(steps[Math.max(0,at)].stepId);};
  return <FormSection title="连接路径"><Stack>
    <RadioGroup label="如何到达服务器" value={f.pathEnabled?'via':'direct'} options={[{value:'direct',label:'直接连接'},{value:'via',label:'经由中转'}]} onChange={v=>patch({pathEnabled:v==='via'})}/>
    {f.pathEnabled&&<>
      <small>按顺序经过每一站，只使用 SSH 节点本身的身份。最终目标始终在列表末尾。</small>
      <ol className="connection-path">{f.steps.map((step,index)=><li key={step.stepId}>
        <RouteStepFields {...{step,index,catalog,controller:c,draft:d}} source={arrivalSource(f.steps,index,catalog)}
          change={next=>patch({steps:f.steps.map(s=>s.stepId===step.stepId?next:s)})}
          move={delta=>{patch({steps:moveStep(f.steps,step.stepId,delta)});requestAnimationFrame(()=>{const direction=delta<0?'up':'down';const button=document.getElementById(`route-${direction}-${step.stepId}`) as HTMLButtonElement|null;if(button&&!button.disabled)button.focus();else document.getElementById(`route-${delta<0?'down':'up'}-${step.stepId}`)?.focus();});}}
          remove={()=>{const inline=d.inlineId?c.store.items.get(d.inlineId):undefined;if(inline?.inlineTarget?.targetStepId===step.stepId)c.store.remove(inline.id);patch({steps:f.steps.filter(s=>s.stepId!==step.stepId)});const next=f.steps[index+1]??f.steps[index-1];if(next)focus(next.stepId);else requestAnimationFrame(()=>document.getElementById('add-route-ssh')?.focus());}}/>
      </li>)}</ol>
      <SelectField label="插入位置" value={position} onChange={e=>setPosition(e.target.value)}><option value="end">最后一站之后</option>{f.steps.map((s,i)=><option key={s.stepId} value={s.stepId}>第 {i+1} 站之前</option>)}</SelectField>
      <Inline><Button id="add-route-ssh" onClick={()=>add('ssh')}>添加 SSH</Button><Button onClick={()=>add('socks5')}>添加 SOCKS5</Button></Inline>
      <small>最终目标 · {f.host||'目标服务器'} · 由{arrivalSource(f.steps,f.steps.length,catalog)}访问</small>
    </>}
    {d.errors.path&&<Notice kind="error">{d.errors.path}</Notice>}
  </Stack></FormSection>;
}
