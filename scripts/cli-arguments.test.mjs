import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parsePluginBuildArgs } from './package-plugin.mjs';
import { parseReleaseMetadataArgs, pluginConfig } from './release-metadata.mjs';
import { parseTestPrepareArgs } from './prepare-test-plugins.mjs';
import { selectChecks } from './select-checks.mjs';
import { parseOpenSshTestArgs, parseTerminalInputTestArgs, parseProductionUiTestArgs } from './test-cli-arguments.mjs';

test('plugin build parses existing flags and leaves runtime defaults to the build entry', () => {
  assert.deepEqual(parsePluginBuildArgs(['--plugin', 'terminal']), { plugin: 'terminal', target: undefined, outDir: undefined });
  assert.deepEqual(parsePluginBuildArgs(['--out-dir', 'output with spaces', '--target=aarch64-pc-windows-msvc', '--plugin=files']), {
    plugin: 'files', target: 'aarch64-pc-windows-msvc', outDir: 'output with spaces',
  });
  assert.deepEqual(parsePluginBuildArgs(['--plugin', 'terminal', '--plugin', 'files', '--target', 'first', '--target', 'second', '--out-dir', 'first', '--out-dir', 'second']), {
    plugin: 'terminal', target: 'first', outDir: 'first',
  });
});

test('plugin build rejects missing selections, invalid names and option values', () => {
  assert.throws(() => parsePluginBuildArgs([]), /Specify a plugin/);
  for (const plugin of ['', '../files', 'files/child', 'C:\\files', 'Files']) {
    assert.throws(() => parsePluginBuildArgs(['--plugin', plugin]), /Specify a plugin|Invalid plugin directory/);
  }
  for (const args of [['--plugin', '--target', 'target'], ['--plugin', 'terminal', '--out-dir'], ['--plugin', 'terminal', '--unknown'], ['terminal']]) {
    assert.throws(() => parsePluginBuildArgs(args), error => error.code?.startsWith('ERR_PARSE_ARGS_'));
  }
});

test('release metadata parses flags without applying tag environment policy', () => {
  assert.deepEqual(parseReleaseMetadataArgs([]), { kind: undefined, tag: undefined, plugin: undefined, input: undefined });
  assert.deepEqual(parseReleaseMetadataArgs(['--input', 'input with spaces', '--plugin=terminal', '--tag', 'plugin-terminal-v1.2.3', '--kind=plugin']), {
    kind: 'plugin', tag: 'plugin-terminal-v1.2.3', plugin: 'terminal', input: 'input with spaces',
  });
  assert.deepEqual(parseReleaseMetadataArgs(['--kind', 'core', '--kind', 'plugin', '--tag', 'first', '--tag', 'second', '--plugin', 'terminal', '--plugin', 'files', '--input', 'first', '--input', 'second']), {
    kind: 'core', tag: 'first', plugin: 'terminal', input: 'first',
  });
});

test('test preparation preserves positional flags, CI aliases and matrix precedence', () => {
  assert.deepEqual(parseTestPrepareArgs(['terminal', '--ui-only', 'files']), { plugins: ['terminal', 'files'], uiOnly: true });
  assert.deepEqual(parseTestPrepareArgs(['docker']), { plugins: ['terminal', 'docker'], uiOnly: false });
  assert.deepEqual(parseTestPrepareArgs(['--matrix', '{"network":true,"status":true,"docker":true}']), {
    plugins: ['terminal', 'network-proxy', 'system-status', 'docker'], uiOnly: false,
  });
  assert.deepEqual(parseTestPrepareArgs(['--matrix={"communication":true}', '--ui-only']), {
    plugins: ['terminal', 'files', 'system-status'], uiOnly: true,
  });
  assert.deepEqual(parseTestPrepareArgs(['docker', '--matrix', '{"files":true}', '--matrix', '{"docker":true}']), {
    plugins: ['files'], uiOnly: false,
  });
});

test('a matrix flag requires an object root and cannot fall back to positional plugins', () => {
  for (const raw of ['null', '[]', '"terminal"', '1', 'true', 'false']) {
    assert.throws(() => parseTestPrepareArgs(['--matrix', raw, 'terminal']), /--matrix must be a JSON object/);
  }
  assert.throws(() => parseTestPrepareArgs(['--matrix', '{']), SyntaxError);
  for (const args of [[], ['../files'], ['--matrix', '{}']]) {
    assert.throws(() => parseTestPrepareArgs(args), /Select explicit test plugins/);
  }
  for (const args of [['--matrix'], ['--matrix', '--ui-only'], ['terminal', '--unknown']]) {
    assert.throws(() => parseTestPrepareArgs(args), error => error.code?.startsWith('ERR_PARSE_ARGS_'));
  }
});

test('release metadata rejects missing values and unknown options instead of using core defaults', () => {
  const env = { ...process.env };
  delete env.GITHUB_OUTPUT;
  delete env.GITHUB_REF_NAME;
  delete env.GITHUB_REF_TYPE;
  for (const args of [['--kind'], ['--kind', 'core', '--unknown']]) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL('./release-metadata.mjs', import.meta.url)), ...args], {
      env, encoding: 'utf8', windowsHide: true,
    });
    assert.notEqual(result.status, 0, `Invalid arguments succeeded: ${args.join(' ')}`);
    assert.match(result.stderr, /ERR_PARSE_ARGS_/);
  }
});

test('importing argument functions needs no native tools and performs no preparation', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-cli-import-'));
  t.after(() => { assert.equal(path.dirname(directory), os.tmpdir()); fs.rmSync(directory, { recursive: true, force: true }); });
  const modules = ['package-plugin.mjs', 'release-metadata.mjs', 'prepare-test-plugins.mjs'].map(name => new URL(name, import.meta.url).href);
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
  env.PATH = directory;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `await Promise.all(${JSON.stringify(modules)}.map(href => import(href)));`], {
    cwd: directory, env, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
  assert.deepEqual(fs.readdirSync(directory), []);
});

test('release metadata keeps explicit and environment tag policy in its executable entry', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-cli-release-'));
  t.after(() => { assert.equal(path.dirname(directory), os.tmpdir()); fs.rmSync(directory, { recursive: true, force: true }); });
  fs.mkdirSync(path.join(directory, 'src-tauri'));
  fs.mkdirSync(path.join(directory, 'docs/releases'), { recursive: true });
  fs.writeFileSync(path.join(directory, 'src-tauri/tauri.conf.json'), '{"version":"1.2.3"}');
  fs.writeFileSync(path.join(directory, 'docs/releases/v1.2.3.md'), 'fixture notes');
  const script = fileURLToPath(new URL('./release-metadata.mjs', import.meta.url));
  for (const [index, args, refName, succeeds] of [
    [0, ['--kind', 'core', '--tag', 'v1.2.3'], 'v9.9.9', true],
    [1, ['--kind', 'core'], 'v1.2.3', true],
    [2, ['--kind', 'core'], 'v9.9.9', false],
  ]) {
    const output = path.join(directory, `output-${index}`);
    const result = spawnSync(process.execPath, [script, ...args], {
      cwd: directory, env: { ...process.env, GITHUB_REF_NAME: refName, GITHUB_REF_TYPE: 'tag', GITHUB_OUTPUT: output },
      encoding: 'utf8', windowsHide: true,
    });
    if (succeeds) {
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout.trim(), 'v1.2.3');
      assert.equal(fs.readFileSync(output, 'utf8'), 'tag=v1.2.3\n');
    } else {
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /must match v1.2.3/);
      assert(!fs.existsSync(output));
    }
  }
});

test('release tags keep their inferred plugin ahead of an explicit plugin flag', () => {
  const tag = pluginConfig('terminal').tag;
  const env = { ...process.env, GITHUB_REF_NAME: tag, GITHUB_REF_TYPE: 'tag' };
  delete env.GITHUB_OUTPUT;
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('./release-metadata.mjs', import.meta.url)), '--kind', 'plugin', '--plugin', 'files'], {
    env, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), tag);
});

test('argument unit tests select script checks without native or plugin builds', () => {
  const plan = selectChecks(['scripts/cli-arguments.test.mjs']);
  assert(plan.scripts && plan.checks);
  assert(!plan.web && !plan.browser && !plan.core);
  assert.deepEqual(plan.native, []);
  assert.deepEqual(plan.integration, []);
});

test('remaining OpenSSH arguments retain every flag and opaque packaged launcher tail', () => {
  const names = ['terminal', 'files', 'network', 'system-status', 'communication', 'files-ui-only', 'network-ui-only', 'file-editor-native', 'close-native'];
  const defaults = parseOpenSshTestArgs([]);
  assert.deepEqual({ ...defaults.flags }, {});
  assert.equal(defaults.launcher, null);
  const parsed = parseOpenSshTestArgs(names.map(name => `--${name}`));
  assert.deepEqual({ ...parsed.flags }, Object.fromEntries(names.map(name => [name, true])));
  assert.equal(parseOpenSshTestArgs(['--terminal', '--terminal']).flags.terminal, true);
  const tail = ['dbus-run-session', '--', 'xvfb-run', '-a', '程序 with spaces', '--close-native', '--file-editor-native', '--packaged'];
  const packaged = parseOpenSshTestArgs(['--terminal', '--packaged', ...tail]);
  assert.deepEqual(packaged.launcher, tail);
  assert.deepEqual({ ...packaged.flags }, { terminal: true });
  assert.deepEqual(parseOpenSshTestArgs(['--file-editor-native', '--close-native', '--packaged', 'C:\\程序 with spaces.exe']).launcher, ['C:\\程序 with spaces.exe']);
});

test('remaining terminal and production UI arguments keep defaults and first path priority', () => {
  assert.deepEqual(parseTerminalInputTestArgs([]), { desktopScript: undefined });
  assert.deepEqual(parseTerminalInputTestArgs(['--desktop-script=脚本 with spaces.js']), { desktopScript: '脚本 with spaces.js' });
  assert.deepEqual(parseTerminalInputTestArgs(['--desktop-script', 'first', '--desktop-script', 'second']), { desktopScript: 'first' });
  assert.deepEqual(parseProductionUiTestArgs([]), { updatesOnly: false });
  assert.deepEqual(parseProductionUiTestArgs(['--updates-only', '--updates-only']), { updatesOnly: true });
});

test('remaining test arguments reject missing values, unknown flags and unexpected positions', () => {
  for (const args of [['--unknown'], ['terminal'], ['--terminal=true'], ['--packaged=program']]) {
    assert.throws(() => parseOpenSshTestArgs(args), error => error.code?.startsWith('ERR_PARSE_ARGS_'));
  }
  for (const args of [['--packaged'], ['--packaged', '']]) assert.throws(() => parseOpenSshTestArgs(args), /--packaged requires/);
  for (const args of [['--desktop-script'], ['--desktop-script', '--unknown'], ['--unknown'], ['script.js']]) {
    assert.throws(() => parseTerminalInputTestArgs(args), error => error.code?.startsWith('ERR_PARSE_ARGS_'));
  }
  assert.throws(() => parseTerminalInputTestArgs(['--desktop-script', '']), /nonempty path/);
  for (const args of [['--unknown'], ['updates'], ['--updates-only=true']]) {
    assert.throws(() => parseProductionUiTestArgs(args), error => error.code?.startsWith('ERR_PARSE_ARGS_'));
  }
});

test('remaining test argument import and executable failures occur before resource startup', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-test-cli-'));
  t.after(() => { assert.equal(path.dirname(directory), os.tmpdir()); fs.rmSync(directory, { recursive: true, force: true }); });
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
  env.PATH = directory; env.TMP = directory; env.TEMP = directory;
  const imported = spawnSync(process.execPath, ['--input-type=module', '-e', `await import(${JSON.stringify(new URL('./test-cli-arguments.mjs', import.meta.url).href)});`], {
    cwd: directory, env, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(imported.status, 0, imported.stderr);
  assert.equal(imported.stdout, '');
  for (const [script, args, error] of [
    ['test-openssh.mjs', ['--unknown'], /ERR_PARSE_ARGS_/],
    ['test-openssh.mjs', ['--packaged'], /--packaged requires/],
    ['test-terminal-input.mjs', ['--desktop-script'], /ERR_PARSE_ARGS_/],
    ['test-terminal-input.mjs', ['--desktop-script', ''], /nonempty path/],
    ['test-production-ui.mjs', ['--unknown'], /ERR_PARSE_ARGS_/],
  ]) {
    const result = spawnSync(process.execPath, [fileURLToPath(new URL(script, import.meta.url)), ...args], {
      cwd: directory, env, encoding: 'utf8', windowsHide: true,
    });
    assert.notEqual(result.status, 0, script);
    assert.match(result.stderr, error, script);
    assert.equal(result.stdout, '', script);
    assert.deepEqual(fs.readdirSync(directory), [], script);
  }
});
