import {createAppearance} from './appearance.js';
(() => {
  const host=document.querySelector('#terminal'),status=document.querySelector('#status');
  const terminal=new Terminal({allowProposedApi:true,cursorBlink:true,fontSize:14,lineHeight:1.2,scrollback:1000,convertEol:false});
  const fit=new FitAddon.FitAddon(),serialize=new SerializeAddon.SerializeAddon();terminal.loadAddon(fit);terminal.loadAddon(serialize);terminal.open(host);
  let id,offset=0,inputSequence=0,chain=Promise.resolve(),inputChain=Promise.resolve(),disposed=false,caughtUp=false,restoring=false,visible=false,enabled=false;
  let savedOffset=0,lastSave=0,escape='normal',utf8=0,inputPending=0,mountKey='';
  const encoder=new TextEncoder();
  const fail=error=>{status.hidden=false;status.textContent=String(error);status.classList.add('error');};
  const controls=installTerminalInput(terminal,host,()=>({key:mountKey,canCopy:!disposed&&!restoring,canPaste:enabled&&!restoring&&!disposed&&visible}),fail);
  const enqueueSize=()=>{const resize=inputChain.then(size);inputChain=resize.catch(()=>{});return resize;};
  const appearance=createAppearance(terminal,host,()=>caughtUp&&!restoring&&visible&&!disposed,enqueueSize,()=>controls.invalidate());
  let lastCols=0,lastRows=0;
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
    if(disposed||restoring||!caughtUp||offset===savedOffset||escape!=='normal'||utf8)return;
    if(!force&&offset-savedOffset<128*1024&&performance.now()-lastSave<250)return;
    const bytes=encoder.encode(serialize.serialize());
    if(bytes.length>2*1024*1024)throw new Error('终端显示快照超出上限，请减少终端尺寸或清理滚动历史');
    for(let pos=0;pos<bytes.length||pos===0;pos+=16384){
      await request({op:'checkpoint',offset,data:Array.from(bytes.subarray(pos,pos+16384)),metadata:{cols:terminal.cols,rows:terminal.rows},begin:pos===0,done:pos+16384>=bytes.length});
    }
    savedOffset=offset;lastSave=performance.now();
  }
  async function size(){
    if(!visible||!enabled||disposed||host.clientWidth<20||host.clientHeight<20)return;
    fit.fit();const cols=Math.min(1000,terminal.cols),rows=Math.min(500,terminal.rows);if(cols===lastCols&&rows===lastRows)return;await request({op:'resize',cols,rows});lastCols=cols;lastRows=rows;
  }
  const observer=new ResizeObserver(()=>{if(enabled)void enqueueSize().catch(()=>{});});observer.observe(host);
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
    const mount=event.detail;id=mount.data.streamId;mountKey=mount.surfaceId+':'+JSON.stringify(mount.context);visible=mount.visible;theme(mount.tokens);
    appearance.receive(mount.settings);
    status.textContent=`${mount.serverName||'服务器'} · 正在接续终端…`;
    void request({op:'attach'}).catch(fail);
  });
  addEventListener('sodeck:data',event=>{
    chain=chain.then(async()=>{
      if(disposed)return;
      const frame=event.detail.data;
      if(frame.kind==='error'){fail(frame.metadata);return;}
      if(frame.kind==='reset'){controls.invalidate();restoring=true;enabled=false;terminal.reset();offset=frame.offset;savedOffset=offset;const d=frame.metadata;if(d?.cols&&d?.rows)terminal.resize(d.cols,d.rows);}
      if(frame.kind==='snapshot')await write(Uint8Array.from(frame.data));
      if(frame.kind==='restore')restoring=false;
      if(frame.kind==='data'){const bytes=Uint8Array.from(frame.data);await write(bytes);scan(bytes);offset=frame.offset;}
      await request({op:'ack',packet:frame.packet});
      if(frame.kind==='caughtUp'){
        caughtUp=true;enabled=true;status.textContent='远程终端 · 已连接';status.hidden=true;status.classList.remove('error');document.body.dataset.ready='true';
        // Incoming windows may render before ownership commits. Only the owner saves a checkpoint.
        await checkpoint(true).catch(()=>{});sodeck.mounted();void appearance.apply();void enqueueSize().catch(()=>{});
      }
      if(frame.kind==='end'){controls.invalidate();enabled=false;status.hidden=false;status.textContent=frame.metadata?.error||`终端已退出${frame.metadata?.exitCode==null?'':` · ${frame.metadata.exitCode}`}`;}
      if(frame.kind==='data')await checkpoint().catch(error=>{if(!String(error).includes('窗口'))throw error;});
    }).catch(fail);
  });
  function input(text){if(!enabled||restoring||disposed)return;const bytes=encoder.encode(text);if(inputPending+bytes.length>256*1024){fail('输入待发送内容超过 256 KiB，请稍后分段粘贴；本次输入未发送');return;}inputPending+=bytes.length;inputChain=inputChain.then(async()=>{for(let i=0;i<bytes.length;i+=16384){await request({op:'write',sequence:inputSequence,data:Array.from(bytes.subarray(i,i+16384))});inputSequence++;}}).catch(error=>{enabled=false;fail(error);}).finally(()=>{inputPending-=bytes.length;});}
  terminal.onData(input);
  addEventListener('sodeck:visibilityChanged',event=>{visible=event.detail.visible;if(!visible)controls.invalidate();if(visible){void appearance.apply();void enqueueSize().catch(()=>{});}});
  addEventListener('sodeck:settingsChanged',event=>appearance.receive({...event.detail.settings,retry:true}));
  addEventListener('sodeck:contextChanged',()=>{controls.invalidate();enabled=false;});
  addEventListener('sodeck:themeChanged',event=>theme(event.detail.tokens));
  addEventListener('sodeck:focus',()=>{terminal.focus();void enqueueSize().catch(()=>{});});
  addEventListener('sodeck:unmount',()=>{disposed=true;appearance.dispose();controls.dispose();observer.disconnect();terminal.dispose();});
  // The final output chunk may arrive without another chunk to trigger a checkpoint.
  const timer=setInterval(()=>{if(disposed){clearInterval(timer);return;}chain=chain.then(()=>checkpoint()).catch(()=>{});},500);
})();
