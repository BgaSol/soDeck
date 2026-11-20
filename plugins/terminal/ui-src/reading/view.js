import {parseReading,checkpointFrames,createCheckpointReader} from './protocol.js';
import {createReadingModel} from './model.js';

const paths={up:'m5 9 3-3 3 3M8 6v7',down:'m5 7 3 3 3-3M8 10V3',bookmark:'M4 2h8v12l-4-3-4 3Z',folder:'M2 4h5l1.5 2H14v7H2Zm0 0V3h4l1 1',terminal:'m3 4 4 4-4 4m6 0h4',start:'M3 3h10M8 13V6m-3 3 3-3 3 3',check:'M14 8A6 6 0 1 1 2 8a6 6 0 0 1 12 0Zm-9-0 2 2 4-4',error:'M14 8A6 6 0 1 1 2 8a6 6 0 0 1 12 0ZM6 6l4 4m0-4-4 4',running:'M14 8A6 6 0 1 1 2 8a6 6 0 0 1 12 0ZM7 5l4 3-4 3Z',input:'M4 8h8',unknown:'M14 8A6 6 0 1 1 2 8a6 6 0 0 1 12 0ZM8 11h.01M6 6a2 2 0 1 1 3 1.7L8 9'};
const icon=name=>`<svg viewBox="0 0 16 16" aria-hidden="true"><path d="${paths[name]}"/></svg>`;
export function createReading(terminal,host,buffers,allowed){
  let disposed=false,paused=false,frame,kind,token='',mode='auto',stickyEnabled=true,fill=false,inputEnabled=false,notice='',selectedId,hoverRow,shell='';
  const listeners=new Set();
  const layout=host.parentElement;
  const toolbar=document.createElement('div');toolbar.className='terminal-reading-toolbar';toolbar.setAttribute('aria-label','终端历史阅读');
  const button=(label,name,action)=>{const b=document.createElement('button');b.type='button';b.className='reading-button';b.innerHTML=icon(name)+`<span>${label}</span>`;b.setAttribute('aria-label',label);b.title=label;b.onclick=action;return b;};
  const directory=document.createElement('div');directory.className='reading-directory';directory.innerHTML=icon('folder');
  const directoryText=document.createElement('span');directoryText.textContent='当前终端';directory.append(directoryText);
  const count=document.createElement('span');count.className='reading-position';
  const previous=button('上一处','up',()=>navigate(-1)),next=button('下一处','down',()=>navigate(1));
  const add=button('添加标记','bookmark',()=>toggleManual(terminal.buffer.active.viewportY));
  add.classList.add('reading-add');toolbar.append(directory,count,previous,next,add);layout.before(toolbar);
  const footer=document.createElement('div');footer.className='terminal-reading-footer';
  const readingContext=document.createElement('span'),integrationState=document.createElement('span');integrationState.className='reading-integration';footer.append(readingContext,integrationState);layout.after(footer);
  const overlay=document.createElement('div');overlay.className='terminal-reading-overlay';host.append(overlay);
  const sticky=document.createElement('div');sticky.className='terminal-command-sticky';sticky.hidden=true;
  const title=document.createElement('span');title.className='reading-command-title';
  const stickyIcon=document.createElement('span');stickyIcon.className='reading-sticky-icon';stickyIcon.innerHTML=icon('terminal');
  const jump=button('回到起点','start',()=>{const e=model.state().entries.find(e=>e.id===selectedId);if(e?.start&&!e.start.isDisposed)go(e.start.line);});sticky.append(stickyIcon,title,jump);host.append(sticky);
  const live=document.createElement('span');live.className='reading-live';live.setAttribute('role','status');toolbar.append(live);
  const model=createReadingModel(terminal,buffers,()=>{schedule();for(const listener of listeners)listener();});
  const readCheckpoint=createCheckpointReader(data=>{model.restore(data?.model);inputEnabled=data?.inputEnabled===true;});
  const normal=()=>terminal.buffer.active.type==='normal';
  const active=()=>!disposed&&!paused&&allowed()&&normal()&&mode!=='off';
  function result(e){return e.status==='running'?'执行中':e.exitCode===undefined?'结束未确认':e.exitCode===0?'已完成':'退出 '+e.exitCode;}
  function status(e){return e.kind==='manual'?'阅读标记':e.kind==='input'?'输入位置':(e.title||'已识别命令')+' · '+result(e);}
  function go(row){terminal.scrollToLine(row);hoverRow=row;live.textContent='已定位到第 '+(row+1)+' 行';schedule();}
  function navigate(direction){const y=terminal.buffer.active.viewportY,positions=model.positions();const target=direction<0?positions.filter(p=>p.row<y).at(-1):positions.find(p=>p.row>y);if(target)go(target.row);}
  function toggleManual(row){if(!active())return;const existing=model.state().entries.find(e=>e.kind==='manual'&&!e.start.isDisposed&&e.start.line===row);if(existing)model.remove(existing.id);else model.add('manual',row);schedule();}
  function geometry(){const screen=host.querySelector('.xterm-screen');if(!screen)return;const rect=screen.getBoundingClientRect(),outer=host.getBoundingClientRect();return {x:rect.left-outer.left,y:rect.top-outer.top,height:rect.height,width:rect.width,ch:rect.height/terminal.rows};}
  function paint(){
    frame=undefined;if(disposed)return;
    toolbar.hidden=footer.hidden=mode==='off';overlay.replaceChildren();sticky.hidden=true;
    const enabled=active(),g=geometry(),b=terminal.buffer.active,positions=model.positions();
    previous.disabled=!enabled||!positions.some(p=>p.row<b.viewportY);next.disabled=!enabled||!positions.some(p=>p.row>b.viewportY);add.disabled=!enabled;
    const index=positions.filter(p=>p.row<=b.viewportY).length;
    count.textContent=positions.length?`${index} / ${positions.length} 处`:'暂无标记';
    const explanation=notice||(!normal()?'应用接管中，返回普通终端后可继续标记':mode==='manual'?'仅手动标记':model.state().reason||(model.state().available?'命令识别已连接':'未识别命令，可手动标记'));
    integrationState.textContent=notice?'标记保存暂不可用':!normal()?'应用接管中':mode==='manual'?'仅手动标记':model.state().available?[shell,'UTF-8'].filter(Boolean).join(' · '):inputEnabled?'回车辅助已开启':'手动标记可用';integrationState.title=explanation;readingContext.textContent='终端历史';
    if(!enabled||!g)return;
    const fragment=document.createDocumentFragment();
    for(const position of positions){
      if(position.row<b.viewportY||position.row>=b.viewportY+terminal.rows)continue;
      const e=position.entries.find(e=>e.kind==='manual')||position.entries.find(e=>e.kind==='command')||position.entries[0];
      const symbol=e.kind==='manual'?'bookmark':e.kind==='input'?'input':e.status==='running'?'running':e.exitCode===undefined?'unknown':e.exitCode===0?'check':'error';
      const marker=button(status(e),symbol,()=>go(position.row));marker.classList.add('reading-marker');marker.classList.toggle('is-error',e.exitCode>0);marker.classList.toggle('is-input',e.kind==='input');marker.querySelector('span').remove();
      marker.style.left=Math.max(0,g.x-30)+'px';marker.style.top=(g.y+(position.row-b.viewportY)*g.ch)+'px';marker.style.height=g.ch+'px';
      marker.oncontextmenu=event=>{if(e.kind!=='command'){event.preventDefault();model.remove(e.id);}};fragment.append(marker);
      if(e.kind==='command'){
        const line=document.createElement('div');line.className='reading-separator'+(fill?' has-fill':'');line.style.cssText=`left:${g.x}px;top:${g.y+(position.row-b.viewportY)*g.ch}px;width:${g.width}px;height:${g.ch}px`;fragment.append(line);
        // Only use empty cells at the right edge; shell text and wrapped commands always win.
        const row=b.getLine(position.row),reserved=Math.ceil(100/(g.width/terminal.cols))+1;
        if(terminal.cols>reserved+20&&row&&!row.translateToString(true,terminal.cols-reserved).trim()){
          const meta=document.createElement('span');meta.className='reading-result'+(e.exitCode>0?' is-error':'');meta.textContent=result(e);meta.style.cssText=`left:${g.x+g.width-100}px;top:${g.y+(position.row-b.viewportY)*g.ch}px;height:${g.ch}px`;fragment.append(meta);
        }
      }
    }
    const range=model.state().entries.find(e=>e.kind==='command'&&e.range&&e.output&&!e.output.isDisposed&&e.output.line<=b.viewportY&&((e.end&&!e.end.isDisposed&&e.end.line>b.viewportY)||e.status==='running'));
    selectedId=range?.id;
    const context=range||model.state().entries.find(e=>e.kind==='command'&&e.start&&!e.start.isDisposed&&e.start.line===b.viewportY);
    if(context?.title){readingContext.textContent='正在阅读  '+context.title.replace(/\s+/g,' ');readingContext.title=context.title;}else readingContext.removeAttribute('title');
    if(stickyEnabled&&range?.title&&(!range.start||range.start.isDisposed||range.start.line<b.viewportY)&&!host.querySelector('.terminal-search:not([hidden])')){
      sticky.hidden=false;title.textContent=range.title.replace(/\s+/g,' ');title.title=range.title;
      jump.disabled=!range.start||range.start.isDisposed;jump.title=jump.disabled?'起点已不在历史中':'回到起点';
    }
    const focusRow=hoverRow??b.viewportY;
    const guide=model.state().entries.find(e=>e.kind==='command'&&e.range&&e.output&&!e.output.isDisposed&&(e.start&&!e.start.isDisposed?e.start.line:e.output.line)<=focusRow&&((e.end&&!e.end.isDisposed&&e.end.line>focusRow)||e.status==='running'));
    if(guide&&hoverRow!==undefined){const start=Math.max(b.viewportY,guide.output.line),end=Math.min(b.viewportY+terminal.rows,guide.end&&!guide.end.isDisposed?guide.end.line:b.length),line=document.createElement('div');line.className='reading-range';line.style.cssText=`left:${Math.max(0,g.x-19)}px;top:${g.y+(start-b.viewportY)*g.ch}px;height:${Math.max(0,end-start)*g.ch}px`;fragment.append(line);}
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
  return {
    model,
    mount(mount){token=mount.data.integration?.token||'';shell=mount.data.integration?.shell||'';this.settings(mount.settings);},
    settings(value){const v=value?.values||{},before=mode;mode=['manual','off'].includes(v.readingMode)?v.readingMode:'auto';host.classList.toggle('has-reading-gutter',mode!=='off');stickyEnabled=v.commandSticky!==false;fill=v.commandFill===true;if(mode!==before)model.suspend(mode!=='auto');schedule();},
    location(value){if(value.state==='known'){directoryText.textContent=value.path;directory.title=value.path;}else if(value.activity==='busy'&&directoryText.textContent!=='当前终端'){directory.title='最近确认目录：'+directoryText.textContent;}else{directoryText.textContent='当前终端';directory.title='当前目录未确认';}},
    frame(type){kind=type;if(type==='reset'){model.reset();notice='';}},
    checkpoint(){return checkpointFrames({version:1,inputEnabled,model:model.snapshot()});},
    revision(){return model.revision()+':'+inputEnabled;},
    submit(){if(!active()||mode!=='auto'||!inputEnabled||model.state().available)return;return buffers.marker(terminal.buffer.active.baseY+terminal.buffer.active.cursorY);},
    submitted(marker,success){if(!marker)return;if(success&&!marker.isDisposed&&normal())model.add('input',marker.line);marker.dispose();},
    contextActions(row){if(!active())return [];const target=Number.isInteger(row)?row:terminal.getSelectionPosition()?.start.y??terminal.buffer.active.viewportY;const input=model.state().entries.find(e=>e.kind==='input'&&!e.start.isDisposed&&e.start.line===target);return [{label:'在此处添加／移除阅读标记',action:()=>toggleManual(target)},...(input?[{label:'移除此处输入标记',action:()=>model.remove(input.id)}]:[]),...(mode==='auto'?[{label:'回车辅助标记',checked:inputEnabled,title:'仅当前终端。记录回车位置，不代表执行命令；多行、交互或密码提示也可能产生标记。',action:()=>{inputEnabled=!inputEnabled;schedule();}}]:[]),...(model.state().entries.some(e=>e.kind==='input')?[{label:'清除输入标记',action:()=>model.clearInputs()}]:[])];},
    pause(){paused=true;cancelAnimationFrame(frame);frame=undefined;sticky.hidden=true;overlay.replaceChildren();},
    resume(){paused=false;schedule();},
    update:schedule,
    storageError(message=''){notice=message?'阅读标记保存暂不可用：'+message:'';schedule();},
    interrupt(){model.interrupt();schedule();},
    positions:()=>mode==='off'?[]:model.positions(),
    onChanged(listener){listeners.add(listener);return {dispose(){listeners.delete(listener);}};},
    navigateRow:go,
    dispose(){disposed=true;cancelAnimationFrame(frame);listeners.clear();observe.disconnect();osc.dispose();scroll.dispose();render.dispose();resize.dispose();buffer.dispose();model.dispose();toolbar.remove();footer.remove();overlay.remove();sticky.remove();host.classList.remove('has-reading-gutter');host.removeEventListener('pointermove',move);host.removeEventListener('pointerleave',leave);}
  };
}
