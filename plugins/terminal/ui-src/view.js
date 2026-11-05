(() => {
  const host=document.querySelector('#terminal'),status=document.querySelector('#status');
  const terminal=new Terminal({allowProposedApi:true,cursorBlink:true,fontSize:13,scrollback:1000,convertEol:false});
  const fit=new FitAddon.FitAddon(),serialize=new SerializeAddon.SerializeAddon();terminal.loadAddon(fit);terminal.loadAddon(serialize);terminal.open(host);
  let id,offset=0,inputSequence=0,chain=Promise.resolve(),inputChain=Promise.resolve(),disposed=false,caughtUp=false,restoring=false,visible=false,enabled=false;
  let savedOffset=0,lastSave=0,escape='normal',utf8=0,inputPending=0;
  const encoder=new TextEncoder();
  const fail=error=>{status.textContent=String(error);status.classList.add('error');};
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
    fit.fit();await request({op:'resize',cols:Math.min(1000,terminal.cols),rows:Math.min(500,terminal.rows)});
  }
  const observer=new ResizeObserver(()=>{if(enabled)void inputChain.then(size).catch(()=>{});});observer.observe(host);
  function theme(tokens){Object.entries(tokens||{}).forEach(([k,v])=>document.documentElement.style.setProperty('--'+k,v));terminal.options.theme={background:tokens?.panel||'#fff',foreground:tokens?.text||'#222',cursor:tokens?.text||'#222',selectionBackground:'#7198d866'};}
  addEventListener('sodeck:mount',event=>{
    const mount=event.detail;id=mount.data.streamId;visible=mount.visible;theme(mount.tokens);
    status.textContent=`${mount.serverName||'服务器'} · 正在接续终端…`;
    void request({op:'attach'}).catch(fail);
  });
  addEventListener('sodeck:data',event=>{
    chain=chain.then(async()=>{
      if(disposed)return;
      const frame=event.detail.data;
      if(frame.kind==='error'){fail(frame.metadata);return;}
      if(frame.kind==='reset'){restoring=true;enabled=false;terminal.reset();offset=frame.offset;savedOffset=offset;const d=frame.metadata;if(d?.cols&&d?.rows)terminal.resize(d.cols,d.rows);}
      if(frame.kind==='snapshot')await write(Uint8Array.from(frame.data));
      if(frame.kind==='restore')restoring=false;
      if(frame.kind==='data'){const bytes=Uint8Array.from(frame.data);await write(bytes);scan(bytes);offset=frame.offset;}
      await request({op:'ack',packet:frame.packet});
      if(frame.kind==='caughtUp'){
        caughtUp=true;enabled=true;status.textContent='远程终端 · 已连接';
        // Incoming windows may render before ownership commits. Only the owner saves a checkpoint.
        await checkpoint(true).catch(()=>{});sodeck.mounted();void size().catch(()=>{});
      }
      if(frame.kind==='end'){enabled=false;status.textContent=frame.metadata?.error||`终端已退出${frame.metadata?.exitCode==null?'':` · ${frame.metadata.exitCode}`}`;}
      if(frame.kind==='data')await checkpoint().catch(error=>{if(!String(error).includes('窗口'))throw error;});
    }).catch(fail);
  });
  function input(text){if(!enabled||restoring||disposed)return;const bytes=encoder.encode(text);if(inputPending+bytes.length>256*1024){fail('输入待发送内容超过 256 KiB，请稍后分段粘贴；本次输入未发送');return;}inputPending+=bytes.length;inputChain=inputChain.then(async()=>{for(let i=0;i<bytes.length;i+=16384){await request({op:'write',sequence:inputSequence,data:Array.from(bytes.subarray(i,i+16384))});inputSequence++;}}).catch(error=>{enabled=false;fail(error);}).finally(()=>{inputPending-=bytes.length;});}
  terminal.onData(input);
  addEventListener('sodeck:visibilityChanged',event=>{visible=event.detail.visible;if(visible)void size().catch(()=>{});});
  addEventListener('sodeck:themeChanged',event=>theme(event.detail.tokens));
  addEventListener('sodeck:focus',()=>{terminal.focus();void size().catch(()=>{});});
  addEventListener('sodeck:unmount',()=>{disposed=true;observer.disconnect();terminal.dispose();});
  // The final output chunk may arrive without another chunk to trigger a checkpoint.
  const timer=setInterval(()=>{if(disposed){clearInterval(timer);return;}chain=chain.then(()=>checkpoint()).catch(()=>{});},500);
})();
