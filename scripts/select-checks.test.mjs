import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { changedFiles, selectChecks, checkResults, targets, selectionSummary } from './select-checks.mjs';

test('shared DOM runtime checks its independently packaged network consumer',()=>{
  for(const file of ['src/dom-update.ts','scripts/plugin-dom-assets.mjs']){
    const result=selectChecks([file]);assert(result.browser&&result.scripts);assert(result.plugins.includes('network-proxy'));assert(result.integration.some(row=>row.network));
  }
});
test('file editor SDK selects its real consumers and file integration',()=>{
 const p=selectChecks(['packages/file-editor/src/page.js']);assert(p.plugins.includes('files'));assert(p.integration.some(v=>v.files));assert(p.scripts);
});

test('shared client runs its own native tests and known script-only regressions stay narrow', () => {
  const client = selectChecks(['crates/plugin-client/src/lib.rs']);
  assert.equal(client.native.filter(row => row.client).length, 5);
  const shell = selectChecks(['scripts/test-terminal-shell.py']);
  assert(shell.integration.some(row => row.terminal)); assert.equal(shell.core, false);
  const release = selectChecks(['scripts/core-release.test.mjs']);
  assert(release.scripts); assert.equal(release.core, false); assert.equal(release.integration.length, 0);
  assert(selectChecks(['scripts/core-release.test.mjs', 'unknown-input']).core);
});

test('test helpers select their existing consumers and download unit tests stay script-only',()=>{
  const cli=selectChecks(['scripts/test-cli-arguments.mjs']);
  assert(cli.scripts&&cli.terminalInput&&cli.browser&&cli.ssh);
  assert.equal(cli.native.length,5);assert(cli.integration.some(row=>row.network));
  const theme=selectChecks(['scripts/test-theme-tokens.mjs']);
  assert(theme.scripts);assert.deepEqual(theme.native,[]);
  assert.equal(theme.integration.length,1);assert(theme.integration[0].status&&theme.integration[0].docker);
  assert(!theme.browser&&!theme.terminalInput&&!theme.ssh&&!theme.desktop);
  const download=selectChecks(['scripts/verified-download.test.mjs']);
  assert(download.scripts);assert.deepEqual(download.native,[]);assert.deepEqual(download.integration,[]);
});
test('shared state selects its native source, SDK and actual three-plugin communication',()=>{for(const file of ['src/component-state.ts','src-tauri/src/shared_state.rs','src-tauri/src/shared_state/delivery.rs','plugins/shared/state.js']){const p=selectChecks([file]);assert(p.communication&&p.sharedClient&&p.core);assert(p.integration.some(t=>t.communication&&t.terminal&&t.files&&t.status));assert.deepEqual(p.plugins,['terminal','files','system-status']);}assert(selectChecks(['plugins/files/ui/tree.js']).communication);});

test('documentation produces an explicit lightweight check without product suites', () => {
  const p = selectChecks(['docs/usage.md', 'docs/reference-v2/files-panel.png', 'README.md', 'plugins/files/CHANGELOG.md']);
  assert.equal(p.checks, false);
  assert.deepEqual(p.native, []);
  assert.deepEqual(p.integration, []);
  assert.equal(p.desktop, false);
  assert.equal(p.netem, false);
});

test('file editor regression selects the file plugin integration',()=>{
  for(const file of ['scripts/test-file-editor-ui.mjs','scripts/file-editor-fixture.mjs']){
    const plan=selectChecks([file]);assert(plan.scripts&&plan.integration[0].files);assert.deepEqual(plan.native,[]);assert(!plan.terminalInput&&!plan.desktop);
  }
});

test('frontend layout keeps UI verification without SSH/network/native source suites', () => {
  const p = selectChecks(['src/shell.css', 'src/workspaces.ts']);
  assert(p.web && p.browser && p.scripts && !p.desktop);
  assert(!p.ssh && !p.terminalInput);
  assert.deepEqual(p.integration, []);
  assert.deepEqual(p.native, []);
});

test('a plugin only selects its own source and normal business tests', () => {
  const docker = selectChecks(['plugins/docker/src/runtime.rs']);
  assert.deepEqual(docker.plugins, ['docker']);
  assert.equal(docker.native.length, 5);
  assert(docker.native.every(t => t.docker && !t.core));
  assert.equal(docker.integration.length, 1);
  assert(docker.integration[0].docker);
  assert(selectionSummary(docker).includes('linux-x86_64: docker'));
  const dockerUi = selectChecks(['scripts/test-docker-ui.mjs']);
  assert.deepEqual(dockerUi.native, []);
  assert(dockerUi.integration[0].docker && !dockerUi.integration[0].files && !dockerUi.desktop);
  const status = selectChecks(['plugins/system-status/src/scheduler.rs']);
  assert.deepEqual(status.plugins, ['system-status']);
  assert(status.native.length === 5 && status.native.every(t => t.status && !t.core));
  assert(status.integration.length === 1 && status.integration[0].status);
  assert(selectionSummary(status).includes('linux-x86_64: system-status'));
  for (const [name, field] of [['terminal', 'terminal'], ['files', 'files'], ['network-proxy', 'network']]) {
    const p = selectChecks([`plugins/${name}/src/main.rs`]);
    assert.deepEqual(p.plugins, [name]);
    assert.equal(p.native.length, 5);
    assert(p.native.every(t => t[field] && !t.core));
    assert.equal(p.integration.length, name === 'network-proxy' ? 2 : 1);
    assert(p.integration.every(t => t[field]));
    assert(!p.desktop && !p.web && !p.netem);
  }
  const ui = selectChecks(['plugins/files/ui/tree.js']);
  assert.deepEqual(ui.native, []);
  assert(ui.integration[0].files);
  for (const name of ['terminal', 'files']) {
    const regression = selectChecks([`scripts/test-${name}-ui.mjs`]);
    assert.deepEqual(regression.native, []);
    assert.equal(regression.integration.length, 1);
    assert(regression.integration[0][name]);
  }
  const input = selectChecks(['scripts/test-terminal-input.mjs']);
  assert(input.terminalInput && !input.web && !input.integration.length && !input.native.length);
  for(const file of ['test-terminal-history','test-terminal-search-ui','benchmark-terminal-history','terminal-history.test']){
    const history=selectChecks([`scripts/${file}.mjs`]);assert(history.scripts&&history.terminalInput&&!history.web&&!history.integration.length&&!history.native.length);
  }
});

test('native adapter scopes both architectures of the affected system; common logic stays on Linux', () => {
  for (const [osName, count] of [['windows', 2], ['linux', 2], ['macos', 1]]) {
    const p = selectChecks([`src-tauri/src/desktop_files/${osName}.rs`]);
    assert.equal(p.native.length, count);
    assert(p.native.every(t => t.name.startsWith(`${osName}-`) && t.core));
    assert.equal(p.desktop, false, 'Filesystem adapters use native tests, not an empty desktop job');
  }
  const common = selectChecks(['src-tauri/src/network_metrics.rs']);
  assert.deepEqual(common.native.map(t => t.name), ['linux-x86_64']);
  assert.deepEqual(common.integration, []);
  for (const file of ['src-tauri/src/platform.rs', 'src-tauri/src/updates.rs', 'src-tauri/src/update_network.rs', 'src-tauri/tests/update_network.rs', 'src-tauri/src/storage.rs']) {
    assert.equal(selectChecks([file]).native.length, 5, file);
  }
  const full = selectChecks([], { suite: 'all' });
  for (const file of ['src-tauri/tauri.conf.json', 'src-tauri/tauri.macos.conf.json', 'src-tauri/tauri.release.conf.json', 'src-tauri/capabilities/main.json']) {
    const config = selectChecks([file]);
    assert(config.web && config.browser && !config.desktop && !config.ssh);
    assert.deepEqual(config.integration, []);
    assert.deepEqual(config.plugins, []);
    assert.equal(config.native.length, 5);
    assert(config.native.every(t => t.core && !t.terminal && !t.files && !t.network));
    assert.deepEqual(config.native.map(t => t.workspaces), full.native.map(t => t.workspaces), 'Core scopes must reuse the same cache inputs');
  }
});

test('transport/lifecycle changes retain real integrations and shared inputs/unknown paths expand conservatively', () => {
  const fixtureEvents = selectChecks(['scripts/fixtures/events.js']);
  assert(fixtureEvents.scripts && !fixtureEvents.core && !fixtureEvents.netem);
  assert(fixtureEvents.integration.some(t => t.network));
  assert(fixtureEvents.integration.some(t => t.terminal && t.files && !t.network));
  const nativeClose = selectChecks(['scripts/test-close-native.mjs']);
  assert(nativeClose.desktop && nativeClose.ssh && nativeClose.native.filter(t => t.core).length === 5);
  const pointer = selectChecks(['src-tauri/tests/fixtures/native_pointer.rs']);
  assert(pointer.desktop && pointer.ssh);
  assert.deepEqual(pointer.native.filter(t => t.core).map(t => t.name), ['windows-x86_64', 'windows-aarch64']);
  const opener = selectChecks(['src-tauri/src/desktop_open.rs']);
  assert(opener.browser && !opener.desktop && opener.native.filter(t => t.core).length === 5 && opener.integration[0].files);
  for (const file of ['src-tauri/src/connections/ssh.rs', 'src-tauri/src/plugins/rpc.rs', 'src-tauri/src/session_exec.rs', 'src-tauri/src/lifecycle.rs', 'src-tauri/src/lifecycle/close.rs', 'src-tauri/src/resource_tasks.rs', 'src-tauri/tests/close_lifecycle.rs', 'src-tauri/tests/lifecycle.rs']) {
    const p = selectChecks([file]);
    assert(p.ssh && p.desktop);
    assert.equal(p.native.filter(t => t.core).length, 5);
    assert(p.integration.some(t => t.network));
    assert(p.integration.some(t => t.files && t.terminal && !t.network));
  }
  for (const file of ['crates/plugin-client/src/lib.rs', 'package-lock.json', '.node-version', '.github/workflows/check.yml', 'scripts/package-plugin.mjs', 'new-adapter.rs', 'src-tauri/src/new_service.rs']) {
    const p = selectChecks([file]);
    assert.equal(p.native.length, 5, file);
    assert(p.native.every(t => t.core && t.terminal && t.files && t.network), file);
    assert(!p.netem, file);
  }
  const sharedUi = selectChecks(['src/text-menu.js', 'plugins/shared/clipboard.js']);
  assert(sharedUi.terminalInput && sharedUi.web && sharedUi.browser && !sharedUi.desktop && sharedUi.integration.some(t => t.files));
  const mixed = selectChecks(['docs/usage.md', 'src-tauri/src/platform.rs']);
  assert.equal(mixed.native.length, 5);
  const files = selectChecks(['src-tauri/src/remote_files/operations/copy.rs']);
  assert.equal(files.integration.length, 1);
  assert(files.integration[0].files && !files.integration[0].network && !files.integration[0].terminal);
});

test('published-upgrade verification checks the core fixture and scripts without selecting plugin builds', () => {
  for (const file of ['src-tauri/tests/fixtures/release_upgrade.rs', 'scripts/test-release-upgrade.mjs', '.github/workflows/release-upgrade.yml']) {
    const p = selectChecks([file]);
    assert(p.scripts && p.native.length === 5 && p.native.every(t => t.core));
    assert.deepEqual(p.plugins, []);
    assert.deepEqual(p.integration, []);
    assert.equal(p.workflow, file.endsWith('.yml'));
  }
  for (const file of ['scripts/upgrade-inputs.mjs', 'scripts/upgrade-inputs.test.mjs']) {
    const p = selectChecks([file]);
    assert(p.scripts && !p.native.length && !p.integration.length);
  }
});

test('manual same-ref routes preserve overrides and only netem explicitly enables weak-network tests', () => {
  const standard = selectChecks([], { suite: 'standard' });
  assert(!standard.desktop && !standard.netem && standard.integration[0].network);
  assert.equal(standard.native.length, 1);
  const desktop = selectChecks([], { suite: 'desktop' });
  assert(desktop.desktop && desktop.ssh && desktop.windowsUi && !desktop.browser && !desktop.checks);
  const native = selectChecks([], { suite: 'native-core' });
  assert.equal(native.native.length, 5);
  assert.equal(native.native.filter(t => t.scripts).length, 3);
  const network = selectChecks([], { suite: 'network-plugin' });
  assert(network.native.every(t => t.network && !t.core));
  assert.deepEqual(network.integration, []);
  const all = selectChecks([], { suite: 'all' });
  assert(all.native.every(t => t.core && t.terminal && t.files && t.network));
  assert(!all.netem);
  const netem = selectChecks([], { suite: 'netem' });
  assert(netem.netem && !netem.checks && !netem.desktop);
  assert.deepEqual(netem.native, []);
  assert.throws(() => selectChecks([], { suite: 'bogus' }), /Unknown/);
  assert.equal(selectChecks([], { fallback: 'Missing base' }).native.length, 5);
});

test('PR merge-base excludes upstream-only changes; pushes include deletion and both sides of renames', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-check-diff-'));
  const addedPath = process.platform === 'win32' ? 'docs/new file.md' : 'docs/new\nline.md';
  const git = args => execFileSync('git', args, { cwd: root, encoding: 'utf8', windowsHide: true });
  const write = (name, value = name) => { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), value); };
  const commit = () => { git(['add', '.']); git(['commit', '-m', 'test']); return git(['rev-parse', 'HEAD']).trim(); };
  try {
    git(['init', '-b', 'main']); git(['config', 'user.email', 'test@invalid.test']); git(['config', 'user.name', 'test']);
    write('src/shell.css'); write('docs/中文 file.md'); const before = commit();
    git(['checkout', '-b', 'feature']);
    git(['mv', 'src/shell.css', 'docs/moved.md']);
    fs.unlinkSync(path.join(root, 'docs/中文 file.md')); write(addedPath); const head = commit();
    git(['checkout', 'main']); write('package-lock.json'); const base = commit();
    const result = changedFiles('pull_request', { pull_request: { base: { sha: base }, head: { sha: head } } }, root);
    assert.equal(result.fallback, null);
    assert(!result.files.includes('package-lock.json'));
    assert.deepEqual(new Set(result.files), new Set(['src/shell.css', 'docs/moved.md', 'docs/中文 file.md', addedPath]));
    assert.equal(selectChecks(result.files).web, true);
    assert.deepEqual(changedFiles('push', { before, after: head }, root).files, result.files);
    for (const invalidBase of ['0'.repeat(40), 'a'.repeat(40), '--help']) {
      assert(changedFiles('push', { before: invalidBase, after: head }, root).fallback);
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('summary rejects failed, cancelled or skipped selected jobs rather than reporting a false pass', () => {
  const plan = selectChecks(['src/shell.css']);
  const needs = { select: { result: 'success', outputs: { plan: JSON.stringify(plan) } },
    checks: { result: 'success' }, browser: { result: 'success' }, desktop: { result: 'skipped' }, network: { result: 'skipped' }, native: { result: 'skipped' }, netem: { result: 'skipped' } };
  assert.deepEqual(checkResults(needs).failures, []);
  for (const result of ['failure', 'cancelled', 'skipped']) {
    assert.deepEqual(checkResults({ ...needs, browser: { result } }).failures, ['browser']);
  }
  const missing = { ...needs }; delete missing.browser;
  assert.deepEqual(checkResults(missing).failures, ['browser']);
  assert(checkResults({ ...needs, select: { result: 'failure' } }).failures.includes('select'));
  assert(checkResults({ ...needs, select: { result: 'success', outputs: {} } }).failures.includes('selection output'));
  assert(checkResults({ ...needs, netem: { result: 'success' } }).failures.includes('netem'));
  assert.match(checkResults(needs).summary, /skipped \(not selected\)/);
  assert.equal(new Set(targets.map(t => t.target)).size, 5);
});

test('core-backed UI has one default Linux owner and keeps manual Windows comparison', () => {
  const workflow = fs.readFileSync('.github/workflows/check.yml', 'utf8').replaceAll('\r\n', '\n');
  const common = workflow.split('\n  checks:\n')[1].split('\n  browser:\n')[0];
  const browser = workflow.split('\n  browser:\n')[1].split('\n  network:\n')[0];
  const desktop = workflow.split('\n  desktop:\n')[1].split('\n  native:\n')[0];
  assert(!common.includes('playwright') && !common.includes('ui:test') && !common.includes('settings:test'));
  assert(browser.includes('libwebkit2gtk-4.1-dev') && browser.includes('npm run ci:test -- ui:test'));
  assert(desktop.includes('if: fromJSON(needs.select.outputs.plan).windowsUi'));
  assert(JSON.parse(fs.readFileSync('package.json', 'utf8')).scripts['ui:test'].includes('npm run settings:test'));
  for (const plan of [selectChecks(['src/settings.css']), selectChecks(['src-tauri/src/desktop_files/windows.rs']),
    selectChecks([], { suite: 'standard' }), selectChecks([], { suite: 'desktop' }), selectChecks([], { suite: 'all' }), selectChecks(['docs/usage.md'])]) {
    assert.equal(Number(plan.browser && plan.web) + Number(plan.windowsUi), Number(plan.web || plan.windowsUi));
  }
});

test('integration groups isolate network on both native architectures from other business', () => {
  const full = selectChecks([], { suite: 'all' });
  assert.deepEqual(full.integration.map(t => t.name), ['network-linux-x86_64', 'network-linux-aarch64', 'business-linux-x86_64']);
  for (const row of full.integration.filter(t => t.network)) assert(!row.files && !row.terminal && !row.docker && !row.status && !row.communication);
  const business = full.integration.find(t => !t.network);
  assert(business.files && business.terminal && business.docker && business.status && business.communication);
  assert.equal(business.target, 'x86_64-unknown-linux-gnu');
  assert(!selectChecks(['plugins/files/ui/tree.js']).integration.some(t => t.network));
  assert.equal(selectChecks(['docs/usage.md']).browser, false);
  assert.equal(selectChecks(['docs/usage.md']).workflow, false);
  assert(selectChecks(['.github/workflows/check.yml']).workflow);
  assert(selectChecks([], { suite: 'all' }).workflow);
});
