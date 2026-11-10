// xterm 6.0.0 / SerializeAddon 0.14.0 adapter. No live buffer or addon is mutated.
// Historical rows are immutable except ring recycling; copy only recycled pending rows and the screen.
function capture(terminal){
  const subscriptions=[];let processed=0;
  const copy=view=>{const result=Object.create(Object.getPrototypeOf(view));result._line=view._line.clone();return result;};
  function buffer(source){
    const rows=[],pending=new WeakMap();
    for(let i=0;i<source.length;i++){const row=source.getLine(i);if(!row?._line?.clone)throw new Error('当前 xterm 版本无法生成一致的历史快照');rows.push(i>=source.baseY?copy(row):row);if(i<source.baseY)pending.set(row._line,i);}
    const lines=source._buffer.lines;
    subscriptions.push(lines.onTrim(()=>{const recycled=lines.get(lines.length-1),index=pending.get(recycled);if(index!==undefined){if(index>=processed)rows[index]=copy(rows[index]);pending.delete(recycled);}}));
    return {type:source.type,baseY:source.baseY,cursorX:source.cursorX,cursorY:source.cursorY,viewportY:source.viewportY,length:source.length,getLine:y=>rows[y],getNullCell:()=>source.getNullCell()};
  }
  const normal=buffer(terminal.buffer.normal),alternate=buffer(terminal.buffer.alternate),attr=terminal._core?._inputHandler?._curAttrData;
  if(!attr?.clone){subscriptions.forEach(s=>s.dispose());throw new Error('当前 xterm 版本无法保存终端样式');}
  const frozen={cols:terminal.cols,rows:terminal.rows,modes:{...terminal.modes},buffer:{normal,alternate,active:terminal.buffer.active.type==='alternate'?alternate:normal},_core:{_inputHandler:{_curAttrData:attr.clone()}}};
  return {terminal:frozen,processed(value){processed=value;},dispose(){subscriptions.forEach(s=>s.dispose());}};
}
export async function* snapshotChunks(terminal,serialize,pause) {
  const channel=new MessageChannel();let resume;channel.port1.onmessage=()=>resume?.();
  pause??=()=>new Promise(resolve=>{resume=resolve;channel.port2.postMessage(0);});
  const frozen=capture(terminal),source=frozen.terminal,addon=new serialize.constructor();addon.activate(source);
  const length=source.buffer.normal.length;
  try{
  for(let start=0;start<length;){
    // The final slice must include the screen and one history row; the addon's screen-only path omits trailing blank rows.
    const end=length-start<=64+source.rows?length-1:start+63,last=end===length-1;
    // ECH preserves isWrapped; EL would clear it on the overlapping row.
    const prefix=start?`\x1b[0m\r\x1b[${source.cols}X`:'';
    yield prefix+(last?addon.serialize({scrollback:length-start-source.rows}):addon.serialize({range:{start,end},excludeAltBuffer:true,excludeModes:true}));
    if(last)return;
    start=end;frozen.processed(start);await pause();
  }
  }finally{frozen.dispose();addon.dispose();channel.port1.close();channel.port2.close();}
}
