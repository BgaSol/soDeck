import {useEffect,useRef,useState} from 'react';
import {Button,Dialog,DialogBody,DialogActions,EmptyState,FormFields,Inline,Notice,SelectField,Stack} from '@sodeck/ui';
import {Server as ServerIcon} from 'lucide-react';
import type {CatalogItem} from '@sodeck/sdk';
import {SshForm} from './ssh-form';
import {ProxyFields} from './proxy-fields';
import {ServerList} from './server-list';
import {Managers} from './managers';
import {busy,locked,type Catalog} from './model';
import type {Controller} from './controller';
import './layout.css';
export function ConnectionHome({controller:c,catalog,ready,error,refresh,opened,onOpen,onDelete}:{controller:Controller;catalog:Catalog;ready:boolean;error:string;refresh():Promise<void>;opened:Set<string>;onOpen(item:CatalogItem,newWindow?:boolean):void;onDelete(item:CatalogItem):void}){
  const mode=c.store.mode,d=mode.kind==='draft'?c.store.items.get(mode.id):undefined,scroll=useRef<HTMLDivElement>(null),[list,setList]=useState(c.store.directory.showList);
  const drafts=[...c.store.items.values()].filter(v=>!v.inlineTarget&&(c.store.dirty(v)||v.id===d?.id));
  const savedFocus=d?.focus;
  useEffect(()=>{if(scroll.current&&d){scroll.current.scrollTop=d.scroll;const label=savedFocus?[...scroll.current.querySelectorAll('label')].find(l=>l.textContent===savedFocus):undefined;const field=label?.htmlFor?document.getElementById(label.htmlFor):scroll.current.querySelector<HTMLInputElement>('input:not([disabled])');field?.focus({preventScroll:true});}},[d?.id]);
  useEffect(()=>{c.store.directory.showList=list;},[list]);
  useEffect(()=>{if(d&&Object.keys(d.errors).length)scroll.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();},[d?.errors]);
  const title=d?(d.kind==='ssh'?(d.server?'编辑 SSH 连接':'新建 SSH 连接'):(d.proxy?'编辑代理':'新建代理')):'';
  return <section className="connection-home" data-show-list={list} aria-label="新建连接"><ServerList catalog={catalog} controller={c} opened={opened} onOpen={onOpen} onDelete={onDelete} onCreate={(kind,groupId)=>{c.act(()=>c.store.create(kind,catalog,{groupId:groupId||null}));setList(false);}}/>
    <div className="connection-form-area"><div className="connection-mobile-navigation"><Button onClick={()=>setList(v=>!v)}>{list?'返回表单':'服务器列表'}</Button></div>
      {(error||c.error)&&<Notice kind="error" action={error?<Button onClick={()=>void refresh()}>重试同步</Button>:undefined}>{error||c.error}</Notice>}
      {drafts.length>1&&<div className="connection-drafts"><SelectField label={`未完成配置（${drafts.length}）`} value={d?.id??''} onChange={e=>{c.store.select({kind:'draft',id:e.target.value});setList(false);}}><option value="" disabled>恢复一份配置</option>{drafts.map(v=><option value={v.id} key={v.id}>{v.fields.name||v.fields.host||`新建${v.kind==='ssh'?' SSH 连接':'代理'}`}{c.store.dirty(v)?' · 未完成':''}</option>)}</SelectField></div>}
      {!ready?<EmptyState title="正在读取连接目录">读取完成后即可新建或打开服务器。</EmptyState>:mode.kind==='groups'||mode.kind==='proxies'?<Managers kind={mode.kind} catalog={catalog} controller={c} refresh={refresh}/>:!d?<EmptyState icon={ServerIcon} title="选择服务器，开始工作" action={<Button onClick={()=>c.act(()=>c.store.create('ssh',catalog))}>新建 SSH 连接</Button>}>左侧单击服务器即可打开。使用编辑按钮修改连接配置。</EmptyState>:<form className="connection-form-frame" onSubmit={e=>e.preventDefault()} onFocus={e=>{const field=e.target;const label=field instanceof HTMLInputElement||field instanceof HTMLSelectElement||field instanceof HTMLTextAreaElement?field.labels?.[0]?.textContent:undefined;if(label)d.focus=label;}} onKeyDown={e=>{if(e.nativeEvent.isComposing)return;if(e.key==='Enter'&&e.ctrlKey){e.preventDefault();c.submit(d,d.kind==='ssh'&&!d.server);}else if(e.key==='Enter'&&(e.target as HTMLElement).tagName==='INPUT')e.preventDefault();}}>
        <header className="sd-editor-heading"><div><h1>{title}</h1><p>{d.kind==='ssh'?'先填写服务器地址与身份，再选择连接路径。':'代理保存后可供多个 SSH 连接使用。'}</p></div></header>
        <div className="connection-fields" ref={scroll} onScroll={e=>{d.scroll=e.currentTarget.scrollTop;}}><Stack gap="md">
          {d.message&&<Notice kind="success">{d.message}</Notice>}{d.error&&<Notice kind="error">{d.error}</Notice>}
          {d.server&&!catalog.items.some(i=>i.server.id===d.server!.server.id)&&<Notice kind="warning">此服务器已删除。当前草稿仍保留，无法覆盖已删除的服务器。</Notice>}
          {d.proxy&&<Notice>此代理是共享配置。修改将用于后续连接。</Notice>}
          <FormFields disabled={locked(d)||c.store.frozen}>{d.kind==='ssh'?<SshForm draft={d} catalog={catalog} controller={c} refresh={refresh}/>:<ProxyFields draft={d} controller={c}/>}</FormFields>
        </Stack></div>
        <footer className="sd-editor-actions"><span role="status">{busy(d)?d.phase==='savingCredential'?'正在保存凭据…':d.phase==='opening'?'正在打开工作区…':'正在保存配置…':d.phase==='reconciling'?'保存结果待核对':''}</span><Button disabled={busy(d)||c.store.frozen} onClick={()=>c.cancel(d)}>取消</Button>
          {d.kind==='ssh'&&!d.server&&d.phase!=='reconciling'&&<Button disabled={busy(d)||c.store.frozen} onClick={()=>c.submit(d)}>仅保存</Button>}
          <Button variant="primary" busy={busy(d)} disabled={c.store.frozen} onClick={()=>c.submit(d,d.kind==='ssh'&&!d.server)}>{d.phase==='reconciling'?'核对保存结果':d.phase==='credentialError'?'重试后续步骤':d.kind==='proxy'?'保存代理':d.server?'保存修改':'保存并连接'}</Button>
        </footer>
      </form>}
    </div>

  </section>;
}

// Window-level confirmations remain visible while a server tab is selected.
export function ConnectionHomeDialogs({controller:c}:{controller:Controller}){
  return <>
    {c.discard&&<Dialog title="放弃这份未完成配置？" onClose={c.keep}><DialogBody><p>已单独保存的代理和分组会保留。尚未保存的输入将丢失；已开始的保存仍可能完成。</p></DialogBody><DialogActions><Button data-dialog-default variant="primary" onClick={c.keep}>继续编辑</Button><Button variant="danger" onClick={c.discardNow}>放弃修改</Button></DialogActions></Dialog>}
    {c.closing&&!c.closing.waiting&&<Dialog title="关闭前处理未完成配置" onClose={()=>void c.closeReply('cancel')}><DialogBody><p>{c.saves.operations().length?'正在核对已提交的保存。关闭不会回滚已开始的保存。':'未保存的配置将被丢弃。已保存的代理、分组和服务器不受影响。'}</p></DialogBody><DialogActions><Button data-dialog-default variant="primary" onClick={()=>void c.closeReply('cancel')}>继续编辑</Button><Button variant="danger" disabled={c.saves.operations().length>0&&c.elapsed<10000} onClick={()=>void c.closeReply(c.saves.operations().length?'abandonWait':'discard')}>{c.saves.operations().length?'放弃等待并关闭':'放弃修改'}</Button></DialogActions></Dialog>}
  </>;
}
