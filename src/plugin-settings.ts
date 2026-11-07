import type { Plugin, Snapshot } from "./core";
import type { ViewState } from "./views";
import { button as b, date, empty, escape as e, icon, statusText } from "./ui";
import { canDownload, pluginUpdate, updateStatus, updateWorking } from "./updates";

const attrs = (id: string) => `data-id="${e(id)}"`;
function pluginState(p: Plugin) {
  if (p.pendingRestart) return `${p.status === "running" ? "运行中 · " : ""}重启后${p.enabled ? "启用" : "关闭"}`;
  return !p.enabled && p.status === "unloaded" ? "已关闭" : statusText[p.status];
}
function toggle(p: Plugin, v: ViewState) {
  const busy=v.busy.has(p.manifest.id) || ["starting","stopping"].includes(p.status);
  return b("toggle-plugin", p.enabled ? "已启用" : "已关闭", undefined, `plugin-switch ${p.enabled ? "on" : ""}`, `${attrs(p.manifest.id)} id="plugin-toggle-${e(p.manifest.id)}" role="switch" aria-checked="${p.enabled}" aria-label="启用 ${e(p.manifest.name)}" ${busy ? "disabled" : ""}`);
}
function official(s: Snapshot, v: ViewState) {
  const status = v.officialLoading ? "正在从官方仓库获取插件…" : "自动下载适合本机的版本，安装后重启启用。";
  return `<p class="plugin-list-intro official-catalog-status" role="status">${status}</p>${v.officialError ? `<p class="error-box official-catalog-error" role="alert">${e(v.officialError)}</p>` : ""}${!v.officialPlugins.length && !v.officialLoading && !v.officialError ? '<p class="plugin-list-intro">官方仓库暂未提供插件，可从本地添加。</p>' : ""}<div class="official-plugin-list">${v.officialPlugins.map(p=>{
    const installed=s.plugins.find(x=>x.manifest.id===p.id),u=v.updates[p.id],busy=v.busy.has(`install:${p.id}`)||updateWorking(u);
    const actionAttrs = `${attrs(p.id)} id="official-action-${e(p.id)}"`;
    return `<article class="official-plugin" data-official-plugin="${e(p.id)}"><div class="maintenance-symbol">${icon(p.icon || "plug")}</div><div class="update-content"><h3>${e(p.name)}</h3><p>${e(p.description)}</p>${installed ? `<p class="plugin-install-state" role="status">${icon("check")}已安装 · ${pluginState(installed)}</p>` : busy ? `<p role="status">${updateWorking(u) ? updateStatus(u) : "正在准备安装"}</p>` : ""}</div>${installed ? b("select-plugin","管理",undefined,"quiet",actionAttrs) : b("install-official-plugin",busy ? "安装中…" : u?.phase==="error" ? "重试安装" : "安装","download","quiet",`${actionAttrs} ${busy ? "disabled" : ""}`)}${u?.phase==="downloading"&&!installed ? `<progress aria-label="${e(p.name)}下载进度" ${u.totalBytes ? `value="${u.downloadedBytes}" max="${u.totalBytes}"` : ""}></progress>` : ""}${u?.error&&!installed ? `<p class="error-box" role="alert">${e(u.error)}</p>` : ""}</article>`;
  }).join("")}</div>${v.officialFetchedAt ? `<p class="official-catalog-time hint">列表更新于 ${e(date(v.officialFetchedAt))}</p>` : ""}`;
}
function detail(p: Plugin, v: ViewState) {
  const u=pluginUpdate(p,v), source=v.sourceDrafts[p.manifest.id]??p.manifest.updateSource;
  const pair=(name:string,value:unknown)=>`<div class="pair"><dt>${name}</dt><dd>${e(value)}</dd></div>`;
  return `${b("back-plugins","返回插件管理",undefined,"text-button",'id="back-plugins"')}<div class="section-heading maintenance-heading"><div><h2>${e(p.manifest.name)}</h2><p>${e(p.manifest.version)} · ${pluginState(p)}</p></div>${toggle(p,v)}</div><div class="maintenance-detail-actions">${p.manifest.updateSource ? b("plugin-updates","前往软件更新","download","quiet",attrs(p.manifest.id)) : ""}${p.manifest.settings ? b("plugin-component-settings","组件设置","settings","quiet",attrs(p.manifest.id)) : ""}${b("plugin-logs","查看日志","log","quiet",attrs(p.manifest.id))}</div><p class="maintenance-meta">${updateStatus(u)}${p.pendingVersion ? ` · ${e(p.pendingVersion)}` : ""}</p>${p.error||p.updateError||u.error ? `<p class="error-box" role="alert">${e(p.updateError??p.error??u.error)}</p>` : ""}${p.status === "failed" ? `<div class="error-box"><p>插件异常退出，查看日志排查后重启应用重新加载。</p>${b("plugin-logs","查看日志","log","quiet",attrs(p.manifest.id))}${b("restart-app","重启应用","restart","quiet")}</div>` : ""}<details class="maintenance-details" data-persist="source-${e(p.manifest.id)}"><summary>更新来源 · ${e(p.manifest.updateSource?.repository??"手动管理")}</summary><div class="update-source"><label>GitHub 仓库<input id="update-repository" data-source="repository" value="${e(source?.repository??"")}" placeholder="owner/repository" autocomplete="off"></label><label>插件标签前缀<input id="update-prefix" data-source="tagPrefix" value="${e(source?.tagPrefix??"")}" placeholder="plugin-terminal-v" autocomplete="off"></label><div class="actions">${b("save-update-source","保存来源",undefined,"quiet",attrs(p.manifest.id))}${p.manifest.updateSource ? b("clear-update-source","改为手动管理",undefined,"text-button",attrs(p.manifest.id)) : ""}</div><p class="hint">从你信任的仓库更新；来源不影响插件启用。</p></div></details><details class="maintenance-details" data-persist="technical-${e(p.manifest.id)}"><summary>技术信息</summary><dl>${pair("插件 ID",p.manifest.id)}${pair("核心 API",p.manifest.apiVersion)}${pair("适用平台",p.manifest.target)}${pair("插件目录",p.directory)}${pair("入口",p.manifest.entry)}${p.pid ? pair("进程",p.pid) : ""}${p.startedAt ? pair("启动时间",date(p.startedAt)) : ""}${p.instanceId ? pair("实例",p.instanceId) : ""}</dl></details><div class="detail-bottom">${b("forget-plugin","移除记录",undefined,"quiet danger",`${attrs(p.manifest.id)} ${p.enabled||!p.cleanupComplete||v.busy.has(p.manifest.id) ? "disabled" : ""}`)}<span class="hint">关闭并重启后可移除，不删除程序与数据。</span></div>`;
}
function installed(s: Snapshot, v: ViewState) {
  return s.plugins.length ? `<p class="plugin-list-intro">启用和关闭在重启后生效。组件偏好可在左侧设置中调整。</p><div class="maintenance-plugin-list">${s.plugins.map(p=>{
    const u=pluginUpdate(p,v),description=v.officialPlugins.find(x=>x.id===p.manifest.id)?.description;
    return `<article class="maintenance-plugin-row" data-plugin-id="${e(p.manifest.id)}"><div class="maintenance-symbol">${icon(v.officialPlugins.find(x=>x.id===p.manifest.id)?.icon || "plug")}</div><div class="update-content"><b>${e(p.manifest.name)}</b>${description ? `<p class="maintenance-meta">${e(description)}</p>` : ""}<p class="maintenance-meta">${e(p.manifest.version)} · ${pluginState(p)}${p.pendingVersion ? ` · 新版 ${e(p.pendingVersion)} 待应用` : ""}</p>${p.error || p.updateError ? `<p class="update-error maintenance-meta">${e(p.updateError ?? p.error)}</p>` : ""}</div><div class="maintenance-plugin-actions">${toggle(p,v)}<div class="actions">${canDownload(u) ? b("plugin-updates",`更新至 ${e(u.version)}`,undefined,"text-button",attrs(p.manifest.id)) : updateWorking(u) ? `<span class="hint" role="status">${updateStatus(u)}</span>` : ""}${b("select-plugin","详情",undefined,"text-button",`${attrs(p.manifest.id)} id="plugin-detail-${e(p.manifest.id)}"`)}</div></div></article>`;
  }).join("")}</div>` : `<div class="plugin-empty">${empty("还没有安装插件","先从官方插件添加终端、文件等常用功能。")}${b("plugin-tab","浏览官方插件","plus","primary",'data-id="official"')}</div>`;
}
export function pluginsPage(s:Snapshot,v:ViewState) {
  const p=s.plugins.find(p=>p.manifest.id===v.plugin);
  const error=v.error ? `<div role="alert" class="error-box">${e(v.error)}</div>` : "";
  if(p) return error+detail(p,v);
  const tab=v.pluginTab ?? (s.plugins.length ? "installed" : "official");
  return `<div class="section-heading maintenance-heading"><div><h2>插件管理</h2><p>添加工作台功能，管理已安装的插件。</p></div>${b("choose-plugin","从本地添加","folder","quiet",`id="choose-plugin" ${v.busy.has("choose") ? "disabled" : ""}`)}</div><div class="plugin-tabs" role="group" aria-label="插件列表">${b("plugin-tab",`已安装 <span>${s.plugins.length}</span>`,undefined,"",`id="plugins-installed" data-id="installed" aria-pressed="${tab === "installed"}"`)}${b("plugin-tab","官方插件",undefined,"",`id="plugins-official" data-id="official" aria-pressed="${tab === "official"}"`)}${tab === "official" ? b("refresh-official-plugins",v.officialLoading ? "正在刷新…" : v.officialError ? "重试获取" : "刷新列表","restart","refresh-official",`id="refresh-official-plugins" ${v.officialLoading ? "disabled" : ""}`) : ""}</div>${error}${tab === "official" ? official(s,v) : installed(s,v)}`;
}
