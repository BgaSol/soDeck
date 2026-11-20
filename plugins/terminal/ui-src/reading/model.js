// Reading owns semantic records; xterm owns row movement and rendering.
export function createReadingModel(terminal,buffers,changed){
  let entries=[],prompt,input,running,sequence=-1,nextId=0,revision=0,available=false,reason='',suspended=false;
  const normal=()=>terminal.buffer.active.type==='normal';
  const touch=()=>{revision++;changed();};
  const row=()=>terminal.buffer.normal.baseY+terminal.buffer.normal.cursorY;
  const anchor=y=>buffers.marker(y);
  function release(entry){entry.start?.dispose();entry.end?.dispose();entry.output?.dispose();}
  function valid(entry){return entry.start&&!entry.start.isDisposed;}
  function finishUnknown(){if(running){running.status='unknown';running=undefined;}}
  function prune(){
    entries=entries.filter(e=>{if(valid(e)||e.kind==='command'&&e.end&&!e.end.isDisposed)return true;release(e);if(running===e)running=undefined;return false;});
  }
  function readTitle(){
    if(!input||input.marker.isDisposed||!normal())return;
    const b=terminal.buffer.normal,start=input.marker.line,end=row();let text='';
    if(end-start>64||end<start)return;
    for(let y=start;y<end;y++){const line=b.getLine(y);if(!line)return;const value=line.translateToString(true,y===start?input.column:0);text+=(y>start&&!line.isWrapped?'\n':'')+value;if(text.length>4096)return;}
    const title=text.trim();return title&&!/[\u0000-\u0008\u000b-\u001f\u007f]/.test(title)?title:undefined;
  }
  function clearPending(){prompt?.dispose();input?.marker?.dispose();prompt=input=undefined;}
  function event(event){
    if(event.kind==='unavailable'){finishUnknown();clearPending();available=false;reason='命令识别暂不可用，可手动添加阅读标记';touch();return;}
    if(!normal()||suspended)return;
    if(event.kind==='prompt'){
      if(event.sequence<sequence)return;
      finishUnknown();clearPending();sequence=event.sequence;prompt=anchor(row());available=true;reason='';touch();
    }else if(event.sequence===sequence&&event.kind==='input'){
      input?.marker.dispose();input={marker:anchor(row()),column:terminal.buffer.normal.cursorX};
    }else if(event.sequence===sequence&&event.kind==='start'&&prompt&&!prompt.isDisposed){
      finishUnknown();const title=readTitle(),start=prompt;prompt=undefined;
      running={id:++nextId,kind:'command',status:'running',sequence,title,start,output:anchor(row()),range:true};entries.push(running);input?.marker.dispose();input=undefined;available=true;touch();
    }else if(event.kind==='end'&&running?.sequence===event.sequence){
      running.end=anchor(row());running.status=event.exitCode===undefined?'unknown':'ended';running.exitCode=event.exitCode;running=undefined;touch();
    }
  }
  function add(kind,y){
    if(!normal())return;prune();
    let entry=entries.find(e=>valid(e)&&e.start.line===y&&e.kind===kind);if(entry)return entry;
    const start=anchor(y);if(!start)return;
    entry={id:++nextId,kind,status:'unknown',start};entries.push(entry);touch();return entry;
  }
  function snapshot(){
    prune();return {version:1,sequence,nextId,available,reason,entries:entries.map(e=>({id:e.id,kind:e.kind,status:e===running?'running':e.status,sequence:e.sequence,title:e.title,exitCode:e.exitCode,start:valid(e)?e.start.line:null,end:e.end&&!e.end.isDisposed?e.end.line:null,output:e.output&&!e.output.isDisposed?e.output.line:null,range:e.range===true})),prompt:prompt&&!prompt.isDisposed?prompt.line:null,input:input&&!input.marker.isDisposed?{row:input.marker.line,column:input.column}:null};
  }
  function reset(){for(const e of entries){release(e);e.output?.dispose();}entries=[];running=undefined;clearPending();sequence=-1;available=false;reason='';touch();}
  function restore(data){
    reset();if(!data||data.version!==1||!Array.isArray(data.entries)||data.entries.length>100500){reason='阅读标记恢复不完整';touch();return;}
    for(const e of data.entries){
      if(!['command','manual','input'].includes(e.kind)||!Number.isSafeInteger(e.id)||!['running','ended','unknown'].includes(e.status))continue;
      const start=anchor(e.start),end=anchor(e.end),output=anchor(e.output);
      if(!start&&!end)continue;
      const entry={id:e.id,kind:e.kind,status:e.status,sequence:e.sequence,start,end,output,range:e.range===true,title:typeof e.title==='string'?e.title.slice(0,4096):undefined,exitCode:Number.isInteger(e.exitCode)?e.exitCode:undefined};entries.push(entry);if(e.status==='running')running=entry;
    }
    sequence=Number.isSafeInteger(data.sequence)?data.sequence:-1;nextId=entries.reduce((id,e)=>Math.max(id,e.id),0);available=data.available===true;reason=typeof data.reason==='string'?data.reason:'';
    prompt=anchor(data.prompt);if(data.input&&Number.isInteger(data.input.column)&&data.input.column>=0&&data.input.column<terminal.cols){const marker=anchor(data.input.row);if(marker)input={marker,column:data.input.column};}touch();
  }
  return {event,add,snapshot,restore,reset,
    revision:()=>revision,
    state(){prune();return {entries,available,reason,running};},
    positions(){prune();const byRow=new Map();for(const e of entries)if(valid(e)){const y=e.start.line;const position=byRow.get(y)||{row:y,entries:[]};position.entries.push(e);byRow.set(y,position);}return [...byRow.values()].sort((a,b)=>a.row-b.row);},
    remove(id){const target=entries.find(e=>e.id===id&&e.kind!=='command');if(target){release(target);entries=entries.filter(e=>e!==target);touch();}},
    clearInputs(){for(const e of entries.filter(e=>e.kind==='input'))release(e);entries=entries.filter(e=>e.kind!=='input');touch();},
    interrupt(message='结束未确认'){finishUnknown();clearPending();available=false;reason=message;touch();},
    suspend(value){suspended=value;if(value){finishUnknown();clearPending();touch();}},
    dispose(){reset();}
  };
}
