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
    up: '<path d="m6 10 6-6 6 6M12 4v16"/>',
    home: '<path d="m3 10 9-7 9 7M5 9v12h14V9M9 21v-8h6v8"/>',
    refresh: '<path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 14 6M4 12a8 8 0 0 0 14 6"/>',
    collapse: '<path d="M4 5h16M4 19h16m5-10 3 3 3-3m-6 6 3-3 3 3"/>',
    download: '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    copy: '<path d="M8 8h13v13H8zM16 8V3H3v13h5"/>',
  };
  function icon(name) {
    const span = document.createElement('span');
    span.className = 'glyph ' + name;
    span.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${shapes[name] || shapes.file}</svg>`;
    return span;
  }
  window.FileTree = class FileTree {
    constructor(host, { data, event, request, download, message, state }) {
      Object.assign(this, { data, event, request, download, message });
      this.nodes = new Map(); this.expanded = new Set(); this.selected = ''; this.rootPath = '/';
      this.persistQueue = Promise.resolve(); this.pending = new Set(); this.counter = 0; this.restoring = true;
      this.saved = state; this.home = '';
      host.className = 'file-browser';
      const pathbar = document.createElement('div'); pathbar.className = 'pathbar';
      this.input = document.createElement('input'); this.input.setAttribute('aria-label', '远端路径'); this.input.placeholder = '输入路径并回车定位'; this.input.disabled = true;
      this.input.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); void this.run(() => this.locate(this.input.value)); } });
      pathbar.append(icon('folder'), this.input);
      const toolbar = document.createElement('div'); toolbar.className = 'tree-toolbar';
      this.caption = document.createElement('span'); this.caption.textContent = '服务器文件'; this.caption.title = '当前已展开并加载的条目数';
      toolbar.append(this.caption, this.tool('home', '主目录', () => this.locate(this.home)), this.tool('up', '上一级', () => this.locate(this.selected === this.rootPath ? this.rootPath : parentOf(this.selected))), this.tool('refresh', '刷新选中目录', () => this.refresh()), this.tool('collapse', '全部折叠', () => this.collapseAll()));
      const columns = document.createElement('div'); columns.className = 'tree-columns';
      columns.innerHTML = '<span>名称</span><span>大小</span>';
      this.tree = document.createElement('div'); this.tree.className = 'file-tree'; this.tree.tabIndex = 0; this.tree.setAttribute('role', 'tree'); this.tree.setAttribute('aria-label', '服务器文件');
      this.tree.addEventListener('keydown', e => this.key(e));
      this.tree.addEventListener('scroll', () => { if (!this.restoring) this.persist(); });
      new ResizeObserver(() => this.indent()).observe(this.tree);
      host.append(pathbar, toolbar, columns, this.tree);
      document.addEventListener('pointerdown', e => { if (!this.menu?.contains(e.target)) this.closeMenu(); });
      document.addEventListener('keydown', e => { if (e.key === 'Escape') this.closeMenu(true); });
    }
    tool(name, label, fn) {
      const b = document.createElement('button'); b.type = 'button'; b.title = label; b.setAttribute('aria-label', label); b.append(icon(name)); b.addEventListener('click', () => void this.run(fn)); return b;
    }
    async run(fn) { try { await fn(); } catch (error) { this.message(error.message, true); } }
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
      const work = (async () => {
        try {
          const result = await this.data('list', { path: n.path, independent: true, next, ...(n.cursor ? { cursor: n.cursor } : {}) });
          if (!next) n.children = [];
          for (const entry of result.entries) {
            const path = join(n.path, entry.name); this.node(path, entry.attrs, entry.name);
            if (!n.children.includes(path)) n.children.push(path);
          }
          this.sort(n);
          n.loaded = true; n.pages = next ? n.pages + 1 : 1; n.more = result.more; n.cursor = result.cursor;
        } catch (error) { n.error = error.message; n.cursor = null; this.message(error.message, true); }
        finally { n.loading = null; this.pending.delete(work); this.render(); }
      })();
      n.loading = work; this.pending.add(work); this.render(); return work;
    }
    async toggle(n) {
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
    async reveal(path, expandTarget = false) {
      if (path !== this.rootPath && !path.startsWith(this.rootPath.replace(/\/$/, '') + '/')) throw new Error('路径不在当前根目录中');
      const relative = path.slice(this.rootPath === '/' ? 1 : this.rootPath.length + 1);
      let current = this.nodes.get(this.rootPath);
      const parts = path === this.rootPath ? [] : relative.split('/').filter(Boolean);
      for (const part of parts) {
        this.expand(current.path);
        if (!current.loaded) await this.load(current);
        if (current.error) throw new Error(current.error);
        const child = join(current.path, part);
        // A located entry may live on a later page; stat it without reading all pages.
        if (!this.nodes.has(child)) { const info = await this.data('stat', { path: child }); this.node(child, info.attrs, part); }
        if (!current.children.includes(child)) { current.children.push(child); this.sort(current); }
        current = this.nodes.get(child);
      }
      if (expandTarget && current.attrs.kind === 'directory') { this.expand(path); if (!current.loaded) await this.load(current); }
      return current;
    }
    async locate(target, focus = true) {
      const info = await this.data('stat', { path: target });
      const path = info.path.replace(/\/$/, '') || '/';
      const volume = /^\/[A-Za-z]:/.exec(path)?.[0] || '/';
      if (volume !== this.rootPath) {
        for (const n of this.nodes.values()) if (n.cursor) await this.data('closeDirectory', { cursor: n.cursor });
        this.nodes.clear(); this.expanded.clear(); this.rootPath = volume;
        this.node(volume, { kind: 'directory' }, volume);
      }
      const n = await this.reveal(path, info.attrs.kind === 'directory');
      this.select(n, false); this.render(); this.row(path)?.scrollIntoView({ block: 'nearest' });
      if (focus) this.tree.focus(); this.persist();
    }
    select(n, scroll = true) {
      this.selected = n.path; this.input.value = n.path; this.input.title = n.path;
      this.message(n.attrs.kind === 'directory' ? n.path : `${n.path} · ${bytes(n.attrs.size)}`);
      for (const row of this.tree.querySelectorAll('[role="treeitem"]')) row.setAttribute('aria-selected', String(row.id === n.key));
      this.tree.setAttribute('aria-activedescendant', n.key);
      if (scroll) this.row(n.path)?.scrollIntoView({ block: 'nearest' }); this.persist();
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
    async refresh() {
      let n = this.nodes.get(this.selected) || this.nodes.get(this.rootPath);
      if (n.attrs.kind !== 'directory') n = this.nodes.get(parentOf(n.path));
      if (!n || n.loading) return;
      this.expand(n.path); await this.load(n); this.persist();
    }
    collapseAll() {
      this.expanded.clear(); this.selected = this.rootPath; this.input.value = this.rootPath; this.render(); this.persist();
    }
    render() {
      const fragment = document.createDocumentFragment(); this.visible = []; this.maxDepth = 1;
      const visit = (n, level, pos, count) => {
        this.maxDepth = Math.max(this.maxDepth, level);
        const row = document.createElement('div'); row.className = 'tree-row'; row.id = n.key; row.dataset.path = n.path; row.dataset.kind = n.attrs.kind;
        row.setAttribute('role', 'treeitem'); row.setAttribute('aria-level', level); row.setAttribute('aria-posinset', pos); row.setAttribute('aria-setsize', count); row.setAttribute('aria-label', n.name);
        row.setAttribute('aria-selected', String(n.path === this.selected)); row.style.setProperty('--depth', level - 1); row.title = n.path;
        const directory = n.attrs.kind === 'directory', expanded = this.expanded.has(n.path);
        if (directory) row.setAttribute('aria-expanded', String(expanded));
        if (n.loading) row.setAttribute('aria-busy', 'true');
        const arrow = document.createElement('span'); arrow.className = 'tree-arrow' + (expanded ? ' expanded' : '');
        if (directory) { arrow.append(icon('chevron')); arrow.addEventListener('click', e => { e.stopPropagation(); this.tree.focus(); void this.run(() => this.toggle(n)); }); }
        const name = document.createElement('span'); name.className = 'tree-name'; name.textContent = n.name;
        const size = document.createElement('span'); size.className = 'tree-size'; size.textContent = directory ? '' : bytes(n.attrs.size);
        row.append(arrow, icon(directory ? 'folder' : n.attrs.kind === 'link' ? 'link' : 'file'), name, size);
        row.addEventListener('click', () => { this.tree.focus(); this.select(n, false); });
        row.addEventListener('dblclick', e => { if (!e.target.closest('.tree-arrow')) void this.run(() => this.open(n)); });
        row.addEventListener('contextmenu', e => { e.preventDefault(); this.select(n, false); this.showMenu(n, e.clientX, e.clientY); });
        this.visible.push(n); fragment.append(row);
        if (!directory || !expanded) return;
        for (const [index, child] of n.children.entries()) visit(this.nodes.get(child), level + 1, index + 1, n.children.length);
        if (n.loading || n.error || n.more || !n.children.length) {
          const item = document.createElement('div'); item.className = 'branch-state'; item.style.setProperty('--depth', level);
          if (n.loading) item.textContent = '读取目录…';
          else if (n.error || n.more) {
            const b = document.createElement('button'); b.dataset.directory = n.path; b.textContent = n.error ? '读取失败 · 重试' : '加载更多'; b.title = n.error || '继续读取此目录';
            b.addEventListener('click', () => void this.run(() => this.load(n, !n.error))); item.append(b);
          } else item.textContent = '空目录';
          fragment.append(item);
        }
      };
      const root = this.nodes.get(this.rootPath); if (root) visit(root, 1, 1, 1);
      const scroll = this.tree.scrollTop;
      this.tree.replaceChildren(fragment); this.tree.scrollTop = scroll;
      this.indent();
      if (this.selected && !this.visible.some(n => n.path === this.selected)) {
        let path = parentOf(this.selected);
        while (path !== this.rootPath && path !== '/' && !this.visible.some(n => n.path === path)) path = parentOf(path);
        this.select(this.visible.find(n => n.path === path) || root, false);
      }
      if (this.nodes.has(this.selected)) this.tree.setAttribute('aria-activedescendant', this.nodes.get(this.selected).key);
      this.caption.textContent = `${this.visible.length} 项`;
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
      const choose = i => { const target = this.visible[Math.max(0, Math.min(this.visible.length - 1, i))]; if (target) this.select(target); };
      if (e.key === 'ArrowDown') choose(index + 1);
      else if (e.key === 'ArrowUp') choose(index - 1);
      else if (e.key === 'Home') choose(0);
      else if (e.key === 'End') choose(this.visible.length - 1);
      else if (e.key === 'ArrowRight' && n.attrs.kind === 'directory') { if (!this.expanded.has(n.path)) void this.run(() => this.toggle(n)); else if (n.children.length) this.select(this.nodes.get(n.children[0])); }
      else if (e.key === 'ArrowLeft') { if (this.expanded.has(n.path)) void this.run(() => this.toggle(n)); else if (n.path !== this.rootPath) this.select(this.nodes.get(parentOf(n.path))); }
      else if (e.key === 'Enter') void this.run(() => this.open(n));
      else if (e.key === 'F5') void this.run(() => this.refresh());
      else if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) { const r = this.row(n.path).getBoundingClientRect(); this.showMenu(n, r.x + 30, r.bottom); }
      else return;
      e.preventDefault();
    }
    closeMenu(focus = false) { this.menu?.remove(); this.menu = null; if (focus) this.tree.focus(); }
    showMenu(n, x, y) {
      this.closeMenu();
      const menu = document.createElement('div'); menu.className = 'tree-menu'; menu.setAttribute('role', 'menu');
      const actions = [[n.attrs.kind === 'directory' ? 'folder' : 'file', n.attrs.kind === 'directory' ? '展开 / 折叠' : '打开文件', () => this.open(n)], ['download', '下载文件', () => this.download(n.path)], ['copy', '复制完整路径', async () => { await navigator.clipboard.writeText(n.path); this.message('路径已复制'); }], ['refresh', '刷新所在目录', () => this.refresh()]];
      for (const [name, label, action] of actions) {
        if (name === 'download' && n.attrs.kind === 'directory') continue;
        const b = document.createElement('button'); b.setAttribute('role', 'menuitem'); b.append(icon(name), document.createTextNode(label));
        b.addEventListener('click', () => { this.closeMenu(true); void this.run(action); }); menu.append(b);
      }
      menu.addEventListener('keydown', e => {
        if (!['ArrowDown', 'ArrowUp', 'Tab'].includes(e.key)) return;
        e.preventDefault(); const buttons = [...menu.children], i = buttons.indexOf(document.activeElement);
        buttons[(i + (e.key === 'ArrowUp' ? buttons.length - 1 : 1)) % buttons.length].focus();
      });
      document.body.append(menu); this.menu = menu;
      menu.style.left = Math.max(4, Math.min(x, innerWidth - menu.offsetWidth - 4)) + 'px';
      menu.style.top = Math.max(4, Math.min(y, innerHeight - menu.offsetHeight - 4)) + 'px'; menu.firstElementChild.focus();
    }
    snapshot() {
      const state = { root: this.rootPath, selected: this.selected || this.rootPath, expanded: [], pages: {}, scroll: Math.round(this.tree.scrollTop) };
      // Control frames are small. Never send directory entries or unbounded path arrays.
      for (const path of [...this.expanded].sort((a, b) => a.length - b.length)) {
        state.expanded.push(path);
        if (this.nodes.get(path)?.pages > 1) state.pages[path] = this.nodes.get(path).pages;
        if (state.expanded.length > 128 || new TextEncoder().encode(JSON.stringify(state)).length > 11800) { state.expanded.pop(); delete state.pages[path]; break; }
      }
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
    contextChanged() {
      // Loaded rows are useful while offline; refresh reopens cursors on the new session.
      for (const n of this.nodes.values()) { n.cursor = null; if (n.more) n.error = '连接已更换，请刷新目录'; }
      this.render();
    }
  };
})();
