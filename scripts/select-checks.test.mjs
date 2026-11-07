import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { changedFiles, selectChecks, checkResults, targets } from './select-checks.mjs';

test('documentation produces an explicit lightweight check without product suites', () => {
  const p = selectChecks(['docs/usage.md', 'docs/reference-v2/files-panel.png', 'README.md', 'plugins/files/CHANGELOG.md']);
  assert.equal(p.checks, false);
  assert.deepEqual(p.native, []);
  assert.deepEqual(p.integration, []);
  assert.equal(p.desktop, false);
  assert.equal(p.netem, false);
});

test('frontend layout keeps UI verification without SSH/network/native source suites', () => {
  const p = selectChecks(['src/shell.css', 'src/workspaces.ts']);
  assert(p.web && p.desktop && p.scripts);
  assert(!p.ssh && !p.terminalInput);
  assert.deepEqual(p.integration, []);
  assert.deepEqual(p.native, []);
});

test('a plugin only selects its own source and normal business tests', () => {
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
});

test('native adapter scopes both architectures of the affected system; common logic stays on Linux', () => {
  for (const [osName, count] of [['windows', 2], ['linux', 2], ['macos', 1]]) {
    const p = selectChecks([`src-tauri/src/desktop_files/${osName}.rs`]);
    assert.equal(p.native.length, count);
    assert(p.native.every(t => t.name.startsWith(`${osName}-`) && t.core));
  }
  const common = selectChecks(['src-tauri/src/network_metrics.rs']);
  assert.deepEqual(common.native.map(t => t.name), ['linux-x86_64']);
  assert.deepEqual(common.integration, []);
  for (const file of ['src-tauri/src/platform.rs', 'src-tauri/src/updates.rs', 'src-tauri/src/storage.rs']) {
    assert.equal(selectChecks([file]).native.length, 5, file);
  }
});

test('transport/lifecycle changes retain real integrations and shared inputs/unknown paths expand conservatively', () => {
  for (const file of ['src-tauri/src/connections/ssh.rs', 'src-tauri/src/plugins/rpc.rs', 'src-tauri/tests/lifecycle.rs']) {
    const p = selectChecks([file]);
    assert(p.ssh && p.desktop);
    assert.equal(p.native.filter(t => t.core).length, 5);
    assert(p.integration[0].network && p.integration[0].files && p.integration[0].terminal);
  }
  for (const file of ['crates/plugin-client/src/lib.rs', 'package-lock.json', '.node-version', '.github/workflows/check.yml', 'scripts/package-plugin.mjs', 'new-adapter.rs', 'src-tauri/src/new_service.rs']) {
    const p = selectChecks([file]);
    assert.equal(p.native.length, 5, file);
    assert(p.native.every(t => t.core && t.terminal && t.files && t.network), file);
    assert(!p.netem, file);
  }
  const sharedUi = selectChecks(['src/text-menu.js', 'plugins/shared/clipboard.js']);
  assert(sharedUi.terminalInput && sharedUi.web && sharedUi.desktop && sharedUi.integration[0].files);
  const mixed = selectChecks(['docs/usage.md', 'src-tauri/src/platform.rs']);
  assert.equal(mixed.native.length, 5);
  const files = selectChecks(['src-tauri/src/remote_files/operations/copy.rs']);
  assert.equal(files.integration.length, 1);
  assert(files.integration[0].files && !files.integration[0].network && !files.integration[0].terminal);
});

test('manual same-ref routes preserve overrides and only netem explicitly enables weak-network tests', () => {
  const standard = selectChecks([], { suite: 'standard' });
  assert(!standard.desktop && !standard.netem && standard.integration[0].network);
  assert.equal(standard.native.length, 1);
  const desktop = selectChecks([], { suite: 'desktop' });
  assert(desktop.desktop && desktop.ssh && !desktop.checks);
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
    checks: { result: 'success' }, desktop: { result: 'success' }, network: { result: 'skipped' }, native: { result: 'skipped' }, netem: { result: 'skipped' } };
  assert.deepEqual(checkResults(needs).failures, []);
  for (const result of ['failure', 'cancelled', 'skipped']) {
    assert.deepEqual(checkResults({ ...needs, desktop: { result } }).failures, ['desktop']);
  }
  assert(checkResults({ ...needs, select: { result: 'failure' } }).failures.includes('select'));
  assert(checkResults({ ...needs, select: { result: 'success', outputs: {} } }).failures.includes('selection output'));
  assert(checkResults({ ...needs, netem: { result: 'success' } }).failures.includes('netem'));
  assert.match(checkResults(needs).summary, /skipped \(not selected\)/);
  assert.equal(new Set(targets.map(t => t.target)).size, 5);
});
