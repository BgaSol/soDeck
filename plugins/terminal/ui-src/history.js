export function rowText(line,cell){let end=0;for(let x=0;x<line.length;x++){line.getCell(x,cell);if(cell.getChars())end=x+Math.max(1,cell.getWidth());}return line.translateToString(false,0,end);}
export function textColumns(line,offset,end=false){
  let position=0;const cell=line.getCell(0);if(!cell)return 0;
  for(let x=0;x<line.length;x++){line.getCell(x,cell);if(!cell.getWidth())continue;const size=(cell.getChars()||' ').length;if(offset<position+size)return x+(end?cell.getWidth():0);if(offset===position+size)return x+cell.getWidth();position+=size;}
  return line.length;
}
export function createHistory(terminal,onMessage){
  let worker,disposed=false,epoch=0,head=0,lastScreen=0,dirty=0,revision=0,job=false,timer,sequence=0,list,bufferType,subscriptions=[],structural=false;
  const pending=new Map();
  function start(){worker=new Worker(new URL('./history-worker.js',import.meta.url),{type:'module'});worker.onmessage=event=>{const m=event.data;if(m.type==='ack'){pending.get(m.id)?.();pending.delete(m.id);}else onMessage(m);};worker.onerror=()=>{onMessage({type:'error',message:'历史索引任务已停止'});for(const done of pending.values())done();pending.clear();};}
  start();
  function bind(){
    // xterm 6.0.0: public events do not report recycling at a full scrollback. Read only its structural events.
    const next=terminal.buffer.active._buffer?.lines;
    if(!next?.onTrim||!next.onInsert||!next.onDelete)throw new Error('当前 xterm 版本不支持历史变化观察');
    if(next===list)return false;
    subscriptions.forEach(s=>s.dispose());list=next;
    subscriptions=[list.onTrim(amount=>{head+=amount;}),list.onInsert(()=>{structural=true;}),list.onDelete(()=>{structural=true;})];return true;
  }
  function invalidate(){const previousHead=head,previousType=bufferType;bufferType=terminal.buffer.active.type;epoch++;head=0;lastScreen=0;dirty=0;worker.postMessage({type:'reset',epoch});onMessage({type:'reset',epoch,previousHead,previousType});}
  function update(reset=false){
    if(disposed)return;try{if(bind()||reset||structural){structural=false;invalidate();}dirty=Math.min(dirty,lastScreen);lastScreen=head+terminal.buffer.active.baseY;revision++;schedule();}catch(error){onMessage({type:'error',message:String(error)});}
  }
  function schedule(){if(!job&&!timer)timer=setTimeout(()=>{timer=undefined;void drain();},60);}
  async function drain(){
    if(disposed||job)return;job=true;const ticket=epoch,target=head+terminal.buffer.active.length,version=revision;let start=Math.max(head,dirty);dirty=Infinity;onMessage({type:'updating',epoch});
    try{while(start<target&&!disposed&&ticket===epoch){
      const rows=[],transfers=[],cell=terminal.buffer.active.getNullCell(),until=performance.now()+2;let bytes=0;
      while(start<target&&performance.now()<until&&bytes<48*1024){
        const id=start++;if(id<head)continue;const line=terminal.buffer.active.getLine(id-head);if(!line)continue;
        const colors=new Uint32Array(32);let end=0;
        for(let x=0;x<line.length;x++){line.getCell(x,cell);const chars=cell.getChars();if(chars)end=x+Math.max(1,cell.getWidth());let code=0;
          const palette=cell.getFgColor();const foreground=cell.isFgRGB()?0x1000000|palette:cell.isFgPalette()?0x2000000|(palette<8&&cell.isBold()&&terminal.options.drawBoldTextInBrightColors?palette+8:palette):0x3000000;
          const background=cell.isBgRGB()?0x1000000|cell.getBgColor():cell.isBgPalette()?0x2000000|cell.getBgColor():0x4000000;
          if(chars&&chars!==' '&&!cell.isInvisible())code=cell.isInverse()?background:foreground;
          else if(cell.isInverse()||!cell.isBgDefault())code=cell.isInverse()?foreground:background;
          const at=Math.min(31,Math.floor(x/line.length*32));if(code&&(code>>>24)!==3||!colors[at])colors[at]=code;
        }
        const text=line.translateToString(false,0,end);rows.push({id,text,wrapped:line.isWrapped,colors});transfers.push(colors.buffer);bytes+=text.length*2+128;
      }
      const id=++sequence;const ack=new Promise(resolve=>pending.set(id,resolve));worker.postMessage({type:'rows',epoch:ticket,id,head,length:terminal.buffer.active.length,rows},transfers);await ack;
    }
    if(!disposed&&ticket===epoch)worker.postMessage({type:'done',epoch,revision:version});
    }catch(error){onMessage({type:'error',message:String(error)});}finally{job=false;if(dirty!==Infinity&&!disposed)schedule();}
  }
  const resize=terminal.onResize(()=>update(true)),switchBuffer=terminal.buffer.onBufferChange(()=>update(true));
  return {update,send(message){if(!disposed)worker.postMessage({...message,epoch});},state(){return {epoch,head,revision,length:terminal.buffer.active.length,indexing:job||dirty!==Infinity||Boolean(timer)};},retry(){worker.terminate();for(const done of pending.values())done();pending.clear();start();list=undefined;update(true);},dispose(){disposed=true;clearTimeout(timer);worker.terminate();for(const done of pending.values())done();pending.clear();subscriptions.forEach(s=>s.dispose());resize.dispose();switchBuffer.dispose();}};
}
