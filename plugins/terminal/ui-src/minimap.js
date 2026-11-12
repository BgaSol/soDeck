export function createMinimap(terminal,host,find,visible=()=>true,resized=()=>{}){
  const rail=document.createElement('div');rail.className='terminal-minimap';rail.setAttribute('aria-label','终端历史预览');
  const canvas=document.createElement('canvas'),marks=document.createElement('canvas'),thumb=document.createElement('div');
  canvas.className='minimap-content';marks.className='minimap-search';thumb.className='minimap-viewport';thumb.tabIndex=0;thumb.setAttribute('role','slider');thumb.setAttribute('aria-label','终端历史阅读位置');thumb.setAttribute('aria-valuemin','1');rail.append(canvas,marks,thumb);host.after(rail);
  let data,current,searching=false,drag,disposed=false,frame;
  const colors=['#000','#cd3131','#0dbc79','#e5b553','#2472c8','#bc3fbc','#11a8cd','#e5e5e5','#666','#f14c4c','#23d18b','#f5d37a','#3b8eea','#d670d6','#29b8db','#fff'];
  const color=code=>{const kind=code>>>24,value=code&0xffffff;if(kind===1)return '#'+value.toString(16).padStart(6,'0');if(kind===3)return terminal.options.theme?.foreground||'#777';if(kind===4)return terminal.options.theme?.background||'#fff';if(kind===2){if(value<16){const names=['black','red','green','yellow','blue','magenta','cyan','white','brightBlack','brightRed','brightGreen','brightYellow','brightBlue','brightMagenta','brightCyan','brightWhite'];return terminal.options.theme?.[names[value]]||colors[value];}if(value>=232){const n=8+(value-232)*10;return `rgb(${n},${n},${n})`;}const n=value-16,r=Math.floor(n/36),g=Math.floor(n/6)%6,b=n%6,level=n=>n?55+n*40:0;return `rgb(${level(r)},${level(g)},${level(b)})`;}return '';};
  function layout(){
    if(disposed||!visible())return;const height=rail.clientHeight,buffer=terminal.buffer.active,length=Math.max(1,buffer.length);
    const size=Math.min(height,Math.max(16,height*terminal.rows/length)),top=length>terminal.rows?buffer.viewportY/(length-terminal.rows)*(height-size):0;
    thumb.style.height=size+'px';thumb.style.top=top+'px';thumb.setAttribute('aria-valuemax',String(Math.max(1,buffer.baseY+1)));thumb.setAttribute('aria-valuenow',String(buffer.viewportY+1));thumb.setAttribute('aria-valuetext',`第 ${buffer.viewportY+1} 行，共 ${buffer.length} 行`);
  }
  function paint(){
    if(!data||disposed||!visible())return;const ctx=canvas.getContext('2d'),overlay=marks.getContext('2d'),scale=devicePixelRatio||1;
    const width=Math.max(1,Math.round(rail.clientWidth*scale)),height=Math.max(1,Math.round(rail.clientHeight*scale));
    for(const target of [canvas,marks]){if(target.width!==width)target.width=width;if(target.height!==height)target.height=height;}
    ctx.clearRect(0,0,canvas.width,canvas.height);overlay.clearRect(0,0,marks.width,marks.height);
    const cw=canvas.width/32,ch=canvas.height/data.height;
    for(let y=0;y<data.height;y++)for(let x=0;x<32;x++){const code=data.colors[y*32+x];if(code){ctx.globalAlpha=(code>>>24)===3?.28:.75;ctx.fillStyle=color(code);ctx.fillRect(x*cw,y*ch,Math.max(1,cw*.7),Math.max(1,ch*.7));}}
    if(searching){overlay.fillStyle='#dba000';for(let y=0;y<data.height;y++)if(data.density[y])overlay.fillRect(0,y*ch,marks.width,Math.max(scale,Math.min(3*scale,ch)));if(current){overlay.fillStyle=terminal.options.theme?.cursor||'#7650b7';const y=(current.start.row-data.head)/Math.max(1,data.length)*marks.height;overlay.fillRect(0,y,marks.width,3*scale);}}
    layout();
  }
  function scroll(top){const buffer=terminal.buffer.active;terminal.scrollToLine(Math.max(0,Math.min(buffer.baseY,Math.round(top))));layout();}
  const down=event=>{if(event.button!==0||!visible())return;event.preventDefault();event.stopPropagation();const rect=rail.getBoundingClientRect();const y=event.clientY-rect.top;
    if(event.target===thumb){drag={id:event.pointerId,offset:y-parseFloat(thumb.style.top||'0')};rail.classList.add('is-dragging');rail.setPointerCapture(event.pointerId);return;}
    const at=data?Math.min(data.height-1,Math.floor(y/rect.height*data.height)):0;
    if(searching&&data?.density[at])find(data.head+Math.floor(at/data.height*data.length));else scroll(y/rect.height*terminal.buffer.active.length-terminal.rows/2);
  };
  const move=event=>{if(!visible()||!drag||drag.id!==event.pointerId)return;const rect=rail.getBoundingClientRect(),space=Math.max(1,rect.height-thumb.offsetHeight);scroll((event.clientY-rect.top-drag.offset)/space*terminal.buffer.active.baseY);};
  const stop=()=>{if(drag&&rail.hasPointerCapture(drag.id))rail.releasePointerCapture(drag.id);drag=undefined;rail.classList.remove('is-dragging');};
  const keys=event=>{if(!visible())return;let y=terminal.buffer.active.viewportY;const changes={ArrowUp:-3,ArrowDown:3,PageUp:-terminal.rows,PageDown:terminal.rows};if(event.key in changes)y+=changes[event.key];else if(event.key==='Home')y=0;else if(event.key==='End')y=terminal.buffer.active.baseY;else if(event.key==='Escape'){stop();terminal.focus();return;}else return;event.preventDefault();event.stopPropagation();scroll(y);};
  rail.addEventListener('pointerdown',down);rail.addEventListener('pointermove',move);rail.addEventListener('pointerup',stop);rail.addEventListener('pointercancel',stop);rail.addEventListener('lostpointercapture',stop);thumb.addEventListener('keydown',keys);window.addEventListener('blur',stop);
  const scrollSub=terminal.onScroll(()=>{if(visible()&&!frame)frame=requestAnimationFrame(()=>{frame=undefined;layout();});});const resize=new ResizeObserver(()=>{paint();layout();resized();});resize.observe(rail);
  function pause(){stop();cancelAnimationFrame(frame);frame=undefined;}
  return {receive(message){data=message;paint();},selection(open,match){if(searching===open&&JSON.stringify(current)===JSON.stringify(match))return;searching=open;current=match;paint();},theme:paint,layout,height(){return Math.max(1,Math.min(1200,rail.clientHeight));},pause,dispose(){disposed=true;pause();scrollSub.dispose();resize.disconnect();window.removeEventListener('blur',stop);rail.remove();}};
}
