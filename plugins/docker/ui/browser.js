import {ResourceTree,icon} from './tree.js';
import {TreeMenu} from './tree-menu.js';
const $=id=>document.getElementById(id);
const operationNames={start:'启动',stop:'停止',restart:'重启',pause:'暂停',unpause:'恢复',remove:'删除容器','resource.remove':'删除资源'};
const phases={checking:'检查中',running:'执行中',opening:'启动中',succeeded:'成功',failed:'失败',unknown:'结果未确认',skipped:'无需操作'};
const available=(op,status)=>({start:['created','exited','dead'],stop:['running','paused','restarting'],restart:['running'],pause:['running'],unpause:['paused'],remove:['created','exited','dead']}[op]||[]).includes(status);
export function installBrowserPage(releaseIncumbent){
  let active=false,enabled=false,frozen=false,visible=true,snapshot,epoch=0,timer,nav={},navTail=Promise.resolve(),navSequence=0,navPainted=0,composing=false,searchBefore,restoreScroll,selectedNode,formTarget,submitting=false,taskOffset=0,outputId,outputOffset='0',outputRemainder='',outputText='';
  const pending=new Set();
  let outputDecoder=new TextDecoder(),sourceDecoders=new Map();
  const menu=new TreeMenu($('tree-menu'),$('browser-panel'));
  const tree=new ResourceTree($('tree-viewport'),{select,toggle,open,menu:openMenu,window:windowPage,focusIndex,focusKey,rootPath:()=>snapshot?.engine?.dockerRootDir||'未读取'});
  const identity=()=>({engineId:snapshot.engine?.id,endpoint:snapshot.endpoint,expectedContext:structuredClone(snapshot.context),scopeEpoch:snapshot.scopeEpoch});
  const current=target=>enabled&&!frozen&&!submitting&&snapshot?.state==='ready'&&!snapshot.busy&&target?.engineId===snapshot.engine?.id&&target?.endpoint===snapshot.endpoint&&target?.scopeEpoch===snapshot.scopeEpoch&&JSON.stringify(target?.expectedContext)===JSON.stringify(snapshot.context);
  function message(text){SoDeckMotion.setMotionText($('notice'),text||'');$('notice').hidden=!text;}
  function theme(values){for(const [key,value]of Object.entries(values||{}))key==='color-scheme'?document.documentElement.style.colorScheme=value:document.documentElement.style.setProperty('--'+key,value);}
  async function request(data){const local=epoch,promise=sodeck.request('event',data);pending.add(promise);try{const result=await promise;if(local!==epoch)return null;return result;}finally{pending.delete(promise);}}
  async function send(data){if(!enabled||frozen)return null;try{const result=await request(data);if(result?.kind&&result?.revision)apply(result);message('');return result;}catch(error){message(error.message||String(error));return null;}}
  function apply(data,paint=true){if(!data)return;if(snapshot&&data.scopeEpoch===snapshot.scopeEpoch&&BigInt(data.revision||0)<BigInt(snapshot.revision||0))return;
    if(snapshot&&data.scopeEpoch!==snapshot.scopeEpoch){nav.selected='';nav.expandedKeys=['engine','category:container'];nav.offset=0;menu.close(false);formTarget=null;searchBefore=null;tree.positions.clear();}
    snapshot=data;render(paint&&navPainted===navSequence);
  }
  function state(){return {...nav,schemaVersion:2,query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked,scrollTop:$('tree-viewport').scrollTop,anchor:tree.anchor(),engineId:snapshot.engine?.id,endpoint:snapshot.endpoint};}
  function remember(){if(!enabled||frozen)return Promise.resolve();const data=state(),local=epoch,serial=++navSequence;
    navTail=navTail.catch(()=>{}).then(async()=>{if(local!==epoch)return;const result=await request({action:'browser.navigation',scopeEpoch:snapshot.scopeEpoch,state:data});if(local!==epoch||!result)return;if(serial===navSequence){navPainted=serial;apply(result,true);}}).catch(error=>message(error.message));return navTail;
  }
  function nodeInCache(key){return tree.node(key)||selectedNode?.key===key&&selectedNode;}
  function render(paint=true){if(!active||!snapshot)return;
    $('title').textContent='Docker';SoDeckMotion.setMotionText($('state'),({ready:'已连接',loading:'连接中',stale:'数据待刷新',gone:'已删除'}[snapshot.state]||'等待连接'));$('state').dataset.state=snapshot.state;
    $('engine-name').textContent=snapshot.engine?.version?'Engine '+snapshot.engine.version:'等待 Engine';$('engine-version').textContent=snapshot.engine?.arch||'';
    $('refresh').disabled=!enabled||frozen||snapshot.busy;$('cancel').hidden=!snapshot.busy;$('pause').hidden=true;
    $('browser-more').disabled=!enabled||frozen;
    $('connection').hidden=false;if(document.activeElement!==$('socket'))$('socket').value=snapshot.endpoint;
    $('freshness').textContent=snapshot.state==='stale'?'离线快照 · 待刷新':snapshot.observedAt?'采集于 '+new Date(snapshot.observedAt).toLocaleTimeString():'等待首次采集';
    $('busy').textContent=snapshot.busy?'请求处理中':'';$('count').textContent='';$('operation').hidden=true;
    if(snapshot.error)message(snapshot.error);
    if(paint&&snapshot.tree){tree.render(snapshot.tree,nav.selected);if(restoreScroll!==undefined){$('tree-viewport').scrollTop=restoreScroll;restoreScroll=undefined;}const operation=snapshot.operation;if(selectedNode?.resource&&!tree.node(nav.selected)&&operation?.state==='succeeded'&&['remove','resource.remove'].includes(operation.action)&&(operation.containerId===selectedNode.resource.id||operation.resource?.id===selectedNode.resource.id)){const index=tree.positions.get(nav.selected)||0;nav.selected=tree.page.nodes[Math.max(0,Math.min(tree.page.nodes.length-1,index-tree.page.offset))]?.key;void remember();}selectedNode=nodeInCache(nav.selected);}
    renderSelection();
    if(formTarget)menu.valid(current(formTarget)&&(!formTarget.observationRevision||snapshot.indexes?.[formTarget.resource.kind]?.revision===formTarget.observationRevision));
    const pull=snapshot.pull;$('pull-status').hidden=!pull;$('pull-status').textContent=pull?`${pull.reference} · ${phases[pull.state]||pull.state}：${pull.message||''}`:'';
    $('pull-cancel').hidden=!pull||!['running','opening'].includes(pull.state);$('pull-cancel').disabled=!enabled||frozen;
  }
  function renderSelection(){const n=nodeInCache(nav.selected);const box=$('browser-selection');box.hidden=!n;if(!n)return;
    $('selected-name').textContent=n.label;$('selected-kind').textContent=n.type==='root'?'数据目录':({container:'容器',image:'镜像',network:'网络',volume:'卷'}[n.resource?.kind]||'');
    $('selected-value').textContent=n.type==='root'?(snapshot.engine?.dockerRootDir||'Docker 未返回数据根目录'):n.resource?.id||n.meta||'';
    const tools=$('selected-actions');const rebuilding=box.dataset.key!==n.key;if(rebuilding)tools.replaceChildren();box.dataset.key=n.key;if(rebuilding)SoDeckMotion.revealMotion(box);
    const button=(text,fn,disabled=false)=>{const b=document.createElement('button');b.type='button';b.textContent=text;b.disabled=disabled;b.onclick=fn;tools.append(b);};
    if(rebuilding) {if(n.type==='root'){button('完整路径',()=>showRoot(n));button('复制路径',()=>copy(snapshot.engine?.dockerRootDir),!snapshot.engine?.dockerRootDir);}
    else if(n.resource?.kind==='container'&&n.type==='resource'){button('日志',()=>openView('openLogs',n));if(rebuilding)tools.lastElementChild.dataset.focus='openLogs';button('终端',()=>terminal(n,nav.shell||'/bin/sh'),n.status!=='running'||!current(identity())||snapshot.terminal?.status==='pending');}
    else if(n.resource&&n.type==='resource'){button('详情',()=>openView('openResource',n),!enabled||frozen);}
    button('更多',()=>openMenu(nodeInCache(nav.selected),tools.lastElementChild),!enabled||frozen);}
    for(const b of tools.children)b.disabled=!enabled||frozen||(b.textContent==='终端'&&(n.status!=='running'||!current(identity())||snapshot.terminal?.status==='pending'))||(b.textContent==='复制路径'&&!snapshot.engine?.dockerRootDir);
    const op=snapshot.operation,terminalResult=snapshot.terminal;let text='';
    if(op&&n.resource&&(op.containerId===n.resource.id||op.resource?.id===n.resource.id))text=`${operationNames[op.action]||op.action} · ${phases[op.state]||op.state}：${op.message||''}`;
    if(terminalResult&&n.resource&&terminalResult.containerId===n.resource.id)text+=(text?'\n':'')+({pending:'终端请求待确认',completed:'终端页面已创建；连接结果请在终端查看',failed:'终端请求失败',unknown:'终端结果未确认'}[terminalResult.status]||terminalResult.status)+(terminalResult.error?.message?'：'+terminalResult.error.message:'');
    $('selection-result').textContent=text;$('selection-result').hidden=!text;
  }
  function select(n,focus){if(!n)return;if(formTarget&&n.key!==nav.selected){menu.close(false);formTarget=null;}nav.selected=n.key;selectedNode=n;tree.render(tree.page,nav.selected);renderSelection();if(focus)tree.focus(n.key);void remember();}
  async function toggle(n){if(!n?.expandable)return;const keys=new Set(nav.expandedKeys||[]);if(keys.has(n.key))keys.delete(n.key);else{if(keys.size>=64){message('最多保存 64 个展开节点，请先折叠部分分支');return;}keys.add(n.key);}nav.expandedKeys=[...keys];await remember();
    if(keys.has(n.key)&&n.type==='category'&&['unloaded','failed','stale'].includes(snapshot.indexes?.[n.branchKind]?.phase))await load(n.branchKind);
    if(keys.has(n.key)&&n.type==='resource')await load(null,n.resource);
  }
  async function load(branchKind,resource){return send({action:'browser.load',branchKind,resource,scopeEpoch:snapshot.scopeEpoch});}
  function windowPage(offset){if(!active||!enabled||frozen)return;const page=tree.page;const count=Math.ceil($('tree-viewport').clientHeight/30)+5;if(offset>=page.offset&&offset+count<=page.offset+(page.nodes?.length||0)||page.offset+(page.nodes?.length||0)>=page.total&&offset>=page.offset)return;
    offset=Math.min(offset,Math.max(0,page.total-10));if(offset===nav.offset)return;nav.offset=offset;void remember();}
  async function focusIndex(index,parent){if(index<0||index>=tree.page.total)return;const previousOffset=nav.offset;let n=tree.page.nodes[index-tree.page.offset];if(!n){nav.offset=Math.max(0,index-5);await remember();n=tree.page.nodes[index-tree.page.offset];}if(parent&&n?.parent!==parent){if(nav.offset!==previousOffset){nav.offset=previousOffset;await remember();tree.focus(parent);}return;}if(n)select(n,true);}
  async function focusKey(key){if(tree.node(key)){select(tree.node(key),true);return;}const index=tree.positions.get(key);if(index!==undefined){await focusIndex(index);return;}nav.offset=0;await remember();const n=tree.node(key);if(n)select(n,true);}
  async function locate(resource){let result=await send({action:'browser.locate',resource,scopeEpoch:snapshot.scopeEpoch});if(!result)return;
    if(!result.found){await load(resource.kind);message('正在读取目标所属分类，请再次定位');return;}
    nav.revealTarget=resource;nav.expandedKeys=[...new Set([...(nav.expandedKeys||[]),...result.expandedKeys])].slice(-64);nav.selected=result.key;nav.offset=0;await remember();
    if(!tree.node(result.key)){for(let offset=tree.page.nodes.length;offset<tree.page.total;offset+=Math.max(1,tree.page.nodes.length)){nav.offset=offset;await remember();if(tree.node(result.key))break;}}
    selectedNode=tree.node(result.key);renderSelection();tree.focus(result.key);$('reveal-condition').hidden=false;
  }
  function open(n){if(!n)return;if(n.type==='reference')void locate(n.resource);else if(n.type==='root')showRoot(n);else if(n.type==='load'){if(n.loading)return;void load(n.branchKind,n.resource);}else if(n.type==='resource')void openView(n.resource.kind==='container'?'openContainer':'openResource',n);else if(n.expandable)void toggle(n);}
  async function openView(action,n){if(!n?.resource)return;await send({action,resource:n.resource,id:n.resource.id,...identity(),requestId:crypto.randomUUID()});}
  async function copy(text){if(!text)return;const local=epoch;try{await sodeckClipboard.writeText(text,()=>local===epoch&&enabled&&!frozen);message('已复制');}catch(error){message(error.message);}}
  function showRoot(n){menu.restore=()=>tree.focus(n.key);formTarget=null;menu.form({title:'Docker Root',text:snapshot.engine?.dockerRootDir||'未读取。请刷新 Engine 连接。',impact:'当前 Engine 的远端数据根目录。',submit:'复制路径',onSubmit:()=>copy(snapshot.engine?.dockerRootDir)});}
  function openMenu(n,trigger){if(!n||!enabled||frozen)return;select(n,false);formTarget=null;const target=identity();const writable=current(target);const items=[];const add=(label,run,disabled=false,reason='',danger=false)=>items.push({label,run,disabled,reason,danger});
    if(n.type==='root'){add('查看完整路径',()=>showRoot(n));add('复制路径',()=>copy(snapshot.engine?.dockerRootDir),!snapshot.engine?.dockerRootDir);}
    else if(n.type==='engine'){
      add('全部折叠',()=>{nav.expandedKeys=['engine'];nav.offset=0;void remember();},!!nav.query,'清空搜索后可折叠');add('刷新资源树',()=>send({action:'refresh'}),snapshot.busy);add('连接信息',()=>{menu.form({title:'Docker Engine',text:[snapshot.engine?.name,snapshot.engine?.version,snapshot.engine?.arch,snapshot.engine?.id,snapshot.endpoint,JSON.stringify(snapshot.context)].filter(Boolean).join('\n'),submit:'关闭',onSubmit:()=>{}});});
      add('连接设置',()=>{$('connection').open=true;$('socket').focus();});add(snapshot.paused?'恢复自动刷新':'暂停自动刷新',()=>send({action:'pauseRefresh',paused:!snapshot.paused}));add('拉取公开镜像',()=>pull());add('查看任务',()=>{$('browser-tasks').open=true;void tasks();});
    }else if(n.type==='category'||n.type==='group'){
      add(n.expanded?'折叠':'展开',()=>toggle(n));if(n.branchKind)add('刷新此分类',()=>load(n.branchKind));if(n.branchKind==='image')add('拉取公开镜像',()=>pull(n.type==='group'?n.label:''));
    }else if(n.type==='reference'){add('定位资源',()=>locate(n.resource));add('复制引用',()=>copy(n.resource.id));}
    else if(n.type==='load'){add('重新读取',()=>load(n.branchKind,n.resource),n.loading);}
    else if(n.resource){
      add('查看详情',()=>openView(n.resource.kind==='container'?'openContainer':'openResource',n));add('读取资源关系',()=>load(null,n.resource));
      if(n.resource.kind==='container'){
        add('查看日志',()=>openView('openLogs',n));add('打开终端',()=>terminal(n,nav.shell||'/bin/sh'),!writable||n.status!=='running'||snapshot.terminal?.status==='pending','容器未运行或终端请求尚未完成');add('终端选项',()=>shell(n),!writable||n.status!=='running');items.push({separator:true});
        for(const action of ['start','stop','restart','pause','unpause','remove'])if(available(action,n.status))add(operationNames[action],()=>operate(action,n,target),!writable,'连接待刷新或请求处理中',action==='remove');
      }else if(!n.system){items.push({separator:true});add('删除'+({image:'镜像',network:'网络',volume:'卷'}[n.resource.kind]||'资源'),()=>remove(n,target),!writable||!n.canDelete,!n.canDelete?'请展开并完整读取引用；有引用的资源不能删除':'连接待刷新',true);}
      items.push({separator:true});add('复制'+(n.resource.kind==='volume'?'名称':'完整 ID'),()=>copy(n.resource.id));
    }
    if(!items.length)add('复制名称',()=>copy(n.label));menu.open(n,items,trigger,()=>trigger?.id==='browser-more'?trigger.focus():tree.focus(n.key));
  }
  function operate(action,n,target){const request={action:'operate',operation:action,id:n.resource.id,...target,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent};
    if(['start','unpause'].includes(action)){void submit(request);return;}formTarget=target;
    menu.form({title:'确认'+operationNames[action],text:`${n.label}\n${n.resource.id}\n${snapshot.engine?.name||'Engine'}`,impact:action==='remove'?'删除容器可写层，保留挂载卷和 bind 数据。':action==='pause'?'容器进程将暂停，服务可能无法响应。':'服务可能中断；取消等待不会回滚 Engine 已接受的操作。',submit:operationNames[action],danger:action==='remove',onSubmit:()=>submit(request)});
  }
  async function submit(request){if(!current(request))return;formTarget=null;submitting=true;render(false);try{await send(request);}finally{submitting=false;await poll();}}
  async function remove(n,target){const object=await send({action:'browser.object',resource:n.resource,scopeEpoch:target.scopeEpoch});if(!object||!current(target))return;formTarget={...target,resource:n.resource,observationRevision:n.revision};const request={action:'resource.operate',operation:'remove',resource:n.resource,...target,observationRevision:n.revision,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent};
    menu.form({title:'确认删除'+({image:'镜像',network:'网络',volume:'卷'}[n.resource.kind]),text:`${n.label}\n${n.resource.id}\n${snapshot.engine?.name||'Engine'}${object.aliases?.length?'\n受影响标签：\n'+object.aliases.join('\n')+(object.aliasesTruncated?'\n标签过多，共 '+object.aliasesTotal+' 个，请在详情核对。':''):''}`,impact:n.resource.kind==='volume'?'将永久删除卷中的数据。删除前重新检查全部容器引用，包括停止容器；Docker 卷没有唯一 ID，无法完全消除外部同名重建的并发风险。':n.resource.kind==='image'?'按完整镜像 ID 删除对象，影响其全部标签和摘要引用。Docker 可能因多标签或引用保护而拒绝；不会强制删除或逐个解除标签。':'仅删除无连接端点的自定义网络，不使用强制删除。',submit:'删除',danger:true,onSubmit:()=>submit(request)});menu.valid(current(formTarget)&&object.revision===n.revision);
  }
  function shell(n){formTarget=identity();menu.form({title:'容器终端',text:n.label,input:true,label:'容器 Shell',value:nav.shell||'/bin/sh',submit:'打开终端',onSubmit:value=>{nav.shell=value;void remember();void terminal(n,value);}});}
  async function terminal(n,shell){const target=identity();if(!current(target)||snapshot.terminal?.status==='pending')return;await send({action:'openTerminal',id:n.resource.id,...target,shell,requestId:crypto.randomUUID()});renderSelection();}
  function pull(prefill=''){const target=identity();formTarget=target;menu.form({title:'拉取公开镜像',text:'使用当前服务器的 Docker Engine。未填写标签时使用并展示 :latest。',input:true,label:'镜像引用',value:prefill,submit:'检查引用',onSubmit:value=>{
    const tail=value.trim().split('/').at(-1);const reference=value.includes('@')||tail?.includes(':')?value.trim():value.trim()+':latest';
    formTarget=target;menu.form({title:'确认拉取镜像',text:reference,impact:'仅支持无需登录的公开源；拉取会继续运行，隐藏侧栏不会停止任务。',submit:'拉取',onSubmit:()=>submit({action:'pull.start',reference,...target,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent})});
  }});}
  async function tasks(){if(!enabled||frozen)return;const page=await send({action:'browser.tasks',offset:taskOffset});if(!page)return;const list=$('task-list');list.replaceChildren();for(const task of page.rows){const row=document.createElement('div');row.className='docker-task';const title=document.createElement('span');title.textContent=[operationNames[task.action]||'拉取',task.name||task.reference,phases[task.state]||task.state].filter(Boolean).join(' · ');const detail=document.createElement('p');detail.textContent=task.message||'';row.append(title,detail);list.append(row);}$('task-page').textContent=page.total?`${taskOffset+1}–${taskOffset+page.rows.length} / ${page.total}`:'暂无任务';$('task-prev').disabled=!taskOffset;$('task-next').disabled=page.nextOffset==null;$('task-next').onclick=()=>{taskOffset=page.nextOffset;void tasks();};await output();}
  async function output(){const pull=snapshot.pull;if(!pull?.streamId){$('pull-output').hidden=true;return;}if(outputId!==pull.streamId){outputId=pull.streamId;outputOffset='0';outputText='';outputRemainder='';outputDecoder=new TextDecoder();sourceDecoders=new Map();}
    if(!pull.producedBytes||outputOffset===pull.producedBytes)return;
    try{for(let i=0;i<8&&outputOffset!==pull.producedBytes;i++){const packet=await sodeck.request('data',{op:'sessionStream',action:'read',id:outputId,offset:outputOffset,watermark:pull.producedBytes});const bytes=Uint8Array.from(packet.data||[]);if(!bytes.length)break;outputRemainder+=outputDecoder.decode(bytes,{stream:true});const lines=outputRemainder.split('\n');outputRemainder=lines.pop();for(const line of lines){try{const envelope=JSON.parse(line);if(!sourceDecoders.has(envelope.source))sourceDecoders.set(envelope.source,new TextDecoder());outputText+=sourceDecoders.get(envelope.source).decode(Uint8Array.from(atob(envelope.dataBase64),c=>c.charCodeAt(0)),{stream:true});}catch{}}outputText=outputText.slice(-16384);outputOffset=packet.nextOffset;}$('pull-output').textContent=outputText;$('pull-output').hidden=!outputText;}catch(error){$('pull-output').textContent='输出已释放或不可读取：'+error.message;$('pull-output').hidden=false;}
  }
  async function poll(){if(!active||!enabled||frozen||!visible)return;const local=epoch;try{apply(await request({action:'snapshot'}));if($('browser-tasks').open)await tasks();}catch(error){if(local===epoch)message(error.message);}finally{clearTimeout(timer);if(local===epoch&&active&&enabled&&!frozen)timer=setTimeout(()=>void poll(),1000);}}
  function control(id,type,handler){$(id).addEventListener(type,e=>{if(!active)return;e.stopImmediatePropagation();handler(e);},true);}
  control('refresh','click',()=>void send({action:'refresh'}));control('cancel','click',()=>void send({action:'cancel'}));
  control('browser-more','click',e=>openMenu({key:'engine',type:'engine',label:'Docker Engine'},e.target));
  control('endpoint-form','submit',e=>{e.preventDefault();void send({action:'setEndpoint',endpoint:$('socket').value.trim()});});
  function filter(){if(composing)return;const query=$('query').value;
    if(query&&!searchBefore)searchBefore={expandedKeys:[...(nav.expandedKeys||[])],selected:nav.selected,offset:nav.offset,scrollTop:$('tree-viewport').scrollTop};
    if(!query&&searchBefore){Object.assign(nav,searchBefore);restoreScroll=searchBefore.scrollTop;searchBefore=null;}else{nav.offset=0;$('tree-viewport').scrollTop=0;}
    menu.close(false);formTarget=null;void remember();if(query){for(const [kind,index]of Object.entries(snapshot.indexes||{}))if(['unloaded','failed'].includes(index.phase))void load(kind);}
    for(const b of $('status-tabs').children)b.setAttribute('aria-pressed',String(b.dataset.status===$('status-filter').value));
  }
  control('query','input',filter);control('query','compositionstart',()=>composing=true);control('query','compositionend',()=>{composing=false;filter();});
  for(const id of ['status-filter','health-filter','group'])control(id,'change',filter);
  control('status-tabs','click',e=>{const button=e.target.closest('[data-status]');if(button){$('status-filter').value=button.dataset.status;filter();}});
  control('clear-filters','click',()=>{$('query').value='';$('status-filter').value='all';$('health-filter').value='all';filter();});
  control('reveal-condition','click',()=>{nav.revealTarget=null;$('reveal-condition').hidden=true;void remember();});
  control('browser-tasks','toggle',()=>{if($('browser-tasks').open)void tasks();});control('task-prev','click',()=>{taskOffset=Math.max(0,taskOffset-10);void tasks();});
  control('pull-cancel','click',()=>void send({action:'pull.cancel'}));
  const lifecycle=(name,handler)=>addEventListener('sodeck:'+name,e=>{if(!active)return;e.stopImmediatePropagation();Promise.resolve(handler(e.detail)).catch(error=>message(error.message));});
  addEventListener('sodeck:mount',e=>{if(e.detail.data.kind!=='browser'){if(active){clearTimeout(timer);menu.close(false);}active=false;$('browser-more').hidden=true;document.body.classList.remove('browser-page');return;}e.stopImmediatePropagation();releaseIncumbent();epoch++;active=true;enabled=false;frozen=!!e.detail.readOnly;visible=e.detail.visible!==false;snapshot=e.detail.data;navTail=Promise.resolve();navSequence=navPainted=0;nav={schemaVersion:2,expandedKeys:['engine','category:container'],status:'all',health:'all',group:true,offset:0,...snapshot.navigation};if(/^[a-f0-9]{64}$/i.test(nav.selected||''))nav.selected='container:'+nav.selected;selectedNode=null;formTarget=null;searchBefore=null;delete $('browser-selection').dataset.key;menu.close(false);tree.positions.clear();document.body.classList.add('browser-page');
    for(const id of ['container','logs','resource'])$(id).hidden=true;$('browser').hidden=false;$('browser-more').hidden=false;
    $('query').value=nav.query||'';$('status-filter').value=nav.status||'all';$('health-filter').value=nav.health||'all';$('group').checked=nav.group!==false;$('reveal-condition').hidden=!nav.revealTarget;theme(e.detail.tokens);render();$('tree-viewport').scrollTop=nav.scrollTop||0;sodeck.mounted();
  });
  lifecycle('surfaceReady',async()=>{enabled=true;frozen=false;await send({action:'resumeInput'});await send({action:'browser.visibility',visible});await remember();void poll();});
  lifecycle('visibilityChanged',async e=>{visible=e.visible!==false;if(enabled&&!frozen)await send({action:'browser.visibility',visible});if(visible)void poll();});
  lifecycle('themeChanged',e=>theme(e.tokens));lifecycle('settingsChanged',()=>poll());
  lifecycle('contextChanged',()=>{epoch++;menu.close(false);formTarget=null;nav.selected='';nav.expandedKeys=['engine','category:container'];nav.offset=0;void poll();});
  lifecycle('transferCancelled',async()=>{enabled=true;frozen=false;await send({action:'resumeInput'});void poll();});
  lifecycle('flush',async e=>{if(composing||menu.composing){sodeck.flushed(e.id,'请完成输入法组合后移动');return;}const wasEnabled=enabled&&!frozen;try{if(wasEnabled)await remember();frozen=true;await Promise.allSettled([...pending]);frozen=!!e.freeze;menu.close(false);formTarget=null;sodeck.flushed(e.id);if(!frozen)void poll();}catch(error){frozen=false;sodeck.flushed(e.id,error.message);}});
  lifecycle('unmount',()=>{epoch++;active=false;enabled=false;clearTimeout(timer);menu.close(false);});
  addEventListener('keydown',e=>{if(!active)return;if(e.key==='Escape'){menu.close();formTarget=null;}if((e.ctrlKey||e.metaKey)&&e.key==='f'){e.preventDefault();$('query').focus();}});
}
