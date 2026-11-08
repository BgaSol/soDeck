import {SharedState} from './state.js';
export function parseOsc7(text,host){
  if(!host||text.length>4096||!/^file:\/\//i.test(text)||/[\u0000-\u001f\u007f]/.test(text)||/%(?![\da-f]{2})/i.test(text))return null;
  try{
    const uri=new URL(text);if(uri.protocol!=='file:'||uri.username||uri.password||uri.port||uri.search||uri.hash||uri.hostname.toLowerCase()!==host.toLowerCase())return null;
    const path=decodeURIComponent(uri.pathname);if(!path.startsWith('/')||/[\u0000-\u001f\u007f]/.test(path)||path.length>4096)return null;
    // shortcut: configured host opts into the default filesystem; same-host nested shells need follow disabled.
    const windows=/^\/[a-z]:\//i.test(path);return {state:'known',path:windows?path.slice(1).replaceAll('/','\\'):path,pathStyle:windows?'windows':'posix',filesystemView:'session-default',method:'shell-integration'};
  }catch{return null;}
}
export function createLocation(terminal){
  let client,mount,host='',kind='',frameOffset=0,report,latest,ready=false,busy=false,disposed=false,timer,epoch=0;
  const valid=()=>!disposed&&ready&&client?.available&&JSON.stringify(mount.context)===JSON.stringify(mount.data.openedContext);
  const handler=terminal.parser.registerOscHandler(7,text=>{if(kind==='data')report=parseOsc7(text,host)||{state:'unknown',path:null,reason:'目录报告无法验证'};return true;});
  async function publish(){
    if(!valid()||busy||!latest)return;busy=true;const ticket=epoch,target=latest;
    try{
      const current=(await client.request('get',{topic:'terminal.location.v1',key:'current'})).record;
      if(ticket!==epoch||disposed)return;
      if(current.present&&current.value?.terminalId===mount.data.streamId&&(BigInt(current.value.outputOffset||'0')>BigInt(target.outputOffset)||(current.value.outputOffset===target.outputOffset&&current.value.state===target.state))) {if(latest===target)latest=null;return;}
      await client.request('put',{topic:'terminal.location.v1',key:'current',expectedRevision:current.revision,value:target});
      if(latest===target)latest=null;
    }catch{if(ticket===epoch&&!disposed)timer=setTimeout(()=>{timer=undefined;void publish();},1000);}
    finally{busy=false;if(!timer&&latest&&valid())queueMicrotask(publish);}
  }
  return {
    mount(value){client?.dispose();mount=value;client=new SharedState(value);host=value.settings?.values?.directoryReportHost||'';ready=false;latest=null;++epoch;},
    settings(value){const next=value?.values?.directoryReportHost||'';if(next!==host){host=next;report=null;latest={state:'unknown',path:null,reason:'目录报告配置已更换',terminalId:mount.data.streamId,outputOffset:String(frameOffset),observedAt:Date.now()};++epoch;void publish();}},
    frame(type,offset){kind=type;frameOffset=offset;report=null;},
    completed(){if(report&&Number.isSafeInteger(frameOffset)){latest={...report,terminalId:mount.data.streamId,outputOffset:String(frameOffset),observedAt:Date.now()};void publish();}report=null;},
    ready(){ready=true;void publish();},
    unknown(offset,reason){if(Number.isSafeInteger(offset)){latest={state:'unknown',path:null,reason,terminalId:mount.data.streamId,outputOffset:String(offset),observedAt:Date.now()};void publish();}},
    invalidate(){ready=false;latest=null;++epoch;clearTimeout(timer);timer=undefined;},
    dispose(){disposed=true;++epoch;clearTimeout(timer);handler.dispose();client?.dispose();},
  };
}
