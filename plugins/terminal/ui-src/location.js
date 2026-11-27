export function parseIntegration(text,token){
  if(!token||text.length>17000)return null;
  const parts=text.split(';');if(parts[0]!=='SODECK'||parts[1]!==token)return null;
  const activity=parts[2];
  if(activity==='busy')return {state:'unknown',path:null,activity:'busy',reasonCode:'busy'};
  if(activity==='unavailable')return {state:'unknown',path:null,activity:'unavailable',reasonCode:parts[3]||'integration-failed'};
  if(activity!=='ready')return null;
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
export function createLocation(terminal,onLocation=()=>{}){
  let mount,host='',kind='',frameOffset=0,report,integration={},observed=false,confirmed=false,disposed=false,invalid=false,handshake;
  const emit=value=>{const record={...value,terminalId:mount.data.streamId,title:mount.data.title,shell:integration.shell,outputOffset:String(frameOffset),observedAt:Date.now()};onLocation(record);return record;};
  const handler=terminal.parser.registerOscHandler(7,text=>{if(!invalid&&kind==='data'&&integration.mode==='manual')report=parseOsc7(text,host)||{state:'unknown',path:null,reasonCode:'invalid-report'};return true;});
  const automatic=terminal.parser.registerOscHandler(633,text=>{
    if(!invalid&&kind==='data'&&integration.mode==='auto'){
      const parsed=parseIntegration(text,integration.token);
      if(parsed){observed=true;if(parsed.state==='known')confirmed=true;if(parsed.activity==='unavailable')confirmed=false;clearTimeout(handshake);report=parsed;return parsed.activity!=='unavailable';}
    }
    return false;
  });
  return {
    mount(value){mount=value;host=value.settings?.values?.directoryReportHost||'';integration=value.data.integration||{mode:'manual'};observed=false;confirmed=false;invalid=false;clearTimeout(handshake);onLocation({state:'unknown'});},
    restore(value){if(!observed&&value?.terminalId===mount.data.streamId&&integration.mode!=='off'){confirmed=value.state==='known';observed=true;onLocation(value,{restored:true});}},
    settings(value){const next=value?.values?.directoryReportHost||'';if(integration.mode==='manual'&&next!==host){host=next;report=null;confirmed=false;emit({state:'unknown',path:null,reason:'目录报告配置已更换'});}},
    frame(type,offset){kind=type;frameOffset=offset;report=null;},
    completed(){if(!invalid&&report&&Number.isSafeInteger(frameOffset)){observed=true;emit(report);}report=null;},
    ready(){if(observed||invalid||disposed)return;emit({state:'unknown',path:null,activity:'starting',reasonCode:integration.mode==='auto'?'starting':integration.mode==='off'?'disabled':integration.mode==='manual'?'manual-setup':integration.reasonCode||'integration-failed'});clearTimeout(handshake);if(integration.mode==='auto')handshake=setTimeout(()=>{if(!observed&&!disposed&&!invalid)emit({state:'unknown',path:null,activity:'unavailable',reasonCode:'handshake-timeout'});},8000);},
    input(text){if(!invalid&&integration.mode==='auto'&&confirmed&&/[\r\n\x03\x04]/.test(text))emit({state:'unknown',path:null,activity:'busy',reasonCode:'busy'});},
    unknown(offset,reason){if(Number.isSafeInteger(offset)){frameOffset=offset;emit({state:'unknown',path:null,reason});}},
    reset(){invalid=false;observed=false;report=null;clearTimeout(handshake);onLocation({state:'unknown'});},
    invalidate(){invalid=true;report=null;clearTimeout(handshake);onLocation({state:'unknown'});},
    dispose(){disposed=true;invalid=true;clearTimeout(handshake);handler.dispose();automatic.dispose();},
  };
}
