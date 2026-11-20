function setText(el,value){value=String(value ?? '');if(el.textContent!==value)el.textContent=value;}
export function createConfigPopover({request,host,owner}){
  const $=id=>document.getElementById(id),popup=$('config-popover'),trigger=$('config-toggle'),motion=matchMedia('(prefers-reduced-motion: reduce)');
  let isOpen=false,disposed=false,generation=0,keySequence=0,settingsRead=0,snapshot,endpoint='',dirty=false,conflict=false,saving=false,ticket,animation,configReady=false,copyState='idle';
  const current=(context,seq)=>!disposed && isOpen && owner()===context && context!==null && generation===seq;
  function message(value){setText($('config-notice'),value);$('config-notice').hidden=!value;}
  function copyStatus(value){
    copyState=value;
    setText($('copy'),{idle:'复制',copying:'复制中…',copied:'已复制',failed:'重试复制'}[value]);
    $('copy').disabled=!configReady;$('copy').setAttribute('aria-disabled',String(!configReady || value==='copying'));$('copy').setAttribute('aria-busy',String(value==='copying'));
  }
  function clearSecrets(){
    configReady=false;copyStatus('idle');keySequence++;setText($('config'),'');setText($('access-token'),'••••••••••••••••');
    setText($('show-token'),'显示');$('show-token').setAttribute('aria-expanded','false');$('settings').open=false;message('');
  }
  function finishMotion(){
    animation?.cancel();animation=null;popup.style.height='';popup.hidden=!isOpen;
  }
  function transition(from,to,duration){
    const value=popup.animate([from,to],{duration,easing:'cubic-bezier(.16,1,.3,1)',fill:'both'});animation=value;
    value.finished.then(()=>{if(animation!==value)return;animation=null;popup.hidden=!isOpen;popup.style.height='';value.cancel();}).catch(()=>{});
  }
  function close(restore=false,animated=true){
    const animate=animated && isOpen && owner()!==null && !motion.matches;
    const box=animate?popup.getBoundingClientRect():null,style=animate?getComputedStyle(popup):null;
    const from=style?{opacity:style.opacity,transform:style.transform}:null;
    isOpen=false;generation++;finishMotion();popup.inert=true;popup.setAttribute('aria-hidden','true');trigger.setAttribute('aria-expanded','false');
    if(restore)trigger.focus({preventScroll:true});
    else if(popup.contains(document.activeElement))document.activeElement.blur();
    clearSecrets();
    if(animate){popup.hidden=false;popup.style.height=box.height+'px';transition(from,{opacity:0,transform:'translateY(-4px) scale(.98)'},100);}
  }
  async function open(){
    const context=owner();if(context===null || disposed || isOpen)return;
    const style=popup.hidden?null:getComputedStyle(popup),from=style?{opacity:style.opacity,transform:style.transform}:{opacity:0,transform:'translateY(-5px) scale(.98)'};
    finishMotion();isOpen=true;const seq=++generation;clearSecrets();
    popup.hidden=false;popup.inert=false;popup.removeAttribute('aria-hidden');trigger.setAttribute('aria-expanded','true');$('server-info').open=false;
    setText($('config'),'正在读取配置…');popup.tabIndex=-1;popup.focus({preventScroll:true});
    if(!motion.matches)transition(from,{opacity:1,transform:'none'},160);
    void readSettings();
    try {
      const data=await request({action:'config'});
      if(current(context,seq)){setText($('config'),data.config);configReady=true;copyStatus('idle');if(document.activeElement===popup)$('copy').focus({preventScroll:true});}
    }catch(error){if(current(context,seq)){setText($('config'),'');message('配置读取失败：'+(error.message || error));}}
  }
  function paint(){
    if(disposed)return;
    $('port').disabled=!snapshot || saving;$('save-port').disabled=!snapshot || !dirty || conflict || saving;$('reload-port').disabled=saving;$('reload-port').hidden=!conflict;
    const port=endpoint?new URL(endpoint).port:'';
    const value=conflict?'其他窗口已更新端口，草稿已保留。请载入最新值后再编辑。':snapshot && String(snapshot.values.port)!==port?'端口已保存，重启 SoDeck 后生效；之后需重新复制客户端配置。':'';
    setText($('port-notice'),value);$('port-notice').hidden=!value;
  }
  function settings(value){
    if(disposed || !value || (snapshot?.definitionToken===value.definitionToken && BigInt(snapshot.revision)>BigInt(value.revision)))return;
    if(dirty && snapshot && (snapshot.revision!==value.revision || snapshot.definitionToken!==value.definitionToken))conflict=true;
    snapshot=value;if(!dirty)$('port').value=value.values.port;paint();
  }
  async function readSettings(){
    const context=owner(),seq=++settingsRead;if(context===null || disposed)return;
    try{const result=await host('settings',{op:'get'});if(context===owner() && seq===settingsRead)settings(result);}
    catch(error){if(!disposed && context===owner() && seq===settingsRead){setText($('port-notice'),'端口设置读取失败：'+(error.message || error));$('port-notice').hidden=false;}}
  }
  trigger.onclick=()=>{if(isOpen)close(true);else void open();};$('empty-config').onclick=()=>void open();
  $('copy').onclick=async()=>{
    if(!isOpen || !configReady || copyState==='copying')return;
    const context=owner(),seq=generation,value=$('config').textContent;copyStatus('copying');message('');
    try{
      const copied=await sodeckClipboard.writeText(value,()=>current(context,seq));
      if(current(context,seq)){copyStatus(copied?'copied':'failed');if(!copied)message('配置未复制，请重试。');}
    }catch(error){if(current(context,seq)){copyStatus('failed');message('复制失败：'+(error.message || error));}}
  };
  $('show-token').onclick=async()=>{
    const show=$('show-token').getAttribute('aria-expanded')!=='true',key=++keySequence,context=owner(),seq=generation;
    $('show-token').setAttribute('aria-expanded',String(show));setText($('show-token'),show?'隐藏':'显示');setText($('access-token'),'••••••••••••••••');if(!show)return;
    try{const value=await request({action:'config'});if(current(context,seq) && key===keySequence)setText($('access-token'),value.token);}
    catch(error){if(current(context,seq) && key===keySequence)message('密钥读取失败：'+(error.message || error));}
  };
  $('settings').ontoggle=()=>{if(!$('settings').open){keySequence++;setText($('access-token'),'••••••••••••••••');$('show-token').setAttribute('aria-expanded','false');setText($('show-token'),'显示');}};
  $('port').oninput=()=>{dirty=Boolean(snapshot && String(snapshot.values.port)!==$('port').value);ticket=null;paint();};
  $('reload-port').onclick=()=>{dirty=conflict=false;ticket=null;if(snapshot)settings(snapshot);};
  $('save-port').onclick=async()=>{
    if(!snapshot || saving || conflict || !dirty || !$('port').reportValidity())return;
    ticket ??= {requestId:crypto.randomUUID(),definitionToken:snapshot.definitionToken,baseRevision:snapshot.revision,values:{...snapshot.values,port:Number($('port').value)}};
    const context=owner();saving=true;paint();
    try{
      const result=await host('settings',{op:'save',...ticket});if(context!==owner() || disposed)return;
      if(result.conflict)conflict=true;
      else if(result.persisted){dirty=false;ticket=null;message('端口已保存，重启 SoDeck 后生效。');}
      else throw new Error('端口未保存，请重试。');
      settings(result.snapshot);
    }catch(error){if(!disposed && context===owner())message('端口保存失败：'+(error.message || error));}
    finally{saving=false;paint();}
  };
  const outside=event=>{if(isOpen && !popup.contains(event.target) && !trigger.contains(event.target) && !$('empty-config').contains(event.target))close();};
  const escape=event=>{if(event.key==='Escape' && isOpen){event.preventDefault();close(true);}};
  document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);motion.addEventListener('change',finishMotion);
  return {
    open,close,settings,readSettings,finishMotion,
    update(value){endpoint=value;setText($('endpoint'),value);paint();},
    stop(){close(false,false);settingsRead++;},
    destroy(){close(false,false);disposed=true;settingsRead++;document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);motion.removeEventListener('change',finishMotion);}
  };
}

