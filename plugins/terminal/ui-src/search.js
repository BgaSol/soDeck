import {createHistory,rowText,textColumns} from './history.js';
import {createMinimap} from './minimap.js';
import {fold} from './history-worker.js';

export function createSearch(terminal,host,save,state){
  const panel=document.createElement('div');panel.className='terminal-search';panel.hidden=true;panel.setAttribute('role','search');
  const input=document.createElement('input');input.type='search';input.placeholder='搜索当前终端历史';input.setAttribute('aria-label','搜索当前终端历史');input.maxLength=1024;input.spellcheck=false;
  const button=(label,path)=>{const b=document.createElement('button');b.type='button';b.className='sd-control search-button';b.setAttribute('aria-label',label);b.title=label;b.innerHTML=`<svg viewBox="0 0 20 20" aria-hidden="true"><path d="${path}"/></svg>`;return b;};
  const sensitive=document.createElement('button');sensitive.type='button';sensitive.className='sd-control search-button search-case';sensitive.textContent='Aa';sensitive.title='区分大小写';sensitive.setAttribute('aria-label','区分大小写');
  const count=document.createElement('span');count.className='search-count';count.setAttribute('role','status');count.setAttribute('aria-live','polite');
  const previous=button('上一处（Shift+Enter）','m5 12 5-5 5 5'),next=button('下一处（Enter）','m5 8 5 5 5-5'),close=button('关闭搜索（Esc）','m5 5 10 10M15 5 5 15');
  const retry=document.createElement('button');retry.type='button';retry.className='sd-control search-retry';retry.textContent='重试';retry.hidden=true;
  const scope=document.createElement('span');scope.className='search-scope';scope.textContent='当前终端 · 最多 10 万行历史';
  panel.append(input,sensitive,count,previous,next,close,retry,scope);host.append(panel);
  const overlay=document.createElement('canvas');overlay.className='terminal-highlights';host.append(overlay);
  const latest=document.createElement('button');latest.type='button';latest.className='sd-control terminal-latest';latest.textContent='回到最新输出';latest.hidden=true;host.append(latest);
  let open=false,caseSensitive=false,query='',current,ordinal,total=0,queryRevision=0,requestId=0,visibleId=0,disposed=false,composing=false,busy=false,failed=false,updating=false,timer,paintTimer,mapTimer,saveTimer,pendingJump;
  const geometry=()=>{const screen=terminal.element?.querySelector('.xterm-screen');if(!screen)return;const a=host.getBoundingClientRect(),b=screen.getBoundingClientRect();return {x:b.left-a.left,y:b.top-a.top,cw:b.width/terminal.cols,ch:b.height/terminal.rows};};
  const history=createHistory(terminal,receive),minimap=createMinimap(terminal,host,row=>near(row,0,true));
  function persist(){clearTimeout(saveTimer);saveTimer=setTimeout(()=>{saveTimer=undefined;if(!disposed)void save(metadata());},300);}
  function metadata(){const head=history.state().head,buffer=terminal.buffer.active;return {search:{open,query,caseSensitive,current:current?{start:{...current.start,row:current.start.row-head},end:{...current.end,row:current.end.row-head}}:undefined},viewportY:buffer.viewportY,atBottom:buffer.viewportY>=buffer.baseY,selection:terminal.getSelectionPosition()||null};}
  function matches(match){
    const {head}=history.state(),cell=terminal.buffer.active.getNullCell();let text='';
    for(let id=match.start.row;id<=match.end.row;id++){const line=terminal.buffer.active.getLine(id-head);if(!line||id>match.start.row&&!line.isWrapped)return false;const s=rowText(line,cell);text+=s.slice(id===match.start.row?match.start.offset:0,id===match.end.row?match.end.offset:undefined);}
    return (caseSensitive?text:fold(text)).includes(caseSensitive?query:fold(query));
  }
  function draw(match,active=false){
    const g=geometry();if(!g)return;const ctx=overlay.getContext('2d'),head=history.state().head,top=terminal.buffer.active.viewportY;
    ctx.fillStyle=active?'#8b5bc24a':'#dba00045';ctx.strokeStyle=terminal.options.theme?.cursor||'#7650b7';ctx.lineWidth=1.5;
    for(let id=match.start.row;id<=match.end.row;id++){const y=id-head-top;if(y<0||y>=terminal.rows)continue;const line=terminal.buffer.active.getLine(id-head);if(!line)continue;
      const from=id===match.start.row?textColumns(line,match.start.offset):0,to=id===match.end.row?textColumns(line,match.end.offset,true):line.length;
      const x=g.x+from*g.cw,width=Math.max(1,(to-from)*g.cw),py=g.y+y*g.ch;ctx.fillRect(x,py,width,g.ch);if(active)ctx.strokeRect(x+.75,py+.75,Math.max(1,width-1.5),Math.max(1,g.ch-1.5));
    }
  }
  function clear(){const scale=devicePixelRatio||1;overlay.width=Math.max(1,Math.round(host.clientWidth*scale));overlay.height=Math.max(1,Math.round(host.clientHeight*scale));overlay.getContext('2d').scale(scale,scale);}
  function refresh(){
    if(disposed||!state().visible)return;latest.hidden=terminal.buffer.active.viewportY>=terminal.buffer.active.baseY;
    clear();minimap.selection(open&&!busy&&!failed&&Boolean(query),current);history.send({type:'map',height:minimap.height()});
    if(open&&query&&!failed){const h=history.state(),start=h.head+terminal.buffer.active.viewportY;history.send({type:'visible',queryRevision,start,end:start+terminal.rows-1,requestId:++visibleId});}
  }
  function schedulePaint(){if(!paintTimer)paintTimer=setTimeout(()=>{paintTimer=undefined;refresh();},60);}
  function status(){
    sensitive.setAttribute('aria-pressed',String(caseSensitive));previous.disabled=next.disabled=!query||!total||failed;retry.hidden=!failed;
    count.textContent=failed?'搜索暂不可用':!query?'':busy?'正在查找…':!total?(updating?'结果更新中':'没有匹配'):`${ordinal==null?'—':ordinal+1} / ${total.toLocaleString()}${updating?' · 更新中':''}`;
    scope.textContent=terminal.buffer.active.type==='alternate'?'当前应用屏幕':'当前终端 · 最多 10 万行历史';
  }
  function execute(saved){clearTimeout(timer);timer=undefined;if(composing||disposed)return;query=input.value;queryRevision++;current=undefined;ordinal=undefined;total=0;failed=false;busy=Boolean(query);pendingJump=undefined;
    if(saved&&['start','end'].every(k=>Number.isInteger(saved[k]?.row)&&saved[k].row>=0&&saved[k].row<terminal.buffer.active.length&&Number.isInteger(saved[k].offset)&&saved[k].offset>=0)&&saved.end.row>=saved.start.row){const head=history.state().head;const match={start:{...saved.start,row:saved.start.row+head},end:{...saved.end,row:saved.end.row+head}};if(matches(match))current=match;}
    status();clear();minimap.selection(false);history.send({type:'query',query,queryRevision,caseSensitive,anchorRow:history.state().head+terminal.buffer.active.viewportY});persist();}
  function find(delay=60){clearTimeout(timer);timer=setTimeout(execute,delay);}
  function near(row,offset=0,jump=false){pendingJump={id:++requestId,jump};history.send({type:'near',row,offset,queryRevision,requestId});}
  function navigate(delta){if(!query||!total)return;delta+=pendingJump?.delta||0;pendingJump={id:++requestId,jump:true,delta};if(current&&matches(current))history.send({type:'near',row:current.start.row,offset:current.start.offset,delta,queryRevision,requestId});else history.send({type:'locate',ordinal:delta>0?delta-1:delta,queryRevision,requestId});}
  function receive(message){
    if(disposed)return;if(message.type==='error'){failed=true;busy=false;status();clear();return;}
    if(message.type==='updating'){updating=true;status();return;}
    if(message.type==='reset'){let candidate;if(current&&message.previousType===terminal.buffer.active.type)candidate={start:{...current.start,row:current.start.row-message.previousHead},end:{...current.end,row:current.end.row-message.previousHead}};current=candidate&&matches(candidate)?candidate:undefined;ordinal=undefined;pendingJump=undefined;busy=Boolean(query);status();clear();return;}
    if(message.epoch!==history.state().epoch)return;
    if(message.type==='map'){minimap.receive(message);return;}
    if(message.queryRevision!==queryRevision)return;
    if(message.type==='visible'){
      if(message.requestId!==visibleId||!open)return;if(message.begin)clear();for(const match of message.matches)draw(match);if(message.complete&&current&&matches(current))draw(current,true);return;
    }
    if(message.type!=='result')return;
    if(!message.match&&(history.state().indexing||message.revision!==history.state().revision)){busy=!total&&Boolean(query);updating=true;status();return;}
    total=message.total;busy=false;failed=false;updating=false;
    if(message.match){if(message.requestId!==pendingJump?.id)return;current=message.match;ordinal=message.ordinal;const jump=pendingJump.jump;pendingJump=undefined;
      if(matches(current)){if(jump)terminal.scrollToLine(Math.max(0,current.start.row-history.state().head-Math.floor(terminal.rows/2)));persist();}else{current=undefined;ordinal=undefined;}
    }else if(query&&total&&!pendingJump?.jump){
      if(current&&matches(current))near(current.start.row,current.start.offset,false);else near(history.state().head+terminal.buffer.active.viewportY,0,open);
    }else{current=undefined;ordinal=undefined;}
    status();schedulePaint();
  }
  function show(){if(disposed||!state().ready)return;open=true;panel.hidden=false;const selection=terminal.getSelection();if(selection&&!/[\r\n]/.test(selection)&&selection.length<=1024)input.value=selection;else input.value=query;input.focus();input.select();if(input.value!==query)execute();else{status();refresh();}persist();}
  function hide(){open=false;panel.hidden=true;clear();minimap.selection(false);terminal.focus();persist();}
  input.addEventListener('input',()=>{if(!composing)find();});input.addEventListener('compositionstart',()=>{composing=true;clearTimeout(timer);});input.addEventListener('compositionend',()=>{composing=false;find();});
  panel.addEventListener('keydown',event=>{if(composing||event.isComposing)return;if(event.key==='Escape'){event.preventDefault();event.stopPropagation();hide();}else if(event.key==='Enter'){event.preventDefault();event.stopPropagation();if(timer)execute();navigate(event.shiftKey?-1:1);}});
  sensitive.onclick=()=>{caseSensitive=!caseSensitive;execute();input.focus();};previous.onclick=()=>navigate(-1);next.onclick=()=>navigate(1);close.onclick=hide;
  retry.onclick=()=>{failed=false;history.retry();execute();};latest.onclick=()=>{terminal.scrollToBottom();terminal.focus();persist();};
  const scroll=terminal.onScroll(()=>{schedulePaint();persist();}),render=terminal.onRender(schedulePaint),selection=terminal.onSelectionChange(persist),parsed=terminal.onWriteParsed(()=>{if(state().ready)history.update();});
  mapTimer=setInterval(()=>{if(!disposed&&state().visible)refresh();},250);
  return {show,focus(){if(open){input.focus();return true;}return false;},update(reset=false){history.update(reset);if(reset&&query)history.send({type:'query',query,queryRevision,caseSensitive,anchorRow:history.state().head+terminal.buffer.active.viewportY});schedulePaint();},metadata,restore(value){if(!value?.search)return;open=value.search.open===true;query=typeof value.search.query==='string'?value.search.query.slice(0,1024):'';caseSensitive=value.search.caseSensitive===true;input.value=query;panel.hidden=!open;execute(value.search.current);},theme(){minimap.theme();schedulePaint();},stop(){minimap.stop();},flush(){clearTimeout(saveTimer);return save(metadata());},dispose(){disposed=true;clearTimeout(timer);clearTimeout(paintTimer);clearTimeout(saveTimer);clearInterval(mapTimer);scroll.dispose();render.dispose();selection.dispose();parsed.dispose();history.dispose();minimap.dispose();panel.remove();overlay.remove();latest.remove();}};
}
