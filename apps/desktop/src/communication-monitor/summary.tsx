import {core,formatByteCount,formatTrafficRate,errorMessage} from '@sodeck/sdk';
import {Button,MetricPair} from '@sodeck/ui';
import {trafficState,useTrafficSummary} from './client';
import {useCommunicationPolicy} from './policy';

export function CommunicationSummary({serverId,onError}:{serverId:string;onError(message:string):void}) {
  const {value,error}=useTrafficSummary(undefined,serverId);
  const {policy}=useCommunicationPolicy(),unit=policy?.rateUnit??'bytes';
  const label=error?'通信统计暂不可用':!value?'正在采集通信数据':trafficState(value.state);
  const tooltip=value?`当前服务器 · 通道数据\n本次采集始于 ${new Date(value.startedAt).toLocaleString()}\n累计发送 ${formatByteCount(value.sentBytes)} · 接收 ${formatByteCount(value.receivedBytes)}${value.coverage==='partial'?'\n部分覆盖，展开查看详情':''}`:label;
  return <Button className="communication-summary" size="compact" variant="quiet" title={error||tooltip}
    aria-label={`打开通信监控，${label}${value?.state==='active'?`，发送 ${formatTrafficRate(value.sendRate,unit)}，接收 ${formatTrafficRate(value.receiveRate,unit)}`:''}`}
    onClick={()=>void (async()=>{
      const summary=value??await core.call<import('@sodeck/sdk').CommunicationSummary>('communication.summary',{});
      await core.call('ui.communicationMonitor.open',{operationId:crypto.randomUUID(),target:{target:{kind:'server',configEpoch:summary.configEpoch,serverId},layer:'channel'}});
    })().catch(e=>onError(errorMessage(e)))}>
    {!error&&value?.state==='active'?<MetricPair compact send={formatTrafficRate(value.sendRate,unit)} receive={formatTrafficRate(value.receiveRate,unit)}/>:label}
    {value?.coverage==='partial'&&' · 部分覆盖'}
  </Button>;
}
