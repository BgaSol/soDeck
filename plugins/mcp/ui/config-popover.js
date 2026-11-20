import {setText} from './task-view.js';
export function createConfigPopover({request,host,owner}) {
  const $=id=>document.getElementById(id), popup=$('config-popover'), trigger=$('config-toggle');
  let generation=0, keySequence=0, settingsRead=0, snapshot, endpoint='', dirty=false, conflict=false, saving=false, ticket, animation;
  const current=(context,seq)=>owner()===context && context!==null && generation===seq && !popup.hidden;
  function message(text) {setText($('config-notice'),text);}
  function paint() {
    $('port').disabled=!snapshot || saving;$('save-port').disabled=!snapshot || !dirty || conflict || saving;$('reload-port').disabled=saving;$('reload-port').hidden=!conflict;
    const port=endpoint?new URL(endpoint).port:'';
    const value=conflict?'其他窗口已更新端口，草稿已保留。请载入最新值后再编辑。':snapshot && String(snapshot.values.port)!==port?'端口已保存，重启 SoDeck 后生效；之后需重新复制客户端配置。':'';
    setText($('port-notice'),value);$('port-notice').hidden=!value;
  }
  function settings(value) {
    if(!value || (snapshot?.definitionToken===value.definitionToken && BigInt(snapshot.revision)>BigInt(value.revision)))return;
    if(dirty && snapshot && (snapshot.revision!==value.revision || snapshot.definitionToken!==value.definitionToken))conflict=true;
    snapshot=value;if(!dirty)$('port').value=value.values.port;paint();
  }
  async function readSettings() {
    const context=owner(),seq=++settingsRead;if(context===null)return;
    try {const result=await host('settings',{op:'get'});if(context===owner() && seq===settingsRead)settings(result);}
    catch(error) {if(context===owner() && seq===settingsRead){setText($('port-notice'),'端口设置读取失败：'+(error.message || error));$('port-notice').hidden=false;}}
  }
  function close(restore=false) {
    generation++;keySequence++;animation?.cancel();popup.hidden=true;trigger.setAttribute('aria-expanded','false');
    setText($('config'),'');setText($('access-token'),'••••••••••••••••');setText($('show-token'),'显示');$('show-token').setAttribute('aria-expanded','false');$('settings').open=false;message('');
    if(restore)trigger.focus({preventScroll:true});
  }
  async function open() {
    if(owner()===null)return;
    close();const context=owner(),seq=generation;popup.hidden=false;trigger.setAttribute('aria-expanded','true');$('server-info').open=false;
    setText($('copy'),'复制');$('copy').disabled=true;setText($('config'),'正在读取配置…');$('copy').focus({preventScroll:true});
    if(!matchMedia('(prefers-reduced-motion: reduce)').matches)animation=popup.animate([{opacity:0,transform:'translateY(-5px) scale(.98)'},{opacity:1,transform:'none'}],{duration:160,easing:'cubic-bezier(.16,1,.3,1)'});
    void readSettings();
    try {const data=await request({action:'config'});if(current(context,seq)){setText($('config'),data.config);$('copy').disabled=false;$('copy').focus({preventScroll:true});}}
    catch(error) {if(current(context,seq)){setText($('config'),'');message('配置读取失败：'+(error.message || error));}}
  }
  trigger.onclick=()=>{if(popup.hidden)void open();else close(true);};$('empty-config').onclick=()=>void open();
  $('copy').onclick=async()=>{
    const context=owner(),seq=generation;const value=$('config').textContent;$('copy').disabled=true;
    try {const copied=await sodeckClipboard.writeText(value,()=>current(context,seq));if(current(context,seq)){message(copied?'配置已复制':'配置未复制，请重试。');if(copied)setText($('copy'),'已复制');}}
    catch(error){if(current(context,seq))message('复制失败：'+(error.message || error));}
    finally {if(current(context,seq))$('copy').disabled=false;}
  };
  $('show-token').onclick=async()=>{
    const show=$('show-token').getAttribute('aria-expanded')!=='true', key=++keySequence, context=owner(),seq=generation;
    $('show-token').setAttribute('aria-expanded',String(show));setText($('show-token'),show?'隐藏':'显示');setText($('access-token'),'••••••••••••••••');if(!show)return;
    try {const value=await request({action:'config'});if(current(context,seq) && key===keySequence)setText($('access-token'),value.token);}
    catch(error){if(current(context,seq) && key===keySequence)message('密钥读取失败：'+(error.message || error));}
  };
  $('settings').ontoggle=()=>{if(!$('settings').open){keySequence++;setText($('access-token'),'••••••••••••••••');$('show-token').setAttribute('aria-expanded','false');setText($('show-token'),'显示');}};
  $('port').oninput=()=>{dirty=Boolean(snapshot && String(snapshot.values.port)!==$('port').value);ticket=null;paint();};
  $('reload-port').onclick=()=>{dirty=conflict=false;ticket=null;if(snapshot)settings(snapshot);};
  $('save-port').onclick=async()=>{
    if(!snapshot || saving || conflict || !dirty || !$('port').reportValidity())return;
    ticket ??= {requestId:crypto.randomUUID(),definitionToken:snapshot.definitionToken,baseRevision:snapshot.revision,values:{...snapshot.values,port:Number($('port').value)}};
    const context=owner();saving=true;paint();
    try {
      const result=await host('settings',{op:'save',...ticket});if(context!==owner())return;
      if(result.conflict)conflict=true;
      else if(result.persisted){dirty=false;ticket=null;message('端口已保存，重启 SoDeck 后生效。');}
      else throw new Error('端口未保存，请重试。');
      settings(result.snapshot);
    } catch(error){if(context===owner())message('端口保存失败：'+(error.message || error));}
    finally {saving=false;paint();}
  };
  const outside=event=>{if(!popup.hidden && !popup.contains(event.target) && !trigger.contains(event.target) && !$('empty-config').contains(event.target))close();};
  const escape=event=>{if(event.key==='Escape' && !popup.hidden){event.preventDefault();close(true);}};
  document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);
  return {open,close,settings,readSettings,update(value){endpoint=value;setText($('endpoint'),value);paint();},stop(){close();settingsRead++;},destroy(){close();settingsRead++;document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);}};
}

