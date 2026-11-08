import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createServer } from 'node:http';
import { chromium } from 'playwright';
import { path as chartPath } from '../plugins/system-status/ui/charts.js';

const root = path.resolve(process.env.SODECK_TEST_SYSTEM_STATUS_PLUGIN || 'plugins/system-status');
const observed = Date.now();
const base = {
  context: { workspaceId: 'status-workspace', serverId: 'server-1', connectionId: 'connection-1', generation: 1 },
  revision: 1, connected: true, paused: false, family: 'Linux', intervalSeconds: 5, collecting: false, navigation: {},
  data: { identity: { hostname: 'prod-web-01', system: 'Ubuntu 24.04 LTS', cpuModel: 'Intel Xeon Platinum', cores: 8 }, cpu: { value: 23.4 }, memory: { value: 62.1, total: '17179869184', available: '6519760810' }, uptime: { seconds: 1578240 }, load: ['0.42', '0.38', '0.31'] },
  network: { device: { id: 'eth0', name: 'eth0' }, rates: { first: 131481.6, second: 43827.2 } },
  disk: { device: { id: '8:0', name: 'nvme0n1' }, rates: { first: 1258291, second: 393216 } },
  groups: ['identity', 'dynamic', 'topology', 'capacity', 'uptime'].map(name => ({ name, observedAt: observed - (name === 'capacity' ? 32000 : 0), staleAfterMs: name === 'dynamic' ? 10000 : 120000 }))
};
const normalFiles = [
  { id: 'root', path: '/', total: '85899345920', used: '36721970380', available: '49177375540', percent: 42.8, observedAt: observed - 32000 },
  { id: 'data', path: '/data', total: '536870912000', used: '307090161664', available: '229780750336', percent: 57.2, observedAt: observed - 32000 }
];
let snapshot = structuredClone(base), files = structuredClone(normalFiles), requests = 0, actions = [], seriesKeys = [], historyStep = 5000, allGaps = false, failRefresh = false, failSnapshots = false;
const windowEnd = 300000;
assert.equal(chartPath([{ at: windowEnd, value: 20 }], 100, 15000, windowEnd + 120000), 'M144.0 37.2');
assert.equal(chartPath([{ at: 1000, value: 20 }], 100, 15000, windowEnd + 120000), '');
assert.equal((chartPath([{ at: 280000, value: 20 }, { at: 285000, value: null }, { at: 290000, value: 30 }, { at: 300000, value: 40 }], 100, 15000, windowEnd).match(/M/g) || []).length, 2);

const themesCss = fs.readFileSync('src/themes.css', 'utf8');
const parseTokens = source => Object.fromEntries([...source.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(m => [m[1], m[2].trim()]));
const light = parseTokens(themesCss.match(/:root \{([\s\S]*?)\n\s*\}/)[1]);
const themeTokens = theme => ({ ...light, ...(theme === 'light' ? {} : parseTokens(themesCss.match(new RegExp(':root\\[data-theme="' + theme + '"\\] \\{([\\s\\S]*?)\\n\\s*\\}'))[1])), 'color-scheme': theme === 'dark' ? 'dark' : 'light' });
const server = createServer((req, res) => {
  if (req.url === '/') {
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><html><body style="margin:0"><iframe title="状态测试" style="border:0;width:100vw;height:100vh" src="/ui/index.html"></iframe></body></html>');
    return;
  }
  const file = path.join(root, req.url.split('?')[0]);
  if (!file.startsWith(root + path.sep) || !fs.existsSync(file)) { res.statusCode = 404; res.end(); return; }
  res.setHeader('Content-Type', { '.js': 'text/javascript', '.html': 'text/html', '.css': 'text/css' }[path.extname(file)] || 'application/octet-stream');
  res.end(fs.readFileSync(file));
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 320, height: 760 }, reducedMotion: 'reduce' });
const errors = [];
page.on('pageerror', e => errors.push(e.message));
await page.exposeFunction('statusRequest', params => {
  requests++; actions.push(params.action);
  if (params.action === 'pause') { snapshot.paused = true; snapshot.revision++; }
  if (params.action === 'resume') { snapshot.paused = false; snapshot.revision++; }
  if (params.action === 'refresh') { if (failRefresh) throw new Error('刷新失败：远端查询超时'); snapshot.revision++; }
  if (params.action === 'navigation') {
    Object.assign(snapshot.navigation, params.state);
    if (params.state.interfaceId) { snapshot.network.device = { id: params.state.interfaceId, name: params.state.interfaceId }; snapshot.revision++; }
    if (params.state.diskId) { snapshot.disk.device = { id: params.state.diskId, name: params.state.diskId }; snapshot.revision++; }
    return { accepted: true };
  }
  if (params.action === 'list') {
    const rows = params.kind === 'interfaces' ? [{ id: 'eth0', name: 'eth0' }, { id: 'eth1', name: 'eth1' }] : params.kind === 'disks' ? [{ id: '8:0', name: 'nvme0n1' }, { id: '8:16', name: 'sda' }] : files;
    const offset = params.offset || 0, items = rows.slice(offset, offset + 32);
    return { items, total: rows.length, offset, nextOffset: offset + items.length < rows.length ? offset + items.length : null };
  }
  if (params.action === 'history') {
    seriesKeys.push(params.seriesKey);
    const at = snapshot.connected ? Date.now() : snapshot.groups.find(g => g.name === 'dynamic').observedAt;
    if (allGaps) return { points: [{ at: at - 5000, value: null }, { at, value: null }] };
    if (historyStep !== 5000) return { points: [{ at: at - historyStep * 3, value: 20 }, { at: at - historyStep * 2, value: null }, { at: at - historyStep, value: 30 }, { at, value: 23.4 }] };
    if (snapshot.data.cpu.state === 'warmingUp') return { points: [] };
    if (params.seriesKey === 'memory' && snapshot.data.memory.state === 'unsupported') return { points: [] };
    const network = params.seriesKey.startsWith('net:'), send = params.seriesKey.endsWith(':second');
    const n = params.seriesKey === 'memory' ? 62.1 : network ? send ? 43827.2 : 131481.6 : 23.4;
    return { points: Array.from({ length: 61 }, (_, i) => ({
      at: at - (60 - i) * 5000,
      value: i === 24 ? null : i === 60 ? n : Math.max(0, n + Math.sin(i * .72) * (network ? n * .2 : params.seriesKey === 'memory' ? 1 : 5) + (i > 35 && i < 43 ? Math.sin((i - 35) / 8 * Math.PI) * (network ? n * 1.5 : params.seriesKey === 'memory' ? 1 : 40) : 0))
    })) };
  }
  if (failSnapshots) throw new Error('首次读取失败：连接服务暂不可用');
  return params.knownRevision === snapshot.revision ? { revision: snapshot.revision, unchanged: true } : structuredClone(snapshot);
});
await page.addInitScript(() => {
  if (window !== top) return;
  addEventListener('message', async e => {
    const m = e.data;
    if (m?.channel !== 'sodeck-ui-v1') return;
    if (m.type === 'ready') e.source.postMessage({ channel: m.channel, type: 'mount', surfaceId: 'test', data: window.statusInitial, tokens: {}, visible: true }, '*');
    if (m.type === 'request') {
      try { const data = await window.statusRequest(m.params); e.source.postMessage({ channel: m.channel, type: 'response', surfaceId: 'test', id: m.id, data }, '*'); }
      catch (error) { e.source.postMessage({ channel: m.channel, type: 'response', surfaceId: 'test', id: m.id, error: error.message }, '*'); }
    }
  });
});
async function applyTheme(theme) {
  await page.evaluate(tokens => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'themeChanged', surfaceId: 'test', tokens }, '*'), themeTokens(theme));
}
const frame = page.frameLocator('iframe');
async function refresh() { await frame.getByRole('button', { name: '刷新', exact: true }).click(); await page.waitForFunction(() => !document.querySelector('iframe').contentDocument.querySelector('#refresh').disabled); }
try {
  await page.goto('http://127.0.0.1:' + server.address().port);
  await frame.locator('#cpu').filter({ hasText: '23.4%' }).waitFor();
  assert.match(await frame.locator('#memory-caption').innerText(), /已用 9.9 GiB \/ 16 GiB/);
  await page.waitForFunction(() => document.querySelector('iframe').contentDocument.querySelectorAll('#network-chart .series').length === 2);
  const receivePath = await frame.locator('#network-chart .series.first').getAttribute('d');
  const sendPath = await frame.locator('#network-chart .series.second').getAttribute('d');
  assert.notEqual(receivePath, sendPath);
  assert(seriesKeys.includes('net:eth0:second'));
  await frame.locator('#device summary').click();
  await frame.locator('#device summary').focus();
  await new Promise(resolve => setTimeout(resolve, 1100));
  assert(await frame.locator('#device summary').evaluate(e => e === document.activeElement));
  await frame.locator('#device summary').click();
  await frame.locator('#cpu-chart').focus();
  await page.keyboard.press('Home');
  const firstReadout = await frame.locator('#cpu-chart').evaluate(e => e.parentElement.querySelector('.chart-tooltip').textContent);
  await page.keyboard.press('ArrowRight');
  const secondReadout = await frame.locator('#cpu-chart').evaluate(e => e.parentElement.querySelector('.chart-tooltip').textContent);
  assert.notEqual(firstReadout, secondReadout);
  await page.keyboard.press('Escape');
  assert.equal(await frame.locator('#cpu-chart').evaluate(e => e.parentElement.querySelector('.chart-tooltip').hidden), true);
  await frame.getByRole('button', { name: '暂停', exact: true }).click();
  await frame.getByRole('button', { name: '恢复', exact: true }).waitFor();
  assert.match(await frame.locator('#notice').innerText(), /暂停前数据/);
  await refresh();
  assert.equal(snapshot.paused, true, 'Refresh must retain pause');
  await frame.getByRole('button', { name: '恢复', exact: true }).click();
  await frame.getByRole('button', { name: '暂停', exact: true }).waitFor();

  fs.mkdirSync('artifacts/system-status-ui', { recursive: true });
  for (const theme of ['light', 'dark', 'dopamine']) {
    await applyTheme(theme);
    for (const width of [160, 280, 320, 400, 480]) {
      await page.setViewportSize({ width, height: 760 });
      await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
      assert(await frame.locator('body').evaluate(e => e.scrollWidth <= e.clientWidth), theme + ' horizontal overflow at ' + width);
      await page.screenshot({ path: 'artifacts/system-status-ui/' + theme + '-' + width + '.png' });
    }
  }
  await page.setViewportSize({ width: 320, height: 760 });
  await applyTheme('light');
  snapshot.data.identity.hostname = '生产服务器 / 很长的中文名称设备';
  files = [{ id: 'large', path: '/很长的挂载目录 / volume with spaces', total: '9007199254740993', used: '4000000000000000', available: '5007199254740993', percent: 44.4, observedAt: Date.now() }];
  snapshot.revision++; await refresh();
  assert.match(await frame.locator('.filesystem-stats').innerText(), /3.6 PiB \/ 8 PiB/);
  assert(await frame.locator('body').evaluate(e => e.scrollWidth <= e.clientWidth));

  snapshot = structuredClone(base); snapshot.revision += 100; files = structuredClone(normalFiles); Object.assign(files[1], { percent: 91.8, used: '492847497216', available: '44023414784' });
  await refresh(); assert.equal(await frame.locator('.filesystem.warning').count(), 1);
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/capacity-light.png' });
  failRefresh = true;
  await refresh(); await frame.locator('#notice').filter({ hasText: '远端查询超时' }).waitFor();
  const beforeAge = snapshot.groups.find(g => g.name === 'dynamic').observedAt;
  await frame.locator('#interface').selectOption('eth1');
  await frame.locator('#notice').filter({ hasText: '远端查询超时' }).waitFor();
  await frame.locator('#disks summary').click(); await frame.locator('#disk').selectOption('8:16');
  await frame.locator('#notice').filter({ hasText: '远端查询超时' }).waitFor();
  assert.equal(snapshot.groups.find(g => g.name === 'dynamic').observedAt, beforeAge);
  await page.setViewportSize({ width: 320, height: 980 });
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/refresh-failed-light.png' });
  await page.setViewportSize({ width: 320, height: 760 });
  await frame.locator('#disks summary').click();
  failRefresh = false; await refresh(); assert.equal(await frame.locator('#notice').isVisible(), false);

  files = Array.from({ length: 34 }, (_, i) => ({ ...normalFiles[0], id: 'fs-' + i, path: '/mnt/' + i }));
  snapshot.revision++; await refresh();
  await frame.locator('#next').click();
  await frame.locator('#page-label').filter({ hasText: '33–34 / 34' }).waitFor();
  await frame.locator('#previous').click(); await frame.locator('#page-label').filter({ hasText: '1–32 / 34' }).waitFor();
  files = structuredClone(normalFiles); snapshot.revision++; await refresh();

  snapshot.intervalSeconds = 30; historyStep = 30050; snapshot.revision++; await refresh();
  await frame.locator('#collection-summary').filter({ hasText: '30 秒 / 次' }).waitFor();
  await page.waitForFunction(() => { const d = document.querySelector('iframe').contentDocument.querySelector('#cpu-chart .series')?.getAttribute('d') || ''; return (d.match(/M/g) || []).length >= 2 && d.includes('L'); });
  const before = requests;
  await page.evaluate(() => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'visibilityChanged', surfaceId: 'test', visible: false }, '*'));
  await new Promise(resolve => setTimeout(resolve, 1200));
  assert(requests <= before + 1, 'Hidden UI kept polling');
  snapshot.connected = false; snapshot.groups.forEach(g => g.observedAt = Date.now() - 120000 - (g.name === 'capacity' ? 32000 : 0)); snapshot.revision++;
  files.forEach(f => f.observedAt = snapshot.groups.find(g => g.name === 'capacity').observedAt);
  await page.evaluate(() => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'visibilityChanged', surfaceId: 'test', visible: true }, '*'));
  await frame.locator('#notice').filter({ hasText: '连接已断开' }).waitFor();
  assert.equal(await frame.locator('#refresh').isDisabled(), true);
  await frame.locator('#collection > summary').click();
  assert.match(await frame.locator('#group-details').innerText(), /容量 · 2 分/);
  await page.setViewportSize({ width: 320, height: 1100 });
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/disconnected-light.png' });
  await page.setViewportSize({ width: 320, height: 760 });
  await frame.locator('#collection > summary').click();

  historyStep = 5000; snapshot.groups.forEach(g => g.observedAt = Date.now() - 360000); snapshot.revision++;
  await page.evaluate(() => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'visibilityChanged', surfaceId: 'test', visible: true }, '*'));
  await frame.locator('#network-peak').filter({ hasText: '等待有效采样' }).waitFor();
  assert.equal(await frame.locator('#network-chart .series.first').getAttribute('d'), '');
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/expired-history-light.png' });
  snapshot.connected = true; snapshot.groups.forEach(g => g.observedAt = Date.now()); snapshot.revision++; allGaps = true;
  await page.evaluate(() => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'visibilityChanged', surfaceId: 'test', visible: true }, '*'));
  await frame.locator('#refresh:enabled').waitFor();
  await refresh();
  assert.equal(await frame.locator('#network-peak').innerText(), '等待有效采样');
  assert.equal(await frame.locator('#network-chart .series.first').getAttribute('d'), '');
  allGaps = false;

  const newContext = { ...base.context, connectionId: 'connection-2', generation: 2 };
  failSnapshots = true;
  await page.evaluate(context => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'contextChanged', surfaceId: 'test', context }, '*'), newContext);
  await frame.locator('#notice').filter({ hasText: '首次读取失败' }).waitFor();
  assert.equal(await frame.locator('#cpu').innerText(), '采样中');
  assert.equal(await frame.locator('#host-name').innerText(), '服务器');
  assert.equal(await frame.locator('.filesystem').count(), 0);
  assert.equal(await frame.locator('#pause').isDisabled(), true);
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/context-failed-light.png' });
  failSnapshots = false;
  await new Promise(resolve => setTimeout(resolve, 1200));
  assert.equal(await frame.locator('#host-name').innerText(), '服务器', 'Old-context response restored the previous host');
  snapshot.context = newContext; snapshot.groups.forEach(g => g.observedAt = null); snapshot.revision++;
  await frame.locator('#connection').filter({ hasText: '已连接' }).waitFor();
  assert.equal(await frame.locator('#cpu').innerText(), '采样中');
  assert.equal(await frame.locator('#memory').innerText(), '采样中');
  assert.equal(await frame.locator('#receive').innerText(), '采样中');
  assert.equal(await frame.locator('#host-name').innerText(), '服务器');
  assert.equal(await frame.locator('#uptime').textContent(), '正在读取');
  assert.equal(await frame.locator('.filesystem').count(), 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/context-warming-light.png' });
  snapshot.data.identity.hostname = 'new-connection-host'; snapshot.groups.find(g => g.name === 'identity').observedAt = Date.now(); snapshot.revision++;
  await frame.locator('#host-name').filter({ hasText: 'new-connection-host' }).waitFor();
  assert.equal(await frame.locator('#cpu').innerText(), '采样中');
  snapshot.groups.forEach(g => g.observedAt = Date.now()); snapshot.revision++;
  await frame.locator('#cpu').filter({ hasText: '23.4%' }).waitFor();
  await frame.locator('.filesystem').first().waitFor();

  snapshot = structuredClone(base); snapshot.revision += 200; snapshot.data.memory = { value: null, state: 'unsupported' }; snapshot.network = { device: null, rates: null };
  await page.evaluate(() => document.querySelector('iframe').contentWindow.postMessage({ channel: 'sodeck-ui-v1', type: 'contextChanged', surfaceId: 'test' }, '*'));
  await frame.locator('#memory').filter({ hasText: '暂不可用' }).waitFor();
  await frame.locator('#receive').filter({ hasText: '暂不可用' }).waitFor();
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/partial-light.png' });
  snapshot.data.cpu = { value: null, state: 'warmingUp' }; snapshot.revision++; await refresh();
  await frame.locator('#cpu').filter({ hasText: '采样中' }).waitFor();
  await frame.locator('#scroll').evaluate(e => e.scrollTop = 0);
  await page.screenshot({ path: 'artifacts/system-status-ui/warming-light.png' });
  snapshot = structuredClone(base); snapshot.revision += 400; failSnapshots = true;
  await page.reload();
  await frame.locator('#notice').filter({ hasText: '首次读取失败' }).waitFor();
  failSnapshots = false;
  await frame.locator('#cpu').filter({ hasText: '23.4%' }).waitFor();
  assert.equal(await frame.locator('#notice').isVisible(), false);
  assert(actions.includes('refresh') && actions.includes('pause') && actions.includes('resume'));
  assert.deepEqual(errors, []);
  console.log('Status UI: real three-theme tokens, five widths, two-way history and keyboard, pause/refresh, failure retention, pagination, large/long paths, 30-second gaps, hidden polling, disconnect and unavailable metrics passed.');
} finally {
  await browser.close();
  await new Promise(resolve => server.close(resolve));
}
