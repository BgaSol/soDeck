import {useEffect,useRef,useState} from 'react';
import {onDialogCancel,dialogContext} from '@sodeck/sdk';
import {Button,Checkbox,Dialog,FormFields,FormSection,Inline,Notice,RadioGroup,Stack,TextField} from '@sodeck/ui';
import type {Rule} from './rules';
import {draft,edit,type Draft} from './editor-model';
export {draft,edit,ruleFromDraft,type Draft} from './editor-model';
export function Editor({value,onChange,old,automatic=false,dirty:allDirty,running,busy,pending=false,error,onClose,onSave}:{value:Draft;onChange(value:Draft):void;old?:Rule;automatic?:boolean;dirty?:boolean;running:boolean;busy:boolean;pending?:boolean;error?:string;onClose():void;onSave(action:'save'|'start'|'restart'):void}) {
  const [discard,setDiscard]=useState(false);
  const composing=useRef(false);
  const invalidName=[...value.name].length>128;
  const invalidPort=!value.auto&&(!/^\d+$/.test(value.port)||Number(value.port)<1||Number(value.port)>65535);
  const dirty=allDirty??(JSON.stringify(value)!==JSON.stringify(old?edit(old,automatic):draft(value.type)));
  const close=()=>{if(busy)return;if(dirty)setDiscard(true);else onClose();};
  useEffect(()=>onDialogCancel(()=>{if(discard)setDiscard(false);else close();}));
  return <Dialog width={dialogContext()?.width} height={dialogContext()?.height} title={old?'编辑网络隧道':'创建网络隧道'} onClose={close}>
    <form className="network-editor" onCompositionStart={()=>{composing.current=true;}} onCompositionEnd={()=>{composing.current=false;}} onKeyDown={e=>{if(e.key==='Enter'&&(composing.current||e.nativeEvent.isComposing||e.keyCode===229))e.preventDefault();}} onSubmit={e=>{e.preventDefault();if(!composing.current&&!busy)onSave('save');}}>
      <ScrollFields><FormFields disabled={busy||pending}><Stack gap="md">
        {error&&<Notice kind="error">{error}</Notice>}
        {!old&&<RadioGroup label="用途" value={value.type} options={[{value:'port-forward',label:'端口转发'},{value:'network-proxy',label:'内网代理'}]} onChange={type=>onChange({...value,type:type as Rule['type']})}/>}
        <TextField label="连接名称（可选）" placeholder="留空时自动命名" maxLength={128} autoFocus value={value.name} error={value.name&&invalidName?'名称最多 128 个字符':undefined} onChange={e=>onChange({...value,name:e.target.value})}/>
        <FormSection title="本地入口 · 127.0.0.1"><Stack>
          <Checkbox label="自动分配端口" checked={value.auto} onChange={e=>onChange({...value,auto:e.target.checked})}/>
          {!value.auto&&<TextField label="本地端口" inputMode="numeric" value={value.port} error={value.port&&invalidPort?'端口范围是 1～65535':undefined} onChange={e=>onChange({...value,port:e.target.value})}/>}
        </Stack></FormSection>
        {value.type==='port-forward'?<FormSection title="由最终 SSH 节点访问的服务"><Stack>
          <TextField label="目标主机" placeholder="IP 或域名" value={value.host} maxLength={255} onChange={e=>onChange({...value,host:e.target.value})}/>
          <TextField label="目标端口" inputMode="numeric" value={value.targetPort} onChange={e=>onChange({...value,targetPort:e.target.value})}/>
        </Stack></FormSection>:<Notice>同一端口支持 HTTP、HTTPS CONNECT 和 SOCKS5。目标地址由浏览器或工具的每次请求指定。</Notice>}
        <details><summary>更多设置</summary><Checkbox label="断线后自动尝试重连（最多 3 次）" checked={value.autoReconnect} onChange={e=>onChange({...value,autoReconnect:e.target.checked})}/></details>
        {running&&<Notice>保存后下次启动生效。“应用并重启”会断开这条规则的当前连接。</Notice>}
      </Stack></FormFields></ScrollFields>
      <Inline className="network-actions"><Button onClick={close} disabled={busy}>取消</Button><Button type="submit" busy={busy}>{pending?'核对保存结果':running?'保存，下次启动生效':'保存'}</Button><Button variant="primary" disabled={pending} busy={busy} onClick={()=>onSave(running?'restart':'start')}>{running?'应用并重启':'保存并启动'}</Button></Inline>
    </form>
    {discard&&<Dialog title="放弃未保存的修改？" onClose={()=>setDiscard(false)}><Stack className="network-pad"><Inline><Button onClick={()=>setDiscard(false)}>继续编辑</Button><Button variant="danger" onClick={onClose}>放弃修改</Button></Inline></Stack></Dialog>}
  </Dialog>;
}
function ScrollFields({children}:{children:React.ReactNode}) {return <div className="network-fields">{children}</div>;}
