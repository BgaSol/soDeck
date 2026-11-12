import {createHistory,rowText,textColumns} from './history.js';
import {createMinimap} from './minimap.js';
import {fold,originalOffset} from './history-worker.js';

export function createSearch(terminal,host,save,state){
  const panel=document.createElement('div');panel.className='terminal-search';panel.hidden=true;panel.setAttribute('role','search');
  const input=document.createElement('input');input.type='search';input.placeholder='搜索当前终端历史';input.setAttribute('aria-label','搜索当前终端历史');input.maxLength=1024;input.spellcheck=false;
  const button=(label,path)=>{const b=document.createElement('button');b.type='button';b.className='sd-control search-button';b.setAttribute('aria-label',label);b.title=label;b.innerHTML=`<svg viewBox="0 0 20 20" aria-hidden="true"><path d="${path}"/></svg>`;return b;};
  const sensitive=document.createElement('button');sensitive.type='button';sensitive.className='sd-control search-button search-case';sensitive.textContent='Aa';sensitive.title='区分大小写';sensitive.setAttribute('aria-label','区分大小写');
  const count=document.createElement('span');count.className='search-count';count.setAttribute('role','status');count.setAttribute('aria-live','polite');
  const previous=button('上一处（Shift+Enter）','m5 12.5 5-5 5 5'),next=button('下一处（Enter）','m5 7.5 5 5 5-5'),close=button('关闭搜索（Esc）','m5 5 10 10M15 5 5 15');
  const retry=document.createElement('button');retry.type='button';retry.className='sd-control search-retry';retry.textContent='重试';retry.hidden=true;
  const scope=document.createElement('span');scope.className='search-scope';scope.textContent='当前终端 · 最多 10 万行历史';
  panel.append(input,sensitive,count,previous,next,close,retry,scope);host.append(panel);
  const overlay=document.createElement('canvas');overlay.className='terminal-highlights';host.append(overlay);
  const latest=document.createElement('button');latest.type='button';latest.className='sd-control terminal-latest';latest.textContent='回到最新输出';latest.hidden=true;host.append(latest);
  let open=false,caseSensitive=false,query='',current,ordinal,total=0,queryRevision=0,requestId=0,visibleId=0,mapId=0,disposed=false,composing=false,busy=false,failed=false,updating=false;
  let timer,paintFrame,saveTimer,lagTimer,pendingJump,prepared,displayed,mapRequest,result,resolution;
  const staging=document.createElement('canvas');
  const allowed=()=>!disposed&&state().ready&&state().visible&&!state().frozen;
  const history=createHistory(terminal,receive),minimap=createMinimap(terminal,host,row=>near(row,0,true),allowed,schedulePaint);
  function geometry(){
    const screen=terminal.element?.querySelector('.xterm-screen');if(!screen)return;
    const a=host.getBoundingClientRect(),b=screen.getBoundingClientRect(),h=history.state();
    const g={x:b.left-a.left,y:b.top-a.top,cw:b.width/terminal.cols,ch:b.height/terminal.rows,width:host.clientWidth,height:host.clientHeight,scale:devicePixelRatio||1,head:h.head,start:h.head+terminal.buffer.active.viewportY,rows:terminal.rows};
    g.key=[g.x,g.y,g.cw,g.ch,g.width,g.height,g.scale,g.head,g.start,g.rows,terminal.cols].join(',');return g;
  }
  function persist(){if(!allowed())return;clearTimeout(saveTimer);saveTimer=setTimeout(()=>{saveTimer=undefined;if(allowed())void save(metadata());},300);}
  function metadata(){
    const head=history.state().head,buffer=terminal.buffer.active,draft=open?input.value:query,match=draft===query&&current&&matches(current)?current:undefined;
    return {search:{open,query:draft,caseSensitive,current:match?{start:{...match.start,row:match.start.row-head},end:{...match.end,row:match.end.row-head}}:undefined},viewportY:buffer.viewportY,atBottom:buffer.viewportY>=buffer.baseY,selection:terminal.getSelectionPosition()||null};
  }
  function matches(match){
    if(!query||!match||match.end.row<match.start.row)return false;
    const {head}=history.state(),cell=terminal.buffer.active.getNullCell();let text='';
    for(let id=match.start.row;id<=match.end.row;id++){
      const line=terminal.buffer.active.getLine(id-head);if(!line||id>match.start.row&&!line.isWrapped)return false;
      const s=rowText(line,cell),from=id===match.start.row?match.start.offset:0,to=id===match.end.row?match.end.offset:s.length;
      if(!Number.isInteger(from)||!Number.isInteger(to)||from<0||to<from||to>s.length)return false;text+=s.slice(from,to);
    }
    if(caseSensitive)return text===query;
    const folded=fold(text),needle=fold(query);
    // A lowercase expansion can map a partial folded match to one whole original character.
    for(let at=folded.indexOf(needle);at>=0;at=folded.indexOf(needle,at+1))if(originalOffset(text,at)===0&&originalOffset(text,at+needle.length,true)===text.length)return true;
    return false;
  }
  function canvasContext(canvas,g){
    const width=Math.max(1,Math.round(g.width*g.scale)),height=Math.max(1,Math.round(g.height*g.scale));
    if(canvas.width!==width)canvas.width=width;if(canvas.height!==height)canvas.height=height;
    const ctx=canvas.getContext('2d');ctx.setTransform(g.scale,0,0,g.scale,0,0);return ctx;
  }
  function clear(){const ctx=overlay.getContext('2d');ctx.clearRect(0,0,overlay.width,overlay.height);displayed=undefined;}
  function invalidate(){visibleId++;prepared=undefined;clear();}
  function draw(ctx,match,g,active=false){
    ctx.fillStyle=active?'#8b5bc24a':'#dba00045';ctx.strokeStyle=terminal.options.theme?.cursor||'#7650b7';ctx.lineWidth=1.5;
    for(let id=match.start.row;id<=match.end.row;id++){
      const y=id-g.start;if(y<0||y>=g.rows)continue;const line=terminal.buffer.active.getLine(id-g.head);if(!line)continue;
      const from=id===match.start.row?textColumns(line,match.start.offset):0,to=id===match.end.row?textColumns(line,match.end.offset,true):line.length;
      const x=g.x+from*g.cw,width=Math.max(1,(to-from)*g.cw),py=g.y+y*g.ch;ctx.fillRect(x,py,width,g.ch);if(active)ctx.strokeRect(x+.75,py+.75,Math.max(1,width-1.5),Math.max(1,g.ch-1.5));
    }
  }
  function activeKey(){return current?[current.start.row,current.start.offset,current.end.row,current.end.offset].join(','):'';}
  function valid(view){return view&&view.g.key===geometry()?.key;}
  function commit(){
    if(!prepared?.complete||!allowed()||!open||!valid(prepared))return;
    const ctx=canvasContext(overlay,prepared.g);ctx.clearRect(0,0,prepared.g.width,prepared.g.height);ctx.drawImage(staging,0,0,prepared.g.width,prepared.g.height);
    if(current&&matches(current))draw(ctx,current,prepared.g,true);
    displayed={...prepared,active:activeKey()};prepared=undefined;
  }
  function refresh(){
    if(!allowed())return;const g=geometry();if(!g)return;
    latest.hidden=terminal.buffer.active.viewportY>=terminal.buffer.active.baseY;minimap.layout();
    if(displayed&&!valid(displayed)||prepared&&!valid(prepared))invalidate();
    commit();minimap.selection(open&&!busy&&!failed&&Boolean(query),current);
    const h=history.state();if(h.indexing||result?.epoch!==h.epoch||result?.revision!==h.revision||result?.queryRevision!==queryRevision)return;
    const height=minimap.height();
    if(!mapRequest||mapRequest.revision!==h.revision||mapRequest.queryRevision!==queryRevision||mapRequest.height!==height){mapRequest={revision:h.revision,queryRevision,height,id:++mapId};history.send({type:'map',height,queryRevision,requestId:mapId});}
    if(!open||!query||failed||displayed&&displayed.active===activeKey())return;
    if(prepared&&prepared.revision===h.revision)return;
    const ctx=canvasContext(staging,g);ctx.clearRect(0,0,g.width,g.height);
    prepared={g,id:++visibleId,revision:h.revision,start:h.head,end:h.head+h.length-1,complete:false};
    history.send({type:'visible',queryRevision,start:g.start,end:g.start+g.rows-1,requestId:visibleId});
  }
  function schedulePaint(){if(allowed()&&!paintFrame)paintFrame=requestAnimationFrame(()=>{paintFrame=undefined;refresh();});}
  function text(node,value){if(node.textContent!==value)node.textContent=value;}
  function status(){
    if(!allowed())return;
    const pressed=String(caseSensitive);if(sensitive.getAttribute('aria-pressed')!==pressed)sensitive.setAttribute('aria-pressed',pressed);
    previous.disabled=next.disabled=!query||!total||failed;retry.hidden=!failed;
    text(count,failed?'搜索暂不可用':!query?'':busy?'正在查找…':!total?'没有匹配':`${ordinal==null?'—':ordinal+1} / ${total.toLocaleString()}`);
    text(scope,(terminal.buffer.active.type==='alternate'?'当前应用屏幕':'当前终端 · 最多 10 万行历史')+(updating?' · 结果更新中':''));
  }
  function markUpdating(){
    if(!allowed()||!open||!query||busy||lagTimer||updating)return;
    lagTimer=setTimeout(()=>{lagTimer=undefined;if(allowed()){updating=true;status();}},500);
  }
  function sendQuery(){history.send({type:'query',query:open?query:'',queryRevision,caseSensitive,anchorRow:history.state().head+terminal.buffer.active.viewportY});}
  function execute(saved){
    clearTimeout(timer);timer=undefined;if(composing||disposed)return;query=input.value;queryRevision++;current=undefined;ordinal=undefined;total=0;failed=false;busy=open&&Boolean(query);pendingJump=result=undefined;
    clearTimeout(lagTimer);lagTimer=undefined;updating=false;
    if(saved&&['start','end'].every(k=>Number.isInteger(saved[k]?.row)&&saved[k].row>=0&&saved[k].row<terminal.buffer.active.length&&Number.isInteger(saved[k].offset)&&saved[k].offset>=0)&&saved.end.row>=saved.start.row){const head=history.state().head;const match={start:{...saved.start,row:saved.start.row+head},end:{...saved.end,row:saved.end.row+head}};if(matches(match))current=match;}
    if(allowed()){invalidate();minimap.selection(false);status();}sendQuery();persist();
  }
  function find(delay=60){clearTimeout(timer);timer=setTimeout(execute,delay);}
  function near(row,offset=0,jump=false){
    if(!allowed()||!open||!query)return;
    const message={type:'near',row,offset,queryRevision,requestId:++requestId};pendingJump={id:requestId,jump,message};history.send(message);
  }
  function navigate(delta){
    if(!allowed()||!open||!query||!total||composing)return;delta+=pendingJump?.delta||0;
    const message=current&&matches(current)?{type:'near',row:current.start.row,offset:current.start.offset,delta,queryRevision,requestId:++requestId}:{type:'locate',ordinal:delta>0?delta-1:delta,queryRevision,requestId:++requestId};
    pendingJump={id:requestId,jump:true,delta,message};history.send(message);
  }
  function receive(message){
    if(disposed)return;
    if(message.type==='reset'){
      let candidate;if(current&&message.previousType===terminal.buffer.active.type)candidate={start:{...current.start,row:current.start.row-message.previousHead},end:{...current.end,row:current.end.row-message.previousHead}};
      current=candidate&&matches(candidate)?candidate:undefined;ordinal=undefined;pendingJump=result=mapRequest=undefined;busy=open&&Boolean(query);visibleId++;prepared=displayed=undefined;
      if(allowed()){clear();status();}sendQuery();return;
    }
    if(message.type==='changed'){
      for(const view of [displayed,prepared])if(view&&(view.g.head!==message.head||message.start<=view.end&&message.end>=view.start)){if(allowed())invalidate();else{visibleId++;prepared=displayed=undefined;}break;}
      if(current&&!matches(current)){current=undefined;ordinal=undefined;}markUpdating();schedulePaint();return;
    }
    if(message.type==='updating')return;
    if(message.type==='error'&&message.fatal){failed=true;busy=false;result=undefined;if(allowed()){status();invalidate();}return;}
    const h=history.state();if(message.epoch!==h.epoch||message.queryRevision!==queryRevision||!allowed())return;
    if(message.type==='error'){
      if(message.revision!==h.revision)return;
      if(message.operation==='visible'&&message.requestId!==prepared?.id||message.operation==='map'&&message.requestId!==mapRequest?.id||['near','locate'].includes(message.operation)&&message.requestId!==pendingJump?.id)return;
      failed=true;busy=false;status();invalidate();return;
    }
    if(message.type==='map'){
      if(message.requestId!==mapRequest?.id||message.revision!==h.revision||h.indexing)return;minimap.receive(message);return;
    }
    if(message.type==='visible'){
      if(!open||message.requestId!==prepared?.id||!valid(prepared))return;
      if(message.begin){prepared.start=message.start;prepared.end=message.end;}
      const ctx=staging.getContext('2d');for(const match of message.matches){if(!matches(match)){invalidate();return;}draw(ctx,match,prepared.g);}
      if(message.complete){prepared.complete=true;schedulePaint();}return;
    }
    if(message.type!=='result'||message.revision!==h.revision||h.indexing)return;
    if(message.match){
      if(!open||message.requestId!==pendingJump?.id||!matches(message.match))return;
      const jump=pendingJump.jump;pendingJump=undefined;current=message.match;ordinal=message.ordinal;total=message.total;
      if(jump)terminal.scrollToLine(Math.max(0,current.start.row-h.head-Math.floor(terminal.rows/2)));persist();
    }else{
      result=message;
      if(open){total=message.total;if(!total){current=undefined;ordinal=undefined;pendingJump=undefined;invalidate();}else if(pendingJump)history.send(pendingJump.message);else if(current&&matches(current))near(current.start.row,current.start.offset,false);else near(h.head+terminal.buffer.active.viewportY,0,true);}
    }
    busy=false;failed=false;updating=false;clearTimeout(lagTimer);lagTimer=undefined;status();schedulePaint();
  }
  function show(){
    if(disposed||!allowed())return;
    if(open){input.focus();input.select();return;}
    open=true;panel.hidden=false;const selection=terminal.getSelection(),saved=metadata().search.current;
    input.value=selection&&!/[\r\n]/.test(selection)&&selection.length<=1024?selection:query;input.focus();input.select();execute(saved);
  }
  function hide(){
    clearTimeout(timer);timer=undefined;query=input.value;open=false;panel.hidden=true;queryRevision++;pendingJump=result=undefined;invalidate();minimap.selection(false);sendQuery();terminal.focus();persist();
  }
  function pause(){
    clearTimeout(timer);timer=undefined;clearTimeout(saveTimer);saveTimer=undefined;clearTimeout(lagTimer);lagTimer=undefined;cancelAnimationFrame(paintFrame);paintFrame=undefined;
    visibleId++;mapId++;mapRequest=undefined;prepared=displayed=undefined;requestId++;pendingJump=undefined;minimap.pause();
  }
  function resume(){if(!allowed())return;invalidate();if(open&&input.value!==query&&!composing)execute();else{queryRevision++;status();result=mapRequest=undefined;sendQuery();schedulePaint();}}
  function watchResolution(){resolution?.removeEventListener('change',resolutionChanged);resolution=matchMedia(`(resolution: ${devicePixelRatio||1}dppx)`);resolution.addEventListener('change',resolutionChanged);}
  function resolutionChanged(){watchResolution();if(allowed()){invalidate();schedulePaint();}}
  watchResolution();const resize=new ResizeObserver(schedulePaint);resize.observe(host);
  input.addEventListener('input',()=>{if(!composing)find();});input.addEventListener('compositionstart',()=>{composing=true;clearTimeout(timer);timer=undefined;});input.addEventListener('compositionend',()=>{composing=false;find();});
  panel.addEventListener('keydown',event=>{if(composing||event.isComposing)return;if(event.key==='Escape'){event.preventDefault();event.stopPropagation();hide();}else if(event.key==='Enter'){event.preventDefault();event.stopPropagation();if(timer)execute();navigate(event.shiftKey?-1:1);}});
  sensitive.onclick=()=>{caseSensitive=!caseSensitive;execute();input.focus();};previous.onclick=()=>navigate(-1);next.onclick=()=>navigate(1);close.onclick=hide;
  retry.onclick=()=>{failed=false;history.retry();execute();};latest.onclick=()=>{terminal.scrollToBottom();terminal.focus();persist();};
  const scroll=terminal.onScroll(()=>{schedulePaint();persist();}),render=terminal.onRender(schedulePaint),selection=terminal.onSelectionChange(persist),parsed=terminal.onWriteParsed(()=>{if(state().ready)history.update();});
  return {show,focus(){if(open&&allowed()){input.focus();return true;}return false;},update(reset=false){history.update(reset);schedulePaint();},metadata,restore(value){if(!value?.search)return;open=value.search.open===true;query=typeof value.search.query==='string'?value.search.query.slice(0,1024):'';caseSensitive=value.search.caseSensitive===true;input.value=query;panel.hidden=!open;execute(value.search.current);},theme(){minimap.theme();if(allowed()){displayed=undefined;schedulePaint();}},pause,resume,composing(){return composing;},flush(){if(composing)throw new Error('搜索输入仍在组合，请完成输入后再移动');clearTimeout(saveTimer);saveTimer=undefined;return save(metadata());},dispose(){disposed=true;pause();resolution?.removeEventListener('change',resolutionChanged);resize.disconnect();scroll.dispose();render.dispose();selection.dispose();parsed.dispose();history.dispose();minimap.dispose();staging.width=staging.height=1;panel.remove();overlay.remove();latest.remove();}};
}
