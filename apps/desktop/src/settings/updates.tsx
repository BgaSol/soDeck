import {Button,Checkbox,Notice,Progress} from '@sodeck/ui';
import type {Component,Stage} from '@sodeck/sdk/updates';
import type {UpdatesController} from './update-model';
const names:Record<string,string>={core:'SoDeck 核心','sodeck.terminal':'终端','sodeck.files':'文件','sodeck.network-proxy':'网络代理','sodeck.monitor':'资源监控'};
const stages:Record<Stage,string>={available:'可用更新',downloading:'正在下载',verifying:'正在验证',downloaded:'已下载，等待安装',preparing:'正在准备',ready:'重启后生效',waitingForCore:'等待核心更新',reviewing:'等待关闭审查',handoffPending:'等待安装交接',handoffAttempted:'已尝试安装',installed:'已完成',failed:'未完成',cancelled:'已取消',requiresUserAction:'请在更新页继续',unknown:'结果尚未确定',blocked:'暂不可更新'};
const size=(n:number)=>`${(n/1024/1024).toFixed(1)} MB`;
function Detail({row}:{row:Component}){return <><p>当前 {row.currentVersion??'未安装'}{row.availableVersion?` → ${row.availableVersion}`:' · 暂无更高兼容版本'}</p>{row.availableVersion&&<p>{stages[row.stage]} · {size(row.total)}</p>}{row.requiredComponentIds.length>0&&<p>需要同时处理：{row.requiredComponentIds.map(id=>names[id]??id).join('、')}</p>}{row.blockedReasons.map(r=><p key={r}>{r}</p>)}{['downloading','verifying'].includes(row.stage)&&<Progress label={`${names[row.componentId]??row.componentId} 下载进度`} value={row.total?row.received/row.total*100:undefined}/ >}{row.error&&<Notice kind="error">{row.error.message}</Notice>}</>;}
export function PluginUpdate({controller:c,id,digest,disabled}:{controller:UpdatesController;id:string;digest?:string;disabled:boolean}){
  const row=c.rows.find(r=>r.componentId===id);if(!row)return null;
  return <div><p>更新来源：{row.followOfficial?'跟随官方更新':'本地管理'}</p><Detail row={row}/><div className="settings-actions">
    {!row.followOfficial&&digest?<Button disabled={disabled||c.working} onClick={()=>void c.follow(id,digest)}>跟随官方更新</Button>:row.availableVersion&&!row.blockedReasons.length&&<>
      <Button disabled={disabled||c.working||['downloading','verifying','downloaded','ready','waitingForCore'].includes(row.stage)} onClick={()=>void c.run('download',id)}>下载此插件</Button>
      <Button disabled={disabled||c.working||row.stage!=='downloaded'} onClick={()=>void c.run('install',id)}>安装此插件</Button>
    </>}
  </div></div>;
}
export function UpdateSettings({controller:c,disabled}:{controller:UpdatesController;disabled:boolean}){
  const rows=c.plan?.components??c.rows, selected=rows.filter(r=>c.selected.includes(r.componentId));
  const active=!!c.operation?.running, unavailable=!c.status?.configured;
  return <section className="settings-form plugin-settings"><div className="settings-heading"><h1>更新</h1><Button disabled={disabled||c.working||c.status?.checking||unavailable} onClick={()=>void c.run('check')}>{c.status?.checking?'正在检查…':'检查更新'}</Button></div>
    <p>核心与官方插件可以分别更新。检查只获取版本信息，下载和安装由你操作。</p>
    {c.status&&<p>当前核心 {c.status.appVersion} · {{development:'开发版本',preview:'预览渠道',stable:'稳定渠道'}[c.status.channel]}{c.status.lastCheck?` · 上次检查 ${new Date(c.status.lastCheck).toLocaleString()}`:''}</p>}
    <Checkbox label="自动检查并提示更新" checked={c.draft??false} disabled={disabled||!!c.saving||!c.status||c.status.policy.generation==='unavailable'} onChange={e=>c.edit(e.target.checked)}/>
    <p>此选项点击底部“应用”后保存。下载、安装操作即时生效。</p>
    {unavailable&&<Notice>当前构建未配置官方发布源。仍可在插件管理中手动导入插件包。</Notice>}
    {c.status?.distribution!=='installed'&&!unavailable&&<Notice>当前从目录运行，核心需要手动安装新版本。官方插件仍可独立更新。</Notice>}
    {c.error&&<Notice kind="error">{c.error}</Notice>}{c.status?.error&&<Notice kind="warning">{c.status.error.message}</Notice>}{c.status?.warnings.map(w=><Notice key={w} kind="warning">{w}</Notice>)}
    {c.conflict&&<Notice kind="warning" action={<Button onClick={()=>void c.participant.discard().catch(c.reportError)}>载入最新值</Button>}>更新设置已在其他位置改变，当前草稿已保留。</Notice>}
    {c.saving&&<Notice action={<Button onClick={()=>void c.participant.reconcile().catch(c.reportError)}>核对结果</Button>}>正在核对更新设置的保存结果。</Notice>}
    <div className="plugin-settings-list">{rows.map(row=><article className="plugin-settings-row" key={row.componentId}><Checkbox label="选择" aria-label={`选择 ${names[row.componentId]??row.componentId}`} checked={c.selected.includes(row.componentId)} disabled={disabled||c.working||!row.availableVersion} onChange={e=>c.select(row.componentId,e.target.checked)}/><div><h2>{names[row.componentId]??row.componentId}</h2><Detail row={row}/></div></article>)}</div>
    {!!c.plan?.blockedReasons.length&&<Notice kind="warning">{c.plan.blockedReasons.join('；')}</Notice>}
    {!!rows.length&&<div className="settings-actions"><Button disabled={disabled||c.working||active||!selected.length||!!c.plan?.blockedReasons.length} onClick={()=>void c.run('download')}>下载所选更新{selected.length?` · ${size(c.plan?.downloadBytes??0)}`:''}</Button><Button variant="primary" disabled={disabled||c.working||active||!selected.length||selected.some(r=>r.stage!=='downloaded')||!!c.plan?.blockedReasons.length} onClick={()=>void c.run('install')}>{c.selected.includes('core')?'安装所选更新并重启':'安装所选插件'}</Button></div>}
    {c.operation&&<Notice kind={c.operation.error?'warning':'status'} action={active&&!['reviewing','handoffPending','handoffAttempted'].includes(c.operation.stage)?<Button onClick={()=>void c.cancel().catch(c.reportError)}>取消本次操作</Button>:undefined}>{stages[c.operation.stage]}{c.operation.error?`：${c.operation.error.message}`:''}</Notice>}
    {c.status?.restartRequired&&<Notice action={<Button disabled={disabled||c.working} onClick={()=>void c.run('restart')}>重启应用</Button>}>已准备的插件更新将在完整重启后生效。</Notice>}
  </section>;
}
