// xterm 6.0.0 adapter: keep ring/structural observation shared by search and reading.
export function createBufferState(terminal) {
  let head=0,revision=0,structural=false,list,subscriptions=[];
  const bind=()=>{
    const next=terminal.buffer.active._buffer?.lines;
    if(!next?.onTrim||!next.onInsert||!next.onDelete)throw new Error('当前 xterm 版本不支持历史变化观察');
    if(next===list)return false;
    subscriptions.forEach(s=>s.dispose());list=next;
    subscriptions=[list.onTrim(n=>{head+=n;}),list.onInsert(()=>{structural=true;}),list.onDelete(()=>{structural=true;})];return true;
  };
  return {
    update(reset=false){const changed=bind()||reset||structural;structural=false;const previousHead=head;if(changed)head=0;revision++;return {reset:changed,previousHead};},
    state(){const b=terminal.buffer.active;return {head,revision,type:b.type,length:b.length,viewportY:b.viewportY,baseY:b.baseY,cursorRow:b.baseY+b.cursorY};},
    marker(row){const b=terminal.buffer.normal;if(!Number.isInteger(row)||row<0||row>=b.length)return;return terminal.registerMarker(row-b.baseY-b.cursorY);},
    dispose(){subscriptions.forEach(s=>s.dispose());}
  };
}
