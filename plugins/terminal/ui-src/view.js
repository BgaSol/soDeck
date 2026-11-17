import {createAppearance} from './appearance.js';
import {createLocation} from './location.js';
import {createSearch} from './search.js';
import {snapshotChunks} from './snapshot.js';
(() => {
  const host=document.querySelector('#terminal'),status=document.querySelector('#status');
  const terminal=new Terminal({allowProposedApi:true,cursorBlink:true,fontSize:14,lineHeight:1.2,scrollback:100000,convertEol:false});
  const fit=new FitAddon.FitAddon(),serialize=new SerializeAddon.SerializeAddon();terminal.loadAddon(fit);terminal.loadAddon(serialize);
  let id,offset=0,inputSequence=0,chain=Promise.resolve(),inputChain=Promise.resolve(),disposed=false,caughtUp=false,restoring=false,visible=false,enabled=false;
  let savedOffset=0,lastSave=0,escape='normal',utf8=0,inputPending=0,mountKey='';
  let transferFrozen=false,readOnly=false,inputError,restoredDisplay;
  let ended=false,disconnected=false,opening=false,surfaceReady=false,createParams={},canOpen=false,commandsApi=false;
  let snapshotJob,commandTimer,commandPending=false;
  let runsApi=false,runState,sourceRevision=0,mountValue,startup,startTimer,startRequest=false,displayError;
  const loginShell=()=>createParams.launch?.kind!=='command';
  const inputReady=()=>caughtUp&&surfaceReady&&visible&&!readOnly&&!transferFrozen&&!disconnected&&!ended&&!inputError&&(!runsApi||(runState?.state==='running'&&offset>=Number(runState.readyOffset)&&JSON.stringify(runState.context)===JSON.stringify(mountValue.context)));
  const recoverable=()=>runsApi&&loginShell()&&(ended||(runState?.state==='idle'&&startup?.status==='failed'))&&!opening&&startup?.status!=='pending'&&surfaceReady&&visible&&!readOnly&&!transferFrozen&&!disposed;
  function recoveryControls(){
    if(!runsApi)return;
    newTerminal.hidden=loginShell()?!(ended||disconnected||startup?.status==='failed'):!ended;
    newTerminal.disabled=loginShell()?!recoverable():disconnected||opening||commandPending||readOnly||transferFrozen||disposed;
    if(loginShell())newTerminal.textContent=disconnected?'重新连接':'恢复终端';
    let text='',error=false;
    if(recoveryUnavailable){text=recoveryNotice;error=true;}
    else if(displayError){text=displayError;error=true;}
    else if(startup?.status==='pending')text=startup.phase==='connection'?'正在连接服务器…':startup.phase==='preparing'?'正在准备终端…':'正在打开终端…';
    else if(startup?.status==='failed'){text=startup.error+(loginShell()?' · 按回车重试':'');error=true;}
    else if(disconnected)text='连接已断开'+(loginShell()?' · 按回车重新连接':' · 连接后可重新打开终端');
    else if(ended){text=(runState.result?.error||'终端已退出')+(loginShell()?' · 按回车恢复':'');error=Boolean(runState.result?.error);}
    else if(!caughtUp||runState?.state!=='running')text='正在接续终端…';
    status.hidden=!text;status.textContent=text;status.classList.toggle('error',error);
  }
  function scheduleStart(){
    if(startTimer||startRequest||!runsApi||startup?.status!=='pending'||disposed||!surfaceReady||readOnly||transferFrozen||!visible||!caughtUp)return;
    startTimer=setTimeout(()=>{
      startTimer=undefined;
      if(disposed||!surfaceReady||readOnly||transferFrozen||!visible||startup?.status!=='pending')return;
      startRequest=true;
      void sodeck.request('event',{action:'continueStart',key:startup.key}).then(startupResult).catch(error=>{if(!disposed)fail(error);}).finally(()=>{startRequest=false;scheduleStart();});
    },250);
  }
  function startupResult(result){
    if(disposed)return;
    if(result?.startup){startup=result.startup;if(!inputError)displayError=undefined;}
    recoveryControls();scheduleStart();
  }
  function recoverTerminal(){
    if(!recoverable())return;
    opening=true;displayError=undefined;controls.invalidate();enabled=false;recoveryControls();
    void sodeck.request('event',{action:'beginRecovery',requestId:crypto.randomUUID(),expectedRunId:runState?.runId??null,cols:terminal.cols,rows:terminal.rows})
      .then(startupResult).catch(error=>{fail(error);void sodeck.request('event',{action:'startupStatus'}).then(startupResult).catch(()=>{});})
      .finally(()=>{opening=false;recoveryControls();scheduleStart();});
  }
  function sourceState(frame){
    const revision=frame.metadata?.revision;if(!Number.isSafeInteger(revision)||revision<=sourceRevision)return;
    sourceRevision=revision;
    const next=frame.metadata.state;
    if(next.runId&&next.runId!==runState?.runId){
      enabled=false;controls.invalidate();stopSize();inputSequence=0;inputError=displayError=undefined;
      mountKey=mountValue.surfaceId+':'+JSON.stringify(mountValue.context)+':'+next.runId;
      location.mount({...mountValue,data:{...mountValue.data,openedContext:next.context,integration:next.metadata?.integration||{mode:'off'}}});
    }
    runState=next;ended=next.state==='ended';
    if(ended){
      controls.invalidate();stopSize();location.unknown(offset,'终端已退出');
    }else if(next.state==='running'&&surfaceReady&&JSON.stringify(next.context)===JSON.stringify(mountValue.context)){
      location.ready();
    }
    enabled=inputReady();recoveryControls();if(enabled){void enqueueSize(true).catch(fail);}scheduleStart();
  }
  async function commandStatus(result) {
    if(disposed)return;
    clearTimeout(commandTimer);
    commandPending=result?.status==='pending';
    newTerminal.disabled=disconnected||commandPending;
    if(commandPending){status.hidden=false;status.textContent='终端请求待确认';clearTimeout(commandTimer);commandTimer=setTimeout(()=>{if(!disposed&&surfaceReady&&!readOnly&&!transferFrozen)void sodeck.request('event',{action:'terminalCommandStatus'}).then(commandStatus).catch(error=>{fail(error);commandTimer=setTimeout(()=>commandStatus({status:'pending'}),1000);});else commandStatus({status:'pending'});},1000);}
    else if(result?.error)fail(result.error.message||result.error);
    else if(result?.status==='completed'){status.hidden=false;status.textContent='终端页面已创建';}
  }
  let recoveryUnavailable=false;const recoveryNotice='当前终端可继续使用，历史恢复暂不可用；请保留此页面';
  const newTerminal=document.createElement('button');newTerminal.textContent='新建终端';newTerminal.hidden=true;newTerminal.className='sd-control';status.after(newTerminal);
  newTerminal.onclick=()=>{
    if(runsApi&&loginShell()){recoverTerminal();return;}
    if(opening||commandPending||readOnly||transferFrozen||disposed||disconnected||!ended)return;
    opening=true;newTerminal.disabled=true;
    const requestId=crypto.randomUUID();
    void sodeck.request(canOpen?'event':'openView',canOpen?{action:'createTerminal',requestId,params:createParams}:{viewType:'terminal',params:createParams}).then(commandStatus).catch(error=>{fail(error);if(commandsApi)void commandStatus({status:'pending'});}).finally(()=>{opening=false;newTerminal.disabled=disconnected||commandPending;});
  };
  SoDeckMotion.observeMotion(document);
  const encoder=new TextEncoder();
  const fail=error=>{displayError=String(error);if(runsApi)recoveryControls();else{status.hidden=false;status.textContent=displayError;status.classList.add('error');}};
  const controls=installTerminalInput(terminal,host,()=>({key:mountKey,canCopy:!disposed&&!restoring,canPaste:enabled&&!restoring&&!disposed&&visible,canRecover:recoverable()}),fail,()=>search.show(),recoverTerminal);
  const appearance=createAppearance(terminal,host,()=>caughtUp&&!restoring&&visible&&!disposed,()=>enqueueSize(true),()=>controls.invalidate());
  const location=createLocation(terminal);
  let lastCols=0,lastRows=0;
  let sizeFrame,sizeJob,sizeDone,sizeReject,desiredSize,resizeQueued=false,sizeEpoch=0,resizeFailed=false,resizeRetries=0,retryTimer;
  const request=p=>sodeck.request('data',{id,...p,...(runsApi&&['write','resize'].includes(p.op)?{runId:runState?.runId}: {})});
  const write=bytes=>new Promise(resolve=>terminal.write(bytes,resolve));
  const search=createSearch(terminal,host,metadata=>{
    if(disposed||!surfaceReady||readOnly||restoring||!caughtUp)return Promise.resolve();
    return request({op:'metadata',metadata}).catch(error=>{if(!String(error).includes('窗口'))fail(error);});
  },()=>({ready:caughtUp&&!restoring&&!disposed,visible,frozen:transferFrozen}));
  async function resumeDisplay(){enabled=inputReady();await appearance.apply();await enqueueSize(true);if(!disposed)search.resume();recoveryControls();}
  function scan(bytes){for(const b of bytes){
    if(utf8){if((b&0xc0)===0x80){utf8--;continue;}utf8=0;}
    if(b>=0xc2){utf8=b<0xe0?1:b<0xf0?2:3;continue;}
    if(b===24||b===26){escape='normal';continue;}
    if(escape==='string'){if(b===7)escape='normal';else if(b===27)escape='stringEscape';}
    else if(escape==='stringEscape')escape=b===92?'normal':'string';
    else if(b===27)escape='escape';
    else if(escape==='escape'){if(b===91)escape='csi';else if([93,80,94,95,88].includes(b))escape='string';else if(b>=0x30&&b<=0x7e)escape='normal';}
    else if(escape==='csi'&&b>=0x40&&b<=0x7e)escape='normal';
  }}
  async function checkpoint(force=false){
    if(snapshotJob){if(!force)return;await snapshotJob;}
    if(force)await search.flush();
    if(disposed||!surfaceReady||restoring||!caughtUp||offset===savedOffset||escape!=='normal'||utf8)return;
    if(!force&&performance.now()-lastSave<10000)return;
    const watermark=offset;
    const metadata={cols:terminal.cols,rows:terminal.rows,viewportY:terminal.buffer.active.viewportY,selection:terminal.getSelectionPosition()};
    snapshotJob=(async()=>{
      let began=false,size=0,used=0;const pending=new Uint8Array(16384);
      const upload=async(done=false)=>{if(disposed||!surfaceReady||readOnly)throw new Error('终端窗口已更换');await request({op:'checkpoint',offset:watermark,data:Array.from(pending.subarray(0,used)),metadata,begin:!began,done});began=true;used=0;};
      for await(const text of snapshotChunks(terminal,serialize)){
        const bytes=encoder.encode(text);size+=bytes.length;if(size>128*1024*1024)throw new Error('终端历史超过恢复容量，当前页面仍可使用');
        for(let pos=0;pos<bytes.length;){const amount=Math.min(16384-used,bytes.length-pos);pending.set(bytes.subarray(pos,pos+amount),used);used+=amount;pos+=amount;if(used===16384)await upload();}
      }
      await upload(true);
      savedOffset=watermark;lastSave=performance.now();
    })();
    try{await snapshotJob;}finally{snapshotJob=undefined;if(!disposed)void enqueueSize().catch(()=>{});}
  }
  function enqueueSize(retry=false){
    if(disposed)return Promise.resolve();
    if(retry){resizeFailed=false;resizeRetries=0;}
    if(sizeJob)return sizeJob;
    sizeJob=new Promise((resolve,reject)=>{sizeDone=resolve;sizeReject=reject;});
    sizeFrame=requestAnimationFrame(()=>{
      const done=sizeDone,reject=sizeReject;sizeFrame=sizeJob=sizeDone=sizeReject=undefined;
      try{
        if(visible&&caughtUp&&!restoring&&!readOnly&&!transferFrozen&&!snapshotJob&&!disposed&&host.clientWidth>=20&&host.clientHeight>=20){
          fit.fit();const cols=Math.min(1000,terminal.cols),rows=Math.min(500,terminal.rows);
          if(cols!==terminal.cols||rows!==terminal.rows)terminal.resize(cols,rows);
          if(desiredSize?.cols!==cols||desiredSize?.rows!==rows){resizeFailed=false;resizeRetries=0;}
          desiredSize={cols,rows};queueResize();
        }
        done();
      }catch(error){reject(error);}
    });
    return sizeJob;
  }
  function queueResize(){
    if(resizeQueued||retryTimer||resizeFailed||!desiredSize||!enabled||readOnly||transferFrozen||!visible||restoring||disposed||disconnected||(desiredSize.cols===lastCols&&desiredSize.rows===lastRows))return;
    resizeQueued=true;const epoch=sizeEpoch;
    // One replaceable queue position preserves writes while fit stays local to the next frame.
    inputChain=inputChain.then(async()=>{
      if(epoch!==sizeEpoch)return;
      resizeQueued=false;
      if(!enabled||readOnly||transferFrozen||!visible||restoring||disposed||disconnected||!desiredSize)return;
      const target=desiredSize;if(target.cols===lastCols&&target.rows===lastRows)return;
      await request({op:'resize',...target});
      if(epoch!==sizeEpoch||disposed)return;
      lastCols=target.cols;lastRows=target.rows;resizeRetries=0;
    }).catch(error=>{
      if(epoch!==sizeEpoch||disposed)return;
      if(String(error).includes('忙碌')&&resizeRetries++<3)retryTimer=setTimeout(()=>{retryTimer=undefined;queueResize();},40*2**resizeRetries);
      else{resizeFailed=true;fail(error);}
    }).finally(()=>{if(epoch===sizeEpoch)queueResize();});
  }
  function stopSize(){
    sizeEpoch++;if(sizeFrame!==undefined)cancelAnimationFrame(sizeFrame);clearTimeout(retryTimer);
    sizeDone?.();sizeFrame=sizeJob=sizeDone=sizeReject=retryTimer=desiredSize=undefined;
    resizeQueued=false;lastCols=lastRows=0;resizeFailed=false;resizeRetries=0;
  }
  const observer=new ResizeObserver(()=>{if(caughtUp)void enqueueSize().catch(fail);});observer.observe(host);
  function theme(tokens){
    Object.entries(tokens||{}).forEach(([k,v])=>document.documentElement.style.setProperty('--'+k,v));
    const palette={background:tokens?.['terminal-background']||tokens?.panel||'#fff',foreground:tokens?.['terminal-foreground']||tokens?.text||'#222',cursor:tokens?.['terminal-cursor']||tokens?.accent||'#5346a6',selectionBackground:tokens?.['terminal-selection']||tokens?.['accent-bg']||'#dad3f3',selectionForeground:tokens?.['terminal-foreground']||tokens?.text||'#222'};
    for(const name of ['black','red','green','yellow','blue','magenta','cyan','white','brightBlack','brightRed','brightGreen','brightYellow','brightBlue','brightMagenta','brightCyan','brightWhite'])if(tokens?.['terminal-'+name])palette[name]=tokens['terminal-'+name];
    palette.scrollbarSliderBackground=tokens?.['sd-scrollbar-thumb']||'#a7adbb';
    palette.scrollbarSliderHoverBackground=tokens?.['sd-scrollbar-hover']||'#818a9b';
    palette.scrollbarSliderActiveBackground=tokens?.['sd-scrollbar-active']||tokens?.accent||'#7661d1';
    terminal.options.theme=palette;controls.themeChanged();
    search?.theme();
  }
  addEventListener('sodeck:mount',event=>{
    const mount=event.detail;mountValue=mount;runsApi=mount.data.runsApi===true;startup=mount.data.startup;readOnly=mount.readOnly===true;surfaceReady=mount.capabilities?.sharedState!==1;id=mount.data.streamId;mountKey=mount.surfaceId+':'+JSON.stringify(mount.context);disconnected=mount.connected===false;visible=mount.visible;theme(mount.tokens);
    terminal.open(host);
    commandsApi=mount.data.commandsApi===true;createParams=mount.data.createParams||{};canOpen=mount.capabilities?.uiOpenView===1;newTerminal.textContent=createParams.launch?.kind==='command'?'重新打开终端':'新建终端';if(mount.data.commandPending)void commandStatus({status:'pending'});
    appearance.receive(mount.settings);
    location.mount(mount);
    status.textContent=`${mount.serverName||'服务器'} · 正在接续终端…`;
    void request({op:'attach'}).catch(fail);recoveryControls();
  });
  addEventListener('sodeck:surfaceReady',()=>{readOnly=false;transferFrozen=false;surfaceReady=true;enabled=caughtUp&&inputReady();void resumeDisplay().catch(fail);if(enabled)location.ready();scheduleStart();});
  addEventListener('sodeck:data',event=>{
    chain=chain.then(async()=>{
      if(disposed)return;
      const frame=event.detail.data;
      location.frame(frame.kind,frame.offset);
      if(frame.kind==='error'){location.unknown(offset,'终端输出不可用');fail(frame.metadata);return;}
      if(frame.kind==='recovery'){recoveryUnavailable=frame.metadata?.available===false;if(recoveryUnavailable)fail(recoveryNotice);else if(status.textContent===recoveryNotice){status.hidden=true;status.classList.remove('error');}}
      if(frame.kind==='reset'){location.invalidate();controls.invalidate();stopSize();restoring=true;enabled=false;terminal.reset();offset=frame.offset;savedOffset=offset;const d=frame.metadata;restoredDisplay=d;if(d?.cols&&d?.rows)terminal.resize(d.cols,d.rows);}
      if(frame.kind==='snapshot')await write(Uint8Array.from(frame.data));
      if(frame.kind==='restore')restoring=false;
      if(frame.kind==='data'){const bytes=Uint8Array.from(frame.data);await write(bytes);scan(bytes);offset=frame.offset;location.completed();}
      await request({op:'ack',packet:frame.packet});
      if(frame.kind==='caughtUp'){
        caughtUp=true;enabled=inputReady();if(!runsApi){status.textContent=recoveryUnavailable?recoveryNotice:'远程终端 · 已连接';status.hidden=!recoveryUnavailable;status.classList.toggle('error',recoveryUnavailable);}document.body.dataset.ready='true';
        // Incoming windows may render before ownership commits. Only the owner saves a checkpoint.
        search.update(true);sodeck.mounted(true);if(restoredDisplay?.ui?.atBottom)terminal.scrollToBottom();else if(restoredDisplay?.ui?.viewportY!=null)terminal.scrollToLine(restoredDisplay.ui.viewportY);else if(restoredDisplay?.viewportY!=null)terminal.scrollToLine(restoredDisplay.viewportY);const selection=restoredDisplay?.ui?.selection!==undefined?restoredDisplay.ui.selection:restoredDisplay?.selection;if(selection){const p=selection;terminal.select(p.start.x,p.start.y,(p.end.y-p.start.y)*terminal.cols+p.end.x-p.start.x);}search.restore(restoredDisplay?.ui);restoredDisplay=undefined;if(surfaceReady){void appearance.apply();void enqueueSize().catch(()=>{});}
        if(surfaceReady&&!disconnected&&!ended&&(!runsApi||runState?.state==='running'))location.ready();startupResult({startup});
      }
      if(frame.kind==='source'&&runsApi)sourceState(frame);
      if(frame.kind==='end'){location.unknown(offset,'终端已退出');controls.invalidate();enabled=false;ended=true;stopSize();void enqueueSize().catch(()=>{});status.hidden=false;status.textContent=disconnected?'连接已断开，请在工作台重新连接后新建终端。':frame.metadata?.error||`终端已退出${frame.metadata?.exitCode==null?'':` · ${frame.metadata.exitCode}`}`;newTerminal.hidden=disconnected;}
      if(runsApi&&frame.kind==='data')enabled=inputReady();
    }).catch(fail);
  });
  function input(text){if(transferFrozen||readOnly||!visible||!enabled||restoring||disposed)return;const key=mountKey,bytes=encoder.encode(text);if(inputPending+bytes.length>256*1024){fail('输入待发送内容超过 256 KiB，请稍后分段粘贴；本次输入未发送');return;}inputPending+=bytes.length;location.input(text);inputChain=inputChain.then(async()=>{for(let i=0;i<bytes.length;i+=16384){if(disposed||!visible||!enabled||restoring||mountKey!==key)return;await request({op:'write',sequence:inputSequence,data:Array.from(bytes.subarray(i,i+16384))});inputSequence++;}}).catch(error=>{if(!disposed&&mountKey===key){enabled=false;inputError=error;fail(error);}}).finally(()=>{inputPending-=bytes.length;});}
  terminal.onData(input);
  addEventListener('sodeck:flush',async event=>{
    try {
      if(controls.composing()||search.composing())throw new Error('中文输入仍在组合，请完成输入后再移动');
      transferFrozen=event.detail.freeze===true;if(transferFrozen){stopSize();search.pause();}await inputChain;await chain;
      if(inputError)throw inputError;
      await checkpoint(true);
      parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:event.detail.surfaceId,id:event.detail.id},'*');
    }catch(error){if(transferFrozen){transferFrozen=false;void resumeDisplay().catch(fail);}parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:event.detail.surfaceId,id:event.detail.id,error:String(error)},'*');}
  });
  addEventListener('sodeck:transferCancelled',()=>{transferFrozen=false;void resumeDisplay().catch(fail);scheduleStart();});
  addEventListener('sodeck:visibilityChanged',event=>{visible=event.detail.visible;if(!visible){controls.invalidate();stopSize();search.pause();}if(visible){void resumeDisplay().catch(fail);scheduleStart();}else{clearTimeout(startTimer);startTimer=undefined;}});
  addEventListener('sodeck:settingsChanged',event=>{
    const next=event.detail.settings,current=mountValue.settings;
    if(current&&current.definitionToken===next.definitionToken&&BigInt(current.revision)>BigInt(next.revision))return;
    mountValue.settings=next;appearance.receive(next);location.settings(next);
  });
  addEventListener('sodeck:contextChanged',()=>location.invalidate());
  addEventListener('sodeck:connectionChanged',event=>{if(!event.detail.connected)location.invalidate();});
  addEventListener('sodeck:contextChanged',event=>{controls.invalidate();enabled=false;stopSize();if(runsApi){mountValue={...mountValue,context:event.detail.context};recoveryControls();scheduleStart();}else{newTerminal.hidden=disconnected||!ended;status.hidden=false;status.textContent='连接已更换，请在工作台新建终端；此页保留原输出。';}});
  addEventListener('sodeck:connectionChanged',event=>{disconnected=!event.detail.connected;if(disconnected){controls.invalidate();enabled=false;stopSize();if(!runsApi){newTerminal.hidden=true;status.hidden=false;status.textContent='连接已断开，请在工作台重新连接；此页保留原输出。';}}else if(ended){newTerminal.hidden=false;newTerminal.disabled=opening||commandPending;}recoveryControls();scheduleStart();});
  addEventListener('sodeck:themeChanged',event=>theme(event.detail.tokens));
  addEventListener('sodeck:focus',()=>{if(!search.focus())terminal.focus();void enqueueSize(true).catch(()=>{});});
  addEventListener('sodeck:unmount',()=>{disposed=true;clearTimeout(commandTimer);clearTimeout(startTimer);stopSize();search.dispose();location.dispose();appearance.dispose();controls.dispose();observer.disconnect();terminal.dispose();});
  // The final output chunk may arrive without another chunk to trigger a checkpoint.
  const timer=setInterval(()=>{if(disposed){clearInterval(timer);return;}chain=chain.then(()=>{void checkpoint().catch(()=>{});}).catch(()=>{});},1000);
})();
