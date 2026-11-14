import { servers, proxies, tags, files, nginx, containers, plugins, messages, logs, stateLabel, networkInterfaces } from './data.js';
import { routeRecords, buildConnectionTree, filterConnectionTree, visibleConnectionNodes } from './connection-tree.js';

const paths = {
  server: '<rect x="3" y="3" width="18" height="7" rx="2"/><rect x="3" y="14" width="18" height="7" rx="2"/><path d="M7 6.5h.01M7 17.5h.01M12 6.5h5M12 17.5h5"/>',
  folder: '<path d="M3 7h7l2 2h9v11H3zM3 7V4h7l2 3"/>',
  file: '<path d="M5 3h9l5 5v13H5zM14 3v6h5M9 13h6M9 17h6"/>',
  terminal: '<path d="m4 6 6 6-6 6m9 0h7"/>',
  network: '<circle cx="12" cy="5" r="3"/><circle cx="5" cy="19" r="3"/><circle cx="19" cy="19" r="3"/><path d="M12 8v5M5 16v-3h14v3"/>',
  activity: '<path d="M3 12h4l3-8 4 16 3-8h4"/>',
  box: '<path d="m12 3 9 5v9l-9 5-9-5V8zM3 8l9 5 9-5M12 13v9M7.5 5.5l9 5"/>',
  settings: '<path d="m9 3-1 3-3 1-1 4 2 2v3l3 2 3-1 3 1 3-2v-3l2-2-1-4-3-1-1-3z"/><circle cx="12" cy="11" r="3"/>',
  bell: '<path d="M5 17h14l-2-3V9a5 5 0 0 0-10 0v5zM10 21h4"/>',
  plug: '<path d="M8 3v5M16 3v5M5 8h14v3a7 7 0 0 1-14 0zM12 18v4"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
  plus: '<path d="M12 5v14M5 12h14"/>', close: '<path d="m6 6 12 12M6 18 18 6"/>',
  check: '<path d="m5 12 4 4L19 6"/>', refresh: '<path d="M20 8a8 8 0 1 0 0 8M20 3v5h-5"/>',
  chevron: '<path d="m9 5 7 7-7 7"/>', down: '<path d="m5 9 7 7 7-7"/>',
  back: '<path d="m10 5-7 7 7 7M3 12h18"/>', split: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>',
  pause: '<path d="M8 5v14M16 5v14"/>', play: '<path d="m7 4 13 8-13 8z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="1"/>', info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7h.01"/>',
  warning: '<path d="m12 3 10 18H2zM12 9v5M12 17h.01"/>',
  lock: '<rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3M12 14v3"/>',
  upload: '<path d="m7 8 5-5 5 5M12 3v12M4 15v6h16v-6"/>',
  download: '<path d="m7 10 5 5 5-5M12 15V3M4 16v5h16v-5"/>',
  copy: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M16 8V3H3v13h5"/>',
  log: '<path d="M5 3h14v18H5zM8 7h8M8 11h8M8 15h5"/>',
  more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  save: '<path d="M4 3h13l3 3v15H4zM8 3v6h8V3M8 21v-8h8v8"/>',
};
const e = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const icon = name => `<svg viewBox="0 0 24 24" aria-hidden="true">${paths[name] || paths.file}</svg>`;
const b = (action, label, glyph = '', cls = '', attrs = '') => `<button type="button" class="${cls}" data-action="${action}" ${attrs}>${glyph ? icon(glyph) : ''}${glyph && label ? ' ' : ''}${e(label)}</button>`;
const ib = (action, label, glyph, attrs = '', cls = '') => b(action, '', glyph, 'icon-button ' + cls, `aria-label="${e(label)}" title="${e(label)}" ${attrs}`);
function pluginButton(pluginId, action, label, glyph, attrs = '', cls = '') {
  if (!plugins.find(plugin => plugin.id === pluginId)?.running) return '';
  return ib(action, label, glyph, `data-plugin="${e(pluginId)}" ${attrs}`, cls);
}
const status = (value, label = stateLabel[value] || value) => `<span class="status"><i class="dot ${e(value)}"></i>${e(label)}</span>`;
const options = (items, value) => items.map(item => `<option value="${e(item[0])}" ${item[0] === value ? 'selected' : ''}>${e(item[1])}</option>`).join('');
const params = new URLSearchParams(location.search);
plugins.forEach(plugin => { plugin.running = plugin.enabled; });
let savedTheme;
try { savedTheme = localStorage.getItem('sodeck-prototype-theme'); } catch {}
const state = {
  theme: params.get('theme') || savedTheme || 'dark', page: params.get('page') || 'workspace', current: 'web',
  selected: 'web', selectedPath: '', catalogKind: 'servers', selectedTags: new Set(), collapsed: new Set(), query: '', showDetail: false, tagsOpen: false,
  filesOpen: true, tool: 'system', toolOpen: false, folder: '', selectedFile: 'nginx.conf', follow: true,
  newPageMenu: false, transfer: false, pluginTab: 'installed', pending: new Set(),
  settingsSection: 'appearance', settingsDirty: false, terminalSize: 13, density: 'comfortable',
  settings: { font: 'Consolas', lineHeight: '1.8', cursor: '块状', hidden: false, wrap: true, tabWidth: '4', editorSize: '13', sampleInterval: '5', defaultHost: '127.0.0.1', defaultPort: '0' },
  messageFilter: 'all', logQuery: '', logSource: '', logLevel: '',
  dockerFilter: 'all', dockerQuery: '', dockerSection: 'overview', container: 'api',
};
if (!['light', 'dark', 'dopamine'].includes(state.theme)) state.theme = 'dark';
const workspaces = {};
let sequence = 1, modal = null, dialogReturn = null, connectTimer, toastTimer, selectionTimer;
let catalogNodes = [], catalogFiltering = false, revealSelected = true;
const app = document.querySelector('#app'), dialog = document.querySelector('#dialog');
const current = () => servers.find(server => server.id === state.current) || servers[0];
const routeNames = target => routeRecords(target, servers, proxies).map(step => step.record.name);
const shellHost = () => ({ web: 'prod-web', stage: 'staging' })[current().id] || current().host;
function ws() {
  return workspaces[state.current] ||= {
    tabs: [{ id: 'terminal-1', type: 'terminal', title: '终端 1', output: [], input: '' }, { id: 'editor-nginx.conf', type: 'editor', title: 'nginx.conf', draft: nginx, saved: nginx }],
    active: 'terminal-1', split: false, paused: false, networkInterface: 'eth0',
    tunnels: state.current === 'web' ? [
      { id: 'socks', name: '浏览器代理', type: 'socks5', host: '127.0.0.1', port: '1080', target: '', targetPort: '', running: true, active: { type: 'socks5', host: '127.0.0.1', port: '1080' } },
      { id: 'database', name: '数据库连接', type: 'tcp', host: '127.0.0.1', port: '15432', target: '10.20.0.12', targetPort: '5432', running: false },
    ] : [],
  };
}
const dirty = tab => !!tab && tab.type === 'editor' && tab.draft !== tab.saved;
const anyDirty = () => Object.values(workspaces).some(workspace => workspace.tabs.some(dirty));
function focusIdentity(node) {
  if (!node?.matches('button, input, select, textarea, [tabindex]')) return null;
  if (node.id) return { id: node.id };
  const scope = dialog.contains(node) ? 'dialog' : 'app';
  const attrs = Object.fromEntries(node.getAttributeNames().filter(name => name.startsWith('data-') || ['name', 'type', 'role', 'aria-label'].includes(name)).map(name => [name, node.getAttribute(name)]));
  const matches = [...(scope === 'dialog' ? dialog : app).querySelectorAll('button, input, select, textarea, [tabindex]')].filter(candidate => candidate.tagName === node.tagName && Object.entries(attrs).every(([name, value]) => candidate.getAttribute(name) === value));
  return { scope, tag: node.tagName, attrs, index: matches.indexOf(node) };
}
function restoreFocus(key) {
  if (!key) return null;
  const target = key.id ? document.getElementById(key.id) : [...(key.scope === 'dialog' ? dialog : app).querySelectorAll('button, input, select, textarea, [tabindex]')].filter(node => node.tagName === key.tag && Object.entries(key.attrs).every(([name, value]) => node.getAttribute(name) === value))[key.index];
  if (target && !target.disabled) target.focus({ preventScroll: true });
  return target;
}
function toast(text) {
  const node = document.querySelector('#toast');
  node.textContent = text; node.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { node.hidden = true; }, 3500);
}
function render() {
  const focused = document.activeElement, focusKey = focusIdentity(focused);
  const selection = focused && 'selectionStart' in focused ? [focused.selectionStart, focused.selectionEnd, focused.selectionDirection] : null;
  const scrolls = new Map([...app.querySelectorAll('[data-scroll]')].map(node => [node.dataset.scroll, [node.scrollTop, node.scrollLeft]]));
  document.documentElement.dataset.theme = state.theme;
  document.documentElement.dataset.density = state.density;
  const unread = messages.filter(message => !message.read).length;
  app.innerHTML = `<header class="titlebar">${b('navigate', 'SoDeck', 'server', 'brand ' + (state.page !== 'workspace' ? 'active' : ''), 'id="core-console-trigger" data-page="catalog" aria-label="SoDeck 管理台" title="SoDeck 管理台" aria-pressed="' + (state.page !== 'workspace') + '"')}
    <nav class="session-tabs" aria-label="服务器会话">
      ${servers.filter(server => ['connected', 'disconnected'].includes(server.state)).map(server => b('session', server.name, '', 'session-tab ' + (state.page === 'workspace' && state.current === server.id ? 'active' : ''), `data-id="${e(server.id)}"`).replace('>' + e(server.name), '>' + status(server.state, '') + e(server.name))).join('')}
      ${ib('choose-server', '选择服务器', 'plus')}</nav>
    <div class="global-actions"><select id="theme-quick" class="theme-quick" aria-label="切换主题">${options([['light', 'IDEA 浅色'], ['dark', 'IDEA 深色'], ['dopamine', '多巴胺']], state.theme)}</select>
      ${ib('navigate', '消息中心', 'bell', 'data-page="messages"').replace('</button>', `${unread ? '<i class="unread-dot"></i>' : ''}</button>`)}
      ${ib('navigate', '设置', 'settings', 'data-page="settings"')}</div></header>
    <div class="application">${state.page === 'workspace' ? `<nav class="plugin-rail" aria-label="插件页面入口">
      ${pluginButton('files', 'show-files', '文件工具', 'folder')}</nav>` : ''}
      <main class="main-area">${mainContent()}</main></div>
    <footer class="footer"><div class="connection">${state.page === 'workspace' ? `${status(current().state)}<span>${e(current().user)}@${e(current().host)}:${current().port}${current().route.length ? '　经 ' + e(routeNames(current()).join(' → ')) : ''}</span>${ib('connection-action', current().state === 'connected' ? '断开连接' : '重新连接', current().state === 'connected' ? 'stop' : 'refresh')}` : `${icon('server')}<span>${servers.length} 台服务器　${servers.filter(s => s.state === 'connected').length} 个连接</span>`}</div>
    <div class="footer-right"><span class="traffic">${state.page === 'workspace' && current().state === 'connected' ? '↓ 48.2 KiB/s　↑ 2.4 KiB/s' : ''}</span><span class="prototype-label" title="交互原型，所有数据为演示">原型 · 演示数据</span></div></footer>
    ${state.transfer ? transferDrawer() : ''}${state.newPageMenu ? newPageMenu() : ''}`;
  for (const node of app.querySelectorAll('[data-scroll]')) {
    const pos = scrolls.get(node.dataset.scroll);
    if (pos) { node.scrollTop = pos[0]; node.scrollLeft = pos[1]; }
  }
  for (const input of app.querySelectorAll('.code-input')) syncCodeScroll(input);
  drawConnectionEdges();
  if (revealSelected && state.page === 'catalog') {
    const selected = app.querySelector('.connection-node.selected');
    const scroll = app.querySelector('.map-scroll');
    if (selected && scroll) {
      const nodeBounds = selected.getBoundingClientRect(), scrollBounds = scroll.getBoundingClientRect();
      if (nodeBounds.bottom > scrollBounds.bottom) scroll.scrollTop += nodeBounds.bottom - scrollBounds.bottom + 12;
      else if (nodeBounds.top < scrollBounds.top) scroll.scrollTop += nodeBounds.top - scrollBounds.top - 12;
    }
    revealSelected = false;
  }
  if (!dialog.open) {
    const replacement = restoreFocus(focusKey);
    if (selection && selection[0] != null) try { replacement.setSelectionRange(...selection); } catch {}
  }
}
function navigate(page) { state.page = page; state.newPageMenu = false; state.toolOpen = false; if (page === 'catalog') { state.showDetail = false; revealSelected = true; } render(); }
function mainContent() {
  if (state.page === 'workspace') return workbench();
  return coreConsole();
}
function coreConsole() {
  const pages = [['catalog', '服务器连接', 'server', catalog], ['plugins', '插件管理', 'plug', pluginsPage], ['settings', '设置', 'settings', settingsPage], ['messages', '消息中心', 'bell', messagesPage], ['logs', '诊断日志', 'log', logsPage], ['application', '应用状态', 'activity', applicationPage], ['updates', '软件更新', 'download', updatesPage]];
  const active = pages.find(([id]) => id === state.page) || pages[0];
  state.page = active[0];
  return `<section class="core-console" aria-label="SoDeck 管理台"><aside class="core-sidebar"><header class="sidebar-header"><h2>管理台</h2></header>
    <nav class="console-nav" aria-label="管理台功能">${pages.map(([id, label, glyph]) => b('navigate', label, glyph, 'nav-item ' + (id === state.page ? 'active' : ''), `data-page="${id}" ${id === state.page ? 'aria-current="page"' : ''}`)).join('')}</nav>
    <div class="sidebar-footer muted small">连接资料与应用配置</div></aside>
    <div class="core-content"><header class="core-topline"><span class="console-breadcrumb">管理台<span aria-hidden="true"> / </span>${e(active[1])}</span><span class="grow"></span>${b('session', '返回工作台', 'back', 'quiet row', `data-id="${e(current().id)}"`)}</header>
      <div class="core-page">${active[3]()}</div></div></section>`;
}
function heading(title, description, action = '') {
  return `<header class="page-heading"><div><h1>${title}</h1><p>${description}</p></div>${action}</header>`;
}
function catalog() {
  const proxy = state.catalogKind === 'proxies', records = proxy ? proxies : servers;
  const query = state.query.toLocaleLowerCase().trim();
  const matches = record => (proxy || [...state.selectedTags].every(tag => record.tags.includes(tag))) && [record.name, record.host, record.user, ...record.tags].join(' ').toLocaleLowerCase().includes(query);
  const tree = filterConnectionTree(buildConnectionTree(records, servers, proxies), matches);
  catalogFiltering = !!query || (!proxy && !!state.selectedTags.size);
  catalogNodes = visibleConnectionNodes(tree, state.collapsed, catalogFiltering);
  const allRows = visibleConnectionNodes(tree), targets = allRows.filter(node => node.matched);
  const selected = catalogNodes.find(node => node.key === state.selectedPath) || catalogNodes.find(node => node.record.id === state.selected && node.matched);
  state.selectedPath = selected?.key || '';
  return `<section class="content-page connection-page ${state.showDetail ? 'mobile-detail' : ''}" aria-label="服务器连接">
      <div class="toolbar catalog-toolbar"><select id="catalog-type" class="catalog-type" aria-label="资料类型">${options([['servers', '服务器 · ' + servers.length], ['proxies', 'SOCKS5 代理 · ' + proxies.length]], state.catalogKind)}</select><label class="search">${icon('search')}<input id="catalog-search" type="search" value="${e(state.query)}" aria-label="${proxy ? '搜索代理资料' : '搜索服务器资料'}" placeholder="搜索名称、地址、账号或标签"></label>${b('manage-tags', '管理标签', '', 'quiet')}${ib('refresh-catalog', '刷新目录', 'refresh')}${b('new-server', proxy ? '新增代理' : '新增服务器', 'plus', 'primary row')}</div>
      ${proxy ? '' : `<div class="tag-filter ${state.tagsOpen ? '' : 'mobile-collapsed'}"><div class="tag-filter-header"><div class="mobile-filter-control">${b('toggle-tags', '标签筛选' + (state.selectedTags.size ? ' (' + state.selectedTags.size + ')' : ''), 'down', 'quiet row', `aria-expanded="${state.tagsOpen}" aria-controls="tag-options"`)}</div><span class="small muted filter-label">标签</span><span class="grow"></span><span class="small muted">同时包含所选标签</span>${state.selectedTags.size || query ? b('clear-filters', '清除筛选', '', 'quiet') : ''}</div><div id="tag-options" class="tag-options" role="group" aria-label="标签筛选">${tags.map(tag => b('filter-tag', tag, state.selectedTags.has(tag) ? 'check' : '', 'tag-option ' + (state.selectedTags.has(tag) ? 'active' : ''), `data-tag="${e(tag)}" aria-pressed="${state.selectedTags.has(tag)}"`)).join('')}</div></div>`}
      <div class="catalog-body ${state.showDetail ? 'show-detail' : ''}"><div class="connection-map"><div class="map-toolbar"><span class="small"><strong>${targets.length}</strong> 个目标${allRows.length > targets.length ? ' · ' + (allRows.length - targets.length) + ' 个路径节点' : ''}</span><span class="grow"></span>${b('fold-tree', '展开全部', '', 'quiet', 'data-fold="expand" ' + (catalogFiltering ? 'disabled' : ''))}${b('fold-tree', '收起全部', '', 'quiet', 'data-fold="collapse" ' + (catalogFiltering ? 'disabled' : ''))}</div>
        <div class="map-scroll" data-scroll="connection-tree">${targets.length ? `<div class="node-stage"><svg class="connection-edges" aria-hidden="true"></svg><div class="origin-node">${icon('server')}<span><strong>本机</strong><small>连接起点</small></span></div><div class="connection-tree" role="tree" aria-label="连接路径树">${catalogNodes.map((node, index) => connectionNode(node, index)).join('')}</div></div>` : `<div class="empty">${icon('search')}<h2>没有匹配的资料</h2><p>换个关键词，或清除当前标签。</p>${b('clear-filters', '清除筛选')}</div>`}</div>
        <div class="map-note muted small">${catalogFiltering ? '只显示匹配目标及其祖先；路径节点不计入匹配。' : '相同路径前缀合并；同一跳板可出现在不同路线中。'}</div></div>
      <aside class="catalog-detail" data-scroll="connection-detail">${selected ? serverDetail(selected.record, selected.kind === 'socks5', selected) : '<div class="empty"><p>选择一个节点，查看连接资料与路径。</p></div>'}</aside></div></section>`;
}
function connectionNode(node, index) {
  const record = node.record, selected = node.key === state.selectedPath;
  const onRoute = state.selectedPath === node.key || state.selectedPath.startsWith(node.key + '/');
  return `<div class="node-row" data-node-key="${e(node.key)}" data-parent-key="${e(node.parentKey)}" data-on-route="${onRoute}" style="--depth:${node.depth}">
    ${node.children.length ? ib('toggle-node', (node.expanded ? '收起 ' : '展开 ') + record.name, node.expanded ? 'down' : 'chevron', `data-key="${e(node.key)}" tabindex="-1" ${catalogFiltering ? 'disabled title="筛选时自动展开匹配路径"' : ''}`, 'node-toggle') : '<span class="node-toggle-spacer"></span>'}
    <button type="button" role="treeitem" class="connection-node ${selected ? 'selected' : ''} ${onRoute ? 'on-route' : ''} ${!node.matched ? 'context' : ''}" data-action="select-server" data-id="${e(record.id)}" data-key="${e(node.key)}" data-target="${node.matched}" aria-level="${node.depth}" aria-posinset="${node.position}" aria-setsize="${node.siblings}" aria-selected="${selected}" ${node.children.length ? `aria-expanded="${node.expanded}"` : ''} tabindex="${selected || (!state.selectedPath && index === 0) ? 0 : -1}">
      <span class="node-icon">${icon(node.kind === 'socks5' ? 'network' : 'server')}</span><span class="node-content"><span class="node-heading"><strong title="${e(record.name)}">${e(record.name)}</strong><span class="node-protocol">${node.kind === 'socks5' ? 'SOCKS5' : 'SSH'}</span><span class="grow"></span>${!node.matched ? '<span class="node-context-label">路径节点</span>' : node.kind === 'ssh' ? status(record.state) : ''}</span>
      <span class="node-meta"><span class="node-endpoint" title="${record.user ? e(record.user) + '@' : ''}${e(record.host)}:${record.port}">${record.user ? e(record.user) + '@' : ''}${e(record.host)}:${record.port}</span><span class="node-tags" title="${e(record.tags.join('、'))}">${record.tags.map(tag => `<span>${e(tag)}</span>`).join('')}</span></span></span></button></div>`;
}
function drawConnectionEdges() {
  const stage = app.querySelector('.node-stage');
  if (!stage) return;
  const origin = stage.querySelector('.origin-node'), svg = stage.querySelector('.connection-edges'), bounds = stage.getBoundingClientRect();
  const rows = new Map([...stage.querySelectorAll('.node-row')].map(row => [row.dataset.nodeKey, row]));
  svg.setAttribute('viewBox', `0 0 ${stage.clientWidth} ${stage.clientHeight}`);
  svg.style.height = stage.clientHeight + 'px';
  svg.innerHTML = [...rows.values()].map(row => {
    const parent = row.dataset.parentKey === 'local' ? origin : rows.get(row.dataset.parentKey)?.querySelector('.connection-node');
    if (!parent) return '';
    const a = parent.getBoundingClientRect(), z = row.querySelector('.connection-node').getBoundingClientRect();
    const x = a.left - bounds.left + 10, y = z.top - bounds.top + z.height / 2, end = z.left - bounds.left;
    return `<path class="${row.dataset.onRoute === 'true' ? 'active' : ''}" d="M${x} ${a.bottom - bounds.top} V${y - 6} Q${x} ${y} ${x + 6} ${y} H${end}"/><circle class="${row.dataset.onRoute === 'true' ? 'active' : ''}" cx="${end}" cy="${y}" r="3"/>`;
  }).join('');
}
function serverDetail(server, proxy, node) {
  const context = !node.matched, path = [];
  for (let part = node; part; part = catalogNodes.find(candidate => candidate.key === part.parentKey)) path.unshift(part);
  return `${b('catalog-back', '返回列表', 'back', 'mobile-back quiet row')}<div class="detail-top"><h2>${e(server.name)}</h2><p class="muted small">${e(server.host)}:${server.port}</p></div>
    ${context ? '<p class="detail-context muted small">这是当前路线的中转节点。连接目标时会依次经过左侧高亮路径。</p>' : proxy ? '' : `${status(server.state)}<div class="detail-actions">${b('connect-selected', server.state === 'connected' ? '进入工作台' : '连接服务器', server.state === 'connected' ? 'terminal' : 'play', 'primary row', `data-id="${e(server.id)}"`)}</div>`}
    <div class="detail-actions">${b('edit-server', '编辑', '', '', `data-id="${e(server.id)}"`)}${b('copy-server', '复制', '', '', `data-id="${e(server.id)}"`)}${b('delete-server', '删除', '', 'danger', `data-id="${e(server.id)}"`)}</div>
    <section class="route"><h3>${context ? '当前引用路径' : '完整连接路径'}</h3><div class="route-step"><span class="route-number">0</span><span>本机</span></div>${path.map((part, index) => `<div class="route-step"><span class="route-number">${index + 1}</span><div>${e(part.record.name)}<br><span class="muted small">${part.kind === 'socks5' ? 'SOCKS5' : 'SSH'} · ${e(part.record.host)}:${part.record.port}</span></div></div>`).join('')}</section>
    <details class="connection-properties"><summary>连接资料与认证</summary><dl class="fields"><dt>协议</dt><dd>${proxy ? 'SOCKS5' : 'SSH'}</dd><dt>主机地址</dt><dd>${e(server.host)}</dd><dt>端口</dt><dd>${server.port}</dd>${proxy ? '' : `<dt>用户名</dt><dd>${e(server.user)}</dd>`}<dt>标签</dt><dd>${server.tags.map(tag => `<span class="badge">${e(tag)}</span>`).join(' ') || '无标签'}</dd><dt>认证方式</dt><dd>${proxy ? e(server.auth) : server.auth === 'privateKey' ? 'OpenSSH 私钥' : '密码'}</dd><dt>凭据状态</dt><dd>连接时提供</dd></dl></details>`;
}
function workbench() {
  const workspace = ws(), active = workspace.tabs.find(tab => tab.id === workspace.active);
  const splitTab = workspace.split ? workspace.tabs.find(tab => tab.type === 'editor' && tab.id !== active?.id) || workspace.tabs.find(tab => tab.id !== active?.id) : null;
  return `<section class="workbench" aria-label="服务器工作台">
    ${state.filesOpen ? filePane() : ''}
    <div class="editor-space">${current().state !== 'connected' ? `<div class="inline-banner warning disconnected-bar">${icon('warning')}<span class="grow">连接已断开，保留已读内容和本地草稿。</span>${b('connection-action', '重新连接', 'refresh')}</div>` : ''}
      <div class="page-strip"><div class="page-tab-list" role="tablist" aria-label="工作区页面">${workspace.tabs.map(tab => `<div class="page-tab ${tab.id === workspace.active ? 'active' : ''}" role="presentation">
        <button class="quiet page-tab-select" role="tab" aria-selected="${tab.id === workspace.active}" data-action="page-tab" data-id="${e(tab.id)}">${icon(tab.type === 'terminal' ? 'terminal' : tab.type === 'docker' ? 'box' : 'file')} ${e(tab.title)}${dirty(tab) ? ' <span title="未保存">*</span>' : ''}</button>
        ${ib('close-tab', '关闭 ' + tab.title, 'close', `data-id="${e(tab.id)}"`, 'tab-close')}</div>`).join('')}</div>
        ${ib('new-page-menu', '新建页面', 'plus', `aria-expanded="${state.newPageMenu}"`)}${ib('split', workspace.split ? '取消分屏' : '左右分屏', 'split', workspace.tabs.length < 2 ? 'disabled' : '')}</div>
      <div class="pane-group ${splitTab ? 'split' : ''}">${active ? surface(active) : `<div class="empty"><h2>打开一个工作页面</h2><p>通过新建页面打开终端或工具。</p>${b('new-terminal', '新建终端', 'terminal', 'primary row')}</div>`}${splitTab ? surface(splitTab, true) : ''}</div></div>
    ${state.tool ? `<aside class="tool-pane ${state.toolOpen && state.toolOpen !== 'files' ? 'mobile-open' : ''}"><header class="pane-heading"><h2>${({ system: '系统状态', docker: 'Docker', network: '网络隧道' })[state.tool]}</h2><div class="row">${ib('refresh-tool', '刷新工具', 'refresh')}${ib('hide-tool', '收起工具', 'close')}</div></header><div class="tool-content" data-scroll="tool">${state.tool === 'system' ? systemTool() : state.tool === 'docker' ? dockerTool() : networkTool()}</div></aside>` : ''}
    <nav class="tool-rail" aria-label="插件工具入口">${pluginButton('system', 'tool', '系统状态', 'activity', 'data-tool="system"')}${pluginButton('docker', 'tool', 'Docker', 'box', 'data-tool="docker"')}${pluginButton('network', 'tool', '网络隧道', 'network', 'data-tool="network"')}<div class="rail-bottom">${pluginButton('files', 'transfer', '文件传输', 'download')}</div></nav></section>`;
}
function filePane() {
  return `<aside class="file-pane ${state.toolOpen === 'files' ? 'mobile-open' : ''}"><header class="pane-heading"><h2>文件</h2><div class="row">${ib('refresh-files', '刷新文件', 'refresh')}${ib('show-files', '收起文件', 'close')}</div></header>
    <div class="file-toolbar"><div class="path-field">${icon('folder')}<span>/etc/nginx${state.folder ? '/' + e(state.folder) : ''}</span></div><label class="check-label"><input id="follow-terminal" type="checkbox" ${state.follow ? 'checked' : ''}>跟随终端目录</label></div>
    <div class="file-list" data-scroll="files"><div class="file-row tree-root">${icon('down')}${icon('server')}<strong>${e(current().name)}</strong></div>
      ${files.map(file => `<button type="button" class="file-row ${file.type} ${state.selectedFile === file.name ? 'selected' : ''}" data-action="file-select" data-name="${e(file.name)}">${icon(file.type === 'folder' ? (state.folder === file.name ? 'down' : 'chevron') : 'file')}${file.type === 'folder' ? icon('folder') : ''}<span>${e(file.name)}</span></button>${state.folder === file.name ? b('file-open', 'site.conf', 'file', 'file-row file-indent', 'data-name="site.conf"') : ''}`).join('')}</div>
    <div class="file-footer"><div class="row">${b('upload', '上传', 'upload', 'quiet row', current().state !== 'connected' ? 'disabled' : '')}${b('download-file', '下载', 'download', 'quiet row', current().state !== 'connected' ? 'disabled' : '')}</div></div></aside>`;
}
function highlightCode(text) {
  const pattern = /#[^\n]*|"[^"]*"|'[^']*'|\$[\w_]+|\b\d+\b|\b(?:user|worker_processes|events|worker_connections|http|include|default_type|sendfile|keepalive_timeout|gzip|server|listen|server_name|location|proxy_pass|proxy_set_header|root)\b/g;
  let html = '', index = 0;
  for (const token of text.matchAll(pattern)) {
    const value = token[0], cls = value.startsWith('#') ? 'code-comment' : /^["']/.test(value) ? 'code-string' : value.startsWith('$') ? 'code-variable' : /^\d/.test(value) ? 'code-number' : 'code-keyword';
    html += e(text.slice(index, token.index)) + '<span class="' + cls + '">' + e(value) + '</span>';
    index = token.index + value.length;
  }
  return html + e(text.slice(index)) + '\n';
}
function syncCodeScroll(input) {
  input.previousElementSibling.style.transform = 'translate(' + (-input.scrollLeft) + 'px,' + (-input.scrollTop) + 'px)';
  input.closest('.code-editor').querySelector('.line-numbers').style.transform = 'translateY(' + (-input.scrollTop) + 'px)';
}
function surface(tab, secondary = false) {
  const suffix = secondary ? '-secondary' : '';
  if (tab.type === 'docker') return `<section class="editor-surface">${dockerDetail()}</section>`;
  if (tab.type === 'editor') return `<section class="editor-surface"><div class="editor-toolbar"><span>/etc/nginx/${e(tab.title)}</span><div class="row"><span class="muted small">${dirty(tab) ? '未保存' : '已保存'}</span>${ib('save-file', '保存文件', 'save', `data-id="${e(tab.id)}" ${current().state !== 'connected' ? 'disabled' : ''}`)}</div></div>
    <div class="code-editor"><div class="code-gutter"><pre class="line-numbers" aria-hidden="true">${Array.from({ length: tab.draft.split('\n').length }, (_, index) => index + 1).join('\n')}</pre></div><div class="code-body"><pre class="code-preview" aria-hidden="true">${highlightCode(tab.draft)}</pre><textarea id="code-input-${e(tab.id)}${suffix}" data-id="${e(tab.id)}" data-scroll="code-${e(tab.id)}${suffix}" class="code-input" aria-label="编辑 ${e(tab.title)}" spellcheck="false" wrap="off">${e(tab.draft)}</textarea></div></div></section>`;
  if (tab.container) return `<section class="editor-surface"><header class="editor-surface-header"><span>${e(tab.container)}　/app</span><span>/bin/sh</span></header><div class="terminal"><p class="dim">容器 Shell · ${e(tab.container)}</p><p>/app # pwd</p><p>/app</p><p></p><p class="dim">此页面独立于容器详情，关闭详情不会关闭终端。</p><form class="terminal-input-row" data-form="terminal" data-id="${e(tab.id)}"><label class="prompt" for="terminal-input-${e(tab.id)}">/app #</label><input id="terminal-input-${e(tab.id)}" data-id="${e(tab.id)}" name="command" class="terminal-input" aria-label="演示容器终端输入" autocomplete="off" ${current().state !== 'connected' ? 'disabled' : ''}></form>${tab.output.map(line => '<p>' + e(line) + '</p>').join('')}</div></section>`;
  return `<section class="editor-surface"><header class="editor-surface-header"><span>${e(current().user)}@${e(current().name)}　/etc/nginx</span><span>bash</span></header>
    <div class="terminal" data-scroll="terminal${suffix}" style="font-size:${state.terminalSize}px;line-height:${state.settings.lineHeight};font-family:${e(state.settings.font)},var(--font-code)">
      <p class="dim">Ubuntu 24.04.1 LTS　GNU/Linux 6.8.0　x86_64</p><p class="dim">Last login: Sat Nov 14 14:18:26 2026</p><p></p>
      <p><span class="prompt">${e(current().user)}@${e(shellHost())}:/etc/nginx $</span> <span class="command">ls -lah</span></p>
      <p class="dim">total 84K</p><p>drwxr-xr-x  8 root root 4.0K Nov 14 14:18 <span class="blue">.</span></p>
      <p>drwxr-xr-x 92 root root 4.0K Nov 13 09:42 <span class="blue">..</span></p><p>drwxr-xr-x  2 root root 4.0K Nov 14 14:16 <span class="blue">conf.d</span></p>
      <p>-rw-r--r--  1 root root 1.5K Nov 14 14:18 nginx.conf</p><p>-rw-r--r--  1 root root 5.4K Oct 29 11:30 mime.types</p>
      <p>drwxr-xr-x  2 root root 4.0K Oct 29 11:30 <span class="blue">sites-available</span></p><p>drwxr-xr-x  2 root root 4.0K Nov 14 14:16 <span class="blue">sites-enabled</span></p><p></p>
      <p><span class="prompt">${e(current().user)}@${e(shellHost())}:/etc/nginx $</span> <span class="command">nginx -t</span></p><p>nginx: the configuration file /etc/nginx/nginx.conf syntax is ok</p>
      <p>nginx: configuration file /etc/nginx/nginx.conf test is successful</p><p></p>
      ${tab.output.map(line => `<p>${e(line)}</p>`).join('')}
      <form class="terminal-input-row" data-form="terminal" data-id="${e(tab.id)}"><label class="prompt" for="terminal-input-${e(tab.id)}${suffix}">${e(current().user)}@${e(shellHost())}:/etc/nginx $</label><input id="terminal-input-${e(tab.id)}${suffix}" data-id="${e(tab.id)}" class="terminal-input" name="command" value="${e(tab.input)}" aria-label="演示终端输入" autocomplete="off" spellcheck="false" ${current().state !== 'connected' ? 'disabled' : ''}></form></div></section>`;
}
function chart(memory = false) {
  const values = memory ? [58, 58, 58, 57, 58, 57, 56, 56, 57, 57, 57, 56, 56, 57, 58, 57, 57, 57] : [15, 18, 12, 14, 16, 24, 23, 13, 12, 30, 34, 18, 16, 13, 22, 15, 17, 18];
  return `<svg class="chart ${ws().paused || current().state !== 'connected' ? 'stale' : ''}" viewBox="0 0 240 56" preserveAspectRatio="none" role="img" aria-label="${memory ? '内存' : 'CPU'}演示趋势，最近五分钟"><path class="chart-grid" d="M0 5H240M0 28H240M0 51H240"/><polyline class="chart-line ${memory ? 'memory-line' : ''}" points="${values.map((value, index) => index * 240 / (values.length - 1) + ',' + (51 - value * .46)).join(' ')}"/></svg><div class="chart-labels"><span>5 分钟前</span><span>现在</span></div>`;
}
function systemTool() {
  const network = networkInterfaces.find(item => item.id === ws().networkInterface) || networkInterfaces[0];
  return `<div class="host-summary"><h3>${e(current().name)}</h3><p class="muted small">Ubuntu 24.04 LTS　x86_64</p><p class="muted small">运行 21 天 6 小时</p></div>
    <section class="metric"><div class="row"><h3 class="grow">CPU</h3><span class="muted small">4 vCPU</span></div><div class="metric-value">18.2<small>%</small></div>${chart()}<p class="muted small">负载 0.42 / 0.38 / 0.31</p></section>
    <section class="metric"><div class="row"><h3 class="grow">内存</h3><span class="muted small">8 GiB</span></div><div class="metric-value">57.4<small>%</small></div>${chart(true)}<p class="muted small">已用 4.59 GiB / 8 GiB</p></section>
    <section class="metric"><div class="row"><h3 class="grow">文件系统</h3><span class="muted small">/</span></div><meter class="disk-meter" min="0" max="100" value="42" aria-label="根文件系统已用百分比">42%</meter><div class="row small"><span class="grow">42.0 GiB / 100 GiB</span><span class="muted">42%</span></div></section>
    <section class="metric network-metric"><h3>网络</h3><select id="network-interface" class="network-interface" aria-label="选择网卡">${options(networkInterfaces.map(item => [item.id, item.id + ' · ' + (item.addresses.join('、') || '未分配 IP')]), network.id)}</select><dl class="fields"><dt>接收</dt><dd>${e(network.received)}</dd><dt>发送</dt><dd>${e(network.sent)}</dd><dt>地址</dt><dd>${e(network.addresses.join('、') || '未分配 IP')}</dd></dl></section>
    <div class="row" style="margin-top:14px">${b('pause-sampling', ws().paused ? '继续采集' : '暂停采集', ws().paused ? 'play' : 'pause', 'quiet row')}<span class="grow"></span></div><p class="muted small">${current().state !== 'connected' ? '连接已断开 · 保留最后读数' : ws().paused ? '已暂停 · 保留最后读数' : '每 ' + state.settings.sampleInterval + ' 秒采集 · 最近成功 14:32:10'}</p>`;
}
function dockerTool() {
  const query = state.dockerQuery.toLocaleLowerCase();
  const filtered = containers.filter(container => [container.name, container.image].join(' ').toLocaleLowerCase().includes(query) && (state.dockerFilter === 'all' || (state.dockerFilter === 'failed' ? container.health === 'unhealthy' : container.state === 'stopped')));
  return `<div class="host-summary"><h3>Docker Engine</h3><p class="muted small">27.3.1　4 个容器</p></div><div style="margin-top:14px"><label class="search">${icon('search')}<input id="docker-search" type="search" value="${e(state.dockerQuery)}" placeholder="搜索名称或镜像" aria-label="搜索容器"></label></div>
    <div class="filter-row">${[['all', '全部'], ['failed', '异常'], ['stopped', '已停止']].map(([filter, label]) => b('docker-filter', label, '', state.dockerFilter === filter ? 'active' : '', `data-filter="${filter}"`)).join('')}</div>
    ${filtered.length ? filtered.map(container => `<button type="button" class="container-item ${state.container === container.id ? 'active' : ''}" data-action="container" data-id="${container.id}"><span class="row">${status(container.health === 'unhealthy' ? 'unhealthy' : container.state, '')}<strong>${e(container.name)}</strong></span><span class="container-meta"><span>${container.image.startsWith('ghcr') ? 'shop / ' + container.id : e(container.image)}</span><span class="${container.health === 'unhealthy' ? 'error' : ''}">${container.health === 'unhealthy' ? '不健康' : container.state === 'running' ? '运行中' : '已停止'}</span></span></button>`).join('') : '<div class="empty"><p>没有匹配的容器</p></div>'}
    <p class="muted small" style="margin-top:20px">点击容器，在中央页面查看详情。</p>`;
}
function dockerDetail() {
  const container = containers.find(container => container.id === state.container) || containers[0];
  if (!container) return '<div class="empty"><h2>没有可查看的容器</h2><p>容器列表为空，刷新后查看最新结果。</p></div>';
  return `<div class="docker-detail" data-scroll="docker-detail">${heading(e(container.name), e(container.image), `<div class="row">${b('container-operation', container.state === 'running' ? '停止' : '启动', container.state === 'running' ? 'stop' : 'play', '', 'data-op="' + (container.state === 'running' ? 'stop' : 'start') + '"')}${b('container-operation', '重启', 'refresh', '', 'data-op="restart" ' + (container.state !== 'running' ? 'disabled' : ''))}</div>`)}
      ${container.health === 'unhealthy' ? `<div class="inline-banner error">${icon('warning')}<div><strong>健康检查未通过</strong><p>容器仍在运行，请查看最近日志并核对服务状态。</p></div></div>` : ''}
      <div class="row">${status(container.state, container.state === 'running' ? '运行中' : '已停止')}<span class="badge ${container.health === 'unhealthy' ? 'error' : ''}">${container.health === 'unhealthy' ? '不健康' : container.health === 'healthy' ? '健康' : '无健康检查'}</span><span class="grow"></span>${ib('refresh-tool', '刷新容器详情', 'refresh')}</div>
      <nav class="detail-nav" aria-label="容器详情">${[['overview', '概况'], ['logs', '日志'], ['metrics', '指标']].map(([section, label]) => b('docker-section', label, '', state.dockerSection === section ? 'active' : '', `data-section="${section}"`)).join('')}</nav>
      ${state.dockerSection === 'logs' ? `<div class="toolbar"><label class="search">${icon('search')}<input id="container-log-search" type="search" placeholder="搜索当前日志快照" aria-label="搜索容器日志"></label><select aria-label="日志行数"><option>最近 200 行</option><option>最近 50 行</option><option>最近 500 行</option></select></div><pre class="logs-output" id="container-log-output">${e(containerLog(container))}</pre><p class="page-note">日志快照 · 手动刷新读取最新内容</p>` : state.dockerSection === 'metrics' ? `<h2>最近采集</h2><dl class="fields"><dt>CPU</dt><dd>${container.cpu}</dd><dt>内存</dt><dd>${container.memory}</dd><dt>网络接收 / 发送</dt><dd>12.8 MiB / 4.2 MiB（累计）</dd><dt>进程数</dt><dd>8</dd></dl>` : `<dl class="fields"><dt>容器名称</dt><dd>${e(container.name)}</dd><dt>镜像</dt><dd class="mono">${e(container.image)}</dd><dt>项目 / 服务</dt><dd>shop / ${e(container.id)}</dd><dt>创建时间</dt><dd>2026-11-12 09:42:18</dd><dt>端口</dt><dd class="mono">${e(container.port)}</dd><dt>挂载</dt><dd>/srv/shop/config → /app/config（只读）</dd><dt>网络</dt><dd>shop_default</dd><dt>环境变量</dt><dd>${b('show-environment', '显示演示值', 'lock', 'quiet row')}</dd></dl><div class="detail-actions">${b('container-shell', '打开终端', 'terminal', '', container.state !== 'running' ? 'disabled' : '')}${b('container-operation', '删除容器', '', 'danger', 'data-op="delete" ' + (container.state === 'running' ? 'disabled title="先停止容器，再删除"' : ''))}</div>`}</div>`;
}
function containerLog(container) {
  return `2026-11-14 14:31:52 INFO  Starting ${container.name}\n2026-11-14 14:31:53 INFO  Listening on 127.0.0.1:3000\n2026-11-14 14:32:00 INFO  GET /api/products 200 18ms\n2026-11-14 14:32:06 WARN  Health probe timed out after 5s\n2026-11-14 14:32:08 ERROR GET /health 503 dependency unavailable\n2026-11-14 14:32:09 INFO  Waiting for the next health probe`;
}
function networkTool() {
  return `<div class="row" style="margin-bottom:14px"><span class="muted small grow">${ws().tunnels.length} 条规则</span>${b('new-tunnel', '新增', 'plus', 'quiet row')}</div>
    ${ws().tunnels.map(rule => { const live = rule.running ? rule.active : rule; return `<section class="tool-section"><div class="row"><h3 class="grow">${e(rule.name)}</h3>${status(rule.running ? 'running' : 'idle', rule.running ? '运行中' : '已停止')}</div>
      <p class="mono">${live.type === 'socks5' ? 'SOCKS5' : 'TCP'}　${e(live.host)}:${e(live.port)}</p>
      <p class="muted">${live.type === 'socks5' ? '目标域名由远端服务器解析' : '远端目标 ' + e(live.target) + ':' + e(live.targetPort)}</p>
      ${rule.running ? '<p class="muted">2 个连接　↓ 12.8 KiB/s　↑ 1.2 KiB/s</p>' : ''}
      <div class="row">${b('tunnel-toggle', rule.running ? '停止' : '启动', rule.running ? 'stop' : 'play', '', `data-id="${rule.id}" ${current().state !== 'connected' ? 'disabled' : ''}`)}${b('edit-tunnel', '编辑', '', 'quiet', `data-id="${rule.id}"`)}${ib('copy-tunnel', '复制隧道入口', 'copy', `data-id="${rule.id}" ${!rule.running ? 'disabled' : ''}`)}</div></section>`; }).join('')}
      <p class="page-note">收起工具不停止监听。连接断开后需手动重新启动。</p>`;
}
function settingsPage() {
  const names = { appearance: '外观', terminal: '终端', files: '文件与编辑器', network: '网络隧道', system: '系统状态' };
  return `<section class="content-page" data-scroll="settings">${heading(names[state.settingsSection], '偏好在所有服务器和窗口中共用。')}
    <div class="toolbar"><select id="settings-section-mobile" aria-label="设置分类">${options(Object.entries(names), state.settingsSection)}</select><span class="grow"></span><span class="muted small">${state.settingsDirty ? '有未保存的更改' : '设置已保存'}</span></div>
    <form class="settings-form" data-form="settings">${state.settingsSection === 'appearance' ? `<h2>主题</h2><p class="form-help">选择适合当前工作环境的外观。</p><div class="theme-options">${[['light', 'IDEA 浅色'], ['dark', 'IDEA 深色'], ['dopamine', '多巴胺']].map(([theme, label]) => `<button type="button" class="theme-choice ${theme === state.theme ? 'active' : ''}" data-action="theme" data-theme="${theme}" aria-pressed="${theme === state.theme}"><span class="theme-preview ${theme}" aria-hidden="true"><span class="mini-sidebar"></span><span class="mini-editor"><i></i><i></i><i></i></span></span><span class="theme-name">${label}${theme === state.theme ? icon('check') : ''}</span></button>`).join('')}</div>
      <fieldset class="form-section"><legend>信息密度</legend><div class="form-grid"><label class="field">列表密度<select id="setting-density" name="density">${options([['comfortable', '标准'], ['compact', '紧凑']], state.density)}</select></label></div></fieldset>` : state.settingsSection === 'terminal' ? `<fieldset class="form-section"><legend>终端文字</legend><div class="form-grid"><label class="field">字体<select name="font">${options([['JetBrains Mono', 'JetBrains Mono'], ['Consolas', 'Consolas'], ['monospace', '系统等宽字体']], state.settings.font)}</select></label><label class="field">字号<input id="setting-font-size" name="fontSize" type="number" min="10" max="24" value="${state.terminalSize}"></label><label class="field">行高<select name="lineHeight">${options([['1.8', '1.8'], ['1.5', '1.5'], ['1.6', '1.6']], state.settings.lineHeight)}</select></label><label class="field">光标<select name="cursor">${options([['块状', '块状'], ['竖线', '竖线'], ['下划线', '下划线']], state.settings.cursor)}</select></label></div></fieldset>` :
      state.settingsSection === 'files' ? `<fieldset class="form-section"><legend>文件与编辑器</legend><div class="form-grid"><label class="check-label"><input name="hidden" type="checkbox" ${state.settings.hidden ? 'checked' : ''}>显示隐藏文件</label><label class="check-label"><input name="wrap" type="checkbox" ${state.settings.wrap ? 'checked' : ''}>编辑器自动换行</label><label class="field">Tab 宽度<select name="tabWidth">${options([['4', '4'], ['2', '2'], ['8', '8']], state.settings.tabWidth)}</select></label><label class="field">字号<input name="editorSize" type="number" min="10" max="24" value="${state.settings.editorSize}"></label></div></fieldset>` :
      state.settingsSection === 'system' ? `<fieldset class="form-section"><legend>采集</legend><label class="field">采样间隔<select name="sampleInterval">${options([['5', '5 秒'], ['3', '3 秒'], ['10', '10 秒'], ['30', '30 秒']], state.settings.sampleInterval)}</select></label><p class="form-help">隐藏面板不会暂停采集，可在服务器工具中手动暂停。</p></fieldset>` :
      `<fieldset class="form-section"><legend>新建规则默认值</legend><div class="form-grid">${field('defaultHost', '监听地址', state.settings.defaultHost)}${field('defaultPort', '监听端口', state.settings.defaultPort, 'min="0" max="65535"', 'number')}</div><p class="form-help">端口 0 表示自动分配。默认值只影响之后新建的规则。</p></fieldset>`}
      <div class="form-actions"><button type="submit" class="primary">保存设置</button>${b('reset-settings', '恢复默认', '', 'quiet')}</div><p class="page-note">更改终端和编辑器外观不会重建远程会话。</p></form></section>`;
}
function pluginsPage() {
  return `<section class="content-page" data-scroll="plugins">${heading('插件管理', '管理已安装的工具，按需添加官方插件。', b('local-plugin', '从本地添加', 'folder', 'row'))}
    <nav class="page-tabs" aria-label="插件目录">${b('plugin-tab', '已安装 ' + plugins.filter(p => p.installed).length, '', state.pluginTab === 'installed' ? 'active' : '', 'data-tab="installed"')}${b('plugin-tab', '官方插件', '', state.pluginTab === 'official' ? 'active' : '', 'data-tab="official"')}</nav>
    <p class="muted small">插件启用、关闭和已启用插件升级在重启后生效。</p>
    ${plugins.filter(plugin => state.pluginTab === 'official' || plugin.installed).map(plugin => `<article class="plugin-row">${icon(plugin.icon)}<div class="grow"><h2>${plugin.name}</h2><p>${plugin.description}</p><p class="mono" style="margin-top:5px">sodeck.${plugin.id}　演示版本</p></div><span class="plugin-state ${state.pending.has(plugin.id) ? 'warning' : plugin.running ? 'good' : 'muted'}">${state.pending.has(plugin.id) ? '重启后' + (plugin.enabled ? '启用' : '关闭') : plugin.installed ? plugin.running ? '运行中' : '已关闭' : '未安装'}</span>
      ${plugin.installed ? b('plugin-toggle', plugin.enabled ? '关闭' : '启用', '', '', `data-id="${plugin.id}"`) : b('install-plugin', '安装', 'download', 'primary row', `data-id="${plugin.id}"`)}</article>`).join('')}
    ${state.pending.size ? `<div class="restart-banner"><div class="grow"><strong>${state.pending.size} 项更改等待重启</strong><p class="muted">先完成手头工作。重启会结束当前连接和运行任务。</p></div>${b('restart', '重启并应用', 'refresh', 'row')}</div>` : ''}
    <p class="page-note">核心与插件各自更新，核心安装包不包含业务插件。</p></section>`;
}
function messagesPage() {
  const filtered = messages.filter(message => state.messageFilter === 'all' || !message.read);
  return `<section class="content-page" data-scroll="messages">${heading('消息中心', '查看操作结果和需要处理的中断。', b('read-all', '全部标为已读', 'check', 'row'))}
    <nav class="page-tabs" aria-label="消息筛选">${b('message-filter', '全部消息', '', state.messageFilter === 'all' ? 'active' : '', 'data-filter="all"')}${b('message-filter', '未读 ' + messages.filter(m => !m.read).length, '', state.messageFilter === 'unread' ? 'active' : '', 'data-filter="unread"')}</nav>
    ${filtered.length ? filtered.map(message => `<article class="message-row">${icon(message.type === 'error' ? 'warning' : message.type === 'success' ? 'check' : 'info')}<div class="grow"><h2>${e(message.title)} ${!message.read ? '<span class="badge">未读</span>' : ''}</h2><p class="${message.type === 'error' ? 'error' : 'muted'}">${e(message.text)}</p><div class="message-meta"><span>${e(message.source)}</span><span>${e(message.server)}</span><time>2026-11-14 ${message.time}</time></div></div>${!message.read ? b('read-message', '标为已读', '', 'quiet', `data-id="${message.id}"`) : ''}</article>`).join('') : '<div class="empty"><h2>没有未读消息</h2><p>需要关注的新结果会出现在这里。</p></div>'}</section>`;
}
function logsPage() {
  const filtered = logs.filter(log => (!state.logSource || log.source === state.logSource) && (!state.logLevel || log.level === state.logLevel) && log.text.toLocaleLowerCase().includes(state.logQuery.toLocaleLowerCase()));
  return `<section class="content-page" data-scroll="logs">${heading('诊断日志', '按来源查找运行记录和问题线索。', ib('refresh-logs', '刷新日志', 'refresh'))}
    <div class="toolbar"><label class="search">${icon('search')}<input id="log-search" value="${e(state.logQuery)}" placeholder="搜索日志内容" aria-label="搜索日志内容" type="search"></label>
      <select id="log-source" aria-label="日志来源">${options([['', '全部来源'], ['核心', '核心'], ['Docker', 'Docker'], ['文件', '文件'], ['网络隧道', '网络隧道']], state.logSource)}</select><select id="log-level" aria-label="日志级别">${options([['', '全部级别'], ['info', 'info'], ['warn', 'warn'], ['error', 'error']], state.logLevel)}</select></div>
    <table class="data-table"><thead><tr><th>时间</th><th>来源</th><th class="optional-col">级别</th><th>内容</th></tr></thead><tbody>${filtered.map(log => `<tr><td>${log.time}</td><td>${log.source}</td><td class="optional-col ${log.level === 'warn' ? 'warning' : 'muted'}">${log.level}</td><td>${e(log.text)}</td></tr>`).join('')}</tbody></table>${!filtered.length ? '<div class="empty"><p>没有匹配的日志</p></div>' : ''}
    <p class="page-note">普通日志用于排查问题，日志级别不会自动触发通知。</p></section>`;
}
function applicationPage() {
  return `<section class="content-page" data-scroll="application">${heading('应用状态', '查看核心、连接与插件的运行情况。')}
    <div class="row good">${icon('check')}核心运行正常</div><dl class="fields"><dt>当前连接</dt><dd>${servers.filter(server => server.state === 'connected').length} 个</dd><dt>运行插件</dt><dd>${plugins.filter(plugin => plugin.running).length} 个</dd><dt>待重启更改</dt><dd>${state.pending.size} 项</dd></dl>
    <div class="detail-actions">${b('navigate', '查看插件', 'plug', 'row', 'data-page="plugins"')}${b('navigate', '查看日志', 'log', 'row', 'data-page="logs"')}</div><p class="page-note">核心和插件的版本由各自发行独立管理。</p></section>`;
}
function updatesPage() {
  return `<section class="content-page" data-scroll="updates">${heading('软件更新', '核心与插件各自独立更新。')}
    <h2>当前版本已是演示目录中的最新版本</h2><p class="page-note">核心与各插件可分别下载更新；准备完成后再重启应用。</p>
    <div class="form-section"><label class="check-label"><input type="checkbox" checked>启动时检查更新</label></div>${b('check-updates', '检查更新', 'refresh', 'primary row')}</section>`;
}
function transferDrawer() {
  return `<aside class="drawer" aria-label="文件传输"><header class="pane-heading"><h2>文件传输</h2>${ib('transfer', '关闭传输面板', 'close')}</header><div class="transfer-row"><div class="row">${icon('check')}<strong class="grow">site.conf</strong><span class="good small">已完成</span></div><p class="muted small">上传到 /etc/nginx/conf.d　2.4 KiB</p></div><div class="transfer-row"><div class="row">${icon('check')}<strong class="grow">nginx.conf</strong><span class="good small">已完成</span></div><p class="muted small">下载到本机　1.5 KiB</p></div></aside>`;
}
function newPageMenu() {
  return `<div class="new-page-menu" role="menu" aria-label="新建页面">${b('new-terminal', '新建终端', 'terminal', 'nav-item', 'role="menuitem"')}${b('file-open', '打开 nginx.conf', 'file', 'nav-item', 'role="menuitem" data-name="nginx.conf"')}${b('container', '容器详情', 'box', 'nav-item', 'role="menuitem" data-id="api"')}</div>`;
}
function openModal(value) {
  dialogReturn = focusIdentity(document.activeElement); modal = value; renderModal(); if (!dialog.open) dialog.showModal();
}
function closeModal() {
  clearTimeout(connectTimer);
  if (modal?.type === 'connection' && modal.step === 'progress') {
    const server = servers.find(server => server.id === modal.id);
    if (server) server.state = modal.prior || 'idle';
  }
  modal = null; dialog.close(); render();
  restoreFocus(dialogReturn);
}
function modalShell(title, body, actions) {
  return `<header class="dialog-header"><h2>${title}</h2>${ib('cancel-dialog', '关闭对话框', 'close')}</header><div class="dialog-body">${body}</div><footer class="dialog-footer">${actions}</footer>`;
}
const field = (name, label, value = '', attrs = '', type = 'text') => `<label class="field">${label}<input name="${name}" value="${e(value)}" type="${type}" ${attrs}></label>`;
function captureServerDraft() {
  const form = dialog.querySelector('[data-form="server"]');
  if (form) Object.assign(modal.draft, Object.fromEntries(new FormData(form)));
}
function routeChoices(draft, kind, except = -1) {
  const used = new Set(draft.route.filter((step, index) => index !== except && step.kind === kind).map(step => step.serverId || step.proxyId));
  return (kind === 'ssh' ? servers : proxies).filter(record => record.id !== draft.id && !used.has(record.id));
}
function routeEditor(draft) {
  const route = routeRecords(draft, servers, proxies);
  return `<section class="route-editor"><div class="row"><h3>连接路径</h3><span class="grow"></span><span class="muted small">按顺序经过每个节点</span></div><div class="route-preview"><span>本机</span>${route.map(step => `${icon('chevron')}<span>${e(step.record.name)}</span>`).join('')}${icon('chevron')}<strong>${e(draft.name || '目标服务器')}</strong></div>
    ${draft.route.length ? `<div class="route-steps">${draft.route.map((step, index) => `<div class="route-edit-step"><span class="route-number">${index + 1}</span><label class="field"><span>${step.kind === 'ssh' ? 'SSH 跳板' : 'SOCKS5 代理'}</span><select data-route-index="${index}" aria-label="第 ${index + 1} 跳资料">${options(routeChoices(draft, step.kind, index).map(record => [record.id, record.name + ' (' + record.host + ')']), step.serverId || step.proxyId)}</select></label><div class="route-controls">${ib('route-move', '上移第 ' + (index + 1) + ' 跳', 'down', `data-index="${index}" data-direction="-1" ${index === 0 ? 'disabled' : ''}`, 'move-up')}${ib('route-move', '下移第 ' + (index + 1) + ' 跳', 'down', `data-index="${index}" data-direction="1" ${index === draft.route.length - 1 ? 'disabled' : ''}`)}${ib('route-remove', '移除第 ' + (index + 1) + ' 跳', 'close', `data-index="${index}"`)}</div></div>`).join('')}</div>` : '<p class="muted small">直接连接目标。需要中转时，添加已有跳板或代理。</p>'}
    <div class="route-add">${b('route-add', '添加 SSH 跳板', 'plus', 'row', `data-kind="ssh" ${routeChoices(draft, 'ssh').length ? '' : 'disabled'}`)}${b('route-add', '添加 SOCKS5 代理', 'plus', 'row', `data-kind="socks5" ${routeChoices(draft, 'socks5').length ? '' : 'disabled'}`)}</div><p class="form-help">这里只使用节点的地址和认证资料。中转节点自己的路径不会自动加入；需要经过的节点请逐个添加。</p></section>`;
}
function renderModal() {
  if (!modal) return;
  const focused = document.activeElement, focusKey = dialog.contains(focused) ? focusIdentity(focused) : null;
  const selection = focused && 'selectionStart' in focused ? [focused.selectionStart, focused.selectionEnd] : null;
  const scrollTop = dialog.querySelector('.dialog-body')?.scrollTop || 0;
  dialog.classList.toggle('server-dialog', modal.type === 'server');
  const cancel = b('cancel-dialog', '取消');
  if (modal.type === 'server') {
    const draft = modal.draft, proxy = modal.proxy;
    dialog.innerHTML = `<form data-form="server">${modalShell((draft.id ? '编辑' : '新增') + (proxy ? '代理' : '服务器'),
      `<div class="form-grid">${field('name', '显示名称', draft.name, 'required')}${field('tagsText', '标签（逗号分隔）', draft.tagsText ?? draft.tags.join(', '), 'placeholder="生产环境, Web"')}${field('host', '主机地址', draft.host, 'required placeholder="域名、IPv4 或 IPv6"')}${field('port', '端口', draft.port || (proxy ? 1080 : 22), 'required min="1" max="65535"', 'number')}${!proxy || draft.auth === 'password' ? field('user', '用户名', draft.user, 'required') : ''}
      <label class="field">认证方式<select name="auth" id="server-auth">${options(proxy ? [['none', '无认证'], ['password', '用户名和密码']] : [['privateKey', 'OpenSSH 私钥'], ['password', '密码']], draft.auth || 'privateKey')}</select></label>
      ${draft.auth === 'password' ? field('password', '密码（仅用于演示）', '', 'autocomplete="new-password"', 'password') : !proxy ? field('privateKey', '私钥文件路径', draft.privateKey || '~/.ssh/id_ed25519', 'placeholder="本机私钥路径"') : ''}
      </div>${proxy ? '' : routeEditor(draft)}
      <p class="form-help">原型只保存当前演示资料，不读取真实私钥或系统凭据。</p>`,
      `${cancel}<button class="primary" type="submit">保存资料</button>`)}</form>`;
  } else if (modal.type === 'connection') {
    const server = servers.find(server => server.id === modal.id);
    const body = modal.step === 'fingerprint' ? `<p>首次连接 <strong>${e(server.name)}</strong>，请核对服务器主机密钥。</p><dl class="fields"><dt>目标</dt><dd>${e(server.user)}@${e(server.host)}:${server.port}</dd><dt>密钥类型</dt><dd>ED25519</dd></dl><div class="fingerprint mono">SHA256:6JGhV9cwxrL2XnD4y8mQp0sAk1N7hU3eF5iBzRoTqCc</div><p class="muted small">这是演示指纹。实际连接应与可信渠道提供的指纹核对。</p>` :
      modal.step === 'credentials' ? `<form id="credential-form" data-form="credentials"><p>为 ${e(server.user)}@${e(server.host)} 提供认证资料。</p><div style="margin-top:18px">${field('password', '演示密码', '', 'required autocomplete="off"', 'password')}</div><p class="form-help">输入任意演示值，原型不会发送或保存密码。</p></form>` :
      modal.step === 'failed' ? `<div class="inline-banner error">${icon('warning')}<div><strong>连接超时</strong><p>无法访问 ${e(server.host)}:${server.port}。检查地址、网络和跳板后重试。</p></div></div><p class="muted small">未创建新的工作区，原有会话不受影响。</p>` :
      `<p>正在连接 ${e(server.name)}</p><ol class="connection-stages"><li class="good">${icon('check')}主机密钥已确认</li>${routeRecords(server, servers, proxies).map(step => `<li class="good">${icon('check')}${step.kind === 'ssh' ? 'SSH' : 'SOCKS5'}：${e(step.record.name)}</li>`).join('')}<li class="warning">${icon('refresh')}目标 SSH 握手与认证</li><li class="muted">${icon('terminal')}进入服务器工作台</li></ol>`;
    dialog.innerHTML = modalShell('连接服务器', body, modal.step === 'fingerprint' ? cancel + b('trust-host', '确认并继续', 'check', 'primary row') : modal.step === 'credentials' ? cancel + '<button type="submit" form="credential-form" class="primary">连接</button>' : modal.step === 'failed' ? cancel + b('retry-connection', '重试', 'refresh', 'primary row') : cancel);
  } else if (modal.type === 'tunnel') {
    const draft = modal.draft;
    dialog.innerHTML = `<form data-form="tunnel">${modalShell((draft.id ? '编辑' : '新增') + '隧道规则',
      `<div class="form-grid">${field('name', '规则名称', draft.name, 'required')}<label class="field">类型<select id="tunnel-type" name="type">${options([['socks5', 'SOCKS5'], ['tcp', 'TCP 转发']], draft.type)}</select></label>${field('host', '本机监听地址', draft.host || '127.0.0.1', 'required')}${field('port', '本机监听端口', draft.port || '0', 'required min="0" max="65535"', 'number')}${draft.type === 'tcp' ? field('target', '远端目标地址', draft.target, 'required') + field('targetPort', '远端目标端口', draft.targetPort || '5432', 'required min="1" max="65535"', 'number') : ''}</div><p class="form-help">默认绑定本机回环地址。保存只写配置；运行中的监听需停止后重新启动才使用新配置。</p>`,
      cancel + '<button type="submit" class="primary">保存规则</button>')}</form>`;
  } else if (modal.type === 'tags') {
    dialog.innerHTML = `<form data-form="tag">${modalShell('管理标签', `<div>${tags.map(tag => `<div class="tag-management row"><span class="grow">${e(tag)}</span><span class="muted small">${[...servers, ...proxies].filter(record => record.tags.includes(tag)).length} 项资料</span>${ib('delete-tag', '删除标签 ' + tag, 'close', `data-tag="${e(tag)}"`)}</div>`).join('')}</div><div class="new-tag-field">${field('name', '新标签名称', '', 'required')}</div><p class="form-help">删除标签会从所有资料中移除标签，连接资料仍会保留。</p>`, cancel + '<button type="submit" class="primary">新增标签</button>')}</form>`;
  } else if (modal.type === 'confirm') {
    dialog.innerHTML = modalShell(e(modal.title), `<p>${e(modal.text)}</p>`, cancel + b('confirm', modal.label || '确认', '', modal.danger ? 'danger' : 'primary'));
  } else if (modal.type === 'unsaved') {
    dialog.innerHTML = modalShell('文件有未保存的更改', '<p>' + e(ws().tabs.find(tab => tab.id === modal.id).title) + ' 的更改还没有保存。关闭页面前选择如何处理。</p><p class="muted small">取消会保留页面和草稿。</p>', cancel + b('discard-close', '放弃更改', '', 'danger') + b('save-close', '保存并关闭', '', 'primary', current().state !== 'connected' ? 'disabled' : ''));
  } else {
    dialog.innerHTML = modalShell(e(modal.title), `<p>${e(modal.text)}</p>`, b('cancel-dialog', '关闭'));
  }
  const restored = restoreFocus(focusKey);
  if (selection && restored?.setSelectionRange && !['number', 'checkbox'].includes(restored.type)) restored.setSelectionRange(...selection);
  if (dialog.querySelector('.dialog-body')) dialog.querySelector('.dialog-body').scrollTop = scrollTop;
}
function connect(id) {
  const server = servers.find(server => server.id === id);
  if (!server) return;
  if (server.state === 'connected') { if (dialog.open) closeModal(); state.current = id; navigate('workspace'); return; }
  openModal({ type: 'connection', id, step: 'fingerprint', prior: server.state });
}
function startConnection() {
  const server = servers.find(server => server.id === modal.id);
  modal.step = 'progress'; server.state = 'connecting'; renderModal(); render();
  const id = server.id;
  connectTimer = setTimeout(() => {
    if (modal?.type !== 'connection' || modal.id !== id) return;
    if (id === 'offline') { server.state = 'failed'; modal.step = 'failed'; renderModal(); render(); }
    else { server.state = 'connected'; modal.step = 'done'; closeModal(); state.current = id; navigate('workspace'); toast('已连接 ' + server.name); }
  }, 800);
}
function addPage(type, title, id, extra = {}) {
  const workspace = ws();
  const tab = workspace.tabs.find(tab => tab.id === id);
  if (tab) workspace.active = tab.id;
  else {
    const content = title === 'nginx.conf' ? nginx : title === 'mime.types' ? 'types {\n    text/html html;\n    text/css css;\n    application/json json;\n}\n' : 'server {\n    listen 80;\n    server_name example.test;\n    root /srv/www;\n}\n';
    const created = { id: id || type + '-' + (++sequence), type, title, output: [], input: '', draft: content, saved: content, ...extra };
    workspace.tabs.push(created); workspace.active = created.id;
  }
  state.newPageMenu = false; render();
}
function removeTab(id) {
  const workspace = ws(), index = workspace.tabs.findIndex(tab => tab.id === id);
  workspace.tabs = workspace.tabs.filter(tab => tab.id !== id);
  if (workspace.active === id) workspace.active = workspace.tabs[Math.min(index, workspace.tabs.length - 1)]?.id;
  if (workspace.tabs.length < 2) workspace.split = false;
}
function saveFile(id = ws().active) {
  if (current().state !== 'connected') { toast('连接已断开，草稿仍保留。请重新连接后保存。'); return false; }
  const tab = ws().tabs.find(tab => tab.id === id);
  if (!tab || tab.type !== 'editor') return false;
  tab.saved = tab.draft; render(); toast('文件已保存（演示）'); return true;
}
function confirmAction(title, text, label, action, danger = false) {
  openModal({ type: 'confirm', title, text, label, action, danger });
}
function action(button) {
  const data = button.dataset, name = data.action;
  if (name === 'navigate') return navigate(data.page);
  if (name === 'theme') { state.theme = data.theme; try { localStorage.setItem('sodeck-prototype-theme', state.theme); } catch {} render(); return; }
  if (name === 'session') { state.current = data.id; navigate('workspace'); return; }
  if (name === 'catalog-kind') { state.catalogKind = data.kind; state.selected = data.kind === 'proxies' ? proxies[0]?.id : servers[0]?.id; state.selectedPath = ''; state.showDetail = false; state.query = ''; revealSelected = true; render(); return; }
  if (name === 'toggle-tags') { state.tagsOpen = !state.tagsOpen; render(); return; }
  if (name === 'select-server') { state.selected = data.id; state.selectedPath = data.key; state.showDetail = true; render(); return; }
  if (name === 'catalog-back') { state.showDetail = false; render(); return; }
  if (name === 'clear-filters') { state.query = ''; state.selectedTags.clear(); state.showDetail = false; render(); return; }
  if (name === 'filter-tag') { if (state.selectedTags.has(data.tag)) state.selectedTags.delete(data.tag); else state.selectedTags.add(data.tag); state.showDetail = false; render(); return; }
  if (name === 'toggle-node' || name === 'fold-tree') {
    if (catalogFiltering) return;
    if (name === 'fold-tree') {
      if (data.fold === 'expand') state.collapsed.clear();
      else visibleConnectionNodes(buildConnectionTree(state.catalogKind === 'proxies' ? proxies : servers, servers, proxies)).filter(node => node.children.length).forEach(node => state.collapsed.add(node.key));
    } else if (state.collapsed.has(data.key)) state.collapsed.delete(data.key);
    else { state.collapsed.add(data.key); if (state.selectedPath.startsWith(data.key + '/')) { state.selectedPath = data.key; state.selected = catalogNodes.find(node => node.key === data.key).record.id; } }
    render(); return;
  }
  if (name === 'refresh-catalog') { toast('目录已刷新'); return; }
  if (name === 'choose-server') { state.catalogKind = 'servers'; navigate('catalog'); return; }
  if (name === 'connect-selected') { connect(data.id || state.selected); return; }
  if (['new-server', 'edit-server', 'copy-server'].includes(name)) {
    const proxy = data.id ? proxies.some(record => record.id === data.id) : state.catalogKind === 'proxies';
    const record = (proxy ? proxies : servers).find(server => server.id === data.id);
    const draft = record ? structuredClone(record) : { name: '', user: proxy ? '' : 'ops', host: '', port: proxy ? 1080 : 22, tags: [], auth: proxy ? 'none' : 'privateKey', route: [] };
    if (proxy) draft.auth = draft.auth === '无认证' || draft.auth === 'none' ? 'none' : 'password';
    if (name === 'copy-server') { delete draft.id; draft.name += ' 副本'; }
    openModal({ type: 'server', draft, proxy }); return;
  }
  if (name === 'delete-server') {
    const list = proxies.some(record => record.id === data.id) ? proxies : servers, server = list.find(server => server.id === data.id);
    const references = servers.filter(record => record.route.some(step => (step.serverId || step.proxyId) === data.id));
    if (references.length) { toast('仍被 ' + references.map(record => record.name).join('、') + ' 的路径使用，请先调整这些路径。'); return; }
    if (server.state === 'connected') { toast('请先断开此服务器，再删除资料。'); return; }
    confirmAction('删除连接资料', '删除 ' + server.name + ' 的本机连接资料？', '删除资料', () => { list.splice(list.indexOf(server), 1); state.selected = list[0]?.id; state.selectedPath = ''; state.showDetail = false; }, true); return;
  }
  if (name === 'manage-tags') { openModal({ type: 'tags' }); return; }
  if (name === 'delete-tag') { tags.splice(tags.indexOf(data.tag), 1); [...servers, ...proxies].forEach(record => { record.tags = record.tags.filter(tag => tag !== data.tag); }); state.selectedTags.delete(data.tag); renderModal(); render(); return; }
  if (['route-add', 'route-move', 'route-remove'].includes(name)) {
    captureServerDraft();
    const route = modal.draft.route, index = Number(data.index);
    if (name === 'route-add') {
      const record = routeChoices(modal.draft, data.kind)[0];
      if (!record) return;
      route.push({ id: 'step-' + (++sequence), kind: data.kind, [data.kind === 'ssh' ? 'serverId' : 'proxyId']: record.id });
    } else if (name === 'route-remove') route.splice(index, 1);
    else { const next = index + Number(data.direction); if (next >= 0 && next < route.length) [route[index], route[next]] = [route[next], route[index]]; }
    renderModal();
    if (name === 'route-remove') dialog.querySelector('[data-route-index="' + Math.min(index, route.length - 1) + '"]')?.focus();
    return;
  }
  if (name === 'cancel-dialog') { closeModal(); return; }
  if (name === 'trust-host') { if (servers.find(server => server.id === modal.id).auth === 'password') { modal.step = 'credentials'; renderModal(); } else startConnection(); return; }
  if (name === 'retry-connection') { startConnection(); return; }
  if (name === 'connection-action') {
    if (current().state === 'connected') confirmAction('断开服务器连接', '将结束远程连接并停止此会话的网络监听，已读内容和本地文件草稿会保留。', '断开连接', () => { current().state = 'disconnected'; ws().tunnels.forEach(rule => { rule.running = false; }); }, true);
    else connect(current().id);
    return;
  }
  if (name === 'show-files') { if (innerWidth <= 760) { state.filesOpen = true; state.toolOpen = state.toolOpen === 'files' ? false : 'files'; } else state.filesOpen = !state.filesOpen; render(); return; }
  if (name === 'refresh-files') { toast('文件列表已刷新'); return; }
  if (name === 'file-select') {
    state.selectedFile = data.name;
    if (files.find(file => file.name === data.name)?.type === 'folder') { state.folder = state.folder === data.name ? '' : data.name; render(); }
    else app.querySelectorAll('[data-action="file-select"]').forEach(node => node.classList.toggle('selected', node.dataset.name === data.name));
    return;
  }
  if (name === 'file-open') { state.selectedFile = data.name; state.toolOpen = false; addPage('editor', data.name, 'editor-' + data.name); return; }
  if (name === 'save-file') { saveFile(data.id); return; }
  if (name === 'page-tab') { ws().active = data.id; render(); return; }
  if (name === 'close-tab') {
    if (dirty(ws().tabs.find(tab => tab.id === data.id))) openModal({ type: 'unsaved', id: data.id });
    else { removeTab(data.id); render(); } return;
  }
  if (name === 'save-close' || name === 'discard-close') {
    const id = modal.id;
    if (name === 'save-close' && !saveFile(id)) return;
    if (name === 'discard-close') { const tab = ws().tabs.find(tab => tab.id === id); tab.draft = tab.saved; }
    removeTab(id); closeModal(); return;
  }
  if (name === 'new-page-menu') { state.newPageMenu = !state.newPageMenu; render(); return; }
  if (name === 'new-terminal') { if (current().state !== 'connected') { toast('重新连接后可创建终端。'); return; } if (!plugins.find(plugin => plugin.id === 'terminal').running) { toast('终端插件已关闭，启用并重启后可创建终端。'); return; } addPage('terminal', '终端 ' + (ws().tabs.filter(tab => tab.type === 'terminal').length + 1)); return; }
  if (name === 'split') { ws().split = !ws().split; render(); return; }
  if (name === 'tool') { state.tool = data.tool; state.toolOpen = true; render(); return; }
  if (name === 'hide-tool') { state.tool = ''; state.toolOpen = false; render(); return; }
  if (name === 'refresh-tool') { toast('已读取最新演示快照'); return; }
  if (name === 'pause-sampling') { ws().paused = !ws().paused; render(); return; }
  if (name === 'transfer' || name === 'upload' || name === 'download-file') { state.transfer = !state.transfer; render(); return; }
  if (name === 'docker-filter') { state.dockerFilter = data.filter; render(); return; }
  if (name === 'container') { state.container = data.id; state.tool = 'docker'; state.toolOpen = false; state.dockerSection = 'overview'; addPage('docker', '容器详情', 'docker-detail'); return; }
  if (name === 'docker-section') { state.dockerSection = data.section; render(); return; }
  if (name === 'show-environment') { openModal({ type: 'info', title: '环境变量', text: '演示值：NODE_ENV=production，PORT=3000。实际敏感值默认隐藏。' }); return; }
  if (name === 'container-shell') { const container = containers.find(container => container.id === state.container); addPage('terminal', '容器终端 · ' + container.name, null, { container: container.name }); toast('已创建容器终端页面（演示）'); return; }
  if (name === 'container-operation') {
    const container = containers.find(container => container.id === state.container);
    const labels = { stop: '停止', start: '启动', restart: '重启', delete: '删除' };
    confirmAction(labels[data.op] + '容器', labels[data.op] + ' ' + container.name + '？' + (data.op === 'delete' ? '不会删除挂载卷。' : '可能影响使用此服务的请求。'), labels[data.op] + '容器', () => {
      if (data.op === 'delete') { containers.splice(containers.indexOf(container), 1); state.container = containers[0]?.id; }
      else container.state = data.op === 'stop' ? 'stopped' : 'running';
      toast(labels[data.op] + '操作已完成（演示）');
    }, data.op !== 'start'); return;
  }
  if (name === 'new-tunnel' || name === 'edit-tunnel') { openModal({ type: 'tunnel', draft: name === 'edit-tunnel' ? { ...ws().tunnels.find(rule => rule.id === data.id) } : { name: '', type: 'socks5', host: state.settings.defaultHost, port: state.settings.defaultPort } }); return; }
  if (name === 'tunnel-toggle') { const rule = ws().tunnels.find(rule => rule.id === data.id); rule.running = !rule.running; if (rule.running) rule.active = { ...rule, port: rule.port === '0' ? '49152' : rule.port }; render(); toast(rule.running ? '监听已启动（演示）' : '监听已停止（演示）'); return; }
  if (name === 'copy-tunnel') {
    const rule = ws().tunnels.find(rule => rule.id === data.id).active, address = (rule.type === 'socks5' ? 'socks5h://' : '') + rule.host + ':' + rule.port;
    navigator.clipboard.writeText(address).then(() => toast('已复制 ' + address)).catch(() => toast('入口：' + address)); return;
  }
  if (name === 'reset-settings') { state.terminalSize = 13; state.density = 'comfortable'; state.settings = { font: 'Consolas', lineHeight: '1.8', cursor: '块状', hidden: false, wrap: true, tabWidth: '4', editorSize: '13', sampleInterval: '5', defaultHost: '127.0.0.1', defaultPort: '0' }; state.settingsDirty = true; render(); return; }
  if (name === 'plugin-tab') { state.pluginTab = data.tab; render(); return; }
  if (name === 'plugin-toggle' || name === 'install-plugin') { const plugin = plugins.find(plugin => plugin.id === data.id); plugin.installed = true; plugin.enabled = name === 'install-plugin' ? true : !plugin.enabled; if (plugin.running === plugin.enabled) state.pending.delete(plugin.id); else state.pending.add(plugin.id); render(); return; }
  if (name === 'local-plugin') { openModal({ type: 'info', title: '从本地添加插件', text: '正式应用会打开系统目录选择器，读取插件清单、入口、API 主版本和当前平台声明。原型不读取本机目录。' }); return; }
  if (name === 'restart') { confirmAction('重启并应用插件更改', '当前连接和运行任务将结束。文件草稿需要先保存或放弃。', '重启并应用', () => { if (anyDirty()) { toast('请先处理未保存的文件，再重启。'); return; } state.pending.clear(); plugins.forEach(plugin => { plugin.running = plugin.enabled; }); servers.forEach(server => { if (server.state === 'connected') server.state = 'disconnected'; }); Object.values(workspaces).forEach(workspace => workspace.tunnels.forEach(rule => { rule.running = false; })); toast('已应用插件配置（演示）'); }); return; }
  if (name === 'message-filter') { state.messageFilter = data.filter; render(); return; }
  if (name === 'read-message') { messages.find(message => message.id === data.id).read = true; render(); return; }
  if (name === 'read-all') { messages.forEach(message => { message.read = true; }); render(); return; }
  if (name === 'refresh-logs') { toast('日志已刷新'); return; }
  if (name === 'check-updates') { toast('演示更新目录中没有新版本'); return; }
  if (name === 'confirm') { const callback = modal.action; closeModal(); callback(); render(); }
}
document.addEventListener('click', event => {
  const button = event.target.closest('button[data-action]');
  if (button?.dataset.action === 'select-server') {
    clearTimeout(selectionTimer);
    if (event.detail < 2) selectionTimer = setTimeout(() => action(button), 250);
  } else if (button && !button.disabled) action(button);
  else if (state.newPageMenu && !event.target.closest('.new-page-menu')) { state.newPageMenu = false; render(); }
});
app.addEventListener('dblclick', event => {
  const server = event.target.closest('[data-action="select-server"]');
  if (server && server.dataset.target === 'true' && servers.some(record => record.id === server.dataset.id)) { clearTimeout(selectionTimer); connect(server.dataset.id); }
  const file = event.target.closest('[data-action="file-select"]');
  if (file && files.find(item => item.name === file.dataset.name)?.type === 'file') action({ dataset: { action: 'file-open', name: file.dataset.name } });
});
document.addEventListener('input', event => {
  const input = event.target;
  if (input.id === 'catalog-search') { state.query = input.value; state.showDetail = false; render(); }
  if (input.id === 'docker-search') { state.dockerQuery = input.value; render(); }
  if (input.id === 'log-search') { state.logQuery = input.value; render(); }
  if (input.id.startsWith('code-input')) { ws().tabs.find(tab => tab.id === input.dataset.id).draft = input.value; render(); }
  if (input.id.startsWith('terminal-input')) ws().tabs.find(tab => tab.id === input.dataset.id).input = input.value;
  if (input.closest('[data-form="settings"]')) {
    state.settingsDirty = true;
    if (input.id === 'setting-font-size') state.terminalSize = Math.max(10, Math.min(24, Number(input.value) || 13));
    if (input.id === 'setting-density') state.density = input.value;
    if (input.name in state.settings) state.settings[input.name] = input.type === 'checkbox' ? input.checked : input.value;
    render();
  }
  if (input.id === 'container-log-search') {
    const container = containers.find(container => container.id === state.container);
    document.querySelector('#container-log-output').textContent = containerLog(container).split('\n').filter(line => line.toLocaleLowerCase().includes(input.value.toLocaleLowerCase())).join('\n') || '没有匹配的日志';
  }
});
document.addEventListener('scroll', event => { if (event.target.classList?.contains('code-input')) syncCodeScroll(event.target); }, true);
document.addEventListener('change', event => {
  const input = event.target;
  if (input.id === 'network-interface') { ws().networkInterface = input.value; render(); }
  if (input.id === 'catalog-type') action({ dataset: { action: 'catalog-kind', kind: input.value } });
  if (input.id === 'theme-quick') action({ dataset: { action: 'theme', theme: input.value } });
  if (input.id === 'settings-section-mobile') { state.settingsSection = input.value; render(); }
  if (input.id === 'log-source') { state.logSource = input.value; render(); }
  if (input.id === 'log-level') { state.logLevel = input.value; render(); }
  if (input.id === 'follow-terminal') state.follow = input.checked;
  if (input.id === 'server-auth' || input.id === 'tunnel-type') { modal.draft = { ...modal.draft, ...Object.fromEntries(new FormData(input.closest('form'))) }; renderModal(); }
  if (input.dataset.routeIndex !== undefined) { captureServerDraft(); const step = modal.draft.route[Number(input.dataset.routeIndex)]; step[step.kind === 'ssh' ? 'serverId' : 'proxyId'] = input.value; renderModal(); }
});
document.addEventListener('submit', event => {
  const form = event.target;
  if (!form.dataset.form) return;
  event.preventDefault();
  const data = Object.fromEntries(new FormData(form));
  if (form.dataset.form === 'server') {
    const list = modal.proxy ? proxies : servers, draft = { ...modal.draft, ...data };
    delete draft.password;
    draft.tags = [...new Set((draft.tagsText || '').split(/[,，]/).map(tag => tag.trim()).filter(Boolean))]; delete draft.tagsText;
    const references = draft.route.map(step => step.kind + ':' + (step.serverId || step.proxyId));
    if (new Set(references).size !== references.length || draft.route.some(step => (step.serverId || step.proxyId) === draft.id)) { toast('路径不能重复引用同一资料或引用目标本身。'); return; }
    try { routeRecords(draft, servers, proxies); } catch { toast('路径中的资料已不存在，请重新选择。'); return; }
    draft.port = Number(draft.port); draft.state = modal.draft.state || 'idle';
    if (modal.proxy) { draft.auth = draft.auth === 'none' ? '无认证' : '用户名和密码'; draft.protocol = 'socks5'; }
    if (!draft.id) { draft.id = 'server-' + (++sequence); list.push(draft); }
    else Object.assign(list.find(record => record.id === draft.id), draft);
    draft.tags.forEach(tag => { if (!tags.includes(tag)) tags.push(tag); });
    state.catalogKind = modal.proxy ? 'proxies' : 'servers'; state.query = ''; state.selectedTags.clear(); state.collapsed.clear(); state.selectedPath = ''; state.selected = draft.id; closeModal(); toast('连接资料已保存（演示）');
  }
  if (form.dataset.form === 'credentials') { form.reset(); startConnection(); }
  if (form.dataset.form === 'tunnel') {
    const draft = { ...modal.draft, ...data };
    if (!draft.id) { draft.id = 'tunnel-' + (++sequence); draft.running = false; ws().tunnels.push(draft); }
    else Object.assign(ws().tunnels.find(rule => rule.id === draft.id), draft);
    closeModal(); toast('规则已保存，运行中的监听保持原配置');
  }
  if (form.dataset.form === 'tag') { const name = data.name.trim(); if (!name) return; if (tags.includes(name)) { toast('已有同名标签'); return; } tags.push(name); renderModal(); render(); toast('标签已新增'); }
  if (form.dataset.form === 'settings') { state.settingsDirty = false; render(); toast('设置已保存（演示）'); }
  if (form.dataset.form === 'terminal') {
    const command = data.command.trim(); if (!command) return;
    const tab = ws().tabs.find(tab => tab.id === form.dataset.id);
    tab.output.push((tab.container ? '/app #' : current().user + '@' + shellHost() + ':/etc/nginx $') + ' ' + command);
    const replies = { pwd: tab.container ? '/app' : '/etc/nginx', whoami: current().user, ls: files.map(file => file.name).join('  '), help: '演示命令：pwd、ls、whoami、clear。命令不会发送到服务器。' };
    if (command === 'clear') tab.output = [];
    else tab.output.push(replies[command] || '此命令仅作为原型输入展示，不会在远端执行。输入 help 查看演示命令。');
    tab.input = ''; render(); document.querySelector('.terminal-input')?.focus();
  }
});
dialog.addEventListener('cancel', event => { event.preventDefault(); closeModal(); });
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && state.newPageMenu) { state.newPageMenu = false; render(); }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's' && state.page === 'workspace' && document.activeElement?.classList.contains('code-input')) { event.preventDefault(); saveFile(document.activeElement.dataset.id); }
  const item = event.target.closest('[role="treeitem"]');
  if (item && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'Enter'].includes(event.key)) {
    event.preventDefault(); clearTimeout(selectionTimer);
    const index = catalogNodes.findIndex(node => node.key === item.dataset.key), node = catalogNodes[index];
    let next = node;
    if (event.key === 'Enter') {
      if (node.matched && node.kind === 'ssh') connect(node.record.id);
      else action(item);
      return;
    }
    if (event.key === 'Home') next = catalogNodes[0];
    if (event.key === 'End') next = catalogNodes.at(-1);
    if (event.key === 'ArrowUp') next = catalogNodes[Math.max(0, index - 1)];
    if (event.key === 'ArrowDown') next = catalogNodes[Math.min(catalogNodes.length - 1, index + 1)];
    if (event.key === 'ArrowRight' && node.children.length) {
      if (!node.expanded) state.collapsed.delete(node.key);
      else next = catalogNodes[index + 1];
    }
    if (event.key === 'ArrowLeft') {
      if (node.expanded && !catalogFiltering) state.collapsed.add(node.key);
      else next = catalogNodes.find(candidate => candidate.key === node.parentKey) || node;
    }
    state.selected = next.record.id; state.selectedPath = next.key; render();
    const focused = [...app.querySelectorAll('[role="treeitem"]')].find(candidate => candidate.dataset.key === next.key);
    focused?.focus(); focused?.scrollIntoView({ block: 'nearest' });
    return;
  }
  const tab = event.target.closest('[role="tab"]');
  if (tab && ['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    const tabs = [...app.querySelectorAll('.page-tab-select')];
    const index = tabs.indexOf(tab);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    action(tabs[next]); app.querySelectorAll('.page-tab-select')[next]?.focus();
  }
});
window.addEventListener('resize', drawConnectionEdges);
render();
