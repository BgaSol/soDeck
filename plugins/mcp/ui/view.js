const $ = id => document.getElementById(id);
const event = data => sodeck.request('event', data);
const unwrap = result => result.data ?? result;
let visible = true, mounted = false, pending = false, epoch = 0, timer;
let returnPosition, settingsSnapshot, portDirty = false, portConflict = false, settingsSaving = false, settingsTicket;
let snapshot, active, monitorDetail, selected = null, detail, cursor = '0', nextCursor = '0', pages = [], monitorSequence = 0, detailSequence = 0;
const rows = new Map();
const labels = {opening:'正在启动', running:'执行中', committing:'正在提交', finalizing:'正在收尾', completed:'已结束', cancelled:'已取消', failed:'失败', unavailable:'结果不可用', unknown:'结果未知', rejected:'未接纳', pending:'等待接纳'};
function text(id, value) { const el = $(id); if (el.textContent !== value) el.textContent = value; }
function notice(message) { $('notice').hidden = !message; text('notice', message); }
function theme(tokens) { for (const [key, value] of Object.entries(tokens || {})) document.documentElement.style.setProperty(key === 'color-scheme' ? key : '--' + key, value); }
function command(task) { return task.input?.command ?? task.input?.remotePath ?? (task.inputPreview != null ? task.inputPreview + (task.inputTruncated ? '…' : '') : task.requestId); }
function state(task) { return task.phase === 'finalizing' ? 'finalizing' : task.native?.state || task.native?.admission || 'pending'; }
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
function closeDetail(restore = false) {
  detailSequence++; selected = null; detail = null; cursor = '0'; pages = []; nextCursor = '0'; $('detail').hidden = true; $('overview').hidden = false;
  for (const row of rows.values()) row.button.setAttribute('aria-current', 'false');
  const pane = document.querySelector('main');
  pane.scrollTop = restore ? returnPosition?.scrollTop || 0 : 0;
  if (restore && returnPosition?.button.isConnected && returnPosition.button.getClientRects().length) {
    pane.scrollTop += returnPosition.button.getBoundingClientRect().top - returnPosition.top;
    returnPosition.button.focus({preventScroll:true});
  } else if (restore) {
    $('refresh').focus({preventScroll:true});
  }
  returnPosition = null;
}
function paintDetail(task) {
  $('detail').hidden = false;
  $('overview').hidden = true;
  text('detail-state', status(task)); $('detail-state').className = stateClass(task);
  text('detail-input-label', task.kind === 'exec' ? '实际命令' : '远程文件'); text('detail-command', task.input ? command(task) : '正在读取完整详情…');
  const target = task.target;
  text('detail-server', target ? `${target.name || target.endpoint} · ${target.endpoint} · ${target.username || '账号未提供'}` : '服务器信息不可用');
  text('detail-created', `提交于 ${time(task.createdAt)}`); text('detail-time', duration(task));
  text('detail-request', task.requestId); text('detail-id', task.taskId);
  text('detail-evidence', task.kind === 'download' && task.evidence?.complete ? '文件已保存' : evidence(task));
  text('detail-path', task.evidence?.path || task.directory || '尚未提供');
  const errors = [task.evidence?.error, task.native?.error, task.cleanupError].filter(Boolean);
  text('detail-error', errors.map(error => error.message || String(error)).join('\n')); $('detail-error').hidden = !errors.length;
  $('detail-output-section').hidden = task.kind !== 'exec'; $('output-pager').hidden = task.kind !== 'exec'; $('detail-cancel').hidden = task.done || task.phase === 'finalizing';
  $('detail-cancel').setAttribute('aria-label', `请求取消 ${task.requestId}`);
}
function render(data) {
  if (typeof data?.runId !== 'string' || !data.runId || typeof data.targetId !== 'string' || !data.targetId || !data.target) throw new Error('目标状态快照无效，请刷新。');
  const pane = document.querySelector('main'), pinned = selectedTask(), focus = document.activeElement, anchor = selected ? $('detail') : null;
  const scrollTop = pane.scrollTop, anchorTop = anchor?.getBoundingClientRect().top;
  const changedTarget = snapshot && (data.runId !== snapshot.runId || data.targetId !== snapshot.targetId);
  if (changedTarget) { closeDetail(); monitorSequence++; monitorDetail = null; }
  const tasks = [...(data.tasks || [])];
  if (selected && !tasks.some(task => task.taskId === selected)) {
    if (pinned && !changedTarget && data.targetId === pinned.targetId) tasks.push(pinned);
    else closeDetail();
  }
  snapshot = {...data, tasks};
  text('service', data.service === 'ready' ? '本机接入已启动' : data.service); text('endpoint', data.endpoint);
  paintPort();
  const target = data.target.descriptor, targetError = data.target.error;
  const availability = {ready:'已连接',disconnected:'已断开',closing:'正在关闭',stale:'连接信息已失效',unavailable:'目标信息暂不可用'}[data.target.availability];
  text('server', target ? target.name || target.endpoint : targetError ? '目标信息暂不可用' : '服务器未连接');
  text('server-detail', targetError ? `目标信息读取失败（${targetError.code ?? '未知错误'}）：${targetError.message || '请稍后刷新。'}` : target ? `${target.username || '账号未提供'}@${target.endpoint} · ${availability}${data.target.targetSource === 'lastConnected' ? ' · 最后连接信息' : ''}` : data.target.availability === 'disconnected' ? '请在 SoDeck 中连接服务器。' : availability);
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
      button.onclick = () => selectTask(task.taskId, button); heading.append(label, date); button.append(heading, input, meta); li.append(button);
      row = {li, button, label, date, input, meta}; rows.set(task.taskId, row);
    }
    if ($('tasks').children[index] !== row.li) $('tasks').insertBefore(row.li, $('tasks').children[index] || null);
    row.label.textContent = `${task.kind === 'exec' ? '命令' : '下载'} · ${status(task)}`; row.label.className = stateClass(task);
    row.date.textContent = time(task.createdAt); if (task.createdAt) row.date.dateTime = new Date(task.createdAt).toISOString();
    if (row.input.textContent !== command(task)) row.input.textContent = command(task);
    row.meta.textContent = `${task.target?.name || task.target?.endpoint || '服务器信息待确认'} · ${duration(task)}`;
    const preview = Array.from(command(task).replace(/\s+/g, ' ').trim());
    row.button.setAttribute('aria-label', `查看${task.kind === 'exec' ? '命令' : '下载'}详情 ${task.requestId} · ${task.target?.name || task.target?.endpoint || '服务器信息待确认'} · ${status(task)} · ${preview.slice(0,160).join('')}${preview.length > 160 ? '…' : ''}`); row.button.setAttribute('aria-current', String(selected === task.taskId));
  });
  const running = tasks.filter(task => !task.done), previous = active?.taskId;
  active = running[0]; text('active-count', running.length ? `${running.length} 个操作` : '');
  const total = Math.max(data.recordCount ?? tasks.length, tasks.length);
  text('task-count', `${total > tasks.length ? `显示 ${tasks.length} / ${total} 条 · 更多记录请使用 MCP 或证据目录` : `${tasks.length} 条`}${running.length ? ` · ${running.length} 条执行中` : ''}`);
  $('idle').hidden = Boolean(active); $('monitor-body').hidden = !active;
  if (active) {
    text('monitor-state', status(active)); $('monitor-state').className = stateClass(active);
    text('monitor-time', duration(active)); text('monitor-command', command(monitorDetail?.taskId === active.taskId ? monitorDetail : active));
    $('monitor-cancel').setAttribute('aria-label', `请求取消 ${active.requestId}`);
    $('monitor-cancel').hidden = active.phase === 'finalizing';
    text('monitor-progress', active.kind === 'download' ? '下载远程文件' : `提交于 ${time(active.createdAt)}`);
    if (previous !== active.taskId) { monitorSequence++; text('monitor-output', '等待读取输出'); }
  } else { monitorSequence++; text('monitor-output', ''); }
  if (selectedTask()) {
    const row = selectedTask();
    paintDetail(detail?.taskId === row.taskId ? {...row, input:detail.input, directory:detail.directory, evidence:{...row.evidence,path:detail.evidence?.path}} : row);
  }
  if (focus?.isConnected && pane.contains(focus) && focus.getClientRects().length && document.activeElement !== focus) focus.focus({preventScroll:true});
  pane.scrollTop = changedTarget ? 0 : anchor?.isConnected && !anchor.hidden ? pane.scrollTop + anchor.getBoundingClientRect().top - anchorTop : scrollTop;
}
async function loadMonitor() {
  if (selected || !active || !mounted || !visible) return;
  const id = active.taskId, sequence = ++monitorSequence, current = epoch;
  try {
    const task = unwrap(await event({action:'task', taskId:id, tail:true}));
    if (current !== epoch || sequence !== monitorSequence || active?.taskId !== id) return;
    monitorDetail = task; text('monitor-command', command(task));
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
    detail = result; paintDetail(result); text('detail-output', output(result.output));
    nextCursor = result.output?.nextCursor || start;
    $('previous').disabled = !pages.length; $('next').disabled = !result.output?.hasMore;
    text('page-number', `第 ${pages.length + 1} 页`);
  } catch (error) {
    if (current === epoch && sequence === detailSequence) { text('detail-output', `输出读取失败：${error.message || error}`); $('next').disabled = true; }
  }
}
function selectTask(id, button) {
  if (selected === id) return;
  const pane = document.querySelector('main');
  if (!selected) returnPosition = {scrollTop:pane.scrollTop, button, top:button.getBoundingClientRect().top};
  $('settings').open = false; clearConfig(); monitorSequence++;
  selected = id; detail = null; cursor = '0'; pages = []; nextCursor = '0'; detailSequence++;
  $('previous').disabled = true; $('next').disabled = true; text('page-number','第 1 页'); text('detail-output','等待读取输出');
  render(snapshot); pane.scrollTop = 0; $('detail-close').focus({preventScroll:true}); void loadDetail();
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
function cancel(button, task) { if (task) return operate(button, async current => { const result = unwrap(await event({action:'cancel', taskId:task.taskId})); if (current === epoch) notice(result.cancelRequested ? '已请求取消，等待远端实际结果。' : '操作已进入收尾或结束，请查看实际结果。'); }); }
$('monitor-cancel').onclick = () => cancel($('monitor-cancel'), active);
$('detail-cancel').onclick = () => cancel($('detail-cancel'), selectedTask());
$('monitor-details').onclick = () => { if (active) selectTask(active.taskId, $('monitor-details')); };
$('detail-close').onclick = () => { closeDetail(true); void loadMonitor(); };
$('next').onclick = () => { if (nextCursor !== cursor) { pages.push(cursor); cursor = nextCursor; $('next').disabled = true; void loadDetail(); } };
$('previous').onclick = () => { if (pages.length) { cursor = pages.pop(); $('previous').disabled = true; void loadDetail(); } };
$('copy').onclick = () => operate($('copy'), async current => {
  const result = await event({action:'config'});
  const copied = await sodeckClipboard.writeText(unwrap(result).config, () => mounted && visible && current === epoch);
  if (copied && current === epoch) notice('配置已复制');
});
$('config-details').ontoggle = async () => {
  $('config').hidden = !$('config-details').open;
  if (!$('config-details').open) { text('config',''); return; }
  const current = epoch;
  try { const result = await event({action:'config'}); if (mounted && visible && current === epoch && $('config-details').open) text('config', unwrap(result).config); }
  catch (error) { if (current === epoch) notice(String(error.message || error)); }
};
$('show-token').onclick = async () => {
  const show = $('show-token').getAttribute('aria-expanded') !== 'true';
  $('show-token').setAttribute('aria-expanded',String(show)); text('show-token', show ? '隐藏' : '显示');
  if (!show) { text('access-token','••••••••••••••••'); return; }
  const current = epoch;
  try { const result = unwrap(await event({action:'config'})); if (mounted && visible && current === epoch && $('show-token').getAttribute('aria-expanded') === 'true') text('access-token', result.token); }
  catch (error) { if (current === epoch) { text('access-token','读取失败，请重试'); notice(String(error.message || error)); } }
};
function settings(snapshot) {
  if (!snapshot || (settingsSnapshot?.definitionToken === snapshot.definitionToken && BigInt(settingsSnapshot.revision) > BigInt(snapshot.revision))) return;
  if (portDirty && settingsSnapshot && (settingsSnapshot.revision !== snapshot.revision || settingsSnapshot.definitionToken !== snapshot.definitionToken)) portConflict = true;
  settingsSnapshot = snapshot;
  if (!portDirty) $('port').value = snapshot.values.port;
  paintPort();
}
function paintPort() {
  $('port').disabled = !settingsSnapshot || settingsSaving;
  $('save-port').disabled = !settingsSnapshot || !portDirty || portConflict || settingsSaving;
  $('reload-port').disabled = settingsSaving;
  $('reload-port').hidden = !portConflict;
  const runtimePort = snapshot?.endpoint ? new URL(snapshot.endpoint).port : '';
  const message = portConflict ? '其他窗口已更新端口，草稿已保留。请载入最新值后再编辑。' : settingsSnapshot && String(settingsSnapshot.values.port) !== runtimePort ? '端口已保存，重启 SoDeck 后生效；之后需重新复制客户端配置。' : '';
  $('port-notice').hidden = !message; text('port-notice', message);
}
async function readSettings() {
  if (!mounted || !visible) return;
  const current = epoch;
  try { const result = await sodeck.request('settings',{op:'get'}); if (current === epoch) settings(result); }
  catch (error) { if (current === epoch) { $('port-notice').hidden = false; text('port-notice', `端口设置读取失败：${error.message || error}`); } }
}
$('port').oninput = () => { portDirty = Boolean(settingsSnapshot && String(settingsSnapshot.values.port) !== $('port').value); settingsTicket = null; paintPort(); };
$('reload-port').onclick = () => { portDirty = false; portConflict = false; settingsTicket = null; if (settingsSnapshot) settings(settingsSnapshot); };
$('save-port').onclick = () => {
  if (!settingsSnapshot || settingsSaving || !$('port').reportValidity() || portConflict || !portDirty) return;
  settingsTicket ??= {requestId:crypto.randomUUID(),definitionToken:settingsSnapshot.definitionToken,baseRevision:settingsSnapshot.revision,values:{...settingsSnapshot.values,port:Number($('port').value)}};
  settingsSaving = true; paintPort();
  void operate($('save-port'), async current => {
    const result = await sodeck.request('settings',{op:'save',...settingsTicket});
    if (current !== epoch) return;
    if (result.conflict) portConflict = true;
    else if (result.persisted) { portDirty = false; settingsTicket = null; notice('端口已保存，重启 SoDeck 后生效。'); }
    else throw new Error('端口未保存，请重试。');
    settings(result.snapshot);
  }).finally(() => { settingsSaving = false; paintPort(); });
};
$('refresh').onclick = () => { void refresh(); void readSettings(); };
$('reveal').onclick = () => operate($('reveal'), () => sodeck.request('data', {op:'pluginData', action:'reveal', relative:['evidence']}));
addEventListener('sodeck:mount', ({detail}) => { epoch++; mounted = true; visible = detail.visible !== false; theme(detail.tokens); if (detail.data) render(detail.data); settings(detail.settings); sodeck.mounted(); void refresh(); void readSettings(); });
addEventListener('sodeck:themeChanged', ({detail}) => theme(detail.tokens));
addEventListener('sodeck:settingsChanged', ({detail}) => settings(detail.settings));
function clearConfig() { text('config',''); $('config').hidden = true; $('config-details').open = false; text('access-token','••••••••••••••••'); text('show-token','显示'); $('show-token').setAttribute('aria-expanded','false'); }
$('settings').ontoggle = () => { if (!$('settings').open) clearConfig(); };
addEventListener('sodeck:visibilityChanged', ({detail}) => { epoch++; visible = detail.visible; clearTimeout(timer); if (!visible) { $('settings').open = false; clearConfig(); } else { void refresh(); void readSettings(); } });
addEventListener('sodeck:unmount', () => { epoch++; mounted = false; clearTimeout(timer); clearConfig(); closeDetail(); });
