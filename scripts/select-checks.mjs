import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const targets = [
  { name: 'windows-x86_64', runner: 'windows-2025', target: 'x86_64-pc-windows-msvc' },
  { name: 'windows-aarch64', runner: 'windows-11-arm', target: 'aarch64-pc-windows-msvc' },
  { name: 'linux-x86_64', runner: 'ubuntu-22.04', target: 'x86_64-unknown-linux-gnu' },
  { name: 'linux-aarch64', runner: 'ubuntu-22.04-arm', target: 'aarch64-unknown-linux-gnu' },
  { name: 'macos-aarch64', runner: 'macos-15', target: 'aarch64-apple-darwin' },
];
const pluginNames = ['terminal', 'files', 'network-proxy'];
const git = (cwd, args) => execFileSync('git', args, { cwd, encoding: 'utf8', windowsHide: true,
  stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 16 * 1024 * 1024 });

export function changedFiles(eventName, event, cwd = process.cwd()) {
  try {
    let base, head;
    if (eventName === 'pull_request') {
      base = event.pull_request.base.sha;
      head = event.pull_request.head.sha;
    } else if (eventName === 'push') {
      base = event.before;
      head = event.after;
    } else {
      throw new Error('unsupported event');
    }
    for (const sha of [base, head]) {
      if (!/^[a-f0-9]{40}$/.test(sha) || /^0+$/.test(sha)) throw new Error('missing commit');
      git(cwd, ['cat-file', '-e', `${sha}^{commit}`]);
    }
    if (eventName === 'pull_request') base = git(cwd, ['merge-base', base, head]).trim();
    // Both sides of renames must count; -z preserves spaces, Unicode and newlines.
    const files = git(cwd, ['diff', '--name-only', '--no-renames', '-z', base, head, '--']).split('\0').filter(Boolean);
    return { files, fallback: files.length ? null : 'No changed files; use conservative coverage' };
  } catch {
    return { files: [], fallback: 'Cannot determine the complete diff; use conservative coverage' };
  }
}

export function selectChecks(files, { suite, fallback } = {}) {
  const plan = { web: false, scripts: false, terminalInput: false, desktop: false, ssh: false,
    sharedClient: false, netem: false, plugins: [], native: [], integration: [], reasons: [] };
  const coreTargets = new Set(), scriptTargets = new Set(), plugins = new Set(), integration = new Set();
  const pluginTargets = Object.fromEntries(pluginNames.map(name => [name, new Set()]));
  const allTargets = targets.map(t => t.name);
  const core = (names = allTargets) => names.forEach(name => coreTargets.add(name));
  const plugin = name => { plugins.add(name); allTargets.forEach(t => pluginTargets[name].add(t)); };
  const ui = () => { plan.web = plan.scripts = plan.desktop = true; };
  const sharedUi = () => { ui(); plan.terminalInput = true; pluginNames.forEach(name => integration.add(name)); };
  const full = () => {
    sharedUi(); core(); plan.ssh = plan.sharedClient = true;
    pluginNames.forEach(plugin);
    ['windows-x86_64', 'linux-x86_64', 'macos-aarch64'].forEach(t => scriptTargets.add(t));
  };
  const standard = () => {
    sharedUi(); core(['linux-x86_64']); plan.sharedClient = true;
    for (const name of pluginNames) { plugins.add(name); pluginTargets[name].add('linux-x86_64'); }
  };

  if (suite) {
    plan.reasons.push(`Manual suite: ${suite}`);
    if (suite === 'all') full();
    else if (suite === 'standard') {
      standard();
      // standard preserves the previous common checks; full Windows UI stays in desktop/all.
      plan.desktop = false;
      integration.delete('terminal'); integration.delete('files');
    } else if (suite === 'desktop') plan.desktop = plan.ssh = true;
    else if (suite === 'native-core') {
      core(); ['windows-x86_64', 'linux-x86_64', 'macos-aarch64'].forEach(t => scriptTargets.add(t));
    } else if (suite === 'network-plugin') plugin('network-proxy');
    else if (suite === 'netem') plan.netem = true;
    else throw new Error(`Unknown check suite: ${suite}`);
  } else if (fallback || !files.length) {
    plan.reasons.push(fallback || 'No changed files; use conservative coverage'); full();
  } else {
    for (const file of files) {
      if (file.endsWith('.md') || file.startsWith('docs/')) continue;
      if (file.startsWith('src/')) {
        if (/^src\/(ui\.css|themes\.css|text-menu\.|menu-position\.|font-loader\.)/.test(file)) sharedUi();
        else ui();
        if (file === 'src/clipboard.ts') { sharedUi(); core(); }
        continue;
      }
      if (file.startsWith('plugins/shared/')) { sharedUi(); continue; }
      const match = file.match(/^plugins\/(terminal|files|network-proxy)\/(.+)$/);
      if (match) {
        const [, name, relative] = match;
        plan.scripts = true;
        integration.add(name);
        if (name === 'terminal') plan.terminalInput = true;
        if (!/^(ui\/|ui-src\/|release\.json$)/.test(relative)) plugin(name);
        continue;
      }
      const platform = file.match(/^src-tauri\/src\/desktop_files\/(windows|linux|macos)\.rs$/);
      if (platform) {
        core(allTargets.filter(t => t.startsWith(`${platform[1]}-`)));
        integration.add('files');
        if (platform[1] === 'windows') plan.desktop = true;
        continue;
      }
      if (['src-tauri/src/network_metrics.rs', 'src-tauri/src/settings/declaration.rs'].includes(file)) {
        core(['linux-x86_64']); plan.scripts = true;
        if (file.includes('settings')) ui();
        continue;
      }
      if (/^src-tauri\/(src|tests)\/(connections|streams|plugins|core|lib|main|model|lifecycle|resource_owner|platform|servers)(\/|\.|_)/.test(file)) {
        core(); ui();
        plan.ssh = plan.sharedClient = true;
        pluginNames.forEach(name => integration.add(name));
        continue;
      }
      const service = file.match(/^src-tauri\/(src|tests)\/(network|terminal|remote_files|desktop_files)(\/|\.|_)/);
      if (service) {
        core(); ui();
        integration.add(service[2] === 'network' ? 'network-proxy' : service[2] === 'terminal' ? 'terminal' : 'files');
        continue;
      }
      if (/^src-tauri\/(src|tests)\/(updates|desktop_updates|desktop_behavior|settings|storage|windows|window_config|workspaces|component_ui|official_plugins)(\/|\.)/.test(file)) {
        core(); ui();
        continue;
      }
      if (/^scripts\/(select-checks|check-docs)(\.test)?\.mjs$/.test(file)) { plan.scripts = true; continue; }
      if (/^scripts\/test-network-netem\.mjs$/.test(file)) { plan.scripts = true; integration.add('network-proxy'); continue; }
      if (/^scripts\/(network-tools|test-network-ui)\.mjs$/.test(file)) { plan.scripts = true; integration.add('network-proxy'); continue; }
      plan.reasons.push(`Shared build input or unmapped path: ${file.replace(/[\r\n|`]/g, ' ')}`);
      full();
    }
    if (!plan.reasons.length) plan.reasons.push('Selected from changed components and native adapters');
  }

  plan.plugins = [...plugins];
  plan.native = targets.filter(t => coreTargets.has(t.name) || scriptTargets.has(t.name) || pluginNames.some(p => pluginTargets[p].has(t.name)))
    .map(t => {
      const row = { ...t, core: coreTargets.has(t.name), scripts: scriptTargets.has(t.name) };
      for (const name of pluginNames) row[name === 'network-proxy' ? 'network' : name] = pluginTargets[name].has(t.name);
      row.workspaces = [row.core ? 'src-tauri -> target' : null,
        ...pluginNames.filter(p => pluginTargets[p].has(t.name)).map(p => `plugins/${p} -> target`)].filter(Boolean).join('\n');
      return row;
    });
  plan.integration = targets.filter(t => t.name.startsWith('linux-') && (integration.has('network-proxy') || (t.name === 'linux-x86_64' && integration.size)))
    .map(t => ({ ...t, network: integration.has('network-proxy'),
      terminal: t.name === 'linux-x86_64' && integration.has('terminal'), files: t.name === 'linux-x86_64' && integration.has('files') }));
  plan.core = coreTargets.size > 0;
  plan.checks = plan.web || plan.scripts || plan.terminalInput || plan.sharedClient || plan.plugins.length > 0 || plan.core;
  return plan;
}

export function selectionSummary(plan) {
  return ['## Selected checks', '', ...plan.reasons.map(reason => `- ${reason}`), '',
    '| Scope | Selection |', '| --- | --- |',
    '| Documentation | Always: tracked Markdown structure and local file links |',
    `| Frontend / browser / scripts | ${[plan.web && 'frontend + interactions/settings', plan.terminalInput && 'terminal input', plan.scripts && 'script tests'].filter(Boolean).join(', ') || 'Skipped'} |`,
    `| Plugin format | ${plan.plugins.join(', ') || 'Skipped'} |`,
    `| Native source tests | ${plan.native.map(t => `${t.name}: ${[t.core && 'core', t.terminal && 'terminal', t.files && 'files', t.network && 'network', t.scripts && 'scripts'].filter(Boolean).join(', ')}`).join('<br>') || 'Skipped'} |`,
    `| Normal integration | ${plan.integration.map(t => `${t.name}: ${[t.network && 'network', t.terminal && 'terminal', t.files && 'files'].filter(Boolean).join(', ')}`).join('<br>') || 'Skipped'} |`,
    `| Windows desktop | ${plan.desktop ? `Full UI${plan.ssh ? ' + SSH' : ''}` : 'Skipped'} |`,
    `| Weak network | ${plan.netem ? 'Explicit manual netem: both native Linux targets' : 'Skipped; manual netem only, including for all'} |`, '',
    'Selections are not test results. Installed-package checks and published-package/upgrade verification remain independent.', ''].join('\n');
}

export function checkResults(needs) {
  const failures = [];
  if (needs.select?.result !== 'success') failures.push('select');
  const plan = JSON.parse(needs.select?.outputs?.plan || '{}');
  const valid = typeof plan.checks === 'boolean' && typeof plan.desktop === 'boolean' && typeof plan.netem === 'boolean'
    && Array.isArray(plan.native) && Array.isArray(plan.integration) && Array.isArray(plan.plugins);
  if (!valid) failures.push('selection output');
  const expected = { checks: plan.checks, network: plan.integration?.length > 0, desktop: plan.desktop, native: plan.native?.length > 0, netem: plan.netem };
  const lines = ['## Executed job results', '', '| Job | Result |', '| --- | --- |'];
  for (const [job, selected] of Object.entries(expected)) {
    const result = needs[job]?.result || 'missing';
    lines.push(`| ${job} | ${result}${selected ? '' : ' (not selected)'} |`);
    if ((selected && result !== 'success') || (!selected && result !== 'skipped')) failures.push(job);
  }
  return { summary: `${valid ? selectionSummary(plan) : ''}${lines.join('\n')}\n`, failures };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] === 'report') {
    const result = checkResults(JSON.parse(process.env.CHECK_RESULTS));
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, result.summary);
    if (result.failures.length) throw new Error(`Required selected jobs did not pass: ${result.failures.join(', ')}`);
  } else {
    const event = JSON.parse(fs.readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
    const options = process.env.GITHUB_EVENT_NAME === 'workflow_dispatch' ? { suite: event.inputs?.suite || 'standard' }
      : changedFiles(process.env.GITHUB_EVENT_NAME, event);
    const plan = selectChecks(options.files || [], options);
    fs.appendFileSync(process.env.GITHUB_OUTPUT, `plan=${JSON.stringify(plan)}\nnative_matrix=${JSON.stringify({ include: plan.native.length ? plan.native : [targets[2]] })}\nintegration_matrix=${JSON.stringify({ include: plan.integration.length ? plan.integration : [targets[2]] })}\n`);
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, selectionSummary(plan));
  }
}
