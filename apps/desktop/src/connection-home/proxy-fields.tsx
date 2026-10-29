import {TextField,RadioGroup,PasswordField,Checkbox,Stack} from '@sodeck/ui';
import type {Controller} from './controller';
import {generatedName,type Draft} from './model';
export function ProxyFields({draft:d,controller:c}:{draft:Draft;controller:Controller;inline?:boolean}){
  const f=d.fields,secret=c.store.secrets.get(d.id),edit=(patch:Partial<Draft['fields']>)=>c.act(()=>c.store.edit(d.id,patch));
  return <Stack><Stack>
    <div className="connection-endpoint"><TextField disabled={d.phase==='credentialError'} autoFocus label="代理地址" value={f.host} error={d.errors.host} placeholder="proxy.example.com" onChange={e=>edit({host:e.target.value})}/><TextField disabled={d.phase==='credentialError'} label="代理端口" value={f.port} error={d.errors.port} inputMode="numeric" onChange={e=>edit({port:e.target.value})}/></div>
    <RadioGroup disabled={d.phase==='credentialError'} label="代理认证" value={f.authType} options={[{value:'none',label:'无认证'},{value:'usernamePassword',label:'用户名和密码'}]} onChange={v=>edit({authType:v as 'none'|'usernamePassword',username:v==='none'?'':f.username})}/>
    {f.authType==='usernamePassword'&&<><TextField disabled={d.phase==='credentialError'} label="代理用户名" value={f.username} error={d.errors.username} onChange={e=>edit({username:e.target.value})}/><PasswordField key={d.id} label="代理密码" error={d.errors.password} value={secret.password} onChange={e=>c.act(()=>c.store.secret(d.id,{password:e.target.value}))} help={d.proxy?.hasPreset?'已保存，留空保持':'可留空，连接时输入；最多 255 字节'}/><Checkbox label="在此设备上记住代理密码" checked={f.remember} onChange={e=>edit({remember:e.target.checked})}/><small>这是代理凭据，与 SSH 登录密码分别保存。</small></>}
  </Stack><TextField disabled={d.phase==='credentialError'} label="代理名称" value={f.name} error={d.errors.name} placeholder="留空自动生成" help={f.host?`将使用：${f.name.trim()||generatedName(d)}`:'留空使用代理地址和端口'} onChange={e=>edit({name:e.target.value})}/></Stack>;
}
