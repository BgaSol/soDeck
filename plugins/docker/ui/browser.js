import {ResourceTree,icon,escape} from './tree.js';
import {ResourceDetails} from './resource-details.js';
import {TreeMenu} from './tree-menu.js';
import {installConnectionForm} from './connection.js';
const $=id=>document.getElementById(id);
const operationNames={start:'启动',stop:'停止',restart:'重启',pause:'暂停',unpause:'恢复',remove:'删除容器','resource.remove':'删除资源',terminal:'容器终端',tunnel:'端口隧道'};
const phases={checking:'检查中',running:'执行中',opening:'启动中',succeeded:'成功',failed:'失败',unknown:'结果未确认',skipped:'未执行'};
const available=(op,status)=>({start:['created','exited','dead'],stop:['running','paused','restarting'],restart:['running'],pause:['running'],unpause:['paused'],remove:['created','exited','dead']}[op]||[]).includes(status);
export function installBrowserPage(){
  let active=false,enabled=false,frozen=false,visible=true,snapshot,epoch=0,timer,nav={},navTail=Promise.resolve(),navSequence=0,navPainted=0,composing=false,searchBefore,restoreScroll,selectedNode,formTarget,submitting=false,taskOffset=0,outputId,outputOffset='0',outputRemainder='',outputText='';
  let inlineController,inlineKey='',mountData;
  const pending=new Set();
  let outputDecoder=new TextDecoder(),sourceDecoders=new Map();
  const connection=installConnectionForm(send,()=>enabled&&!frozen);
  const menu=new TreeMenu($('tree-menu'),$('browser-panel'));
  const tree=new ResourceTree($('tree-viewport'),{select,toggle,open,menu:openMenu,window:windowPage,focusIndex,focusKey,visibility:syncVisibility,rootPath:()=>snapshot?.engine?.dockerRootDir||'未读取'});
  const identity=()=>({engineId:snapshot.engine?.id,endpoint:snapshot.endpoint,expectedContext:structuredClone(snapshot.context),scopeEpoch:snapshot.scopeEpoch});
  const current=target=>enabled&&!frozen&&!submitting&&snapshot?.state==='ready'&&!snapshot.busy&&target?.engineId===snapshot.engine?.id&&target?.endpoint===snapshot.endpoint&&target?.scopeEpoch===snapshot.scopeEpoch&&JSON.stringify(target?.expectedContext)===JSON.stringify(snapshot.context);
  function message(text){SoDeckMotion.setMotionText($('notice'),text||'');$('notice').hidden=!text;}
  function theme(values){for(const [key,value]of Object.entries(values||{}))key==='color-scheme'?document.documentElement.style.colorScheme=value:document.documentElement.style.setProperty('--'+key,value);}
  async function request(data){const local=epoch,promise=sodeck.request('event',data);pending.add(promise);try{const result=await promise;if(local!==epoch)return null;return result;}finally{pending.delete(promise);}}
  async function send(data,motion=false){if(!enabled||frozen)return null;try{const result=await request(data);if(result?.kind&&result?.revision)apply(result,true,motion);message('');return result;}catch(error){message(error.message||String(error));return null;}}
  function apply(data,paint=true,motion=false){if(!data)return;if(snapshot&&data.scopeEpoch===snapshot.scopeEpoch&&BigInt(data.revision||0)<BigInt(snapshot.revision||0))return;
    if(snapshot&&data.scopeEpoch!==snapshot.scopeEpoch){nav.inline=null;closeInline();nav.selected='';nav.expandedKeys=['engine','category:container'];nav.offset=0;menu.close(false);formTarget=null;searchBefore=null;tree.positions.clear();}
    snapshot=data;render(paint&&navPainted===navSequence,motion);
  }
  function state(){if(nav.inline?.kind==='resource'&&inlineController)Object.assign(nav.inline,inlineController.navigation());return {...nav,schemaVersion:3,query:$('query').value,status:$('status-filter').value,health:$('health-filter').value,group:$('group').checked,scrollTop:$('tree-viewport').scrollTop,anchor:tree.anchor(),engineId:snapshot.engine?.id,endpoint:snapshot.endpoint};}
  function remember(motion=false){if(!enabled||frozen)return Promise.resolve();const data=state(),local=epoch,serial=++navSequence;
    navTail=navTail.catch(()=>{}).then(async()=>{if(local!==epoch)return;const result=await request({action:'browser.navigation',scopeEpoch:snapshot.scopeEpoch,state:data});if(local!==epoch||!result)return;if(serial===navSequence){navPainted=serial;apply(result,true,motion);}}).catch(error=>message(error.message));return navTail;
  }
  function nodeInCache(key){return tree.node(key)||selectedNode?.key===key&&selectedNode;}
  function render(paint=true,motion=false){if(!active||!snapshot)return;
    $('title').textContent='Docker';SoDeckMotion.setMotionText($('state'),({ready:'已连接',loading:'连接中',stale:'数据待刷新',gone:'已删除'}[snapshot.state]||'等待连接'));$('state').dataset.state=snapshot.state;
    $('engine-name').textContent=snapshot.engine?.version?'Engine '+snapshot.engine.version:'等待 Engine';$('engine-version').textContent=snapshot.engine?.arch||'';
    $('refresh').disabled=!enabled||frozen||snapshot.busy;$('cancel').hidden=!snapshot.busy;$('pause').hidden=true;
    $('browser-more').hidden=false;$('browser-more').disabled=!enabled||frozen;
    connection.render(snapshot);syncVisibility();
    $('freshness').textContent=snapshot.state==='stale'?'离线快照 · 待刷新':snapshot.observedAt?'采集于 '+new Date(snapshot.observedAt).toLocaleTimeString():'等待首次采集';
    $('busy').textContent=snapshot.busy?'请求处理中':'';$('count').textContent='';$('operation').hidden=true;
    if(snapshot.error)message(snapshot.error);else if(snapshot.directoryPublication?.message)message(snapshot.directoryPublication.message);
    if(paint&&snapshot.tree){tree.render(snapshot.tree,nav.selected,motion);if(restoreScroll!==undefined){$('tree-viewport').scrollTop=restoreScroll;restoreScroll=undefined;}const operation=snapshot.operation;if(selectedNode?.resource&&!tree.node(nav.selected)&&operation?.state==='succeeded'&&['remove','resource.remove'].includes(operation.action)&&(operation.containerId===selectedNode.resource.id||operation.resource?.id===selectedNode.resource.id)){const index=tree.positions.get(nav.selected)||0;nav.selected=tree.page.nodes[Math.max(0,Math.min(tree.page.nodes.length-1,index-tree.page.offset))]?.key;void remember();}selectedNode=nodeInCache(nav.selected);}
    if(paint)renderInline();
    if(formTarget)menu.valid(current(formTarget)&&(!formTarget.observationRevision||snapshot.indexes?.[formTarget.resource.kind]?.revision===formTarget.observationRevision));
    const pull=snapshot.pull;$('pull-status').hidden=!pull;$('pull-status').textContent=pull?`${pull.reference} · ${phases[pull.state]||pull.state}：${pull.message||''}`:'';
    $('pull-cancel').hidden=!pull||!['running','opening'].includes(pull.state);$('pull-cancel').disabled=!enabled||frozen;
  }
  function closeInline(){menu.close(false);formTarget=null;inlineController?.dispose();inlineController=null;inlineKey='';for(const id of ['connection','browser-tasks','browser-panel'])$('inline-storage').append($(id));tree.inline.replaceChildren();tree.inline.classList.remove('resource-detail-host');}
  function syncVisibility(){inlineController?.enabled(enabled&&!frozen,visible&&tree.inlineVisible());}
  function actionItems(n){const writable=current(identity()),items=[];const add=(action,label,disabled=false,danger=false)=>items.push({action,label,disabled:!enabled||frozen||disabled,danger});
    if(n.resource?.kind==='container'){add('logs','日志');add('terminal','终端',!writable||n.status!=='running'||snapshot.terminal?.status==='pending');for(const op of ['start','stop','restart','pause','unpause','remove'])if(available(op,n.status)&&!(snapshot.engine?.os==='windows'&&['pause','unpause'].includes(op)))add(op,operationNames[op],!writable,op==='remove');}
    else if(n.resource&&!n.system)add('delete-resource','删除',!writable||!n.canDelete,true);return items;
  }
  async function action(action,n){if(action==='logs')await openView('openLogs',n);else if(action==='terminal')await terminal(n,nav.shell||snapshot.defaultShell||'');else if(action==='copy-id')await copy(n.resource.id);else if(action==='stats')await send({action:'browser.stats',resource:n.resource,...identity()});else if(action==='delete-resource')await remove(n,identity());else if(operationNames[action])operate(action,n,identity());}
  function renderInline(){
    const anchor=snapshot.tree?.inlineAnchor,kind=nav.inline?.kind,key=anchor?`${snapshot.scopeEpoch}:${anchor.node.key}`:'';
    if(!key||!kind){if(inlineKey)closeInline();return;}const n=anchor.node;
    if(inlineKey!==key){closeInline();inlineKey=key;
      if(kind==='resource'){const root=document.createElement('div');root.className='resource-details';root.setAttribute('role','group');root.setAttribute('aria-label',n.label+'的信息与操作');tree.inline.classList.add('resource-detail-host');tree.inline.append(root);inlineController=new ResourceDetails(root,{request,send,copy,locate,actions:actionItems,action,phase:value=>phases[value]||value,focus:()=>tree.focus(n.key),navigation:value=>{Object.assign(nav.inline,value);void remember();}});inlineController.mount(n,snapshot,mountData,nav.inline);inlineController.attachConfirmation($('browser-panel'));}
      else if(kind==='connection'){tree.inline.append($('connection'));$('connection').setAttribute('role','treeitem');$('connection').setAttribute('aria-level',String(n.depth+2));$('connection').hidden=false;}
      else if(kind==='tasks'){tree.inline.append($('browser-tasks'));$('browser-tasks').setAttribute('role','treeitem');$('browser-tasks').setAttribute('aria-level',String(n.depth+2));$('browser-tasks').hidden=false;void tasks();}
      else if(kind==='pull'){tree.inline.append($('browser-panel'));pull();}
    }
    if(inlineController){syncVisibility();inlineController.update(n,snapshot);}if(kind==='tasks')void tasks();
  }
  const renderSelection=()=>renderInline();
  function detailNavigation(key,resource,tab){return {...(nav.inline?.nodeKey===key?nav.inline:{kind:'resource',nodeKey:key,resource}),...(tab?{tab}:{})};}
  async function activate(n,kind='resource',tab){const same=kind==='resource'&&nav.inline?.nodeKey===n.key;menu.close(false);formTarget=null;nav.selected=n.key;nav.inline=kind==='resource'?detailNavigation(n.key,n.resource,tab):{kind};if(same&&tab){inlineController?.selectTab(tab);await navTail;}else await remember(!same);if(kind==='resource'&&!same&&current(identity()))await load(null,n.resource);}
  function select(n,focus){if(!n)return;if(focus&&(n.type==='resource'||n.type==='tool')){void activate(n,n.type==='tool'?n.key.split(':')[1]:'resource').then(()=>tree.focus(n.key));return;}if(formTarget&&n.key!==nav.selected){menu.close(false);formTarget=null;}nav.selected=n.key;selectedNode=n;tree.render(tree.page,nav.selected);renderSelection();if(focus)tree.focus(n.key);void remember();}
  async function toggle(n){if(!n?.expandable)return;if(n.type==='resource'||n.type==='tool'){if(snapshot.tree?.inlineAnchor?.node.key===n.key){nav.inline=null;closeInline();await remember(true);tree.focus(n.key);}else await activate(n,n.type==='tool'?n.key.split(':')[1]:'resource');return;}const keys=new Set(nav.expandedKeys||[]);if(keys.has(n.key)){keys.delete(n.key);nav.inline=null;closeInline();}else{if(keys.size>=64){message('最多保存 64 个展开节点，请先折叠部分分支');return;}keys.add(n.key);}nav.expandedKeys=[...keys];await remember(true);
    if(keys.has(n.key)&&n.type==='category'&&['unloaded','failed','stale'].includes(snapshot.indexes?.[n.branchKind]?.phase))await load(n.branchKind,undefined,true);
    if(keys.has(n.key)&&n.type==='resource')await load(null,n.resource,true);
  }
  async function load(branchKind,resource,motion=false){return send({action:'browser.load',branchKind,resource,scopeEpoch:snapshot.scopeEpoch},motion);}
  function windowPage(offset){if(!active||!enabled||frozen)return;const page=tree.page;const count=Math.ceil($('tree-viewport').clientHeight/tree.layout.rowHeight)+5;if(offset>=page.offset&&offset+count<=page.offset+(page.nodes?.length||0)||page.offset+(page.nodes?.length||0)>=page.total&&offset>=page.offset)return;
    offset=Math.min(offset,Math.max(0,page.total-10));if(offset===nav.offset)return;nav.offset=offset;void remember();}
  async function focusIndex(index,parent){if(index<0||index>=tree.page.total)return;const previousOffset=nav.offset;let n=tree.page.nodes[index-tree.page.offset];if(!n){nav.offset=Math.max(0,index-5);await remember();n=tree.page.nodes[index-tree.page.offset];}if(parent&&n?.parent!==parent){if(nav.offset!==previousOffset){nav.offset=previousOffset;await remember();tree.focus(parent);}return;}if(n)select(n,true);}
  async function focusKey(key){if(tree.node(key)){select(tree.node(key),true);return;}const index=tree.positions.get(key);if(index!==undefined){await focusIndex(index);return;}nav.offset=0;await remember();const n=tree.node(key);if(n)select(n,true);}
  async function locate(resource){let result=await send({action:'browser.locate',resource,scopeEpoch:snapshot.scopeEpoch});if(!result)return;
    if(!result.found){await load(resource.kind);message('正在读取目标所属分类，请再次定位');return;}
    nav.revealTarget=resource;nav.expandedKeys=[...new Set([...(nav.expandedKeys||[]),...result.expandedKeys])].slice(-64);nav.selected=result.key;nav.inline=detailNavigation(result.key,resource);nav.offset=Math.max(0,result.index-5);await remember();tree.focus(result.key);$('reveal-condition').hidden=false;await load(null,resource);
  }
  function open(n){if(!n)return;if(n.type==='root'){void activate({key:'tool:connection'},'connection');}else if(n.type==='load')void load(n.branchKind,n.resource);else void toggle(n);}
  async function openView(action,n){if(!n?.resource)return;await send({action,resource:n.resource,id:n.resource.id,...identity(),requestId:crypto.randomUUID()});}
  async function copy(text){if(!text)return;const local=epoch;try{await sodeckClipboard.writeText(text,()=>local===epoch&&enabled&&!frozen);message('已复制');return true;}catch(error){message(error.message);return false;}}
  async function openMenu(n,trigger){if(!n||!enabled||frozen)return;nav.selected=n.key;formTarget=null;const items=[];const add=(label,run,disabled=false,reason='',danger=false)=>items.push({label,run,disabled,reason,danger});
    if(n.resource){add('展开信息',()=>activate(n));for(const item of actionItems(n))add(item.label,async()=>{await activate(n);void action(item.action,n);},item.disabled,'当前资源或连接状态不可用',item.danger);
      if(n.resource.kind==='container'){add('端口隧道…',async()=>{await activate(n,'resource','ports');tree.inline.querySelector('[data-details-tab=ports]')?.focus({preventScroll:true});});add('终端选项',async()=>{await activate(n);shell(n);},n.status!=='running'||!current(identity()));}add('重新读取信息',()=>load(null,n.resource));add('复制完整标识',()=>copy(n.resource.id));
    }else if(n.composeProject){
      const fixed={...identity(),project:n.composeProject,observationRevision:n.observationRevision,publicationId:crypto.randomUUID()};
      add('跟随',()=>send({action:'compose.follow',...fixed}),!!n.composeLocation?.error||!n.composeLocation?.path,n.composeLocation?.error||('在当前服务器定位；目录来自 Compose 标签：'+n.composeLocation.path));
      add(n.expanded?'折叠':'展开',()=>toggle(n));
    }else if(n.type==='engine'){
      add('全部折叠',()=>{nav.inline=null;nav.expandedKeys=['engine'];nav.offset=0;closeInline();void remember();},!!nav.query,'清空搜索后可折叠');add('刷新资源树',()=>send({action:'refresh'}),snapshot.busy);add(snapshot.paused?'恢复自动刷新':'暂停自动刷新',()=>send({action:'pauseRefresh',paused:!snapshot.paused}));add('连接设置',()=>activate({key:'tool:connection'},'connection'));add('查看任务',()=>activate({key:'tool:tasks'},'tasks'));add('拉取公开镜像',async()=>{nav.expandedKeys=[...new Set([...(nav.expandedKeys||[]),'engine','category:image'])];await activate({key:'tool:pull'},'pull');});
    }else if(n.type==='root'){add('复制路径',()=>copy(snapshot.engine?.dockerRootDir),!snapshot.engine?.dockerRootDir);add('连接设置',()=>activate({key:'tool:connection'},'connection'));}
    else{add(n.expanded?'折叠':'展开',()=>toggle(n));if(n.branchKind)add('刷新此分类',()=>load(n.branchKind));}
    menu.open(n,items,trigger,()=>trigger?.id==='browser-more'?trigger.focus():tree.focus(n.key));
  }
  function operate(action,n,target){menu.restore=()=>tree.focus(n.key);const request={action:'operate',operation:action,id:n.resource.id,...target,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent};
    if(['start','unpause'].includes(action)){void submit(request);return;}formTarget=target;
    menu.form({title:'确认'+operationNames[action],text:`${n.label}\n${n.resource.id}\n${snapshot.engine?.name||'Engine'}`,impact:action==='remove'?'删除容器可写层，保留挂载卷和 bind 数据。':action==='pause'?'容器进程将暂停，服务可能无法响应。':'服务可能中断；取消等待不会回滚 Engine 已接受的操作。',submit:operationNames[action],danger:action==='remove',onSubmit:()=>submit(request)});
  }
  async function submit(request){if(!current(request))return;formTarget=null;submitting=true;render(false);try{await send(request);}finally{submitting=false;await poll();}}
  async function remove(n,target){const object=await send({action:'browser.object',resource:n.resource,scopeEpoch:target.scopeEpoch});if(!object||!current(target))return;formTarget={...target,resource:n.resource,observationRevision:n.revision};const request={action:'resource.operate',operation:'remove',resource:n.resource,...target,observationRevision:n.revision,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent};
    menu.form({title:'确认删除'+({image:'镜像',network:'网络',volume:'卷'}[n.resource.kind]),text:`${n.label}\n${n.resource.id}\n${snapshot.engine?.name||'Engine'}${object.aliases?.length?'\n受影响标签：\n'+object.aliases.join('\n')+(object.aliasesTruncated?'\n标签过多，共 '+object.aliasesTotal+' 个，请核对全部别名。':''):''}`,impact:n.resource.kind==='volume'?'将永久删除卷中的数据。删除前重新检查全部容器引用，包括停止容器；Docker 卷没有唯一 ID，无法完全消除外部同名重建的并发风险。':n.resource.kind==='image'?'按完整镜像 ID 删除对象，影响其全部标签和摘要引用。Docker 可能因多标签或引用保护而拒绝；不会强制删除或逐个解除标签。':'仅删除无连接端点的自定义网络，不使用强制删除。',submit:'删除',danger:true,onSubmit:()=>submit(request)});menu.valid(current(formTarget)&&object.revision===n.revision);
  }
  function shell(n){menu.restore=()=>tree.focus(n.key);formTarget=identity();menu.form({title:'容器终端',text:n.label,input:true,label:'容器 Shell',value:nav.shell||snapshot.defaultShell||'',submit:'打开终端',onSubmit:value=>{nav.shell=value;void remember();void terminal(n,value);}});}
  async function terminal(n,shell){const target=identity();if(!current(target)||snapshot.terminal?.status==='pending')return;await send({action:'openTerminal',id:n.resource.id,...target,shell,requestId:crypto.randomUUID()});renderSelection();}
  function pull(prefill=nav.pullDraft||''){const target=identity();formTarget=target;menu.form({title:'拉取公开镜像',text:'使用当前服务器的 Docker Engine。未填写标签时使用并展示 :latest。',input:true,label:'镜像引用',value:prefill,submit:'检查引用',onSubmit:value=>{
    nav.pullDraft=value;const tail=value.trim().split('/').at(-1);const reference=value.includes('@')||tail?.includes(':')?value.trim():value.trim()+':latest';
    formTarget=target;menu.form({title:'确认拉取镜像',text:reference,impact:'仅支持无需登录的公开源；拉取会继续运行，隐藏侧栏不会停止任务。',submit:'拉取',onSubmit:()=>submit({action:'pull.start',reference,...target,requestId:crypto.randomUUID(),intentSequence:snapshot.nextIntent})});
  }});}
  let readingTasks=false;
  async function tasks(){if(!enabled||frozen||nav.inline?.kind!=='tasks'||readingTasks)return;readingTasks=true;const local=epoch;try{
    const page=await request({action:'browser.tasks',offset:taskOffset});if(!page||local!==epoch||nav.inline?.kind!=='tasks')return;
    SoDeckDOM.updateDOM($('task-list'),page.rows.map((task,index)=>`<div class="docker-task" data-dom-key="task:${escape(task.requestId||[task.action,task.submittedAt,task.containerId||task.resource?.id,index].join(':'))}"><span>${escape([operationNames[task.action]||'拉取',task.name||task.reference,phases[task.state]||task.state].filter(Boolean).join(' · '))}</span><p>${escape(task.message||'')}</p>${task.resource||task.containerId?`<button type="button" data-task-resource="${escape(JSON.stringify(task.resource||{kind:'container',id:task.containerId}))}">定位资源</button>`:''}${task.action==='tunnel'&&['checking','unknown'].includes(task.state)?'<button type="button" data-task-query>查询原请求</button>':''}</div>`).join('')||'<p class="muted">暂无任务</p>',false);
    $('task-page').textContent=page.total?`${taskOffset+1}–${taskOffset+page.rows.length} / ${page.total}`:'暂无任务';$('task-prev').disabled=!taskOffset;$('task-next').disabled=page.nextOffset==null;$('task-next').onclick=()=>{taskOffset=page.nextOffset;void tasks();};await output();
  }catch(error){message(error.message);}finally{readingTasks=false;}}
  async function output(){const pull=snapshot.pull;if(!pull?.processId){$('pull-output').hidden=true;return;}if(outputId!==pull.processId){outputId=pull.processId;outputOffset='0';outputText='';outputRemainder='';outputDecoder=new TextDecoder();sourceDecoders=new Map();}
    if(!pull.producedBytes||outputOffset===pull.producedBytes)return;
    try{for(let i=0;i<8&&outputOffset!==pull.producedBytes;i++){const packet=await sodeck.request('data',{op:'processOutput',action:'read',id:outputId,offset:outputOffset,watermark:pull.producedBytes});const bytes=Uint8Array.from(packet.data||[]);if(!bytes.length)break;outputRemainder+=outputDecoder.decode(bytes,{stream:true});const lines=outputRemainder.split('\n');outputRemainder=lines.pop();for(const line of lines){try{const envelope=JSON.parse(line);if(!sourceDecoders.has(envelope.source))sourceDecoders.set(envelope.source,new TextDecoder());outputText+=sourceDecoders.get(envelope.source).decode(Uint8Array.from(atob(envelope.dataBase64),c=>c.charCodeAt(0)),{stream:true});}catch{}}outputText=outputText.slice(-16384);outputOffset=packet.nextOffset;}$('pull-output').textContent=outputText;$('pull-output').hidden=!outputText;}catch(error){$('pull-output').textContent='输出已释放或不可读取：'+error.message;$('pull-output').hidden=false;}
  }
  async function poll(){if(!active||!enabled||frozen||!visible)return;const local=epoch;try{apply(await request({action:'snapshot'}));if(nav.inline?.kind==='tasks')await tasks();}catch(error){if(local===epoch)message(error.message);}finally{clearTimeout(timer);if(local===epoch&&active&&enabled&&!frozen)timer=setTimeout(()=>void poll(),1000);}}
  function control(id,type,handler){$(id).addEventListener(type,e=>{if(!active)return;handler(e);},true);}
  control('refresh','click',()=>void send({action:'refresh'}));control('cancel','click',()=>void send({action:'cancel'}));
  control('browser-more','click',e=>openMenu({key:'engine',type:'engine',label:'Docker Engine'},e.target));
  function filter(){if(composing)return;const query=$('query').value;
    if(query&&!searchBefore)searchBefore={expandedKeys:[...(nav.expandedKeys||[])],selected:nav.selected,offset:nav.offset,scrollTop:$('tree-viewport').scrollTop};
    if(!query&&searchBefore){Object.assign(nav,searchBefore);restoreScroll=searchBefore.scrollTop;searchBefore=null;}else{nav.offset=0;$('tree-viewport').scrollTop=0;}
    nav.inline=null;closeInline();void remember();if(query){for(const [kind,index]of Object.entries(snapshot.indexes||{}))if(['unloaded','failed'].includes(index.phase))void load(kind);}
    for(const b of $('status-tabs').children)b.setAttribute('aria-pressed',String(b.dataset.status===$('status-filter').value));
  }
  control('query','input',filter);control('query','compositionstart',()=>composing=true);control('query','compositionend',()=>{composing=false;filter();});
  for(const id of ['status-filter','health-filter','group'])control(id,'change',filter);
  control('status-tabs','click',e=>{const button=e.target.closest('[data-status]');if(button){$('status-filter').value=button.dataset.status;filter();}});
  control('clear-filters','click',()=>{$('query').value='';$('status-filter').value='all';$('health-filter').value='all';filter();});
  control('reveal-condition','click',()=>{nav.revealTarget=null;$('reveal-condition').hidden=true;void remember();});
  control('browser-tasks','toggle',()=>{if($('browser-tasks').open)void tasks();});control('task-prev','click',()=>{taskOffset=Math.max(0,taskOffset-10);void tasks();});
  control('task-list','click',e=>{const target=e.target.closest('[data-task-resource]');if(target)void locate(JSON.parse(target.dataset.taskResource));if(e.target.closest('[data-task-query]'))void send({action:'tunnel.status'});});
  control('pull-cancel','click',()=>void send({action:'pull.cancel'}));
  const lifecycle=(name,handler)=>addEventListener('sodeck:'+name,e=>{if(!active)return;Promise.resolve(handler(e.detail)).catch(error=>message(error.message));});
  addEventListener('sodeck:mount',e=>{closeInline();epoch++;active=true;enabled=false;frozen=!!e.detail.readOnly;visible=e.detail.visible!==false;snapshot=e.detail.data;mountData=e.detail;navTail=Promise.resolve();navSequence=navPainted=0;nav={schemaVersion:3,expandedKeys:['engine','category:container'],status:'all',health:'all',group:true,offset:0,...snapshot.navigation};if(/^[a-f0-9]{64}$/i.test(nav.selected||''))nav.selected='container:'+nav.selected;selectedNode=null;formTarget=null;searchBefore=null;tree.positions.clear();document.body.classList.add('browser-page');connection.reset();$('query').value=nav.query||'';$('status-filter').value=nav.status||'all';$('health-filter').value=nav.health||'all';$('group').checked=nav.group!==false;$('reveal-condition').hidden=!nav.revealTarget;theme(e.detail.tokens);render();$('tree-viewport').scrollTop=nav.scrollTop||0;sodeck.prepared(true);});
  lifecycle('surfaceReady',async()=>{enabled=true;frozen=false;await send({action:'resumeInput'});await send({action:'browser.visibility',visible});await remember();void poll();});
  lifecycle('visibilityChanged',async e=>{visible=e.visible!==false;syncVisibility();if(enabled&&!frozen)await send({action:'browser.visibility',visible});if(visible)void poll();});
  lifecycle('themeChanged',e=>theme(e.tokens));lifecycle('settingsChanged',()=>poll());
  lifecycle('contextChanged',()=>{epoch++;nav.inline=null;closeInline();connection.reset();formTarget=null;nav.selected='';nav.expandedKeys=['engine','category:container'];nav.offset=0;void poll();});
  lifecycle('transferCancelled',async()=>{enabled=true;frozen=false;await send({action:'resumeInput'});void poll();});
  lifecycle('flush',async e=>{if(composing||menu.composing||!!tree.inline.querySelector('[data-composing]')){sodeck.flushed(e.id,'请完成输入法组合后移动');return;}const wasEnabled=enabled&&!frozen;try{if(wasEnabled)await remember();frozen=true;syncVisibility();await Promise.allSettled([...pending]);frozen=!!e.freeze;menu.close(false);formTarget=null;sodeck.flushed(e.id);if(!frozen)void poll();}catch(error){frozen=false;sodeck.flushed(e.id,error.message);}});
  tree.inline.addEventListener('input',e=>{if(nav.inline?.kind==='pull'&&e.target.id==='browser-input')nav.pullDraft=e.target.value;});
  tree.inline.addEventListener('compositionstart',e=>e.target.dataset.composing='true');tree.inline.addEventListener('compositionend',e=>delete e.target.dataset.composing);
  lifecycle('unmount',()=>{epoch++;active=false;enabled=false;clearTimeout(timer);closeInline();});
  addEventListener('keydown',e=>{if(!active)return;if(e.key==='Escape'){menu.close();formTarget=null;}if((e.ctrlKey||e.metaKey)&&e.key==='f'){e.preventDefault();$('query').focus();}});
}

SoDeckMotion.observeMotion(document);
installBrowserPage();
