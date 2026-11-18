const $ = id => document.getElementById(id);
const event = data => sodeck.request('event', data);
const unwrap = result => result.data ?? result;
let visible = true, mounted = false, pending = false, epoch = 0, timer;
let snapshot, active, selected = null, cursor = '0', nextCursor = '0', pages = [], monitorSequence = 0, detailSequence = 0;
const rows = new Map();
const labels = {opening:'正在启动', running:'执行中', committing:'正在提交', completed:'已结束', cancelled:'已取消', failed:'失败', unavailable:'结果不可用', unknown:'结果未知', rejected:'未接纳', pending:'等待接纳'};
function text(id, value) { const el = $(id); if (el.textContent !== value) el.textContent = value; }
function notice(message) { $('notice').hidden = !message; text('notice', message); }
function theme(tokens) { for (const [key, value] of Object.entries(tokens || {})) document.documentElement.style.setProperty(key === 'color-scheme' ? key : '--' + key, value); }
function command(task) { return task.input?.command ?? task.input?.remotePath ?? task.requestId; }
function state(task) { return task.native?.state || task.native?.admission || 'pending'; }
function status(task) { return `${labels[state(task)] || state(task)}${task.native?.exitCode != null ? ` · 退出码 ${task.native.exitCode}` : ''}`; }
function stateClass(task) { return 'state' + (['failed','unavailable','unknown','rejected'].includes(state(task)) || task.native?.exitCode ? ' problem' : task.done ? ' ended' : ''); }
function evidence(task) { return task.evidence?.complete ? '完整输出已保存' : task.done ? '证据不完整' : '正在保存'; }
function time(value) { return value ? new Date(value).toLocaleTimeString('zh-CN', {hour12:false}) : '未提供'; }
function duration(task) {
  if (!task.createdAt) return '用时未提供';
  const seconds = Math.max(0, Math.floor(((task.finishedAt || Date.now()) - task.createdAt) / 1000));
  return `${task.done ? '用时' : '已持续'} ${seconds >= 60 ? `${Math.floor(seconds / 60)} 分 ${seconds % 60}` : seconds} 秒`;
}
function output(page) { return [page?.stdout, page?.stderr ? `标准错误：\n${page.stderr}` : ''].filter(Boolean).join('\n') || '暂时没有输出。'; }
function selectedTask() { return snapshot?.tasks?.find(task => task.taskId === selected); }
function closeDetail() {
  detailSequence++; selected = null; $('detail').hidden = true; $('history').append($('detail'));
  for (const row of rows.values()) row.button.setAttribute('aria-expanded', 'false');
}
function paintDetail(task) {
  $('detail').hidden = false;
  if ($('detail').parentElement !== rows.get(task.taskId).li) rows.get(task.taskId).li.append($('detail'));
  text('detail-state', status(task)); $('detail-state').className = stateClass(task);
  text('detail-input-label', task.kind === 'exec' ? '实际命令' : '远程文件'); text('detail-command', command(task));
  const target = task.target || snapshot.target?.target;
  text('detail-server', target ? `${target.name || target.endpoint} · ${target.endpoint} · ${target.username || '账号未提供'}` : '服务器信息不可用');
  text('detail-created', time(task.createdAt)); text('detail-time', duration(task));
  text('detail-request', task.requestId); text('detail-id', task.taskId);
  text('detail-evidence', task.kind === 'download' && task.evidence?.complete ? '文件已保存' : evidence(task));
  text('detail-path', task.evidence?.path || task.directory || '尚未提供');
  const errors = [task.evidence?.error, task.native?.error, task.cleanupError].filter(Boolean);
  text('detail-error', errors.map(error => error.message || String(error)).join('\n')); $('detail-error').hidden = !errors.length;
  $('detail-output-section').hidden = task.kind !== 'exec'; $('detail-cancel').hidden = task.done;
  $('detail-cancel').setAttribute('aria-label', `请求取消 ${task.requestId}`);
}
function render(data) {
  const pinned = selectedTask(), focus = document.activeElement, anchor = rows.get(selected)?.li;
  const scrollTop = $('history').scrollTop, anchorTop = anchor?.getBoundingClientRect().top;
  const tasks = [...(data.tasks || [])];
  if (selected && !tasks.some(task => task.taskId === selected)) {
    if (pinned && (!data.target?.targetId || data.target.targetId === pinned.targetId)) tasks.push(pinned);
    else closeDetail();
  }
  snapshot = {...data, tasks};
  text('service', data.service === 'ready' ? '本机接入已启动' : data.service); text('endpoint', data.endpoint);
  const target = data.target?.target;
  text('server', target ? target.name || target.endpoint : '服务器不可用');
  text('server-detail', target ? `${target.username || '账号未提供'}@${target.endpoint} · ${data.target.state === 'connected' ? '已连接' : '已断开'}` : '请在 SoDeck 中连接服务器。');
  text('directory', data.evidenceDirectory);
  const current = new Set(tasks.map(task => task.taskId));
  for (const [id, row] of rows) if (!current.has(id)) { row.li.remove(); rows.delete(id); }
  $('empty').hidden = Boolean(tasks.length);
  tasks.forEach((task, index) => {
    let row = rows.get(task.taskId);
    if (!row) {
      const li = document.createElement('li'), button = document.createElement('button'), heading = document.createElement('div');
      const label = document.createElement('span'), date = document.createElement('time'), input = document.createElement('span'), meta = document.createElement('span');
      button.type = 'button'; button.className = 'task'; button.setAttribute('aria-controls','detail');
      heading.className = 'heading'; input.className = 'command'; meta.className = 'meta';
      button.onclick = () => selectTask(task.taskId); heading.append(label, date); button.append(heading, input, meta); li.append(button);
      row = {li, button, label, date, input, meta}; rows.set(task.taskId, row);
    }
    if ($('tasks').children[index] !== row.li) $('tasks').insertBefore(row.li, $('tasks').children[index] || null);
    row.label.textContent = `${task.kind === 'exec' ? '命令' : '下载'} · ${status(task)}`; row.label.className = stateClass(task);
    row.date.textContent = time(task.createdAt); if (task.createdAt) row.date.dateTime = new Date(task.createdAt).toISOString();
    if (row.input.textContent !== command(task)) row.input.textContent = command(task);
    row.meta.textContent = `${duration(task)} · ${task.kind === 'download' && task.evidence?.complete ? '文件已保存' : evidence(task)}`;
    row.button.setAttribute('aria-label', `查看${task.kind === 'exec' ? '命令' : '下载'}详情 ${task.requestId}`); row.button.setAttribute('aria-expanded', String(selected === task.taskId));
  });
  const running = tasks.filter(task => !task.done), previous = active?.taskId;
  active = running[0]; text('active-count', running.length ? `${running.length} 个操作` : '');
  $('idle').hidden = Boolean(active); $('monitor-body').hidden = !active;
  if (active) {
    text('monitor-state', status(active)); $('monitor-state').className = stateClass(active);
    text('monitor-time', duration(active)); text('monitor-command', command(active));
    $('monitor-cancel').setAttribute('aria-label', `请求取消 ${active.requestId}`);
    text('monitor-progress', active.kind === 'download' ? '下载远程文件' : `提交于 ${time(active.createdAt)}`);
    if (previous !== active.taskId) { monitorSequence++; text('monitor-output', '等待读取输出'); }
  } else { monitorSequence++; text('monitor-output', ''); }
  if (selectedTask()) paintDetail(selectedTask());
  if (focus?.isConnected && $('history').contains(focus) && document.activeElement !== focus) focus.focus({preventScroll:true});
  $('history').scrollTop = anchor?.isConnected ? $('history').scrollTop + anchor.getBoundingClientRect().top - anchorTop : scrollTop;
}
async function loadMonitor() {
  if (!active || !mounted || !visible) return;
  const id = active.taskId, sequence = ++monitorSequence, current = epoch;
  try {
    const task = unwrap(await event({action:'task', taskId:id, tail:true}));
    if (current !== epoch || sequence !== monitorSequence || active?.taskId !== id) return;
    text('monitor-output', task.kind === 'exec' ? output(task.output) : task.evidence?.complete ? '文件已保存。' : '正在下载，完成后可在详情中查看本地路径。');
    $('monitor-output').scrollTop = $('monitor-output').scrollHeight;
  } catch (error) { if (current === epoch && sequence === monitorSequence) text('monitor-output', `输出读取失败：${error.message || error}`); }
}
async function loadDetail() {
  const task = selectedTask();
  if (!task || !mounted || !visible) return;
  const id = selected, start = cursor, sequence = ++detailSequence, current = epoch;
  try {
    const result = unwrap(await event({action:'task', taskId:id, cursor:start}));
    if (current !== epoch || sequence !== detailSequence || selected !== id || cursor !== start) return;
    paintDetail(result); text('detail-output', output(result.output));
    nextCursor = result.output?.nextCursor || start;
    $('previous').disabled = !pages.length; $('next').disabled = !result.output?.hasMore;
    text('page-number', `第 ${pages.length + 1} 页`);
  } catch (error) {
    if (current === epoch && sequence === detailSequence) { text('detail-output', `输出读取失败：${error.message || error}`); $('next').disabled = true; }
  }
}
function selectTask(id) {
  if (selected === id) { closeDetail(); return; }
  selected = id; cursor = '0'; pages = []; nextCursor = '0'; detailSequence++;
  $('previous').disabled = true; $('next').disabled = true; text('page-number','第 1 页'); text('detail-output','等待读取输出');
  render(snapshot); void loadDetail(); rows.get(id)?.button.scrollIntoView({block:'nearest'});
}
async function refresh() {
  if (!mounted || !visible || pending) return;
  clearTimeout(timer); pending = true; const current = epoch;
  try {
    const result = await event({action:'snapshot'});
    if (current === epoch) { if ($('freshness').textContent === '状态可能已过期') notice(''); render(unwrap(result)); text('freshness', time(Date.now())); await Promise.all([loadMonitor(), loadDetail()]); }
  } catch (error) { if (current === epoch) { notice(String(error.message || error)); text('freshness','状态可能已过期'); } }
  finally { pending = false; if (mounted && visible) timer = setTimeout(refresh, current === epoch ? 1500 : 0); }
}
async function operate(button, action) {
  const current = epoch; button.disabled = true;
  try { await action(current); if (current === epoch) await refresh(); }
  catch (error) { if (current === epoch) notice(String(error.message || error)); }
  finally { button.disabled = false; }
}
function cancel(button, task) { if (task) return operate(button, async current => { await event({action:'cancel', taskId:task.taskId}); if (current === epoch) notice('已请求取消，等待远端实际结果。'); }); }
$('monitor-cancel').onclick = () => cancel($('monitor-cancel'), active);
$('detail-cancel').onclick = () => cancel($('detail-cancel'), selectedTask());
$('monitor-detail').onclick = () => { if (active) { if (selected === active.taskId) rows.get(selected)?.button.scrollIntoView({block:'nearest'}); else selectTask(active.taskId); } };
$('detail-close').onclick = () => { const button = rows.get(selected)?.button; closeDetail(); button?.focus(); };
$('next').onclick = () => { if (nextCursor !== cursor) { pages.push(cursor); cursor = nextCursor; $('next').disabled = true; void loadDetail(); } };
$('previous').onclick = () => { if (pages.length) { cursor = pages.pop(); $('previous').disabled = true; void loadDetail(); } };
$('settings-toggle').onclick = () => { $('settings').hidden = !$('settings').hidden; $('settings-toggle').setAttribute('aria-expanded', String(!$('settings').hidden)); if ($('settings').hidden) { $('config-details').open = false; text('config',''); } };
$('copy').onclick = () => operate($('copy'), async current => {
  const result = await event({action:'config'});
  const copied = await sodeckClipboard.writeText(unwrap(result).config, () => mounted && visible && current === epoch);
  if (copied && current === epoch) notice('配置已复制');
});
$('config-details').ontoggle = async () => {
  if (!$('config-details').open) { text('config',''); return; }
  const current = epoch;
  try { const result = await event({action:'config'}); if (mounted && visible && current === epoch && $('config-details').open) text('config', unwrap(result).config); }
  catch (error) { if (current === epoch) notice(String(error.message || error)); }
};
$('refresh').onclick = () => { void refresh(); };
$('reveal').onclick = () => operate($('reveal'), () => sodeck.request('data', {op:'pluginData', action:'reveal', relative:['evidence']}));
addEventListener('sodeck:mount', ({detail}) => { epoch++; mounted = true; visible = detail.visible !== false; theme(detail.tokens); if (detail.data) render(detail.data); sodeck.mounted(); void refresh(); });
addEventListener('sodeck:themeChanged', ({detail}) => theme(detail.tokens));
function clearConfig() { text('config',''); $('config-details').open = false; $('settings').hidden = true; $('settings-toggle').setAttribute('aria-expanded','false'); }
addEventListener('sodeck:visibilityChanged', ({detail}) => { epoch++; visible = detail.visible; clearTimeout(timer); if (!visible) clearConfig(); else void refresh(); });
addEventListener('sodeck:unmount', () => { epoch++; mounted = false; clearTimeout(timer); clearConfig(); closeDetail(); });
