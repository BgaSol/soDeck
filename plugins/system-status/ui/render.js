import { draw, timeline } from './charts.js';

export function createRenderer(state) {
  const $ = id => document.getElementById(id);
  const names = { identity: '设备信息', dynamic: '动态指标', topology: '拓扑 / 负载', capacity: '容量', uptime: '运行时间' };
  const fileNodes = new Map();
  const readings = new Map(['cpu','memory','receive','send','read','write'].map(id => [id, {
    value: $(id).querySelector('.reading-value'), unit: $(id).querySelector('.unit')
  }]));
  function readingValue(id, value, unit) {
    const nodes = readings.get(id);
    if (nodes.value.textContent !== value) nodes.value.textContent = value;
    if (nodes.unit.textContent !== unit) nodes.unit.textContent = unit;
    nodes.unit.hidden = !unit;
  }
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
  const group = name => state().snapshot?.groups.find(g => g.name === name);
  const stale = name => {
    const g = group(name);
    return Boolean(g?.observedAt && Date.now() - g.observedAt > g.staleAfterMs);
  };

  const readingOld = data => data?.observedAt && (data.state !== 'ok' || Date.now() - data.observedAt > (state().snapshot?.intervalSeconds || 5) * 2000);
  const readingLabel = data => data?.state === 'unavailable' ? '读取失败' : data?.state === 'unsupported' ? '暂不可用' : data?.state === 'pending' ? '正在读取' : '采样中';
  function readingNote(id, data) {
    const old = Boolean(readingOld(data));
    $(id).classList.toggle('reading-old', old);
    $(id).title = [data?.reason, data?.observedAt ? '最后成功 · ' + age(data.observedAt) : '', old ? '保留上次读数' : ''].filter(Boolean).join(' · ');
    const note = $(id + '-note');
    note.textContent = old ? '保留读数 · ' + age(data.observedAt) : data?.state === 'unavailable' ? (data.reason || '数据源读取失败') : '';
    note.hidden = !note.textContent;
  }
  function metric(id, data) {
    readingNote(id, data);
    const known = Number.isFinite(data?.value);
    $(id).classList.toggle('unavailable', !known);
    readingValue(id, known ? data.value.toFixed(1) : readingLabel(data), known ? '%' : '');
  }
  function rateText(id, value, available = true, data) {
    readingNote(id, data);
    const label = value == null ? (available ? readingLabel(data) : '暂不可用') : rate(value);
    const split = label.indexOf(' ');
    $(id).classList.toggle('unavailable', split < 0);
    readingValue(id, split < 0 ? label : label.slice(0, split), split < 0 ? '' : label.slice(split + 1));
  }
  function buttonLabel(id, value) {
    $(id).querySelector('.sr-only').textContent = value;
    $(id).title = value;
  }
  function collectionDetails() {
    const { snapshot } = state();
    const root = $('group-details');
    while (root.children.length > snapshot.groups.length) root.lastElementChild.remove();
    for (const g of snapshot.groups) {
      const source = snapshot.data.sourceErrors?.[g.name] || {};
      const error = g.error || Object.entries(source).map(([key, value]) => key + '：' + value).join(' / ');
      const p = root.children[snapshot.groups.indexOf(g)] || root.appendChild(document.createElement('p'));
      const label = names[g.name] + ' · ' + age(g.observedAt) + (error ? ' · ' + error : '') + (g.quarantined ? ' · 已停止自动重试，刷新可重试' : '');
      if (p.textContent !== label) p.textContent = label;
    }
  }
  function freshness() {
    const { snapshot, actionError, pollError, controlPending } = state();
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
    const sourceFailure = Object.values(snapshot.data.sourceErrors || {}).some(group => Object.keys(group).length);
    for (const [id, data] of [['cpu', snapshot.data.cpu], ['memory', snapshot.data.memory], ['receive', snapshot.network?.rates], ['send', snapshot.network?.rates], ['read', snapshot.disk?.rates], ['write', snapshot.disk?.rates]]) readingNote(id, data);
    const status = !snapshot.connected ? '已断线' : snapshot.paused ? '已暂停' : pollError || actionError ? '读取失败' : sourceFailure ? '部分指标读取失败' : isStale ? '数据已过期' : snapshot.collecting ? '正在采集' : g?.observedAt ? '采集中' : '正在读取';
    text('freshness', status);
    text('update-age', age(g?.observedAt) + ' · ' + snapshot.intervalSeconds + ' 秒 / 次');
    text('connection', snapshot.connected ? '已连接' : '已断开');
    text('collection-summary', snapshot.paused ? '已暂停' : snapshot.intervalSeconds + ' 秒 / 次');
    buttonLabel('pause', snapshot.paused ? '恢复' : '暂停');
    $('pause').querySelector('use').setAttribute('href', snapshot.paused ? '#play-icon' : '#pause-icon');
    $('pause').disabled = controlPending || !snapshot.connected;
    $('refresh').disabled = controlPending || !snapshot.connected;
    $('app').classList.toggle('stale', !snapshot.connected || snapshot.paused || isStale);
    $('app').classList.toggle('offline', !snapshot.connected);
    const message = actionError || pollError || snapshot.data.collectionError || (sourceFailure ? '部分指标读取失败，保留最后成功读数。可刷新重试或展开采集详情。' : '') || (!snapshot.connected ? '连接已断开，保留最后一次数据。请在工作台重新连接。' : snapshot.paused ? '采集已暂停，展示暂停前数据。刷新可读取一轮，之后保持暂停。' : isStale ? '动态数据已过期，保留最后一次读数。可刷新重试或查看采集详情。' : '');
    text('notice', message);
    $('notice').hidden = !message;
    $('notice').classList.toggle('error', Boolean(actionError || pollError || snapshot.data.collectionError || sourceFailure));
    const up = snapshot.data.uptime;
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

  }
  function render() {
    const { snapshot } = state();
    if (!snapshot) {
      metric('cpu', {}); metric('memory', {});
      for (const id of ['cpu-caption','cpu-peak','memory-caption','memory-available','host-cores','host-memory']) text(id, '');
      for (const id of ['host-cores','host-memory','memory-details']) $(id).hidden = true;
      for (const id of ['hostname','system','model','cores','load','disk-summary','collection-note','memory-source']) text(id, '正在读取');
      text('host-name', '服务器'); text('host-system', '识别系统中'); text('host-architecture', '架构待读取'); text('architecture', '正在读取'); text('uptime', '正在读取'); text('uptime-short', '运行时间待读取');
      for (const id of ['receive','send','read','write']) rateText(id, null);
      freshness(); return;
    }
    const d = { ...snapshot?.data };
    d.identity ||= {};

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
    const net = snapshot.network || {}, disk = snapshot.disk || {};
    const devicesKnown = snapshot.data.sources?.network?.attemptedAt || snapshot.data.sources?.interfaces?.attemptedAt || group('dynamic')?.observedAt;
    rateText('receive', net.rates?.first, !devicesKnown || Boolean(net.device && !net.device.removed), net.rates || snapshot?.data?.sources?.network);
    rateText('send', net.rates?.second, Boolean(net.device && !net.device.removed), net.rates || snapshot?.data?.sources?.network);
    rateText('read', disk.rates?.first, !devicesKnown || Boolean(disk.device && !disk.device.removed), disk.rates || snapshot?.data?.sources?.disk);
    rateText('write', disk.rates?.second, Boolean(disk.device && !disk.device.removed), disk.rates || snapshot?.data?.sources?.disk);
    text('disk-summary', disk.device?.name || '暂不可用');
    text('collection-note', (d.cacheLimited ? '缓存达到预算，部分历史或设备列表已缩短。' : '') + '隐藏侧栏继续采集。拓扑每 30 秒、容量和运行时间每 60 秒；设备信息按连接或刷新读取。');
    if (!d.uptime) { text('uptime', '正在读取'); text('uptime-short', '运行时间待读取'); }
    freshness();
  }
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
  function reconcileFiles(root, items) {
    const before=SoDeckMotion.captureMotion(root,'.filesystem');
    const focused = root.contains(document.activeElement) ? document.activeElement : null;
    const desired = items.map(f => {
      let row = fileNodes.get(f.mountKey);
      if (!row) {
        row = document.createElement('details'); row.className = 'filesystem';row.setAttribute('data-motion-details',''); row.dataset.mountKey = f.mountKey;
        row.innerHTML = '<summary><div class="filesystem-top"><strong class="filesystem-path"></strong><div class="meter" role="meter" aria-valuemin="0" aria-valuemax="100"><span></span></div><span class="filesystem-usage"></span></div><div class="filesystem-bottom"><span class="filesystem-stats"></span><span class="filesystem-free"></span></div><p class="filesystem-warning" hidden>空间偏紧</p><p class="capacity-age" data-observed-at="0" hidden></p></summary><dl><dt>路径</dt><dd class="detail-path"></dd><dt>可用</dt><dd class="detail-free"></dd><dt>类型</dt><dd class="detail-type"></dd><dt>访问</dt><dd class="detail-access"></dd><dt>观察</dt><dd class="detail-age" data-observed-at="0"></dd></dl>';
        fileNodes.set(f.mountKey, row);
      }
      const put = (selector, value) => { const node = row.querySelector(selector); if (node.textContent !== value) node.textContent = value; };
      const known = f.capacityState === 'known' && Number.isFinite(f.percent);
      const label = f.path || f.name || f.id;
      row.classList.toggle('warning', f.spaceWarning === true);
      put('.filesystem-path', label); row.querySelector('.filesystem-path').title = label;
      put('.filesystem-usage', known ? f.percent.toFixed(1) + '%' : f.capacityState === 'fixedImage' ? '只读镜像' : f.capacityState === 'notApplicable' ? '容量不适用' : '容量不可读');
      put('.filesystem-stats', f.capacityState === 'fixedImage' ? '镜像 ' + bytes(f.total) : f.capacityState === 'notApplicable' ? '系统运行挂载' : f.capacityState === 'unknown' ? '等待有效容量' : bytes(f.used) + ' / ' + bytes(f.total));
      put('.filesystem-free', known ? '可用 ' + bytes(f.available) : '');
      const meter = row.querySelector('.meter'); meter.hidden = !known;
      if (known) { meter.setAttribute('aria-label', label + ' 已用容量'); meter.setAttribute('aria-valuenow', f.percent.toFixed(1)); meter.firstElementChild.style.width = f.percent + '%'; }
      row.querySelector('.filesystem-warning').hidden = !f.spaceWarning;
      row.classList.toggle('reading-old', f.state === 'unavailable');
      row.title = f.state === 'unavailable' ? '容量读取失败：' + (f.reason || '') + ' · 保留上次读数' : '';
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
    const { snapshot, histories, historyEnd } = state();
    const end = historyEnd, gap = snapshot?.intervalSeconds * 3000 || 15000;
    const resources = [{ points: histories.cpu, label: 'CPU', className: 'first' + (readingOld(snapshot?.data?.cpu) ? ' reading-old' : '') }, { points: histories.memory, label: '内存', className: 'second' + (readingOld(snapshot?.data?.memory) ? ' reading-old' : '') }];
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
  return {render, freshness, renderHistory, reconcileFiles, options, text, resetFiles: () => fileNodes.clear()};
}
