import type {ReactNode} from 'react';

/** Display-only metrics: callers own collection, units and numeric precision. */
export function MetricPair({send,receive,sendTotal,receiveTotal,compact=false}:{send:string;receive:string;sendTotal?:string;receiveTotal?:string;compact?:boolean}) {
  return <span className="sd-metric-pair" data-compact={compact||undefined}>
    <span className="sd-metric-direction"><span className="sd-metric-label">↑ 发送</span><strong>{send}</strong>{sendTotal&&<small>累计 {sendTotal}</small>}</span>
    <span className="sd-metric-direction"><span className="sd-metric-label">↓ 接收</span><strong>{receive}</strong>{receiveTotal&&<small>累计 {receiveTotal}</small>}</span>
  </span>;
}
export function DataCoverage({partial,children}:{partial:boolean;children?:ReactNode}) {
  return <span className="sd-data-coverage" data-partial={partial||undefined}>{partial?'部分覆盖':'完整覆盖'}{children&&<> · {children}</>}</span>;
}
