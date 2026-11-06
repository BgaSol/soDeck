/* Task presentation only. The external plugin process continues when this iframe is hidden. */
(() => {
  const terminal = state => ['completed', 'partial', 'failed', 'cancelled', 'needsReview'].includes(state);
  const labels = { queued: '等待', scanning: '正在统计', running: '处理中', cancelling:'正在取消',awaitingConflict: '等待处理同名', completed: '完成', partial: '部分失败', failed: '失败', cancelled: '已取消', needsReview: '请核对提交结果' };
  const kinds={upload:'上传',download:'下载',copy:'复制',move:'移动',delete:'删除',mkdir:'新建文件夹',createEmpty:'新建文件',rename:'重命名'};
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  window.FileTransfers = class {
    constructor({ fileId, data, event, request, message, host, target, ambiguous, refresh }) {
      Object.assign(this, { fileId, data, event, request, message, host, target, ambiguous, refresh });
      this.closed = new Map(); this.earlyClosed = new Map();
      this.closeListener = e => { const resolve = this.closed.get(e.detail.ticket); if (resolve) { this.closed.delete(e.detail.ticket); resolve(e.detail); } else if(this.earlyClosed.size<16)this.earlyClosed.set(e.detail.ticket,e.detail); };
      addEventListener('sodeck:viewsClosed', this.closeListener);
      this.contextEpoch = 0; this.visible = true; this.running = false; this.last = new Map();
      this.rates=new Map();
      this.conflictAll=new Map();
      this.panel = document.createElement('details'); this.panel.className = 'transfer-panel';
      this.summary = document.createElement('summary'); this.summary.textContent = '文件任务';
      this.body = document.createElement('div'); this.body.className = 'transfer-list';
      this.panel.append(this.summary, this.body); host.append(this.panel); this.timer = setTimeout(() => this.poll(), 350);
    }
    async input(action, args = {}) {
      const epoch = this.contextEpoch;
      let result = await this.data(action, args);
      if (result.state === 'waitingInput') {
        const operationId = result.id;
        try {
          do { await sleep(150); result = await this.data('op.poll', { operationId }); } while (result.state === 'waitingInput');
          if (result.state !== 'completed') throw new Error(result.error?.message || '选择文件失败');
          result = result.result;
        } finally { await this.data('op.release', { operationId }).catch(() => {}); }
      }
      if (epoch !== this.contextEpoch || this.disposed) {
        if(result.inputId)await this.data('input.release',{inputId:result.inputId}).catch(()=>{});
        throw new Error('目标页面或连接已切换，请重新操作');
      }
      return result;
    }
    async start(parameters) {
      const epoch=this.contextEpoch;
      if(parameters.roots) {parameters={...parameters,selectionId:await this.selection(parameters.roots)};delete parameters.roots;}
      try {
      const prepared=await this.event({action:'prepareBatch',parameters});
      if(prepared.closeViews?.length) {
        const result=await this.request('closeViews',{viewIds:prepared.closeViews});
        const closed=this.earlyClosed.get(result.ticket)||await new Promise(resolve=>this.closed.set(result.ticket,resolve));
        this.earlyClosed.delete(result.ticket);
        if(closed.error)throw new Error(closed.error);
        if(!closed.accepted)return;
      }
      if(epoch!==this.contextEpoch||this.disposed)throw new Error('目标页面或连接已切换，请重新操作');
      const result=await this.event({ action: 'startBatch', parameters });
      if(result.batchId)this.last.set(result.batchId,'queued');
      this.panel.open = true; await this.poll();
      } finally { if(parameters.selectionId)await this.event({action:'selection.release',selectionId:parameters.selectionId}).catch(()=>{}); }
    }
    async selection(roots) {
      const selectionId=crypto.randomUUID();
      await this.event({action:'selection.begin',selectionId});
      try {
        let page=[];
        for(const path of roots){if(new TextEncoder().encode(JSON.stringify([...page,path])).length>12000){await this.event({action:'selection.append',selectionId,roots:page});page=[];}page.push(path);}
        if(page.length)await this.event({action:'selection.append',selectionId,roots:page});
        return selectionId;
      }catch(error){await this.event({action:'selection.release',selectionId}).catch(()=>{});throw error;}
    }
    async destination() {return this.ambiguous?.()?this.ask('确认目标目录','远程目录',true,this.target()):this.target();}
    async upload(directory = false, source = null, targetPath = null) {
      const epoch=this.contextEpoch;
      if(targetPath==null)targetPath=await this.destination();if(targetPath==null)return;
      const input = source || await this.input('input.chooseUpload', { directory });
      if (input.cancelled || input.empty) return;
      if(epoch!==this.contextEpoch||this.disposed){await this.data('input.release',{inputId:input.inputId}).catch(()=>{});throw new Error('目标连接已变化，请重新上传');}
      await this.start({ kind: 'upload', inputId: input.inputId, targetPath });
    }
    async download(roots) {
      if (!roots.length) return;
      const epoch=this.contextEpoch;
      const info = roots.length === 1 ? await this.data('stat', { path: roots[0] }) : null;
      const single = info?.attrs.kind === 'file';
      const input = await this.input('input.chooseDownload', { single, name: roots[0].split('/').pop() });
      if (input.cancelled) return;
      if(epoch!==this.contextEpoch||this.disposed){await this.data('input.release',{inputId:input.inputId}).catch(()=>{});throw new Error('来源连接已变化，请重新下载');}
      await this.start({ kind: 'download', roots, inputId: input.inputId, single });
    }
    async pasteState(targetPath, validateTarget = false) {
      const epoch=this.contextEpoch;
      const [revision,info]=await Promise.all([this.input('input.revision'),validateTarget?this.data('stat',{path:targetPath}):Promise.resolve(null)]);
      if(info&&info.attrs.kind!=='directory')throw new Error('目标不是目录');
      const remote=await this.event({action:'clipboardStatus',revision:revision.revision,targetPath});
      if(epoch!==this.contextEpoch||this.disposed)throw new Error('目标连接已变化');
      return {revision:revision.revision,clipboardId:remote.clipboardId,source:remote.remote?'remote':revision.hasFiles===true?'local':revision.hasFiles===false?'none':'unknown',canPaste:remote.remote===true||revision.hasFiles===true,reason:remote.reason||'没有可粘贴的文件'};
    }
    async paste(targetPath = null, expected = null) {
      const epoch=this.contextEpoch;
      if(targetPath==null)targetPath=await this.destination();if(targetPath==null)return;
      if(epoch!==this.contextEpoch||this.disposed)throw new Error('目标连接已变化，请重新粘贴');
      const originalTarget=this.target(),focus=document.activeElement;
      const state=await this.pasteState(targetPath,true);
      if(expected&&(state.revision!==expected.revision||state.clipboardId!==expected.clipboardId||state.source!==expected.source))throw new Error('剪贴板已变化，请重新粘贴');
      if(!document.hasFocus() || document.activeElement!==focus || epoch!==this.contextEpoch || this.target()!==originalTarget)throw new Error('粘贴目标已变化，请重新粘贴');
      if(state.source==='remote'){
        const selectionId=crypto.randomUUID();
        const remote=await this.event({action:'pasteEntries',revision:state.revision,clipboardId:state.clipboardId,selectionId});
        if(!remote.selectionId)throw new Error('剪贴板已变化，请重新粘贴');
        try {
          const current=await this.input('input.revision');
          if(current.revision!==state.revision||!document.hasFocus()||document.activeElement!==focus||epoch!==this.contextEpoch||this.target()!==originalTarget)throw new Error('粘贴目标或剪贴板已变化，请重新粘贴');
          await this.start({kind:remote.kind,selectionId:remote.selectionId,targetPath,clipboard:true});
        } finally {await this.event({action:'selection.release',selectionId}).catch(()=>{});}
        return;
      }
      if(state.source==='none'||(expected&&!state.canPaste))throw new Error(state.reason);
      const input = await this.input('input.clipboard');
      if (input.empty) { this.message('系统剪贴板中没有本地文件或文件夹'); return; }
      if(input.revision!==state.revision||!document.hasFocus() || document.activeElement!==focus || epoch!==this.contextEpoch || this.target()!==originalTarget){await this.data('input.release',{inputId:input.inputId}).catch(()=>{});throw new Error('粘贴目标或剪贴板已变化，请重新粘贴');}
      await this.upload(false, input,targetPath);
    }
    async copy(roots,cut=false) {
      if(!roots.length)return;
      const epoch=this.contextEpoch;
      const revision=await this.input('input.revision');
      const selectionId=await this.selection(roots);
      try {if(epoch!==this.contextEpoch||this.disposed)throw new Error('来源连接已变化，请重新复制');await this.event({action:'copyEntries',selectionId,cut,revision:revision.revision});}
      finally {await this.event({action:'selection.release',selectionId}).catch(()=>{});}
      this.message(`${roots.length} 项已${cut?'剪切':'复制'}，可在本服务器其他目录粘贴`);
      dispatchEvent(new Event('sodeck:fileClipboardChanged'));
    }
    async mutate(kind, roots, targetPath = null) {
      const epoch=this.contextEpoch;
      if (kind === 'delete') {
        if (!await this.ask(`直接删除 ${roots.length} 个选中条目？`, '文件夹会递归删除，远程没有回收站。', false)) return;
        if(epoch!==this.contextEpoch||this.disposed)throw new Error('目标连接已变化，请重新操作');
        await this.start({ kind, roots }); return;
      }
      const base = kind === 'rename' ? roots[0].slice(0, roots[0].lastIndexOf('/')) || '/' : targetPath ?? await this.destination();
      if(base==null)return;
      const name = await this.ask(kind === 'mkdir' ? '新建文件夹' : kind === 'createEmpty' ? '新建文件' : '重命名', '名称', true, kind === 'rename' ? roots[0].split('/').pop() : '');
      if (name == null) return;
      if(epoch!==this.contextEpoch||this.disposed)throw new Error('目标连接已变化，请重新操作');
      if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\0')) throw new Error('文件名无效');
      const path = base.replace(/\/$/, '') + '/' + name;
      await this.start(kind === 'rename' ? { kind, path: roots[0], destination: path } : { kind, path });
    }
    ask(title, detail, input, initial = '') {
      return new Promise(resolve => {
        const dialog = document.createElement('dialog'); dialog.className = 'file-dialog';
        const heading = document.createElement('h3'); heading.textContent = title;
        const text = document.createElement('p'); text.textContent = detail;
        const field = document.createElement('input'); field.value = initial; field.setAttribute('aria-label', detail);
        const footer = document.createElement('div'); footer.className = 'file-dialog-actions';
        const finish = value => { dialog.close(); dialog.remove(); resolve(value); };
        for (const [label, fn] of [['取消', () => finish(null)], ['确定', () => finish(input ? field.value : true)]]) {
          const button = document.createElement('button'); button.className = 'sd-control'; button.textContent = label; button.onclick = fn; footer.append(button);
        }
        dialog.append(heading, text); if (input) dialog.append(field); dialog.append(footer); document.body.append(dialog);
        dialog.oncancel = e => { e.preventDefault(); finish(null); }; dialog.showModal(); if (input) { field.focus(); field.select(); field.onkeydown = e => { if (e.key === 'Enter') finish(field.value); }; }
      });
    }
    async poll() {
      if (this.running || this.disposed || !this.visible) return;
      this.running = true;
      try {
        let result = await this.event({ action: 'readTasks' });
        const tasks = [...(result.tasks || [])];
        while(result.next!=null){result=await this.event({action:'readTasks',offset:result.next});tasks.push(...result.tasks);}
        const active = tasks.filter(t => !terminal(t.state));
        this.summary.textContent = active.length ? `文件任务 · ${active.length} 项进行中` : `文件任务 · ${tasks.length} 项`;
        const fragment = document.createDocumentFragment();
        for (const task of tasks) {
          const row = document.createElement('section'); row.className = 'transfer-item';
          const title = document.createElement('div'); title.textContent = `${labels[task.state] || task.state} · ${kinds[task.kind]||task.kind}${task.current?' · '+task.current:''}`;
          const detail = document.createElement('div'); detail.className = 'transfer-detail';
          detail.textContent = `成功 ${task.completed} · 失败 ${task.failed} · 跳过 ${task.skipped}` + (Number(task.currentBytes) ? ` · ${Math.round(Number(task.currentBytes) / 1024)} KiB` : '');
          const now=performance.now(),received=Number(task.bytes)+Number(task.currentBytes||0),old=this.rates.get(task.id);
          if(old&&now-old.at>500){const rate=Math.max(0,(received-old.bytes)/(now-old.at)*1000),remaining=Math.max(0,Number(task.currentTotal||0)-Number(task.currentBytes||0));if(!terminal(task.state)&&rate>0)detail.textContent+=` · ${(rate/1024).toFixed(1)} KiB/s${remaining?` · 当前文件约 ${Math.ceil(remaining/rate)} 秒`:''}`;this.rates.set(task.id,{at:now,bytes:received});}
          else if(!old)this.rates.set(task.id,{at:now,bytes:received});
          row.append(title, detail);
          if (task.error || task.lastError || task.warning) { const error = document.createElement('div'); error.className = 'transfer-error'; error.textContent = task.error || task.lastError || task.warning; row.append(error); }
          const command = async parameters => { const result=await this.event({ batchId: task.id, conflictId:task.conflict?.itemId, ...parameters });if(result.batchId)this.last.set(result.batchId,'queued'); await this.poll(); };
          const button = (label, fn) => { const b = document.createElement('button'); b.textContent = label; b.className = 'sd-control';b.dataset.control=task.id+'/'+label; b.onclick = () => Promise.resolve().then(fn).catch(e => this.message(e.message, true)); row.append(b); };
          if (task.state === 'awaitingConflict') {
            const text = document.createElement('div'); text.textContent = task.conflict?.invalidName?'目标系统不支持此名称，请改名或跳过':task.conflict?.typeConflict ? '同名条目类型不同' : '目标已有同名文件'; row.append(text);
            const all = document.createElement('input'); all.type = 'checkbox';all.checked=this.conflictAll.get(task.id)||false;all.dataset.control=task.id+'/all';all.onchange=()=>this.conflictAll.set(task.id,all.checked); const label = document.createElement('label'); label.append(all, document.createTextNode('应用到本批同类冲突')); row.append(label);
            for (const [choice, text] of [['replace', '覆盖'], ['keepBoth', '保留两份'], ['skip', '跳过'], ['cancel', '取消任务']]) {
              if (choice === 'replace' && task.conflict?.typeConflict) continue;
              if(choice==='keepBoth'&&task.conflict?.invalidName)continue;
              button(text, () => command({ action: 'resolveConflict', choice, applyAll: all.checked }));
            }
            button('改名',async()=>{const name=await this.ask('为目标改名','新名称',true,task.conflict?.name||'');if(name!=null)await command({action:'resolveConflict',choice:'rename',name});});
          } else if (!terminal(task.state)) button('取消任务', () => command({ action: 'cancelBatch' }));
          else {
            if (Number(task.failed) > 0 && task.state !== 'needsReview') button('重试失败项', () => command({ action: 'retryFailed' }));
            button('查看结果',()=>this.showItems(task.id));
            if(task.kind==='download'&&task.fileId===this.fileId)button('打开所在目录',()=>this.data('input.reveal',{inputId:task.inputId}));
            button('清除记录',()=>command({action:'clearBatch'}));
          }
          fragment.append(row);
          if (terminal(task.state) && this.last.get(task.id) && !terminal(this.last.get(task.id))) {
            const epoch=this.contextEpoch;
            void this.refresh().catch(() => {}).then(()=>{if(!this.disposed&&epoch===this.contextEpoch)this.message(task.state==='completed' ? (task.kind==='download'?'下载完成':task.kind==='upload'?'上传完成':'文件任务完成') : (task.error||task.lastError||labels[task.state]),task.state!=='completed');});
          }
          this.last.set(task.id, task.state);
        }
        const focused=document.activeElement?.dataset.control;
        this.body.replaceChildren(fragment);
        if(focused)[...this.body.querySelectorAll('[data-control]')].find(e=>e.dataset.control===focused)?.focus({preventScroll:true});
      } catch (error) { if (!this.disposed) this.message(error.message, true); }
      finally { this.running = false; clearTimeout(this.timer); if (!this.disposed) this.timer = setTimeout(() => this.poll(), 350); }
    }
    async showItems(batchId) {
      const dialog=document.createElement('dialog');dialog.className='file-dialog';
      const list=document.createElement('div'),more=document.createElement('button'),close=document.createElement('button');
      more.textContent='加载更多';close.textContent='关闭';let offset=0;
      const load=async()=>{const page=await this.event({action:'readItems',batchId,offset});for(const entry of page.entries){const row=document.createElement('p');row.textContent=`${entry.item.name} · ${entry.state==='completed'?'完成':entry.state==='skipped'?'跳过':'失败'}${entry.error?' · '+entry.error:''}`;list.append(row);}more.disabled=!page.entries.length;offset=page.offset;};
      close.onclick=()=>{dialog.close();dialog.remove();};more.onclick=()=>load().catch(e=>this.message(e.message,true));dialog.oncancel=()=>dialog.remove();
      dialog.append(list,more,close);document.body.append(dialog);dialog.showModal();await load();
    }
    visibility(visible) { this.visible = visible; if(!visible)this.invalidate();else void this.poll(); }
    invalidate() { this.contextEpoch++; }
    dispose() { this.disposed = true; clearTimeout(this.timer); removeEventListener('sodeck:viewsClosed',this.closeListener); for(const resolve of this.closed.values())resolve({accepted:false});this.closed.clear(); }
  };
})();
