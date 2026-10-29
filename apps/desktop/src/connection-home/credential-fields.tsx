import {Button,Checkbox,InputGroup,PasswordField,RadioGroup,TextField,FormSection,Stack} from '@sodeck/ui';
import type {Controller} from './controller';
import type {Draft} from './model';
export function CredentialFields({draft:d,controller:c}:{draft:Draft;controller:Controller}){
  const f=d.fields,secret=c.store.secrets.get(d.id),edit=(patch:Partial<Draft['fields']>)=>c.act(()=>c.store.edit(d.id,patch));
  const set=(patch:Partial<typeof secret>)=>c.act(()=>c.store.secret(d.id,patch));
  return <FormSection title="身份认证"><Stack>
    <RadioGroup disabled={d.phase==='credentialError'} label="认证方式" value={f.authType} options={[{value:'password',label:'密码'},{value:'key',label:'私钥'}]} onChange={v=>edit({authType:v as 'password'|'key'})}/>
    {f.authType==='password'?<PasswordField key={`password-${d.id}`} label="登录密码" value={secret.password} onChange={e=>set({password:e.target.value})} help={d.server?.server.hasSecret?'已保存凭据，留空保持':'可留空，连接时再输入'}/>:<>
      <InputGroup><TextField label="私钥文件" error={d.errors.privateKeyPath} value={secret.privateKeyPath} onChange={e=>set({privateKeyPath:e.target.value})} placeholder="选择文件或输入绝对路径" help="可留空，连接时再提供。最大 1 MiB。"/><Button onClick={()=>void c.pick(d)}>选择文件</Button></InputGroup>
      <PasswordField key={`passphrase-${d.id}`} label="私钥口令" value={secret.passphrase} onChange={e=>set({passphrase:e.target.value})} help="私钥未加密时留空"/>
    </>}
    <Checkbox label={`在此设备上记住${f.authType==='key'?'私钥凭据':'密码'}`} checked={f.remember} onChange={e=>edit({remember:e.target.checked})}/><small>加密保存在此设备；取消勾选后，仅在本次 App 运行期间有效。</small>
  </Stack></FormSection>;
}
