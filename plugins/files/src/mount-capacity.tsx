import {BusyIndicator,Text} from '@sodeck/ui';
import type {CacheValue} from './capacity-cache';
import {size,type Capacity} from './format';
export function MountCapacity({value}:{value?:CacheValue<Capacity>}){
  if(!value||value.state==='loading')return <span className="mount-capacity"><BusyIndicator label="正在读取卷容量"/><Text size="small" tone="muted">读取中</Text></span>;
  if(value.state==='unavailable')return <span className="mount-capacity" title={value.message}><Text size="small" tone="muted">容量不可用</Text></span>;
  const label=`${size(value.value.used)} / ${size(value.value.total)}`;
  return <Text className="mount-capacity" numeric size="small" tone="muted" title={`${label}${value.value.source==='df'?' · 容量按 1 KiB 精度提供':''}`}>{label}</Text>;
}
