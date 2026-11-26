import { format } from 'date-fns';
import type { Log, Message, Plugin } from './core';
import { escape as e } from './ui';

export interface ActivityContext { plugins?: Plugin[]; storageErrors: string[] }
export const activityContextKey = (context: ActivityContext) => JSON.stringify([context.plugins?.map(p => [p.manifest.id, p.manifest.name]), context.storageErrors]);
export function sourceName(record: Pick<Log, 'source' | 'pluginId'>, context: ActivityContext) {
  return record.pluginId ? context.plugins?.find(p => p.manifest.id === record.pluginId)?.manifest.name ?? record.pluginId : record.source === 'core' ? 'SoDeck 核心' : '未知来源';
}
export const shortTime = (time: number) => format(time, 'MM/dd HH:mm:ss');
export const exactTime = (time: number) => format(time, 'yyyy/M/d HH:mm:ss.SSS');
export function previewText(value: string, length = 240) {
  let result='', count=0, space=false;
  for(const character of value){
    if(/\s/u.test(character)){space=!!result;continue;}
    if(space){result+=' ';count++;space=false;}
    if(count>=length)break;
    result+=character;count++;
  }
  return result.trimEnd();
}
export const originName = (record: Pick<Log, 'source'>) => record.source === 'core' ? '核心产生' : record.source === 'plugin' ? '插件报告' : record.source;
export function levelName(level: string) { return ({info:'信息', success:'成功', warning:'注意', error:'错误', warn:'警告', debug:'调试'} as Record<string,string>)[level] ?? level; }
export function levelClass(level: string) { return ['info','success','warning','error','warn','debug'].includes(level) ? level : 'unknown'; }
export function sourceFacts(record: Log | Message, context: ActivityContext) {
  const pair = (label: string, value: string, mono = false) => `<dt>${label}</dt><dd${mono ? ' class="activity-mono"' : ''}>${e(value)}</dd>`;
  return `<dl class="activity-facts">${pair('接收时间', exactTime(record.receivedAt))}${pair('来源', sourceName(record, context))}${pair('产生方', originName(record))}${pair('插件 ID', record.pluginId ?? '—', true)}${pair('实例', record.instanceId ?? '—', true)}${pair('记录 ID', record.id, true)}</dl>`;
}
export function storageWarning(context: ActivityContext) {
  return context.storageErrors.length ? `<details class="activity-storage" data-dom-key="storage-warning"><summary>存储需要关注</summary>${context.storageErrors.map(error => `<p>${e(error)}</p>`).join('')}</details>` : '';
}
