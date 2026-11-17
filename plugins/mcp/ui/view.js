const $ = id => document.getElementById(id);
const event = data => sodeck.request('event', data);
let visible = true, mounted = false, pending = false, epoch = 0, timer;
const rows = new Map();
const labels = {opening:'正在启动', running:'执行中', committing:'正在提交', completed:'已结束', cancelled:'已取消', failed:'失败', unavailable:'结果不可用', unknown:'结果未知', rejected:'未接纳', pending:'等待接纳'};
function notice(message) { $('notice').hidden = !message; $('notice').textContent = message; }
function theme(tokens) { for (const [key, value] of Object.entries(tokens || {})) document.documentElement.style.setProperty(key === 'color-scheme' ? key : '--' + key, value); }
function render(data) {
  $('service').textContent = data.service === 'ready' ? '本机接入已启动' : data.service;
  $('endpoint').textContent = data.endpoint;
  const target = data.target?.target;
  $('server').textContent = target ? `${target.name || target.endpoint} · ${target.endpoint} · ${target.username || '账号未提供'} · ${data.target.state === 'connected' ? '已连接' : '已断开'}` : '服务器不可用，请在 SoDeck 中连接。';
  $('directory').textContent = data.evidenceDirectory;
  const tasks = data.tasks || [], current = new Set(tasks.map(t => t.taskId));
  $('empty').hidden = Boolean(tasks.length);
  for (const [id, row] of rows) if (!current.has(id)) { row.li.remove(); rows.delete(id); }
  tasks.forEach((task, index) => {
    let row = rows.get(task.taskId);
    if (!row) {
      const li = document.createElement('li'), body = document.createElement('div');
      const title = document.createElement('strong'), detail = document.createElement('p'), error = document.createElement('p'), cancel = document.createElement('button');
      cancel.type = 'button'; cancel.className = 'sd-control'; cancel.textContent = '取消';
      cancel.onclick = () => operate(cancel, () => event({action:'cancel', taskId:task.taskId}));
      body.append(title, detail, error); li.append(body, cancel);
      row = {li, title, detail, error, cancel}; rows.set(task.taskId, row);
    }
    if ($('tasks').children[index] !== row.li) $('tasks').insertBefore(row.li, $('tasks').children[index] || null);
    row.title.textContent = `${task.kind === 'exec' ? '命令' : '下载'} · ${task.requestId}`;
    const state = task.native?.state || task.native?.admission;
    row.detail.textContent = `${labels[state] || state || '等待接纳'} · ${task.evidence.complete ? '证据已完整保存' : task.done ? '证据不完整' : '正在收集证据'}${task.native?.exitCode != null ? ` · 退出码 ${task.native.exitCode}` : ''}`;
    const error = task.evidence?.error || task.native?.error || task.cleanupError;
    row.error.textContent = error ? error.message || String(error) : ''; row.error.hidden = !error;
    row.cancel.hidden = task.done; row.cancel.setAttribute('aria-label', `取消 ${task.requestId}`);
  });
}
async function refresh() {
  if (!mounted || !visible || pending) return;
  clearTimeout(timer); pending = true; const current = epoch;
  try { const result = await event({action:'snapshot'}); if (current === epoch) render(result.data ?? result); }
  catch (e) { if (current === epoch) notice(String(e.message || e)); }
  finally { pending = false; if (mounted && visible) timer = setTimeout(refresh, current === epoch ? 1500 : 0); }
}
async function operate(button, action) {
  const current = epoch; button.disabled = true;
  try { await action(current); if (current === epoch) await refresh(); }
  catch (e) { if (current === epoch) notice(String(e.message || e)); }
  finally { button.disabled = false; }
}
$('copy').onclick = () => operate($('copy'), async current => {
  const result = await event({action:'config'});
  const copied = await sodeckClipboard.writeText((result.data ?? result).config, () => mounted && visible && current === epoch);
  if (copied && current === epoch) notice('配置已复制');
});
$('config-details').ontoggle = async () => {
  if (!$('config-details').open) { $('config').textContent = ''; return; }
  const current = epoch;
  try { const result = await event({action:'config'}); if (mounted && visible && current === epoch && $('config-details').open) $('config').textContent = (result.data ?? result).config; }
  catch (e) { if (current === epoch) notice(String(e.message || e)); }
};
$('refresh').onclick = () => { void refresh(); };
$('reveal').onclick = () => operate($('reveal'), () => sodeck.request('data', {op:'pluginData', action:'reveal', relative:['evidence']}));
addEventListener('sodeck:mount', ({detail}) => { epoch++; mounted = true; visible = detail.visible !== false; theme(detail.tokens); if (detail.data) render(detail.data); sodeck.mounted(); void refresh(); });
addEventListener('sodeck:themeChanged', ({detail}) => theme(detail.tokens));
addEventListener('sodeck:visibilityChanged', ({detail}) => { epoch++; visible = detail.visible; clearTimeout(timer); if (!visible) { $('config').textContent = ''; $('config-details').open = false; } else void refresh(); });
addEventListener('sodeck:unmount', () => { epoch++; mounted = false; clearTimeout(timer); $('config').textContent = ''; $('config-details').open = false; });
