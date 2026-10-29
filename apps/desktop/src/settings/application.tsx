import {useEffect,useState} from 'react';
import {core,errorMessage} from '@sodeck/sdk';
import type {RuntimeInfo} from '@sodeck/sdk/settings';
import {Button,Notice} from '@sodeck/ui';
export function ApplicationSettings({onPlugins}:{onPlugins():void}){
  const [info,setInfo]=useState<RuntimeInfo>(),[error,setError]=useState('');
  useEffect(()=>{let live=true;void core.call<RuntimeInfo>('app.runtimeInfo.get').then(v=>{if(live)setInfo(v);}).catch(e=>{if(live)setError(errorMessage(e));});return()=>{live=false;};},[]);
  const run=async(method:string)=>{try{await core.call(method);setError('');}catch(e){setError(errorMessage(e));}};
  return <div className="settings-form"><h1>应用与数据</h1>{info?<><dl><dt>版本</dt><dd>{info.version} · {info.channel}</dd><dt>运行模式</dt><dd>{{portable:'便携模式',standard:'标准模式',development:'开发模式'}[info.mode]}</dd><dt>数据目录</dt><dd className="settings-data-path">{info.dataPath}</dd></dl><div className="settings-actions"><Button onClick={()=>void run('app.dataDirectory.open')}>打开目录</Button><Button onClick={()=>void run('app.dataDirectory.copyPath')}>复制路径</Button></div></>:<p role="status">正在读取应用信息</p>}<section><h2>插件</h2><p>插件提供的设置会自动出现在左侧。</p><Button onClick={onPlugins}>管理插件</Button></section>{error&&<Notice kind="error">{error}</Notice>}</div>;
}
