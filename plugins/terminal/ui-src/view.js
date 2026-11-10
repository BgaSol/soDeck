import {createAppearance} from './appearance.js';
import {createLocation} from './location.js';
(() => {
  const host=document.querySelector('#terminal'),status=document.querySelector('#status');
  const terminal=new Terminal({allowProposedApi:true,cursorBlink:true,fontSize:14,lineHeight:1.2,scrollback:1000,convertEol:false});
  const fit=new FitAddon.FitAddon(),serialize=new SerializeAddon.SerializeAddon();terminal.loadAddon(fit);terminal.loadAddon(serialize);
  let id,offset=0,inputSequence=0,chain=Promise.resolve(),inputChain=Promise.resolve(),disposed=false,caughtUp=false,restoring=false,visible=false,enabled=false;
  let savedOffset=0,lastSave=0,escape='normal',utf8=0,inputPending=0,mountKey='';
  let transferFrozen=false,readOnly=false,inputError,restoredDisplay;
  let ended=false,disconnected=false,opening=false,surfaceReady=false,createParams={},canOpen=false;
  const newTerminal=document.createElement('button');newTerminal.textContent='新建终端';newTerminal.hidden=true;newTerminal.className='sd-control';status.after(newTerminal);
  newTerminal.onclick=()=>{
    if(opening||disposed||disconnected||!ended)return;
    opening=true;newTerminal.disabled=true;
    const requestId=crypto.randomUUID();
    void sodeck.request(canOpen?'event':'openView',canOpen?{action:'createTerminal',requestId,params:createParams}:{viewType:'terminal',params:createParams}).catch(fail).finally(()=>{opening=false;newTerminal.disabled=disconnected;});
  };
  const encoder=new TextEncoder();
  const fail=error=>{status.hidden=false;status.textContent=String(error);status.classList.add('error');};
  const controls=installTerminalInput(terminal,host,()=>({key:mountKey,canCopy:!disposed&&!restoring,canPaste:enabled&&!restoring&&!disposed&&visible}),fail);
  const appearance=createAppearance(terminal,host,()=>caughtUp&&!restoring&&visible&&!disposed,()=>enqueueSize(true),()=>controls.invalidate());
  const location=createLocation(terminal);
  let lastCols=0,lastRows=0;
  let sizeFrame,sizeJob,sizeDone,sizeReject,desiredSize,resizeQueued=false,sizeEpoch=0,resizeFailed=false,resizeRetries=0,retryTimer;
  const request=p=>sodeck.request('data',{id,...p});
  const write=bytes=>new Promise(resolve=>terminal.write(bytes,resolve));
  function scan(bytes){for(const b of bytes){
    if(utf8){if((b&0xc0)===0x80){utf8--;continue;}utf8=0;}
    if(b>=0xc2){utf8=b<0xe0?1:b<0xf0?2:3;continue;}
    if(escape==='string'){if(b===7)escape='normal';else if(b===27)escape='stringEscape';}
    else if(escape==='stringEscape')escape=b===92?'normal':'string';
    else if(b===27)escape='escape';
    else if(escape==='escape'){if(b===91)escape='csi';else if([93,80,94,95,88].includes(b))escape='string';else if(b>=0x30&&b<=0x7e)escape='normal';}
    else if(escape==='csi'&&b>=0x40&&b<=0x7e)escape='normal';
  }}
  async function checkpoint(force=false){
    if(disposed||!surfaceReady||restoring||!caughtUp||offset===savedOffset||escape!=='normal'||utf8)return;
    if(!force&&offset-savedOffset<128*1024&&performance.now()-lastSave<250)return;
    const bytes=encoder.encode(serialize.serialize());
    const metadata={cols:terminal.cols,rows:terminal.rows,viewportY:terminal.buffer.active.viewportY,selection:terminal.getSelectionPosition()};
    if(bytes.length>2*1024*1024)throw new Error('终端显示快照超出上限，请减少终端尺寸或清理滚动历史');
    for(let pos=0;pos<bytes.length||pos===0;pos+=16384){
      await request({op:'checkpoint',offset,data:Array.from(bytes.subarray(pos,pos+16384)),metadata,begin:pos===0,done:pos+16384>=bytes.length});
    }
    savedOffset=offset;lastSave=performance.now();
  }
  function enqueueSize(retry=false){
    if(disposed)return Promise.resolve();
    if(retry){resizeFailed=false;resizeRetries=0;}
    if(sizeJob)return sizeJob;
    sizeJob=new Promise((resolve,reject)=>{sizeDone=resolve;sizeReject=reject;});
    sizeFrame=requestAnimationFrame(()=>{
      const done=sizeDone,reject=sizeReject;sizeFrame=sizeJob=sizeDone=sizeReject=undefined;
      try{
        if(visible&&caughtUp&&!restoring&&!readOnly&&!transferFrozen&&!disposed&&host.clientWidth>=20&&host.clientHeight>=20){
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
  }
  addEventListener('sodeck:mount',event=>{
    const mount=event.detail;readOnly=mount.readOnly===true;surfaceReady=mount.capabilities?.sharedState!==1;id=mount.data.streamId;mountKey=mount.surfaceId+':'+JSON.stringify(mount.context);disconnected=mount.connected===false;visible=mount.visible;theme(mount.tokens);
    terminal.open(host);
    createParams=mount.data.createParams||{};canOpen=mount.capabilities?.uiOpenView===1;newTerminal.textContent=createParams.launch?.kind==='command'?'重新打开终端':'新建终端';
    appearance.receive(mount.settings);
    location.mount(mount);
    status.textContent=`${mount.serverName||'服务器'} · 正在接续终端…`;
    void request({op:'attach'}).catch(fail);
  });
  addEventListener('sodeck:surfaceReady',()=>{readOnly=false;transferFrozen=false;surfaceReady=true;enabled=caughtUp&&!disconnected&&!ended;void appearance.apply();void enqueueSize(true).catch(fail);if(enabled)location.ready();});
  addEventListener('sodeck:data',event=>{
    chain=chain.then(async()=>{
      if(disposed)return;
      const frame=event.detail.data;
      location.frame(frame.kind,frame.offset);
      if(frame.kind==='error'){location.unknown(offset,'终端输出不可用');fail(frame.metadata);return;}
      if(frame.kind==='reset'){location.invalidate();controls.invalidate();stopSize();restoring=true;enabled=false;terminal.reset();offset=frame.offset;savedOffset=offset;const d=frame.metadata;restoredDisplay=d;if(d?.cols&&d?.rows)terminal.resize(d.cols,d.rows);}
      if(frame.kind==='snapshot')await write(Uint8Array.from(frame.data));
      if(frame.kind==='restore')restoring=false;
      if(frame.kind==='data'){const bytes=Uint8Array.from(frame.data);await write(bytes);scan(bytes);offset=frame.offset;location.completed();}
      await request({op:'ack',packet:frame.packet});
      if(frame.kind==='caughtUp'){
        caughtUp=true;enabled=surfaceReady&&!disconnected&&!ended;status.textContent='远程终端 · 已连接';status.hidden=true;status.classList.remove('error');document.body.dataset.ready='true';
        // Incoming windows may render before ownership commits. Only the owner saves a checkpoint.
        await checkpoint(true).catch(()=>{});sodeck.mounted(true);if(restoredDisplay?.viewportY!=null)terminal.scrollToLine(restoredDisplay.viewportY);if(restoredDisplay?.selection){const p=restoredDisplay.selection;terminal.select(p.start.x,p.start.y,(p.end.y-p.start.y)*terminal.cols+p.end.x-p.start.x);}restoredDisplay=undefined;if(surfaceReady){void appearance.apply();void enqueueSize().catch(()=>{});}
        if(surfaceReady&&!disconnected&&!ended)location.ready();
      }
      if(frame.kind==='end'){location.unknown(offset,'终端已退出');controls.invalidate();enabled=false;ended=true;stopSize();void enqueueSize().catch(()=>{});status.hidden=false;status.textContent=disconnected?'连接已断开，请在工作台重新连接后新建终端。':frame.metadata?.error||`终端已退出${frame.metadata?.exitCode==null?'':` · ${frame.metadata.exitCode}`}`;newTerminal.hidden=disconnected;}
      if(frame.kind==='data')await checkpoint().catch(error=>{if(!String(error).includes('窗口'))throw error;});
    }).catch(fail);
  });
  function input(text){if(transferFrozen||readOnly||!enabled||restoring||disposed)return;const key=mountKey,bytes=encoder.encode(text);if(inputPending+bytes.length>256*1024){fail('输入待发送内容超过 256 KiB，请稍后分段粘贴；本次输入未发送');return;}inputPending+=bytes.length;location.input(text);inputChain=inputChain.then(async()=>{for(let i=0;i<bytes.length;i+=16384){if(disposed||!enabled||restoring||mountKey!==key)return;await request({op:'write',sequence:inputSequence,data:Array.from(bytes.subarray(i,i+16384))});inputSequence++;}}).catch(error=>{if(!disposed&&mountKey===key){enabled=false;inputError=error;fail(error);}}).finally(()=>{inputPending-=bytes.length;});}
  terminal.onData(input);
  addEventListener('sodeck:flush',async event=>{
    try {
      if(controls.composing())throw new Error('中文输入仍在组合，请完成输入后再移动');
      transferFrozen=event.detail.freeze===true;if(transferFrozen)stopSize();await inputChain;await chain;
      if(inputError)throw inputError;
      await checkpoint(true);
      parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:event.detail.surfaceId,id:event.detail.id},'*');
    }catch(error){transferFrozen=false;parent.postMessage({channel:'sodeck-ui-v2',type:'flushed',surfaceId:event.detail.surfaceId,id:event.detail.id,error:String(error)},'*');}
  });
  addEventListener('sodeck:transferCancelled',()=>{transferFrozen=false;void enqueueSize(true).catch(fail);});
  addEventListener('sodeck:visibilityChanged',event=>{visible=event.detail.visible;if(!visible){controls.invalidate();stopSize();}if(visible){void appearance.apply();void enqueueSize(true).catch(()=>{});}});
  addEventListener('sodeck:settingsChanged',event=>{appearance.receive({...event.detail.settings,retry:true});location.settings(event.detail.settings);});
  addEventListener('sodeck:contextChanged',()=>location.invalidate());
  addEventListener('sodeck:connectionChanged',event=>{if(!event.detail.connected)location.invalidate();});
  addEventListener('sodeck:contextChanged',()=>{controls.invalidate();enabled=false;stopSize();newTerminal.hidden=disconnected||!ended;status.hidden=false;status.textContent='连接已更换，请在工作台新建终端；此页保留原输出。';});
  addEventListener('sodeck:connectionChanged',event=>{disconnected=!event.detail.connected;if(disconnected){controls.invalidate();enabled=false;stopSize();newTerminal.hidden=true;status.hidden=false;status.textContent='连接已断开，请在工作台重新连接；此页保留原输出。';}else if(ended){newTerminal.hidden=false;newTerminal.disabled=opening;}});
  addEventListener('sodeck:themeChanged',event=>theme(event.detail.tokens));
  addEventListener('sodeck:focus',()=>{terminal.focus();void enqueueSize(true).catch(()=>{});});
  addEventListener('sodeck:unmount',()=>{disposed=true;stopSize();location.dispose();appearance.dispose();controls.dispose();observer.disconnect();terminal.dispose();});
  // The final output chunk may arrive without another chunk to trigger a checkpoint.
  const timer=setInterval(()=>{if(disposed){clearInterval(timer);return;}chain=chain.then(()=>checkpoint()).catch(()=>{});},500);
})();
