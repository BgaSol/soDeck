import { createRenderer } from './render.js';
import { createInterfacePicker } from './interface-picker.js';

SoDeckMotion.observeMotion(document);
const $ = id => document.getElementById(id);
const event = data => sodeck.request('event', data);
let snapshot, expectedContext, visible = true, pending = false, queued = false, epoch = 0, contextEpoch = 0, timer;
let lastHistory = 0, restoreScroll = false, restoringNavigation = false;
const restoredToggles = new Set();
let filesystemMode = 'overview';
const filePages = Object.fromEntries(['overview', 'primary', 'auxiliary', 'system'].map(name => [name, {offset: 0, next: null, previous: [], revision: null}]));
let actionError = '', pollError = '', controlPending = false;
let histories = {cpu: [], memory: [], receive: [], send: []};
let historyEnd = Date.now();
const renderer = createRenderer(() => ({snapshot, actionError, pollError, controlPending, histories, historyEnd}));
const {render, renderHistory, reconcileFiles, options, text} = renderer;
let picker = createInterfacePicker(id => void navigate({interfaceId: id}));
const group = name => snapshot?.groups.find(g => g.name === name);
const list = (kind, start = 0, fileGroup) => event({ action: 'list', kind, offset: start, group: fileGroup, expectedRevision: snapshot?.revision });
async function allDevices(kind, localEpoch) {
  const rows = []; let start = 0;
  for (let i = 0; i < 8; i++) {
    const page = await list(kind, start);
    if (localEpoch !== epoch || !visible) return [];
    rows.push(...page.items);
    if (page.nextOffset == null) break;
    start = page.nextOffset;
  }
  return rows;
}
async function lists(localEpoch) {
  const [interfaces, disks] = await Promise.all([allDevices('interfaces', localEpoch), allDevices('disks', localEpoch)]);
  if (localEpoch !== epoch || !visible) return;
  picker.update(interfaces, snapshot.network.device);
  options($('disk'), disks, snapshot.disk.device);
  const summary = snapshot.filesystemSummary || {};
  text('filesystem-warnings', summary.warnings ? summary.warnings + ' 项空间偏紧' : '');
  const hidden = (summary.auxiliary || 0) + (summary.system || 0);
  $('other-mounts').hidden = !hidden;
  text('other-mounts-label', '其他挂载（' + hidden + '）');
  const focused = document.activeElement, focusedRow = focused?.closest('.filesystem'), previousParent = focusedRow?.parentElement;
  await filesystemList(filesystemMode, 'filesystems', '', localEpoch);
  if (localEpoch !== epoch || !visible) return;
  $('all-filesystems').hidden = !summary.primary || (filesystemMode === 'overview' && summary.primary <= filePages.overview.total);
  text('all-filesystems', filesystemMode === 'overview' ? '展开主要存储（' + summary.primary + '）' : '收起主要存储');
  if ($('other-mounts').open && hidden) {
    await Promise.all(['auxiliary', 'system'].map(name => filesystemList(name, name + '-filesystems', name + '-', localEpoch)));
  }
  if (localEpoch === epoch && visible && focusedRow?.isConnected && previousParent !== focusedRow.parentElement && document.activeElement === document.body && focused.getClientRects().length) focused.focus({ preventScroll: true });
}
function resetFiles() {
  filesystemMode = 'overview'; renderer.resetFiles();
  for (const state of Object.values(filePages)) Object.assign(state, { offset: 0, next: null, previous: [], revision: null });
  for (const id of ['filesystems', 'auxiliary-filesystems', 'system-filesystems']) $(id).replaceChildren();
}
async function filesystemList(name, rootId, prefix, localEpoch) {
  const state = filePages[name];
  let page;
  try { page = await list('filesystems', state.offset, name); }
  catch (error) {
    if (localEpoch !== epoch || !visible) return;
    if (!state.offset || !String(error.message).includes('游标已过期')) throw error;
    state.offset = 0; state.previous = []; page = await list('filesystems', 0, name);
  }
  if (localEpoch !== epoch || !visible) return;
  if (state.offset && state.revision !== page.listRevision) {
    state.offset = 0; state.previous = []; page = await list('filesystems', 0, name);
    if (localEpoch !== epoch || !visible) return;
  }
  state.revision = page.listRevision; state.total = page.total; state.next = page.nextOffset;

  $(prefix + 'previous').disabled = !state.previous.length;
  $(prefix + 'next').disabled = state.next == null;
  $(prefix + 'paging').hidden = !state.offset && state.next == null;
  text(prefix + 'page-label', page.total ? (state.offset + 1) + '–' + (state.offset + page.items.length) + ' / ' + page.total : '');
  if (prefix) $(name + '-section').hidden = !page.total;
  reconcileFiles($(rootId), page.items);
}
async function history(localEpoch) {
  if (!group('dynamic')?.observedAt) { histories = { cpu: [], memory: [], receive: [], send: [] }; renderHistory(); return; }
  const key = snapshot.network.device?.id;
  const [cpu, memory, receive, send] = await Promise.all([
    event({ action: 'history', seriesKey: 'cpu' }), event({ action: 'history', seriesKey: 'memory' }),
    key ? event({ action: 'history', seriesKey: 'net:' + key + ':first' }) : Promise.resolve({ points: [] }),
    key ? event({ action: 'history', seriesKey: 'net:' + key + ':second' }) : Promise.resolve({ points: [] })
  ]);
  if (localEpoch !== epoch || !visible) return;
  histories = { cpu: cpu.points, memory: memory.points, receive: receive.points, send: send.points };
  historyEnd = Date.now(); renderHistory();
}
async function poll(force = false) {
  if (!visible) return;
  if (pending) { if (force) queued = true; return; }
  pending = true; const localEpoch = epoch;
  try {
    const data = await event({ action: 'snapshot', knownRevision: force ? undefined : snapshot?.revision });
    if (localEpoch !== epoch || !visible) return;
    if (!data.unchanged && expectedContext && !Object.entries(expectedContext).every(([key, value]) => data.context?.[key] === value)) return;
    pollError = '';
    if (!data.unchanged) {
      snapshot = data;
      if (restoreScroll) restoreNavigation();
      render(); await lists(localEpoch);
      if (localEpoch !== epoch || !visible) return;
      if (data.revision !== lastHistory) { lastHistory = data.revision; await history(localEpoch); }
      if (restoreScroll) { restoreScroll = false; $('scroll').scrollTop = snapshot.navigation?.scrollTop || 0; }
    }
    freshness();
  } catch (error) { if (localEpoch === epoch) { if (String(error.message).includes('修订已过期')) queued = true; else { pollError = error.message || String(error); freshness(); } } }
  finally { pending = false; if (queued) { queued = false; void poll(true); } }
}
function freshness() {
  renderer.freshness();
  if (Date.now() - historyEnd > 5000) { historyEnd = Date.now(); renderHistory(); }
}
function restoreNavigation() {
  const expanded = snapshot?.navigation?.expandedGroups;
  restoringNavigation = true;
  for (const id of ['device', 'collection', 'disks', 'other-mounts']) {
    const open = Array.isArray(expanded) ? expanded.includes(id) : ['device', 'disks'].includes(id);
    if ($(id).open !== open) { restoredToggles.add(id); $(id).open = open; }
  }
  restoringNavigation = false;
}
function schedule() { clearInterval(timer); if (visible) timer = setInterval(() => void poll(), 1000); }
async function control(action) {
  if (controlPending) return;
  const scope = contextEpoch;
  controlPending = true; freshness();
  try { await event({ action }); if (scope !== contextEpoch) return; actionError = ''; await poll(true); }
  catch (error) { if (scope === contextEpoch) actionError = error.message || String(error); }
  finally { if (scope === contextEpoch) { controlPending = false; freshness(); } }
}
$('refresh').onclick = () => void control('refresh');
$('pause').onclick = () => void control(snapshot?.paused ? 'resume' : 'pause');
async function navigate(state) {
  const localEpoch = ++epoch;
  lastHistory = 0;
  if (state.interfaceId) { histories.receive = []; histories.send = []; renderHistory(); }
  try { await event({action: 'navigation', state}); if (localEpoch === epoch && visible) await poll(true); }
  catch (error) { if (localEpoch === epoch && visible) { actionError = error.message || String(error); freshness(); } }
}
$('disk').onchange = () => void navigate({diskId: $('disk').value});
for (const prefix of ['', 'auxiliary-', 'system-']) {
  const state = () => filePages[prefix ? prefix.slice(0, -1) : filesystemMode];
  $(prefix + 'previous').onclick = () => { const s = state(); s.offset = s.previous.pop() ?? 0; void poll(true); };
  $(prefix + 'next').onclick = () => { const s = state(); if (s.next != null) { s.previous.push(s.offset); s.offset = s.next; void poll(true); } };
}
$('all-filesystems').onclick = () => { filesystemMode = filesystemMode === 'overview' ? 'primary' : 'overview'; void poll(true); };
$('other-mounts').addEventListener('toggle', () => { if (visible && $('other-mounts').open) void poll(true); });
let navigationTimer;
$('scroll').onscroll = () => { picker.close(); clearTimeout(navigationTimer); if (!visible || restoreScroll) return; const scope = contextEpoch; navigationTimer = setTimeout(() => { if (scope === contextEpoch && visible) void event({action: 'navigation', state: {scrollTop: $('scroll').scrollTop}}).catch(() => {}); }, 300); };
for (const id of ['device', 'collection', 'disks', 'other-mounts']) $(id).addEventListener('toggle', () => { if (restoredToggles.delete(id) || restoringNavigation || restoreScroll || !visible) return; void event({ action: 'navigation', state: { expandedGroups: ['device', 'collection', 'disks', 'other-mounts'].filter(id => $(id).open) } }).catch(() => {}); });
$('history-details').addEventListener('toggle', renderHistory);
function tokens(values) {
  for (const [key, value] of Object.entries(values || {})) {
    if (key === 'color-scheme') document.documentElement.style.colorScheme = value;
    else document.documentElement.style.setProperty('--' + key, value);
  }
}
function settingsApplied(settings) {
  if (settings) void sodeck.request('settings', { op: 'applied', pageId: settings.pageId, definitionToken: settings.definitionToken, revision: settings.revision }).catch(() => {});
}
addEventListener('sodeck:mount', e => {
  picker.dispose(); picker = createInterfacePicker(id => void navigate({interfaceId: id}));
  contextEpoch++; controlPending = false; clearTimeout(navigationTimer);
  epoch++; lastHistory = 0; snapshot = e.detail.data; expectedContext = e.detail.context || snapshot?.context; restoreScroll = true; actionError = ''; pollError = '';
  histories = { cpu: [], memory: [], receive: [], send: [] };
  resetFiles();
  tokens(e.detail.tokens); settingsApplied(e.detail.settings); visible = e.detail.visible !== false;
  renderHistory();
  restoreNavigation();
  render();
  sodeck.prepared(); void poll(true); schedule();
});
addEventListener('sodeck:themeChanged', e => { picker.close(); tokens(e.detail.tokens); });
addEventListener('sodeck:settingsChanged', e => { settingsApplied(e.detail.settings); void poll(true); });
addEventListener('sodeck:visibilityChanged', e => { picker.close(); visible = e.detail.visible; epoch++; schedule(); if (visible) void poll(true); });
addEventListener('sodeck:contextChanged', e => {
  contextEpoch++; controlPending = false; clearTimeout(navigationTimer); picker.close(); restoreScroll = true;
  epoch++; lastHistory = 0; expectedContext = e.detail.context; snapshot = undefined; actionError = ''; pollError = ''; resetFiles();
  restoreNavigation();
  histories = { cpu: [], memory: [], receive: [], send: [] };
  for (const id of ['filesystems', 'group-details', 'history-values']) { $(id).replaceChildren(); delete $(id).dataset.items; }
  picker.update([], null); options($('disk'), [], null); $('paging').hidden = true;
  render(); renderHistory(); void poll(true);
});
addEventListener('sodeck:unmount', () => { contextEpoch++; controlPending = false; picker.dispose(); visible = false; epoch++; clearInterval(timer); clearTimeout(navigationTimer); });
