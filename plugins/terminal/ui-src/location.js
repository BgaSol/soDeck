import {SharedState} from './state.js';
export function parseIntegration(text,token){
  if(!token||text.length>17000)return null;
  const parts=text.split(';');if(parts[0]!=='SODECK'||parts[1]!==token)return null;
  const activity=parts[2];
  if(activity==='busy')return {state:'unknown',path:null,activity:'busy',reasonCode:'busy'};
  if(activity==='unavailable')return {state:'unknown',path:null,activity:'unavailable',reasonCode:parts[3]||'integration-failed'};
  if(activity!=='ready'||parts.length!==5)return {state:'unknown',path:null,reasonCode:'invalid-report'};
  const decode=value=>{if(!value||value.length>8192||!/^([0-9a-f]{2})+$/.test(value))throw Error();const path=new TextDecoder('utf-8',{fatal:true}).decode(Uint8Array.from(value.match(/../g),s=>parseInt(s,16)));if(!path.startsWith('/')||/[\u0000-\u001f\u007f]/.test(path))throw Error();return path;};
  try{return {state:'known',path:decode(parts[3]),pathStyle:'posix',filesystemView:'shell-reported',witness:{path:decode(parts[4]),value:token},method:'automatic-shell',activity:'ready'};}
  catch{return {state:'unknown',path:null,reasonCode:'invalid-report'};}
}
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
  let client,mount,host='',kind='',frameOffset=0,report,latest,ready=false,busy=false,disposed=false,timer,epoch=0,integration={},observed=false,confirmed=false,handshake;
  const valid=()=>!disposed&&ready&&client?.available&&JSON.stringify(mount.context)===JSON.stringify(mount.data.openedContext);
  const handler=terminal.parser.registerOscHandler(7,text=>{if(kind==='data'&&integration.mode==='manual')report=parseOsc7(text,host)||{state:'unknown',path:null,reasonCode:'invalid-report'};return true;});
  const automatic=terminal.parser.registerOscHandler(633,text=>{if(kind==='data'&&integration.mode==='auto'){const parsed=parseIntegration(text,integration.token);if(parsed){observed=true;if(parsed.state==='known')confirmed=true;if(parsed.activity==='unavailable')confirmed=false;clearTimeout(handshake);report=parsed;}}return true;});
  function observation(value){return {...value,terminalId:mount.data.streamId,title:mount.data.title,shell:integration.shell,outputOffset:String(frameOffset),observedAt:Date.now()};}
  async function publish(){
    if(!valid()||busy||!latest)return;busy=true;const ticket=epoch,target=latest;
    try{
      const current=(await client.request('get',{topic:'terminal.location.v1',key:'current'})).record;
      if(ticket!==epoch||disposed)return;
      if(current.present&&current.value?.terminalId===mount.data.streamId&&(BigInt(current.value.outputOffset||'0')>BigInt(target.outputOffset)||(current.value.outputOffset===target.outputOffset&&current.value.state===target.state&&current.value.activity===target.activity&&current.value.reasonCode===target.reasonCode))) {if(latest===target)latest=null;return;}
      await client.request('put',{topic:'terminal.location.v1',key:'current',expectedRevision:current.revision,value:target});
      if(latest===target)latest=null;
    }catch{if(ticket===epoch&&!disposed)timer=setTimeout(()=>{timer=undefined;void publish();},1000);}
    finally{busy=false;if(!timer&&latest&&valid())queueMicrotask(publish);}
  }
  return {
    mount(value){client?.dispose();mount=value;client=new SharedState(value);host=value.settings?.values?.directoryReportHost||'';integration=value.data.integration||{mode:'manual'};ready=false;latest=null;observed=false;++epoch;},
    settings(value){const next=value?.values?.directoryReportHost||'';if(integration.mode==='manual'&&next!==host){host=next;report=null;latest={state:'unknown',path:null,reason:'目录报告配置已更换',terminalId:mount.data.streamId,outputOffset:String(frameOffset),observedAt:Date.now()};++epoch;void publish();}},
    frame(type,offset){kind=type;frameOffset=offset;report=null;},
    completed(){if(report&&Number.isSafeInteger(frameOffset)){latest=observation(report);void publish();}report=null;},
    ready(){ready=true;void publish();const ticket=epoch;void client?.request('get',{topic:'terminal.location.v1',key:'current'}).then(({record})=>{
      if(ticket!==epoch||disposed||observed||(record?.present&&integration.mode!=='off'))return;
      latest=observation({state:'unknown',path:null,activity:'starting',reasonCode:integration.mode==='auto'?'starting':integration.mode==='off'?'disabled':integration.mode==='manual'?'manual-setup':integration.reasonCode||'old-core'});void publish();
      if(integration.mode==='auto')handshake=setTimeout(()=>{if(ticket===epoch&&!disposed&&!observed){latest=observation({state:'unknown',path:null,activity:'unavailable',reasonCode:'handshake-timeout'});void publish();}},8000);
    }).catch(()=>{});},
    input(text){if(integration.mode==='auto'&&confirmed&&/[\r\n\x03\x04]/.test(text)){latest=observation({state:'unknown',path:null,activity:'busy',reasonCode:'busy'});void publish();}},
    unknown(offset,reason){if(Number.isSafeInteger(offset)){latest={state:'unknown',path:null,reason,terminalId:mount.data.streamId,outputOffset:String(offset),observedAt:Date.now()};void publish();}},
    invalidate(){ready=false;latest=null;++epoch;clearTimeout(timer);clearTimeout(handshake);timer=undefined;},
    dispose(){disposed=true;++epoch;clearTimeout(timer);clearTimeout(handshake);handler.dispose();automatic.dispose();client?.dispose();},
  };
}
