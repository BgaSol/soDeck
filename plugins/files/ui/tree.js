/* Navigation belongs to the file plugin; only directory pages cross the data bridge. */
(() => {
  const join = (parent, name) => parent.replace(/\/$/, '') + '/' + name;
  const parentOf = path => path.replace(/\/$/, '').slice(0, path.replace(/\/$/, '').lastIndexOf('/')) || '/';
  const bytes = value => {
    if (value == null) return '';
    const n = Number(value);
    for (const [unit, scale] of [['GiB', 1073741824], ['MiB', 1048576], ['KiB', 1024]]) if (n >= scale) return (n / scale).toFixed(1) + ' ' + unit;
    return n + ' B';
  };
  const shapes = {
    folder: '<path d="M3 6h6l2 2h10v11H3z"/>',
    file: '<path d="M6 3h8l4 4v14H6zM14 3v5h4M9 12h6M9 16h6"/>',
    link: '<path d="M10 14l4-4M8 16l-1 1a4 4 0 0 1-6-6l4-4a4 4 0 0 1 6 0M16 8l1-1a4 4 0 0 1 6 6l-4 4a4 4 0 0 1-6 0"/>',
    chevron: '<path d="m9 5 7 7-7 7"/>',
    upload: '<path d="M12 3v12m-5-7 5-5 5 5M4 16v5h16v-5"/>',
    up: '<path d="m6 10 6-6 6 6M12 4v16"/>',
    home: '<path d="m3 10 9-7 9 7M5 9v12h14V9M9 21v-8h6v8"/>',
    refresh: '<path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 14 6M4 12a8 8 0 0 0 14 6"/>',
    collapse: '<path d="M4 5h16M4 19h16m5-10 3 3 3-3m-6 6 3-3 3 3"/>',
    download: '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    copy: '<path d="M8 8h13v13H8zM16 8V3H3v13h5"/>',
    cut: '<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="m8 8 12 12M8 16 20 4"/>',
    paste: '<path d="M8 5H5v16h14V5h-3M8 3h8v5H8z"/>',
    rename: '<path d="m4 16 12-12 4 4L8 20H4z"/>',
    delete: '<path d="M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7"/>',
  };
  function icon(name) {
    const span = document.createElement('span');
    span.className = 'glyph ' + name;
    span.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${shapes[name] || shapes.file}</svg>`;
    return span;
  }
  window.FileTree = class FileTree {
    constructor(host, { data, event, request, download, message, state, transfers }) {
      Object.assign(this, { data, event, request, download, message, transfers });
      this.selection = new Set(); this.anchor = ''; this.dropPath = '';
      this.nodes = new Map(); this.expanded = new Set(); this.selected = ''; this.rootPath = '/';
      this.persistQueue = Promise.resolve(); this.pending = new Set(); this.counter = 0; this.restoring = true;
      this.saved = state; this.home = '';
      this.showHidden = true;
      this.navigationEpoch=0;this.contextEpoch=0;this.mounts=new Map();this.disposed=false;
      host.className = 'file-browser';
      const pathbar = document.createElement('div'); pathbar.className = 'pathbar';
      this.input = document.createElement('input'); this.input.setAttribute('aria-label', '远端路径'); this.input.placeholder = '输入路径并回车定位'; this.input.disabled = true;
      this.input.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); void this.run(() => this.locate(this.input.value)); } });
      pathbar.append(icon('folder'), this.input);
      const toolbar = document.createElement('div'); toolbar.className = 'tree-toolbar';
      this.caption = document.createElement('span'); this.caption.textContent = '服务器文件'; this.caption.title = '当前已展开并加载的条目数';
      toolbar.append(this.caption, this.tool('home', '主目录', () => this.locate(this.home)), this.tool('up', '上一级', () => this.locate(this.selected === this.rootPath ? this.rootPath : parentOf(this.selected))), this.tool('refresh', '刷新选中目录', () => this.refresh()), this.tool('collapse', '全部折叠', () => this.collapseAll()));
      this.toolbar=toolbar;
      if (transfers) {
        const upload = this.tool('upload', '上传文件', () => transfers.upload());
        toolbar.append(upload, this.tool('download', '下载选中条目', () => transfers.download(this.selectedPaths())));
        upload.addEventListener('contextmenu', e => { e.preventDefault(); void this.run(() => transfers.upload(true)); });
      }
      const columns = document.createElement('div'); columns.className = 'tree-columns';
      columns.innerHTML = '<span>名称</span><span>大小</span>';
      this.tree = document.createElement('div'); this.tree.className = 'file-tree'; this.tree.tabIndex = 0; this.tree.setAttribute('role', 'tree'); this.tree.setAttribute('aria-label', '服务器文件');
      this.tree.addEventListener('keydown', e => this.key(e));
      this.tree.addEventListener('scroll', () => { if (!this.restoring) this.persist(); });
      this.tree.addEventListener('pointerdown',e=>this.pointerStart(e));
      this.tree.addEventListener('contextmenu',e=>{if(!e.target.closest('.tree-row')){e.preventDefault();this.showMenu(null,e.clientX,e.clientY);}});
      this.tree.addEventListener('click',e=>{if(Date.now()<(this.suppressClick||0)){e.preventDefault();e.stopImmediatePropagation();}},true);
      document.addEventListener('pointermove',e=>this.pointerMove(e));
      document.addEventListener('pointerup',e=>this.pointerEnd(e));
      document.addEventListener('pointercancel',()=>this.pointerEnd(null));
      new ResizeObserver(() => this.indent()).observe(this.tree);
      host.append(pathbar, toolbar, columns, this.tree);
      document.addEventListener('pointerdown', e => { if (!this.menu?.contains(e.target)) this.closeMenu(); });
      document.addEventListener('keydown', e => { if (e.key === 'Escape') {this.closeMenu(true);this.pointerEnd(null);this.clearDrop();} });
      this.menuRefresh=()=>this.readMenuState?.();
      addEventListener('focus',this.menuRefresh);
      addEventListener('sodeck:fileClipboardChanged',this.menuRefresh);
    }
    tool(name, label, fn) {
      const b = document.createElement('button'); b.className='sd-icon'; b.type = 'button'; b.title = label; b.setAttribute('aria-label', label); b.append(icon(name)); b.addEventListener('click', () => void this.run(fn)); return b;
    }
    async run(fn) { try { await fn(); } catch (error) { if(error.code!=='cancelled')this.message(error.message, true); } }
    cancelFollow(manual=false){++this.navigationEpoch;if(manual)this.onManual?.();}
    navigation(){const navigation=this.navigationEpoch,context=this.contextEpoch;return()=>{if(this.disposed||navigation!==this.navigationEpoch||context!==this.contextEpoch)throw Object.assign(new Error('定位已取消'),{code:'cancelled'});};}
    async followLocation(location){this.cancelFollow();const check=this.navigation();const resolved=await this.data('resolveLocation',location);check();await this.locate(resolved.path,{follow:true,check});}
    node(path, attrs = { kind: 'directory' }, name = path.split('/').pop() || path) {
      let n = this.nodes.get(path);
      if (!n) { n = { path, name, attrs, children: [], pages: 0, loaded: false, more: false, cursor: null, error: '', loading: null, key: 'node-' + ++this.counter }; this.nodes.set(path, n); }
      else { n.attrs = attrs; n.name = name; }
      return n;
    }
    sort(n) {
      n.children.sort((a, b) => {
        const x = this.nodes.get(a), y = this.nodes.get(b);
        return Number(y.attrs.kind === 'directory') - Number(x.attrs.kind === 'directory') || (x.name < y.name ? -1 : x.name > y.name ? 1 : 0);
      });
    }
    expand(path) {
      if (this.expanded.size >= 128 && !this.expanded.has(path)) throw new Error('已保留 128 个展开目录，请先收起部分目录');
      this.expanded.add(path);
    }
    async start() {
      try {
        this.home = (await this.data('home')).path;
        const target = this.saved?.selected || this.home;
        const info = await this.data('stat', { path: this.saved?.root || (/^\/[A-Za-z]:\//.exec(target)?.[0] || '/') });
        this.rootPath = info.path.replace(/\/$/, '') || '/';
        this.node(this.rootPath, { kind: 'directory' }, this.rootPath);
        this.expanded.add(this.rootPath);
        this.render();
        await this.load(this.nodes.get(this.rootPath));
        if (this.saved) {
          for (const path of [...this.saved.expanded].sort((a, b) => a.length - b.length)) {
            // Restore navigation only. No recursive crawl of unopened directories.
            try {
              const n = await this.reveal(path, true), pages = this.saved.pages?.[path] || 1;
              while (n.more && !n.error && n.pages < pages) await this.load(n, true);
            } catch { /* Deleted branches do not prevent opening the browser. */ }
          }
        }
        await this.locate(target, false);
        if(this.saved?.selection){this.selection=new Set(this.saved.selection.filter(path=>this.nodes.has(path)));this.render();}
        if (this.saved) this.tree.scrollTop = this.saved.scroll || 0;
      } catch (error) {
        this.message(error.message, true);
        if (!this.nodes.size) { this.node(this.rootPath, { kind: 'directory' }, this.rootPath); this.expanded.add(this.rootPath); }
        const root = this.nodes.get(this.rootPath); root.error = error.message; this.render();
      } finally { this.input.disabled = false; this.restoring = false; this.persist(); }
    }
    load(n, next = false) {
      if (n.loading) return n.loading;
      n.error = '';
      const context=this.contextEpoch;
      const work = (async () => {
        try {
          const result = await this.data('list', { path: n.path, independent: true, next, ...(n.cursor ? { cursor: n.cursor } : {}) });
          if(context!==this.contextEpoch||this.disposed){if(result.cursor)void this.data('closeDirectory',{cursor:result.cursor}).catch(()=>{});return;}
          if (!next) n.children = [];
          for (const entry of result.entries) {
            const path = join(n.path, entry.name); this.node(path, entry.attrs, entry.name);
            if (!n.children.includes(path)) n.children.push(path);
          }
          this.sort(n);
          n.loaded = true; n.pages = next ? n.pages + 1 : 1; n.more = result.more; n.cursor = result.cursor;
        } catch (error) { if(context===this.contextEpoch&&!this.disposed){n.error = error.message; n.cursor = null; this.message(error.message, true);} }
        finally { n.loading = null; this.pending.delete(work); if(context===this.contextEpoch&&!this.disposed)this.render(); }
      })();
      n.loading = work; this.pending.add(work); this.render(); return work;
    }
    async toggle(n) {
      this.cancelFollow(true);
      if (this.expanded.has(n.path)) {
        this.expanded.delete(n.path);
        if (this.selected.startsWith(n.path.replace(/\/$/, '') + '/')) this.select(n, false);
      }
      else {
        this.expand(n.path);
        this.render();
        if (!n.loaded && !n.loading) await this.load(n);
      }
      this.render(); this.persist();
    }
    async reveal(path, expandTarget = false, check=()=>{}) {
      check();
      if (path !== this.rootPath && !path.startsWith(this.rootPath.replace(/\/$/, '') + '/')) throw new Error('路径不在当前根目录中');
      const relative = path.slice(this.rootPath === '/' ? 1 : this.rootPath.length + 1);
      let current = this.nodes.get(this.rootPath);
      const parts = path === this.rootPath ? [] : relative.split('/').filter(Boolean);
      for (const part of parts) {
        check();
        this.expand(current.path);
        if (!current.loaded) await this.load(current);
        check();
        if (current.error) throw new Error(current.error);
        const child = join(current.path, part);
        // A located entry may live on a later page; stat it without reading all pages.
        if (!this.nodes.has(child)) { const info = await this.data('stat', { path: child });check();this.node(child, info.attrs, part); }
        if (!current.children.includes(child)) { current.children.push(child); this.sort(current); }
        current = this.nodes.get(child);
      }
      if (expandTarget && current.attrs.kind === 'directory') { this.expand(path); if (!current.loaded) await this.load(current); }
      check();
      return current;
    }
    async locate(target, focus = true) {
      const options=typeof focus==='object'?focus:{};
      if(!options.follow)this.cancelFollow(!this.restoring);
      const check=options.check||this.navigation();check();
      this.navigationPath=target;
      const info = await this.data('stat', { path: target });
      check();
      const path = info.path.replace(/\/$/, '') || '/';
      const volume = /^\/[A-Za-z]:/.exec(path)?.[0] || '/';
      if (volume !== this.rootPath) {
        for (const n of this.nodes.values()) if (n.cursor) {await this.data('closeDirectory', { cursor: n.cursor });check();}
        this.nodes.clear(); this.expanded.clear(); this.rootPath = volume;
        this.node(volume, { kind: 'directory' }, volume);
      }
      const n = await this.reveal(path, info.attrs.kind === 'directory',check);check();
      this.select(n, false,null,true); this.render(); this.row(path)?.scrollIntoView({ block: 'nearest' });
      if (focus===true) this.tree.focus(); this.persist();
    }
    select(n, scroll = true, modifiers = null, automatic=false) {
      if(!automatic&&!this.restoring)this.cancelFollow(true);
      if (modifiers?.shiftKey && this.anchor && this.visible) {
        const from = this.visible.findIndex(v => v.path === this.anchor), to = this.visible.indexOf(n);
        if (!modifiers.ctrlKey && !modifiers.metaKey) this.selection.clear();
        for (const node of this.visible.slice(Math.min(from, to), Math.max(from, to) + 1)) this.selection.add(node.path);
      } else if (modifiers?.ctrlKey || modifiers?.metaKey) {
        if (this.selection.has(n.path)) this.selection.delete(n.path); else this.selection.add(n.path);
        this.anchor = n.path;
      } else { this.selection = new Set([n.path]); this.anchor = n.path; }
      this.selected = n.path; this.input.value = n.path; this.input.title = n.path;
      this.message(n.attrs.kind === 'directory' ? n.path : `${n.path} · ${bytes(n.attrs.size)}`);
      for (const row of this.tree.querySelectorAll('[role="treeitem"]')) row.setAttribute('aria-selected', String(this.selection.has(row.dataset.path)));
      this.tree.setAttribute('aria-activedescendant', n.key);
      if (scroll) this.row(n.path)?.scrollIntoView({ block: 'nearest' }); this.persist();
    }
    selectedPaths() { const visible=new Set((this.visible||[]).map(n=>n.path)); return [...this.selection].filter(path => this.nodes.has(path)&&visible.has(path)); }
    children(n) { return n.children.filter(path=>this.showHidden||!this.nodes.get(path)?.name.startsWith('.')||this.navigationPath===path||this.navigationPath?.startsWith(path+'/')); }
    preferences(values) {
      const key=JSON.stringify([values.showHidden!==false,values.density,values.fontSize]);
      if(this.preferenceKey===key)return;this.preferenceKey=key;
      this.showHidden=values.showHidden!==false;
      this.tree.style.setProperty('--file-row-height',values.density==='comfortable'?'28px':'24px');
      this.tree.style.setProperty('--file-name-size',(values.fontSize||12)+'px');
      this.closeMenu();this.pointerEnd(null);this.render();
    }
    ambiguousTarget() {return new Set(this.selectedPaths().map(path=>this.nodes.get(path).attrs.kind==='directory'?path:parentOf(path))).size>1;}
    targetDirectory() {
      const node = this.nodes.get(this.selected) || this.nodes.get(this.rootPath);
      return node?.attrs.kind === 'directory' ? node.path : parentOf(node?.path || this.rootPath);
    }
    clearDrop() {
      clearTimeout(this.dropTimer);clearInterval(this.dropScroll);this.dropScroll=null;this.dropPath='';
      this.tree.querySelectorAll('.drop-target').forEach(row => row.classList.remove('drop-target'));
      this.tree.classList.remove('drop-target');
    }
    markDrop(event) {
      this.tree.querySelectorAll('.drop-target').forEach(row=>row.classList.remove('drop-target'));this.tree.classList.remove('drop-target');
      const hit = document.elementFromPoint(event.x, event.y), row = hit?.closest('.tree-row');
      const rect = this.tree.getBoundingClientRect();
      if (!hit || !this.tree.contains(hit) || (row && row.dataset.kind !== 'directory')) {this.clearDrop();return null;}
      const path = row?.dataset.path || this.targetDirectory();
      (row||this.tree).classList.add('drop-target');
      this.dropY=event.y;
      if(!this.dropScroll)this.dropScroll=setInterval(()=>{const r=this.tree.getBoundingClientRect();if(this.dropY<r.top+25)this.tree.scrollTop-=15;if(this.dropY>r.bottom-25)this.tree.scrollTop+=15;},40);
      const node = this.nodes.get(path);
      if(this.dropPath!==path){clearTimeout(this.dropTimer);if(node&&!this.expanded.has(path))this.dropTimer=setTimeout(()=>void this.run(()=>this.toggle(node)),650);}
      this.dropPath=path;return path;
    }
    nativeDrop(event) {
      if (event.dropType === 'leave') {this.clearDrop();return;}
      const path=this.markDrop(event);if(!path)return;
      this.message(`上传到 ${path}`);
      if (event.dropType === 'drop') void this.run(async () => {
        this.clearDrop();
        const input = await this.transfers.input('input.drop', { dropId: event.dropId });
        await this.transfers.upload(false, input, path);
      });
    }
    pointerStart(e) {
      const row=e.target.closest('.tree-row');
      if(!this.transfers || e.button!==0 || !row || e.target.closest('button') || row.dataset.path===this.rootPath)return;
      this.drag={x:e.clientX,y:e.clientY,path:row.dataset.path,active:false};
    }
    pointerMove(e) {
      const drag=this.drag;if(!drag)return;
      if(!drag.active){if(Math.hypot(e.clientX-drag.x,e.clientY-drag.y)<7)return;drag.active=true;if(!this.selection.has(drag.path))this.select(this.nodes.get(drag.path),false);drag.roots=this.selectedPaths();}
      e.preventDefault();this.suppressClick=Date.now()+300;
      const path=this.markDrop({x:e.clientX,y:e.clientY});
      const copy=/Mac/.test(navigator.platform)?e.altKey:e.ctrlKey;
      this.message(path?`${copy?'复制':'移动'} ${drag.roots.length} 项到 ${path}`:'请选择目标文件夹');
    }
    pointerEnd(e) {
      const drag=this.drag;this.drag=null;
      if(drag?.active){const targetPath=e?this.markDrop({x:e.clientX,y:e.clientY}):null;this.suppressClick=Date.now()+300;if(targetPath){const copy=/Mac/.test(navigator.platform)?e.altKey:e.ctrlKey;void this.run(()=>this.transfers.start({kind:copy?'copy':'move',roots:drag.roots,targetPath}));}}
      this.clearDrop();
    }
    row(path) { return document.getElementById(this.nodes.get(path)?.key); }
    async open(n) {
      if (n.attrs.kind === 'directory') return this.toggle(n);
      const info = await this.data('stat', { path: n.path });
      if (info.attrs.kind === 'directory') {
        // Keep a symlink's logical path so its place in the tree stays predictable.
        n.attrs = info.attrs; return this.toggle(n);
      }
      if (info.attrs.kind !== 'file') throw new Error('此条目不是普通文件或目录');
      await this.request('openView', { viewType: 'editor', resourceKey: info.path, params: { path: info.path } });
    }
    async refresh(path = this.selected) {
      let n = this.nodes.get(path) || this.nodes.get(this.rootPath);
      if (!n) return;
      if (n.attrs.kind !== 'directory') n = this.nodes.get(parentOf(n.path));
      if (!n || n.loading) return;
      this.expand(n.path); await this.load(n); this.persist();
    }
    collapseAll() {
      this.cancelFollow(true);
      this.expanded.clear(); this.selected = this.rootPath; this.input.value = this.rootPath; this.render(); this.persist();
    }
    render() {
      const fragment = document.createDocumentFragment(); this.visible = []; this.maxDepth = 1;
      const visit = (n, level, pos, count) => {
        this.maxDepth = Math.max(this.maxDepth, level);
        const row = document.createElement('div'); row.className = 'tree-row'; row.id = n.key; row.dataset.path = n.path; row.dataset.kind = n.attrs.kind;
        row.setAttribute('role', 'treeitem'); row.setAttribute('aria-level', level); row.setAttribute('aria-posinset', pos); row.setAttribute('aria-setsize', count); row.setAttribute('aria-label', n.name);
        row.setAttribute('aria-selected', String(this.selection.has(n.path))); row.style.setProperty('--depth', level - 1); row.title = n.path;
        const directory = n.attrs.kind === 'directory', expanded = this.expanded.has(n.path);
        if (directory) row.setAttribute('aria-expanded', String(expanded));
        if (n.loading) row.setAttribute('aria-busy', 'true');
        const arrow = document.createElement('span'); arrow.className = 'tree-arrow' + (expanded ? ' expanded' : '');
        if (directory) { arrow.append(icon('chevron')); arrow.addEventListener('click', e => { e.stopPropagation(); this.tree.focus(); void this.run(() => this.toggle(n)); }); }
        const name = document.createElement('span'); name.className = 'tree-name'; name.textContent = n.name;
        const size = document.createElement('span'); size.className = 'tree-size'; size.textContent = directory ? '' : bytes(n.attrs.size);
        row.append(arrow, icon(directory ? 'folder' : n.attrs.kind === 'link' ? 'link' : 'file'), name, size);
        this.decorateMount(row);
        row.addEventListener('click', e => { this.tree.focus(); this.select(n, false, e); });
        row.addEventListener('dblclick', e => { if (!e.target.closest('.tree-arrow')) void this.run(() => this.open(n)); });
        row.addEventListener('contextmenu', e => { e.preventDefault(); if (!this.selection.has(n.path)) this.select(n, false); this.showMenu(n, e.clientX, e.clientY); });
        this.visible.push(n); fragment.append(row);
        if (!directory || !expanded) return;
        const children=this.children(n);
        for (const [index, child] of children.entries()) visit(this.nodes.get(child), level + 1, index + 1, children.length);
        if (n.loading || n.error || n.more || !children.length) {
          const item = document.createElement('div'); item.className = 'branch-state'; item.style.setProperty('--depth', level);
          if (n.loading) item.textContent = '读取目录…';
          else if (n.error || n.more) {
            const b = document.createElement('button'); b.dataset.directory = n.path; b.textContent = n.error ? '读取失败 · 重试' : !children.length?'暂无可见条目 · 加载更多':'加载更多'; b.title = n.error || '继续读取此目录';
            b.addEventListener('click', () => void this.run(() => this.load(n, !n.error))); item.append(b);
          } else item.textContent = n.children.length?'暂无可见条目':'空目录';
          fragment.append(item);
        }
      };
      const root = this.nodes.get(this.rootPath); if (root) visit(root, 1, 1, 1);
      const scroll = this.tree.scrollTop;
      this.tree.replaceChildren(fragment); this.tree.scrollTop = scroll;
      const visible=new Set(this.visible.map(n=>n.path));this.selection=new Set([...this.selection].filter(path=>visible.has(path)));if(!visible.has(this.anchor))this.anchor='';
      this.indent();
      if (this.selected && !this.visible.some(n => n.path === this.selected)) {
        let path = parentOf(this.selected);
        while (path !== this.rootPath && path !== '/' && !this.visible.some(n => n.path === path)) path = parentOf(path);
        this.select(this.visible.find(n => n.path === path) || root, false,null,true);
      }
      if (this.nodes.has(this.selected)) this.tree.setAttribute('aria-activedescendant', this.nodes.get(this.selected).key);
      this.caption.textContent = `${this.visible.length} 项`;
      if(this.menu?.dataset.path&&!visible.has(this.menu.dataset.path))this.closeMenu(this.menu.contains(document.activeElement));
    }
    indent() {
      // Preserve every nesting level even for a deep Windows home path in a narrow sidebar.
      const width = this.tree.clientWidth, room = Math.max(0, width - (width < 230 ? 80 : 145));
      this.tree.style.setProperty('--tree-indent', Math.min(15, room / Math.max(1, this.maxDepth - 1)) + 'px');
    }
    key(e) {
      if (e.target !== this.tree) return;
      const n = this.nodes.get(this.selected) || this.nodes.get(this.rootPath), index = this.visible.indexOf(n);
      if (!n) return;
      if (this.transfers && !e.isComposing && (e.ctrlKey || e.metaKey)) {
        if (['c','x'].includes(e.key.toLowerCase())) { e.preventDefault(); void this.run(() => this.transfers.copy(this.selectedPaths(),e.key.toLowerCase()==='x')); return; }
        if (e.key.toLowerCase() === 'v') { e.preventDefault(); void this.run(() => this.transfers.paste()); return; }
        if (e.key.toLowerCase() === 'a') { e.preventDefault(); this.selection = new Set(this.visible.filter(v=>v.path!==this.rootPath).map(v => v.path)); this.render(); this.message(`已选择当前已加载可见的 ${this.selection.size} 项`); return; }
      }
      if (this.transfers && e.key === 'Delete') { e.preventDefault(); void this.run(() => this.transfers.mutate('delete', this.selectedPaths())); return; }
      if (this.transfers && e.key === 'F2') { e.preventDefault(); void this.run(() => this.transfers.mutate('rename', [n.path])); return; }
      const choose = i => { const target = this.visible[Math.max(0, Math.min(this.visible.length - 1, i))]; if (target) this.select(target, true, e); };
      if (e.key === 'ArrowDown') choose(index + 1);
      else if (e.key === 'ArrowUp') choose(index - 1);
      else if (e.key === 'Home') choose(0);
      else if (e.key === 'End') choose(this.visible.length - 1);
      else if (e.key === 'ArrowRight' && n.attrs.kind === 'directory') { if (!this.expanded.has(n.path)) void this.run(() => this.toggle(n)); else if (this.children(n).length) this.select(this.nodes.get(this.children(n)[0])); }
      else if (e.key === 'ArrowLeft') { if (this.expanded.has(n.path)) void this.run(() => this.toggle(n)); else if (n.path !== this.rootPath) this.select(this.nodes.get(parentOf(n.path))); }
      else if (e.key === 'Enter') void this.run(() => this.open(n));
      else if (e.key === 'F5') void this.run(() => this.refresh());
      else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) { const r = this.row(n.path).getBoundingClientRect(); this.showMenu(n, r.x + 30, r.bottom, true); }
      else return;
      e.preventDefault();
    }
    closeMenu(focus = false) { clearTimeout(this.menuTimer);this.menuCleanup?.();this.menuCleanup=null;this.readMenuState=null;this.menu?.remove();this.menu = null; if (focus) this.tree.focus(); }
    showMenu(n, x, y, keyboard = false) {
      this.closeMenu();
      const paths=n?[...this.selectedPaths()]:[],multiple=paths.length>1,directory=n?.attrs.kind==='directory';
      const target=!n?this.rootPath:directory&&!multiple?n.path:null;
      const mutable=paths.length>0&&paths.every(path=>path!=='/'&&path!==this.rootPath);
      const menu = document.createElement('div'); menu.className = 'tree-menu sd-menu'+(keyboard?'':' pointer-pending'); menu.setAttribute('role', 'menu');menu.setAttribute('aria-label','文件操作');menu.tabIndex=-1;
      if(n)menu.dataset.path=n.path;
      const list=document.createElement('div');list.className='tree-menu-list sd-menu-list';menu.append(list);
      let submenu,submenuAnchor,subPosition,hoverTimer,paste,pasteState,reading=false,validateTarget=true;
      const cancelHover=()=>clearTimeout(hoverTimer);
      const hideSub=()=>{cancelHover();subPosition?.dispose();subPosition=null;if(submenu){submenu.hidden=true;submenuAnchor.setAttribute('aria-expanded','false');submenu=null;submenuAnchor=null;}};
      const later=action=>{cancelHover();hoverTimer=setTimeout(()=>{if(this.menu===menu)action();},180);};
      const add=(name,label,action,parent=list)=>{
        const b=document.createElement('button');b.type='button';b.setAttribute('role','menuitem');b.append(icon(name),document.createTextNode(label));
        b.addEventListener('click',()=>{if(b.disabled)return;this.closeMenu(true);void this.run(action);});
        b.addEventListener('pointerenter',e=>{if(e.pointerType==='mouse'&&parent===list)later(hideSub);});parent.append(b);return b;
      };
      const separator=()=>{if(list.childNodes.length&&list.lastChild.tagName!=='HR')list.append(document.createElement('hr'));};
      const addSub=(name,label,entries)=>{
        const b=document.createElement('button');b.type='button';b.setAttribute('role','menuitem');b.setAttribute('aria-haspopup','menu');b.setAttribute('aria-expanded','false');b.append(icon(name),document.createTextNode(label));
        const arrow=icon('chevron');arrow.classList.add('submenu-arrow');b.append(arrow);list.append(b);
        const sub=document.createElement('div');sub.className='tree-submenu sd-menu';sub.setAttribute('role','menu');sub.setAttribute('aria-label',label);sub.hidden=true;
        const subList=document.createElement('div');subList.className='tree-menu-list sd-menu-list';sub.append(subList);menu.append(sub);
        for(const [glyph,text,action]of entries)add(glyph,text,action,subList);
        const open=()=>{cancelHover();if(submenu===sub)return subPosition;hideSub();submenu=sub;submenuAnchor=b;sub.hidden=false;b.setAttribute('aria-expanded','true');subPosition=window.SoDeckMenuPosition.attachMenuPosition(sub,b,{kind:'submenu',isValid:()=>this.menu===menu&&submenu===sub,onInvalid:hideSub});return subPosition;};
        b.addEventListener('click',open);
        b.addEventListener('pointerenter',e=>{if(e.pointerType==='mouse'){if(submenu&&submenu!==sub)later(open);else open();}});
        b.addEventListener('pointerleave',e=>{if(e.pointerType==='mouse')later(hideSub);});
        sub.addEventListener('pointerenter',cancelHover);sub.addEventListener('pointerleave',e=>{if(e.pointerType==='mouse')later(hideSub);});
        b.addEventListener('keydown',e=>{if(e.key==='ArrowRight'){e.preventDefault();e.stopPropagation();const position=open();void position.ready.then(ready=>{if(ready&&this.menu===menu&&submenu===sub)sub.querySelector('button:not(:disabled)')?.focus();});}});
      };
      if(n&&!multiple&&!directory)add('file','打开',()=>this.open(n));
      if(n&&(this.transfers||!directory))add('download','下载',()=>this.transfers?this.transfers.download(paths):this.download(n.path));
      separator();
      if(this.transfers&&mutable){add('cut','剪切',()=>this.transfers.copy(paths,true));add('copy','复制',()=>this.transfers.copy(paths));}
      if(this.transfers&&target){paste=add('paste','粘贴',()=>this.transfers.paste(target,pasteState));paste.disabled=true;paste.title='正在读取剪贴板';}
      if(n)add('copy','复制路径',async()=>{await sodeckClipboard.writeText(paths.join('\n'));this.message('路径已复制');dispatchEvent(new Event('sodeck:fileClipboardChanged'));});
      separator();
      if(this.transfers&&target){
        addSub('file','新建',[['file','文件…',()=>this.transfers.mutate('createEmpty',[],target)],['folder','文件夹…',()=>this.transfers.mutate('mkdir',[],target)]]);
        addSub('upload','上传',[['file','文件…',()=>this.transfers.upload(false,null,target)],['folder','文件夹…',()=>this.transfers.upload(true,null,target)]]);
      }
      if(!multiple)add('refresh','刷新',()=>this.refresh(n?(directory?n.path:parentOf(n.path)):this.rootPath));
      if(this.transfers&&mutable){separator();if(!multiple)add('rename','重命名…',()=>this.transfers.mutate('rename',paths));add('delete','删除',()=>this.transfers.mutate('delete',paths)).classList.add('danger');}
      menu.addEventListener('keydown', e => {
        if(e.key==='Escape'||e.key==='ArrowLeft'){
          if(submenu){e.preventDefault();e.stopPropagation();const anchor=submenuAnchor;hideSub();anchor.focus();}
          else if(e.key==='Escape'){e.preventDefault();e.stopPropagation();this.closeMenu(true);}return;
        }
        if(e.key==='Tab'){e.preventDefault();this.closeMenu(true);return;}
        if(!['ArrowDown','ArrowUp','Home','End'].includes(e.key))return;
        e.preventDefault();const scope=e.target.closest('.tree-submenu')?.querySelector('.tree-menu-list')||list;
        const buttons=[...scope.querySelectorAll('button:not(:disabled)')],i=buttons.indexOf(document.activeElement);
        buttons[e.key==='Home'?0:e.key==='End'?buttons.length-1:i<0?(e.key==='ArrowUp'?buttons.length-1:0):(i+(e.key==='ArrowUp'?-1:1)+buttons.length)%buttons.length]?.focus();
      });
      menu.addEventListener('pointermove',()=>menu.classList.remove('pointer-pending'));
      document.body.append(menu); this.menu = menu;
      const rootPath=this.rootPath;
      const position=window.SoDeckMenuPosition.attachMenuPosition(menu,keyboard?this.row(n.path):{x,y,contextElement:this.tree},{isValid:()=>this.menu===menu&&this.rootPath===rootPath,onInvalid:()=>{if(this.menu===menu)this.closeMenu();}});
      this.menuCleanup=()=>{position.dispose();hideSub();};
      void position.ready.then(ready=>{if(ready&&this.menu===menu)(keyboard?list.querySelector('button:not(:disabled)'):menu)?.focus({preventScroll:true});});
      if(paste){
        const read=async()=>{
          if(this.menu!==menu||reading)return;reading=true;clearTimeout(this.menuTimer);
          try{const state=await this.transfers.pasteState(target,validateTarget);if(this.menu!==menu)return;validateTarget=false;pasteState=state;paste.disabled=!state.canPaste;paste.title=state.canPaste?'':state.reason;}
          catch(error){if(this.menu===menu){paste.disabled=true;paste.title=error.message||'无法读取剪贴板';}}
          finally{reading=false;if(this.menu===menu)this.menuTimer=setTimeout(read,1000);}
        };
        this.readMenuState=read;void read();
      }
    }
    snapshot() {
      const state = { root: this.rootPath, selected: this.selected || this.rootPath, expanded: [], pages: {}, scroll: Math.round(this.tree.scrollTop) };
      // Control frames are small. Never send directory entries or unbounded path arrays.
      for (const path of [...this.expanded].sort((a, b) => a.length - b.length)) {
        state.expanded.push(path);
        if (this.nodes.get(path)?.pages > 1) state.pages[path] = this.nodes.get(path).pages;
        if (state.expanded.length > 128 || new TextEncoder().encode(JSON.stringify(state)).length > 11800) { state.expanded.pop(); delete state.pages[path]; break; }
      }
      state.selection=[];
      for(const path of this.selectedPaths()){state.selection.push(path);if(new TextEncoder().encode(JSON.stringify(state)).length>11800){state.selection.pop();break;}}
      return state;
    }
    persist() {
      if (this.restoring) return;
      this.pendingState = this.snapshot();
      clearTimeout(this.persistTimer);
      this.persistTimer = setTimeout(() => this.writeState(), 100);
    }
    writeState() {
      if (this.persisting) return;
      this.persisting = true;
      this.persistQueue = Promise.resolve().then(async () => {
        while (this.pendingState) { const state = this.pendingState; this.pendingState = null; await this.event({ action: 'navigation', state }); }
      }).finally(() => { this.persisting = false; });
      this.persistQueue.catch(error => this.message(error.message, true));
    }
    async flush() { await Promise.all([...this.pending]); this.persist(); clearTimeout(this.persistTimer); this.writeState(); await this.persistQueue; }
    focus() { this.tree.focus(); }
    setMounts(mounts){this.mounts=mounts;for(const row of this.tree.querySelectorAll('.tree-row'))this.decorateMount(row);}
    decorateMount(row){let detail=row.querySelector('.mount-detail');const mount=this.mounts.get(row.dataset.path);if(!mount){detail?.remove();row.classList.remove('mount-row');row.title=row.dataset.path;return;}if(!detail){detail=document.createElement('span');detail.className='mount-detail';row.append(detail);}detail.textContent=mount.label;row.title=row.dataset.path+' · '+mount.detail;row.classList.add('mount-row');}
    contextChanged() {
      ++this.contextEpoch;this.cancelFollow();this.setMounts(new Map());
      this.closeMenu();
      this.pointerEnd(null);
      // Loaded rows are useful while offline; refresh reopens cursors on the new session.
      for (const n of this.nodes.values()) { n.cursor = null; if (n.more) n.error = '连接已更换，请刷新目录'; }
      this.render();
    }
    dispose(){this.disposed=true;++this.contextEpoch;this.cancelFollow();this.closeMenu();removeEventListener('focus',this.menuRefresh);removeEventListener('sodeck:fileClipboardChanged',this.menuRefresh);}
  };
})();
