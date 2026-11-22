import { draw, timeline } from './charts.js';

SoDeckMotion.observeMotion(document);
const $ = id => document.getElementById(id);
const event = data => sodeck.request('event', data);
const names = { identity: '设备信息', dynamic: '动态指标', topology: '拓扑 / 负载', capacity: '容量', uptime: '运行时间' };
let snapshot, expectedContext, visible = true, pending = false, queued = false, epoch = 0, timer;
let lastHistory = 0, restoreScroll = false;
let filesystemMode = 'overview';
const filePages = Object.fromEntries(['overview', 'primary', 'auxiliary', 'system'].map(name => [name, { offset: 0, next: null, previous: [], revision: null }]));
const fileNodes = new Map();
let actionError = '', pollError = '', controlPending = false;
let histories = { cpu: [], memory: [], receive: [], send: [] };
let historyEnd = Date.now();

const text = (id, value) => { const label = String(value); if ($(id).textContent !== label) {$(id).textContent = label;} };
const validBytes = value => typeof value === 'string' ? /^\d{1,20}$/.test(value) : typeof value === 'number' && Number.isFinite(value) && value >= 0;
const bytes = value => {
  if (!validBytes(value)) return '暂不可用';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  if (typeof value === 'string') {
    const n = BigInt(value); let divisor = 1n, i = 0;
    while (n >= divisor * 1024n && i < units.length - 1) { divisor *= 1024n; i++; }
    const tenths = (n * 10n + divisor / 2n) / divisor;
    return (i ? (tenths / 10n) + (tenths % 10n ? '.' + (tenths % 10n) : '') : n.toString()) + ' ' + units[i];
  }
  let n = value, i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return n.toFixed(i ? 1 : 0).replace(/\.0$/, '') + ' ' + units[i];
};
const rate = value => value == null ? '采样中' : bytes(value) + '/s';
const age = at => {
  if (!at) return '尚未成功读取';
  const n = Math.max(0, Math.floor((Date.now() - at) / 1000));
  return n <= 5 ? '刚刚更新' : n < 60 ? n + ' 秒前' : Math.floor(n / 60) + ' 分 ' + (n % 60) + ' 秒前';
};
const group = name => snapshot?.groups.find(g => g.name === name);
const stale = name => {
  const g = group(name);
  return Boolean(g?.observedAt && Date.now() - g.observedAt > g.staleAfterMs);
};

function metric(id, data) {
  const known = Number.isFinite(data?.value);
  $(id).classList.toggle('unavailable', !known);
  $(id).replaceChildren();
  if (known) {
    $(id).append(data.value.toFixed(1), Object.assign(document.createElement('span'), { className: 'unit', textContent: '%' }));
  } else text(id, data?.state === 'unsupported' ? '暂不可用' : '采样中');
}
function rateText(id, value, available = true) {
  const label = value == null ? (available ? '采样中' : '暂不可用') : rate(value);
  const split = label.indexOf(' ');
  $(id).replaceChildren();
  if (split < 0) { text(id, label); return; }
  $(id).append(label.slice(0, split) + ' ', Object.assign(document.createElement('span'), { className: 'unit', textContent: label.slice(split + 1) }));
}
function buttonLabel(id, value) {
  $(id).querySelector('.sr-only').textContent = value;
  $(id).title = value;
}
function collectionDetails() {
  const root = $('group-details');
  root.replaceChildren();
  for (const g of snapshot.groups) {
    const source = snapshot.data.sourceErrors?.[g.name] || {};
    const error = g.error || Object.entries(source).map(([key, value]) => key + '：' + value).join(' / ');
    const p = document.createElement('p');
    p.textContent = names[g.name] + ' · ' + age(g.observedAt) + (error ? ' · ' + error : '') + (g.quarantined ? ' · 已停止自动重试，刷新可重试' : '');
    root.append(p);
  }
}
function freshness() {
  if (!snapshot) {
    text('notice', actionError || pollError);
    $('notice').hidden = !actionError && !pollError;
    $('notice').classList.toggle('error', Boolean(actionError || pollError));
    text('freshness', actionError || pollError ? '读取失败' : '正在读取');
    text('update-age', '等待当前连接数据');
    text('connection', '正在读取');
    text('collection-summary', '待读取');
    $('pause').disabled = $('refresh').disabled = true;
    $('app').classList.remove('stale', 'offline');
    return;
  }
  const g = group('dynamic');
  const isStale = stale('dynamic');
  const status = !snapshot.connected ? '已断线' : snapshot.paused ? '已暂停' : pollError || actionError ? '读取失败' : isStale ? '数据已过期' : snapshot.collecting ? '正在采集' : g?.observedAt ? '采集中' : '正在读取';
  text('freshness', status);
  text('update-age', age(g?.observedAt));
  text('connection', snapshot.connected ? '已连接' : '已断开');
  text('collection-summary', snapshot.paused ? '已暂停' : snapshot.intervalSeconds + ' 秒 / 次');
  buttonLabel('pause', snapshot.paused ? '恢复' : '暂停');
  $('pause').querySelector('use').setAttribute('href', snapshot.paused ? '#play-icon' : '#pause-icon');
  $('pause').disabled = controlPending || !snapshot.connected;
  $('refresh').disabled = controlPending || !snapshot.connected;
  $('app').classList.toggle('stale', !snapshot.connected || snapshot.paused || isStale);
  $('app').classList.toggle('offline', !snapshot.connected);
  const message = actionError || pollError || snapshot.data.collectionError || (!snapshot.connected ? '连接已断开，保留最后一次数据。请在工作台重新连接。' : snapshot.paused ? '采集已暂停，展示暂停前数据。刷新可读取一轮，之后保持暂停。' : isStale ? '动态数据已过期，保留最后一次读数。可刷新重试或查看采集详情。' : '');
  text('notice', message);
  $('notice').hidden = !message;
  $('notice').classList.toggle('error', Boolean(actionError || pollError || snapshot.data.collectionError));
  const up = group('uptime')?.observedAt || group('dynamic')?.observedAt ? snapshot.data.uptime : null;
  if (up?.seconds != null) {
    const cal = up.observedAt || group('uptime')?.observedAt;
    const increment = !snapshot.paused && snapshot.connected && cal && Date.now() - cal < 120000 && !stale('uptime') && !isStale ? (Date.now() - cal) / 1000 : 0;
    const n = up.seconds + increment;
    text('uptime', Math.floor(n / 86400) + ' 天 ' + Math.floor(n / 3600) % 24 + ' 小时 ' + Math.floor(n / 60) % 60 + ' 分' + (up.estimated ? '（估算）' : ''));
    text('uptime-short', '运行 ' + (n >= 86400 ? Math.floor(n / 86400) + ' 天' : n >= 3600 ? Math.floor(n / 3600) + ' 小时' : Math.floor(n / 60) + ' 分'));
  } else text('uptime-short', group('uptime')?.observedAt || group('dynamic')?.observedAt ? '运行时间未提供' : '运行时间待读取');
  for (const node of document.querySelectorAll('.filesystem [data-observed-at]')) {
    const at = Number(node.dataset.observedAt);
    node.textContent = '容量 · ' + age(at);
    node.hidden = !node.classList.contains('detail-age') && snapshot.connected && !snapshot.paused && at && Date.now() - at < 120000;
  }
  collectionDetails();
  const at = Date.now();
  if (at - historyEnd > 5000) { historyEnd = at; renderHistory(); }
}
function render() {
  const d = { ...snapshot?.data };
  if (!group('identity')?.observedAt) { d.identity = {}; d.memoryTotal = null; }
  if (!group('dynamic')?.observedAt) { d.cpu = {}; d.memory = {}; d.memoryDetails = ''; }
  if (!group('topology')?.observedAt) { d.load = []; d.loadText = ''; }
  if (!group('uptime')?.observedAt && !group('dynamic')?.observedAt) d.uptime = null;
  metric('cpu', d.cpu); metric('memory', d.memory);
  text('cpu-caption', d.cpu?.windowMs ? '本次观察 ' + (d.cpu.windowMs / 1000).toFixed(1) + ' 秒' : d.identity?.cores ? d.identity.cores + ' 个逻辑核心' : '');
  const total = d.memory?.total || d.memoryTotal;
  text('memory-caption', d.memory?.total && validBytes(d.memory.available) ? '已用 ' + bytes((BigInt(d.memory.total) - BigInt(d.memory.available)).toString()) + ' / ' + bytes(d.memory.total) : group('dynamic')?.observedAt ? '此系统未提供可比较的可用内存' : '等待当前连接内存数据');
  text('memory-available', validBytes(d.memory?.available) ? '可用 ' + bytes(d.memory.available) + (d.memory.estimated ? ' · 估算' : '') : '');
  $('memory-details').hidden = !d.memoryDetails;
  text('memory-source', d.memoryDetails || '');
  text('load', d.load?.length ? d.load.join(' / ') : d.loadText || '暂未提供');
  for (const [id, key] of [['hostname', 'hostname'], ['system', 'system'], ['model', 'cpuModel'], ['cores', 'cores']]) text(id, d.identity?.[key] ?? '此系统暂未提供');
  text('host-name', d.identity?.hostname || '服务器');
  text('host-system', d.identity?.system || snapshot?.family || '识别系统中');
  text('host-architecture', d.identity?.architecture || (group('identity')?.observedAt ? '架构未识别' : '架构待读取'));
  text('architecture', d.identity?.architecture || (group('identity')?.observedAt ? '未识别' : '正在读取'));
  text('host-cores', d.identity?.cores ? d.identity.cores + ' 核' : '');
  $('host-cores').hidden = !d.identity?.cores;
  text('host-memory', total ? bytes(total) + ' 内存' : '');
  $('host-memory').hidden = !total;
  const devicesKnown = group('topology')?.observedAt || group('dynamic')?.observedAt;
  const net = devicesKnown ? snapshot.network : {}, disk = devicesKnown ? snapshot.disk : {};
  const dynamic = Boolean(group('dynamic')?.observedAt);
  rateText('receive', dynamic ? net.rates?.first : null, !dynamic || Boolean(net.device && !net.device.removed));
  rateText('send', dynamic ? net.rates?.second : null, !dynamic || Boolean(net.device && !net.device.removed));
  rateText('read', dynamic ? disk.rates?.first : null, !dynamic || Boolean(disk.device && !disk.device.removed));
  rateText('write', dynamic ? disk.rates?.second : null, !dynamic || Boolean(disk.device && !disk.device.removed));
  text('disk-summary', disk.device?.name || '暂不可用');
  text('collection-note', (d.cacheLimited ? '缓存达到预算，部分历史或设备列表已缩短。' : '') + '隐藏侧栏继续采集。拓扑每 30 秒、容量和运行时间每 60 秒；设备信息按连接或刷新读取。');
  if (!d.uptime) { text('uptime', '正在读取'); text('uptime-short', '运行时间待读取'); }
  freshness();
}
const list = (kind, start = 0, fileGroup) => event({ action: 'list', kind, offset: start, group: fileGroup, expectedRevision: snapshot?.revision });
function interfaceRecord(node, item) {
  const name = Object.assign(document.createElement('strong'), { textContent: item?.name || item?.id || '暂不可用' });
  const addresses = Object.assign(document.createElement('span'), { className: 'interface-addresses', textContent: item?.removed ? '设备已移除' : item?.addresses?.length ? item.addresses.join(' · ') : item?.counterInstance ? '性能计数器实例，未映射物理网卡 · 未提供 IP' : '未提供 IP' });
  node.replaceChildren(name, addresses);
}
function interfaceOptions(items, selected) {
  const menu = $('interface-options'), trigger = $('interface');
  const rows = selected?.removed ? [...items, selected] : items;
  const signature = JSON.stringify(rows);
  if (menu.dataset.items !== signature) {
    const focused = menu.contains(document.activeElement) ? document.activeElement.dataset.interfaceId : null;
    menu.replaceChildren();
    for (const item of rows) {
      const button = document.createElement('button');
      button.type = 'button'; button.dataset.interfaceId = item.id; button.disabled = Boolean(item.removed);
      button.setAttribute('role', 'option'); button.tabIndex = -1;
      const record = document.createElement('span'); record.className = 'interface-record'; interfaceRecord(record, item);
      button.append(record); menu.append(button);
    }
    menu.dataset.items = signature;
    if (focused) [...menu.children].find(button => button.dataset.interfaceId === focused)?.focus();
  }
  const focusTarget = [...menu.children].find(button => button.dataset.interfaceId === selected?.id && !button.disabled) || menu.querySelector('button:not(:disabled)');
  for (const button of menu.children) { button.setAttribute('aria-selected', String(button.dataset.interfaceId === selected?.id)); button.autofocus = button === focusTarget; }
  const current = rows.find(item => item.id === selected?.id) || selected;
  const currentSignature = JSON.stringify(current || null);
  if (trigger.dataset.current !== currentSignature) { interfaceRecord($('interface-current'), current); trigger.dataset.current = currentSignature; }
  trigger.disabled = !items.length;
  text('interface-count', items.length ? items.length + ' 个接口' : '');
  if (trigger.disabled) closeInterfaces();
  else if (menu.matches(':popover-open')) positionInterfaces();
}
function closeInterfaces(focus = false) {
  if ($('interface-options').matches(':popover-open')) $('interface-options').hidePopover();
  if (focus) $('interface').focus();
}
function positionInterfaces() {
  const menu = $('interface-options'), r = $('interface').getBoundingClientRect();
  const below = innerHeight - r.bottom - 8, above = r.top - 8;
  const down = below >= Math.min(320, menu.scrollHeight) || below >= above;
  menu.style.width = r.width + 'px'; menu.style.left = r.left + 'px';
  menu.style.maxHeight = Math.max(0, Math.min(320, down ? below : above)) + 'px';
  menu.style.top = (down ? r.bottom + 4 : r.top - menu.offsetHeight - 4) + 'px';
}
$('interface-options').addEventListener('beforetoggle', e => $('interface').setAttribute('aria-expanded', String(e.newState === 'open')));
$('interface-options').addEventListener('toggle', e => { if (e.newState === 'open') positionInterfaces(); });
$('interface-options').addEventListener('click', e => {
  const button = e.target.closest('button'); if (!button || button.disabled) return;
  closeInterfaces(true);
  if (button.dataset.interfaceId === snapshot?.network.device?.id) return;
  epoch++; lastHistory = 0;
  void event({ action: 'navigation', state: { interfaceId: button.dataset.interfaceId } }).then(() => poll(true)).catch(error => { actionError = error.message || String(error); freshness(); });
});
$('interface-options').addEventListener('keydown', e => {
  if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeInterfaces(true); return; }
  if (e.key === 'Tab') { closeInterfaces(true); return; }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
  e.preventDefault();
  const buttons = [...$('interface-options').querySelectorAll('button:not(:disabled)')];
  const index = buttons.indexOf(document.activeElement);
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (index + (e.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length;
  buttons[next]?.focus();
});
$('interface').addEventListener('keydown', e => {
  if (['ArrowDown', 'ArrowUp'].includes(e.key)) { e.preventDefault(); $('interface-options').showPopover(); }
});
addEventListener('resize', () => { if ($('interface-options').matches(':popover-open')) positionInterfaces(); });
function options(select, items, selected) {
  const signature = JSON.stringify([items.map(i => [i.id, i.name]), selected?.removed ? selected : null]);
  if (select.dataset.items !== signature) {
    select.replaceChildren();
    for (const item of items) {
      const o = document.createElement('option'); o.value = item.id; o.textContent = item.name || item.id; select.append(o);
    }
    if (selected?.removed) {
      const o = document.createElement('option'); o.value = selected.id; o.textContent = selected.name; select.append(o);
    } else if (!items.length) {
      select.append(Object.assign(document.createElement('option'), { textContent: '暂不可用' }));
    }
    select.dataset.items = signature;
  }
  if (selected?.id && [...select.options].some(o => o.value === selected.id)) select.value = selected.id;
  select.disabled = !items.length;
}
async function allDevices(kind) {
  const rows = []; let start = 0;
  for (let i = 0; i < 8; i++) {
    const page = await list(kind, start); rows.push(...page.items);
    if (page.nextOffset == null) break;
    start = page.nextOffset;
  }
  return rows;
}
async function lists(localEpoch) {
  const [interfaces, disks] = await Promise.all([allDevices('interfaces'), allDevices('disks')]);
  if (localEpoch !== epoch || !visible) return;
  const devicesKnown = group('topology')?.observedAt || group('dynamic')?.observedAt;
  if (!devicesKnown) { interfaces.length = 0; disks.length = 0; }
  interfaceOptions(interfaces, devicesKnown ? snapshot.network.device : null);
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
  filesystemMode = 'overview'; fileNodes.clear();
  for (const state of Object.values(filePages)) Object.assign(state, { offset: 0, next: null, previous: [], revision: null });
  for (const id of ['filesystems', 'auxiliary-filesystems', 'system-filesystems']) $(id).replaceChildren();
}
async function filesystemList(name, rootId, prefix, localEpoch) {
  const state = filePages[name];
  let page;
  try { page = await list('filesystems', state.offset, name); }
  catch (error) {
    if (!state.offset || !String(error.message).includes('游标已过期')) throw error;
    state.offset = 0; state.previous = []; page = await list('filesystems', 0, name);
  }
  if (localEpoch !== epoch || !visible) return;
  if (state.offset && state.revision !== page.listRevision) {
    state.offset = 0; state.previous = []; page = await list('filesystems', 0, name);
    if (localEpoch !== epoch || !visible) return;
  }
  state.revision = page.listRevision; state.total = page.total; state.next = page.nextOffset;
  if (!group('capacity')?.observedAt) { page.items = []; page.total = 0; state.next = null; }
  $(prefix + 'previous').disabled = !state.previous.length;
  $(prefix + 'next').disabled = state.next == null;
  $(prefix + 'paging').hidden = !state.offset && state.next == null;
  text(prefix + 'page-label', page.total ? (state.offset + 1) + '–' + (state.offset + page.items.length) + ' / ' + page.total : '');
  if (prefix) $(name + '-section').hidden = !page.total;
  reconcileFiles($(rootId), page.items);
}
function reconcileFiles(root, items) {
  const before=SoDeckMotion.captureMotion(root,'.filesystem');
  const focused = root.contains(document.activeElement) ? document.activeElement : null;
  const desired = items.map(f => {
    let row = fileNodes.get(f.mountKey);
    if (!row) {
      row = document.createElement('details'); row.className = 'filesystem';row.setAttribute('data-motion-details',''); row.dataset.mountKey = f.mountKey;
      row.innerHTML = '<summary><div class="filesystem-top"><strong class="filesystem-path"></strong><span class="filesystem-usage"></span></div><div class="filesystem-bottom"><span class="filesystem-stats"></span><div class="meter" role="meter" aria-valuemin="0" aria-valuemax="100"><span></span></div></div><p class="filesystem-warning" hidden>空间偏紧</p><p class="capacity-age" data-observed-at="0" hidden></p></summary><dl><dt>路径</dt><dd class="detail-path"></dd><dt>可用</dt><dd class="detail-free"></dd><dt>类型</dt><dd class="detail-type"></dd><dt>访问</dt><dd class="detail-access"></dd><dt>观察</dt><dd class="detail-age" data-observed-at="0"></dd></dl>';
      fileNodes.set(f.mountKey, row);
    }
    const put = (selector, value) => { const node = row.querySelector(selector); if (node.textContent !== value) node.textContent = value; };
    const known = f.capacityState === 'known' && Number.isFinite(f.percent);
    const label = f.path || f.name || f.id;
    row.classList.toggle('warning', f.spaceWarning === true);
    put('.filesystem-path', label); row.querySelector('.filesystem-path').title = label;
    put('.filesystem-usage', known ? f.percent.toFixed(1) + '%' : f.capacityState === 'fixedImage' ? '只读镜像' : f.capacityState === 'notApplicable' ? '容量不适用' : '容量不可读');
    put('.filesystem-stats', f.capacityState === 'fixedImage' ? '镜像 ' + bytes(f.total) : f.capacityState === 'notApplicable' ? '系统运行挂载' : f.capacityState === 'unknown' ? '等待有效容量' : bytes(f.used) + ' / ' + bytes(f.total));
    const meter = row.querySelector('.meter'); meter.hidden = !known;
    if (known) { meter.setAttribute('aria-label', label + ' 已用容量'); meter.setAttribute('aria-valuenow', f.percent.toFixed(1)); meter.firstElementChild.style.width = f.percent + '%'; }
    row.querySelector('.filesystem-warning').hidden = !f.spaceWarning;
    put('.detail-path', label); put('.detail-free', bytes(f.available)); put('.detail-type', f.filesystemType || '未确认'); put('.detail-access', f.readOnly === true ? '只读' : f.readOnly === false ? '可写' : '未确认');
    for (const node of row.querySelectorAll('[data-observed-at]')) node.dataset.observedAt = f.observedAt || 0;
    return row;
  });
  for (const node of [...root.children]) if (!desired.includes(node)) node.remove();
  for (let i = 0; i < desired.length; i++) if (root.children[i] !== desired[i]) root.insertBefore(desired[i], root.children[i] || null);
  SoDeckMotion.settleMotion(root,before,'.filesystem');
  if (focused?.isConnected && document.activeElement !== focused) focused.focus({ preventScroll: true });
  if (fileNodes.size > 512) for (const [key, node] of fileNodes) if (!node.isConnected) fileNodes.delete(key);
  if (!items.length && root.id === 'filesystems') root.append(Object.assign(document.createElement('p'), { className: 'caption', textContent: '暂未读取到本地文件系统。远程挂载不查询容量。' }));
}
function renderHistory() {
  const end = historyEnd, gap = snapshot?.intervalSeconds * 3000 || 15000;
  const resources = [{ points: histories.cpu, label: 'CPU', className: 'first' }, { points: histories.memory, label: '内存', className: 'second' }];
  draw($('resource-chart'), resources, 100, gap, value => value.toFixed(1) + '%', end);
  const networkPoints = histories.receive.concat(histories.send).filter(p => p.at >= end - 300000 && p.at <= end && Number.isFinite(p.value));
  const peak = Math.max(0, ...networkPoints.map(p => p.value));
  draw($('network-chart'), [{ points: histories.receive, label: '接收', className: 'first' }, { points: histories.send, label: '发送', className: 'second' }], Math.max(1, peak), gap, rate, end);
  const cpuPoints = histories.cpu.filter(p => p.at >= end - 300000 && Number.isFinite(p.value));
  text('cpu-peak', cpuPoints.length ? '峰值 ' + Math.max(...cpuPoints.map(p => p.value)).toFixed(1) + '%' : '');
  text('network-peak', networkPoints.length ? '峰值 ' + rate(peak) : '等待有效采样');
  text('resource-end', snapshot && (!snapshot.connected || snapshot.paused || stale('dynamic')) ? '当前时间 · 保留旧值' : '现在');
  if ($('history-details').open) {
    $('history-values').replaceChildren(...timeline(resources, end).map(p => Object.assign(document.createElement('li'), { textContent: new Date(p.at).toLocaleTimeString() + ' · ' + resources.map(s => { const value = s.points.find(sample => sample.at === p.at)?.value; return s.label + ' ' + (Number.isFinite(value) ? value.toFixed(1) + '%' : '采样缺口'); }).join(' / ') })));
  }
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
      snapshot = data; render(); await lists(localEpoch);
      if (localEpoch !== epoch || !visible) return;
      if (data.revision !== lastHistory) { lastHistory = data.revision; await history(localEpoch); }
      if (restoreScroll) { restoreScroll = false; for (const id of snapshot.navigation?.expandedGroups || []) if ($(id)) $(id).open = true; $('scroll').scrollTop = snapshot.navigation?.scrollTop || 0; }
    }
    freshness();
  } catch (error) { if (localEpoch === epoch) { if (String(error.message).includes('修订已过期')) queued = true; else { pollError = error.message || String(error); freshness(); } } }
  finally { pending = false; if (queued) { queued = false; void poll(true); } }
}
function schedule() { clearInterval(timer); if (visible) timer = setInterval(() => void poll(), 1000); }
async function control(action) {
  if (controlPending) return;
  controlPending = true; freshness();
  try { await event({ action }); actionError = ''; await poll(true); }
  catch (error) { actionError = error.message || String(error); }
  finally { controlPending = false; freshness(); }
}
$('refresh').onclick = () => void control('refresh');
$('pause').onclick = () => void control(snapshot?.paused ? 'resume' : 'pause');
$('disk').onchange = () => {
  epoch++; lastHistory = 0;
  void event({ action: 'navigation', state: { diskId: $('disk').value } }).then(() => poll(true)).catch(error => { actionError = error.message || String(error); freshness(); });
};
for (const prefix of ['', 'auxiliary-', 'system-']) {
  const state = () => filePages[prefix ? prefix.slice(0, -1) : filesystemMode];
  $(prefix + 'previous').onclick = () => { const s = state(); s.offset = s.previous.pop() ?? 0; void poll(true); };
  $(prefix + 'next').onclick = () => { const s = state(); if (s.next != null) { s.previous.push(s.offset); s.offset = s.next; void poll(true); } };
}
$('all-filesystems').onclick = () => { filesystemMode = filesystemMode === 'overview' ? 'primary' : 'overview'; void poll(true); };
$('other-mounts').addEventListener('toggle', () => { if ($('other-mounts').open) void poll(true); });
let navigationTimer;
$('scroll').onscroll = () => { closeInterfaces(); clearTimeout(navigationTimer); navigationTimer = setTimeout(() => void event({ action: 'navigation', state: { scrollTop: $('scroll').scrollTop } }).catch(() => {}), 300); };
for (const id of ['device', 'collection', 'disks', 'other-mounts']) $(id).addEventListener('toggle', () => void event({ action: 'navigation', state: { expandedGroups: ['device', 'collection', 'disks', 'other-mounts'].filter(id => $(id).open) } }).catch(() => {}));
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
  epoch++; lastHistory = 0; snapshot = e.detail.data; expectedContext = e.detail.context || snapshot?.context; restoreScroll = true; actionError = ''; pollError = '';
  histories = { cpu: [], memory: [], receive: [], send: [] };
  resetFiles();
  tokens(e.detail.tokens); settingsApplied(e.detail.settings); visible = e.detail.visible !== false;
  renderHistory();
  if (snapshot) { render(); for (const id of snapshot.navigation?.expandedGroups || []) if ($(id)) $(id).open = true; }
  sodeck.prepared(); void poll(true); schedule();
});
addEventListener('sodeck:themeChanged', e => tokens(e.detail.tokens));
addEventListener('sodeck:settingsChanged', e => { settingsApplied(e.detail.settings); void poll(true); });
addEventListener('sodeck:visibilityChanged', e => { closeInterfaces(); visible = e.detail.visible; epoch++; schedule(); if (visible) void poll(true); });
addEventListener('sodeck:contextChanged', e => {
  epoch++; lastHistory = 0; expectedContext = e.detail.context; snapshot = undefined; actionError = ''; pollError = ''; resetFiles();
  $('other-mounts').open = false;
  histories = { cpu: [], memory: [], receive: [], send: [] };
  for (const id of ['filesystems', 'group-details', 'history-values']) { $(id).replaceChildren(); delete $(id).dataset.items; }
  interfaceOptions([], null); options($('disk'), [], null); $('paging').hidden = true;
  render(); renderHistory(); void poll(true);
});
addEventListener('sodeck:unmount', () => { closeInterfaces(); visible = false; epoch++; clearInterval(timer); clearTimeout(navigationTimer); });
