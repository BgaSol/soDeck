import {parseReading,checkpointFrames,createCheckpointReader} from './protocol.js';
import {createReadingModel} from './model.js';

const paths={up:'m5 9 3-3 3 3M8 6v7',down:'m5 7 3 3 3-3M8 10V3',bookmark:'M4 2h8v12l-4-3-4 3Z',settings:'M3 4h10M3 8h10M3 12h10M6 2v4M10 6v4M6 10v4',start:'M3 3h10M8 13V6m-3 3 3-3 3 3',check:'m4 8 3 3 5-6',error:'m5 5 6 6m0-6-6 6',running:'m6 3 6 5-6 5Z',input:'M4 8h8',unknown:'M8 11h.01M6 6a2 2 0 1 1 3 1.7L8 9'};
const icon=name=>`<svg viewBox="0 0 16 16" aria-hidden="true"><path d="${paths[name]}"/></svg>`;
export function createReading(terminal,host,buffers,allowed){
  let disposed=false,paused=false,frame,kind,token='',mode='auto',stickyEnabled=true,fill=false,inputEnabled=false,notice='',selectedId,hoverRow;
  const listeners=new Set();
  const layout=host.parentElement;
  const toolbar=document.createElement('div');toolbar.className='terminal-reading-toolbar';toolbar.setAttribute('aria-label','终端历史阅读');
  const button=(label,name,action)=>{const b=document.createElement('button');b.type='button';b.className='reading-button';b.innerHTML=icon(name)+`<span>${label}</span>`;b.setAttribute('aria-label',label);b.title=label;b.onclick=action;return b;};
  const count=document.createElement('span');count.className='reading-position';
  const previous=button('上一处','up',()=>navigate(-1)),next=button('下一处','down',()=>navigate(1));
  const add=button('添加标记','bookmark',()=>toggleManual(terminal.buffer.active.viewportY));
  const options=button('阅读选项','settings',()=>{menu.hidden=!menu.hidden;});
  toolbar.append(count,previous,next,add,options);layout.before(toolbar);
  const menu=document.createElement('div');menu.className='terminal-reading-options sd-menu';menu.hidden=true;toolbar.append(menu);
  const explanation=document.createElement('p');explanation.className='reading-explanation';menu.append(explanation);
  const inputToggle=document.createElement('input');inputToggle.type='checkbox';
  const label=document.createElement('label');label.className='reading-toggle';label.append(inputToggle,document.createTextNode('回车辅助标记（仅当前终端）'));menu.append(label);
  const help=document.createElement('p');help.textContent='标记回车时的位置；多行、交互或密码提示也可能产生标记，不代表执行命令。';help.className='reading-help';menu.append(help);
  const clear=button('清除输入标记','input',()=>model.clearInputs());menu.append(clear);
  const overlay=document.createElement('div');overlay.className='terminal-reading-overlay';host.append(overlay);
  const sticky=document.createElement('div');sticky.className='terminal-command-sticky';sticky.hidden=true;
  const title=document.createElement('span');title.className='reading-command-title';
  const jump=button('回到起点','start',()=>{const e=model.state().entries.find(e=>e.id===selectedId);if(e?.start&&!e.start.isDisposed)go(e.start.line);});sticky.append(title,jump);host.append(sticky);
  const live=document.createElement('span');live.className='reading-live';live.setAttribute('role','status');toolbar.append(live);
  const model=createReadingModel(terminal,buffers,()=>{schedule();for(const listener of listeners)listener();});
  const readCheckpoint=createCheckpointReader(data=>{model.restore(data?.model);inputEnabled=data?.inputEnabled===true;inputToggle.checked=inputEnabled;});
  const normal=()=>terminal.buffer.active.type==='normal';
  const active=()=>!disposed&&!paused&&allowed()&&normal()&&mode!=='off';
  inputToggle.onchange=()=>{inputEnabled=inputToggle.checked;schedule();};
  function status(e){return e.kind==='manual'?'阅读标记':e.kind==='input'?'输入位置':(e.title||'已识别命令')+' · '+(e.status==='running'?'执行中':e.exitCode===undefined?'结束未确认':'退出 '+e.exitCode);}
  function go(row){terminal.scrollToLine(row);hoverRow=row;live.textContent='已定位到第 '+(row+1)+' 行';menu.hidden=true;schedule();}
  function navigate(direction){const y=terminal.buffer.active.viewportY,positions=model.positions();const target=direction<0?positions.filter(p=>p.row<y).at(-1):positions.find(p=>p.row>y);if(target)go(target.row);}
  function toggleManual(row){if(!active())return;const existing=model.state().entries.find(e=>e.kind==='manual'&&!e.start.isDisposed&&e.start.line===row);if(existing)model.remove(existing.id);else model.add('manual',row);schedule();}
  function geometry(){const screen=host.querySelector('.xterm-screen');if(!screen)return;const rect=screen.getBoundingClientRect(),outer=host.getBoundingClientRect();return {x:rect.left-outer.left,y:rect.top-outer.top,height:rect.height,width:rect.width,ch:rect.height/terminal.rows};}
  function paint(){
    frame=undefined;if(disposed)return;
    toolbar.hidden=mode==='off';overlay.replaceChildren();sticky.hidden=true;
    const enabled=active(),g=geometry(),b=terminal.buffer.active,positions=model.positions();
    previous.disabled=!enabled||!positions.some(p=>p.row<b.viewportY);next.disabled=!enabled||!positions.some(p=>p.row>b.viewportY);add.disabled=!enabled;
    const index=positions.filter(p=>p.row<=b.viewportY).length;
    count.textContent=positions.length?`${index} / ${positions.length} 处`:'暂无标记';
    explanation.textContent=notice||(!normal()?'应用接管中，返回普通终端后可继续标记':mode==='manual'?'仅手动标记':model.state().reason||(model.state().available?'命令识别已连接':'当前环境未提供命令边界，可手动添加阅读标记'));
    inputToggle.checked=inputEnabled;inputToggle.disabled=!enabled||mode!=='auto';
    if(!enabled||!g)return;
    const fragment=document.createDocumentFragment();
    for(const position of positions){
      if(position.row<b.viewportY||position.row>=b.viewportY+terminal.rows)continue;
      const e=position.entries.find(e=>e.kind==='manual')||position.entries.find(e=>e.kind==='command')||position.entries[0];
      const symbol=e.kind==='manual'?'bookmark':e.kind==='input'?'input':e.status==='running'?'running':e.exitCode===undefined?'unknown':e.exitCode===0?'check':'error';
      const marker=button(status(e),symbol,()=>go(position.row));marker.className='reading-marker'+(e.exitCode>0?' is-error':'');marker.querySelector('span').remove();
      marker.style.left=Math.max(0,g.x-16)+'px';marker.style.top=(g.y+(position.row-b.viewportY)*g.ch)+'px';marker.style.height=g.ch+'px';
      marker.oncontextmenu=event=>{if(e.kind!=='command'){event.preventDefault();model.remove(e.id);}};fragment.append(marker);
      if(e.kind==='command'){const line=document.createElement('div');line.className='reading-separator'+(fill?' has-fill':'');line.style.cssText=`left:${g.x}px;top:${g.y+(position.row-b.viewportY)*g.ch}px;width:${g.width}px;height:${g.ch}px`;fragment.append(line);}
    }
    const range=model.state().entries.find(e=>e.kind==='command'&&e.range&&e.output&&!e.output.isDisposed&&e.output.line<=b.viewportY&&((e.end&&!e.end.isDisposed&&e.end.line>b.viewportY)||e.status==='running'));
    selectedId=range?.id;
    if(stickyEnabled&&range?.title&&(!range.start||range.start.isDisposed||range.start.line<b.viewportY)&&!host.querySelector('.terminal-search:not([hidden])')){
      sticky.hidden=false;title.textContent=range.title.replace(/\s+/g,' ');title.title=range.title;
      jump.disabled=!range.start||range.start.isDisposed;jump.title=jump.disabled?'起点已不在历史中':'回到起点';
    }
    const focusRow=hoverRow??b.viewportY;
    const guide=model.state().entries.find(e=>e.kind==='command'&&e.range&&e.output&&!e.output.isDisposed&&(e.start&&!e.start.isDisposed?e.start.line:e.output.line)<=focusRow&&((e.end&&!e.end.isDisposed&&e.end.line>focusRow)||e.status==='running'));
    if(guide&&hoverRow!==undefined){const start=Math.max(b.viewportY,guide.output.line),end=Math.min(b.viewportY+terminal.rows,guide.end&&!guide.end.isDisposed?guide.end.line:b.length),line=document.createElement('div');line.className='reading-range';line.style.cssText=`left:${Math.max(0,g.x-7)}px;top:${g.y+(start-b.viewportY)*g.ch}px;height:${Math.max(0,end-start)*g.ch}px`;fragment.append(line);}
    overlay.append(fragment);
  }
  function schedule(){if(!disposed&&!paused&&!frame)frame=requestAnimationFrame(paint);}
  const osc=terminal.parser.registerOscHandler(633,text=>{
    if(kind==='snapshot')return readCheckpoint(text);
    if(kind!=='data')return false;
    const event=parseReading(text,token);if(!event)return false;
    if(mode==='auto')model.event(event);return true;
  });
  const scroll=terminal.onScroll(schedule),render=terminal.onRender(schedule),resize=terminal.onResize(schedule),buffer=terminal.buffer.onBufferChange(()=>{if(!normal())model.interrupt('应用接管中');schedule();});
  const observe=new ResizeObserver(schedule);observe.observe(host);
  const move=event=>{const g=geometry();if(g){hoverRow=terminal.buffer.active.viewportY+Math.floor((event.clientY-host.getBoundingClientRect().top-g.y)/g.ch);schedule();}};
  const leave=()=>{hoverRow=undefined;schedule();};host.addEventListener('pointermove',move);host.addEventListener('pointerleave',leave);
  const outside=event=>{if(!toolbar.contains(event.target))menu.hidden=true;};document.addEventListener('pointerdown',outside);
  menu.onkeydown=event=>{if(event.key==='Escape'){menu.hidden=true;options.focus();event.stopPropagation();}};
  return {
    model,
    mount(mount){token=mount.data.integration?.token||'';this.settings(mount.settings);},
    settings(value){const v=value?.values||{},before=mode;mode=['manual','off'].includes(v.readingMode)?v.readingMode:'auto';stickyEnabled=v.commandSticky!==false;fill=v.commandFill===true;if(mode!==before)model.suspend(mode!=='auto');schedule();},
    frame(type){kind=type;if(type==='reset'){model.reset();notice='';}},
    checkpoint(){return checkpointFrames({version:1,inputEnabled,model:model.snapshot()});},
    revision(){return model.revision()+':'+inputEnabled;},
    submit(){if(!active()||mode!=='auto'||!inputEnabled||model.state().available)return;return buffers.marker(terminal.buffer.active.baseY+terminal.buffer.active.cursorY);},
    submitted(marker,success){if(!marker)return;if(success&&!marker.isDisposed&&normal())model.add('input',marker.line);marker.dispose();},
    contextActions(row){if(!active())return [];const target=Number.isInteger(row)?row:terminal.getSelectionPosition()?.start.y??terminal.buffer.active.viewportY;const input=model.state().entries.find(e=>e.kind==='input'&&!e.start.isDisposed&&e.start.line===target);return [{label:'在此处添加／移除阅读标记',action:()=>toggleManual(target)},...(input?[{label:'移除此处输入标记',action:()=>model.remove(input.id)}]:[])];},
    pause(){paused=true;cancelAnimationFrame(frame);frame=undefined;menu.hidden=true;sticky.hidden=true;overlay.replaceChildren();},
    resume(){paused=false;schedule();},
    update:schedule,
    storageError(message=''){notice=message?'阅读标记保存暂不可用：'+message:'';options.title=notice||'阅读选项';schedule();},
    interrupt(){model.interrupt();schedule();},
    positions:()=>mode==='off'?[]:model.positions(),
    onChanged(listener){listeners.add(listener);return {dispose(){listeners.delete(listener);}};},
    navigateRow:go,
    dispose(){disposed=true;cancelAnimationFrame(frame);listeners.clear();observe.disconnect();osc.dispose();scroll.dispose();render.dispose();resize.dispose();buffer.dispose();model.dispose();toolbar.remove();overlay.remove();sticky.remove();host.removeEventListener('pointermove',move);host.removeEventListener('pointerleave',leave);document.removeEventListener('pointerdown',outside);}
  };
}
