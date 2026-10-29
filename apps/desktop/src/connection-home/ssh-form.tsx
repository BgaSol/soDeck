import {pathIdentity} from './route-draft';
import {FormFields,FormSection,Stack,TextField,Notice} from '@sodeck/ui';
import {CredentialFields} from './credential-fields';
import {AccessPathFields} from './access-path-fields';
import {GroupFields} from './group-fields';
import {accessPath,generatedName,type Catalog,type Draft} from './model';
import type {Controller} from './controller';
export function SshForm({draft:d,catalog,controller:c,refresh}:{draft:Draft;catalog:Catalog;controller:Controller;refresh():Promise<void>}){
  const f=d.fields,edit=(patch:Partial<Draft['fields']>)=>c.act(()=>c.store.edit(d.id,patch));
  const identityChanged=d.server&&(d.server.server.host.toLowerCase()!==f.host.trim().toLowerCase()||d.server.server.username!==f.username.trim()||String(d.server.server.port)!==f.port||d.server.server.authType!==f.authType||pathIdentity(d.server.accessPath)!==pathIdentity(accessPath(f)));
  return <Stack gap="lg"><FormSection title="服务器"><Stack gap="md"><div className="connection-endpoint"><TextField disabled={d.phase==='credentialError'} autoFocus label="主机地址" value={f.host} error={d.errors.host} placeholder="dev.example.com" onChange={e=>edit({host:e.target.value})}/><TextField disabled={d.phase==='credentialError'} label="SSH 端口" value={f.port} error={d.errors.port} inputMode="numeric" onChange={e=>edit({port:e.target.value})}/></div><TextField disabled={d.phase==='credentialError'} label="用户名" value={f.username} error={d.errors.username} placeholder="登录此服务器的账号" autoComplete="off" onChange={e=>edit({username:e.target.value})}/></Stack></FormSection>
    {identityChanged&&<Notice kind="warning">连接身份已改变，请提供对应凭据。留空时将在连接时重新请求。</Notice>}
    <CredentialFields draft={d} controller={c}/><FormFields disabled={d.phase==='credentialError'}><AccessPathFields draft={d} catalog={catalog} controller={c}/></FormFields>
    <FormSection title="名称与分组"><Stack gap="md"><TextField disabled={d.phase==='credentialError'} label="显示名称" value={f.name} error={d.errors.name} placeholder="留空使用主机地址" help={f.host?`将使用：${f.name.trim()||generatedName(d)}`:'非 22 端口会附加到自动生成的名称中'} onChange={e=>edit({name:e.target.value})}/><FormFields disabled={d.phase==='credentialError'}><GroupFields draft={d} catalog={catalog} controller={c} refresh={refresh}/></FormFields></Stack></FormSection>
  </Stack>;
}
