import { testPluginDirectory } from "./test-plugin-directory.mjs";
// Linux only: the test process and its plugin inherit a private network namespace.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';

if (process.platform !== 'linux') throw new Error('netem requires native Linux, iproute2 and root/sudo');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-netem-'));
const name = `sodeck-${randomUUID().slice(0, 8)}`;
const privileged = process.getuid() === 0 ? [] : ['sudo', '-n'];
function execute(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, ...options });
  if (result.error) throw result.error;
  return result;
}
function run(command, args, options = {}) {
  const result = execute(command, args, options);
  if (result.status !== 0) throw new Error(`${command} failed (${result.status}): ${result.stderr}\n${result.stdout}`);
  return result.stdout;
}
function admin(command, args) {
  return privileged.length ? run(privileged[0], [...privileged.slice(1), command, ...args]) : run(command, args);
}
const reports = [];
const environment = { kernel: os.release(), architecture: os.arch(), randomSeed: null };
let failure;
let created = false;
try {
  const version = run('rustc', ['-vV']);
  const target = version.match(/^host: (.+)$/m)?.[1];
  if (!['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'].includes(target)) throw new Error('unsupported native Linux target');
  environment.rust = version;
  environment.tc = run('tc', ['-V']).trim();
  const pluginDirectory = testPluginDirectory('network-proxy');
  const build = run('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--features', 'test-fixtures', '--locked',
    '--test', 'connections', '--no-run', '--message-format=json']);
  const artifact = build.split('\n').filter(Boolean).map(line => { try { return JSON.parse(line); } catch { return null; } })
    .find(item => item?.reason === 'compiler-artifact' && item.target?.name === 'connections' && item.executable);
  if (!artifact) throw new Error('connections test executable not found');
  admin('ip', ['netns', 'add', name]); created = true;
  admin('ip', ['-n', name, 'link', 'set', 'lo', 'up']);
  environment.link = admin('ip', ['-n', name, '-details', 'link', 'show', 'lo']).trim();
  const profiles = [
    { name: 'baseline', delay: '0ms', jitter: '0ms', loss: '0%', count: 10 },
    { name: 'remote', delay: '50ms', jitter: '10ms', loss: '0%', count: 10 },
    { name: 'cross-region', delay: '150ms', jitter: '30ms', loss: '1%', count: 20 },
    { name: 'poor', delay: '300ms', jitter: '50ms', loss: '3%', count: 20 },
    { name: 'severe', delay: '500ms', jitter: '100ms', loss: '5%', count: 20 },
    { name: 'blackhole', delay: '0ms', jitter: '0ms', loss: '100%', count: 2 },
  ];
  const selected = process.env.SODECK_NETEM_PROFILES?.split(',');
  if (selected?.some(name => !profiles.some(profile => profile.name === name))) throw new Error('unknown SODECK_NETEM_PROFILES entry');
  for (const profile of profiles.filter(p => !selected || selected.includes(p.name))) {
    admin('ip', ['netns', 'exec', name, 'tc', 'qdisc', 'replace', 'dev', 'lo', 'root', 'netem',
      'delay', profile.delay, profile.jitter, 'loss', profile.loss]);
    const qdisc = admin('ip', ['netns', 'exec', name, 'tc', '-s', 'qdisc', 'show', 'dev', 'lo']);
    console.log(`netem ${profile.name}: ${profile.delay} ± ${profile.jitter}, loss ${profile.loss}, ${profile.count} attempts`);
    const args = ['netns', 'exec', name, 'env', `SODECK_TEST_NETWORK_PLUGIN=${pluginDirectory}`,
      `SODECK_NETEM_PROFILE=${profile.name}`, `SODECK_NETEM_COUNT=${profile.count}`, `SODECK_NETEM_LOSS=${profile.loss}`,
      artifact.executable, 'network_socks_netem', '--ignored', '--exact', '--nocapture'];
    const execution = privileged.length ? execute(privileged[0], [...privileged.slice(1), 'ip', ...args]) : execute('ip', args);
    const output = execution.stdout;
    const marker = output.split('\n').find(line => line.startsWith('SOCKS_NETEM_RESULT '));
    const statistics = admin('ip', ['netns', 'exec', name, 'tc', '-s', 'qdisc', 'show', 'dev', 'lo']);
    const result = marker ? JSON.parse(marker.slice('SOCKS_NETEM_RESULT '.length)) : null;
    reports.push({ ...profile, qdisc, statistics, exitCode: execution.status, result,
      ...(execution.status !== 0 || !marker ? { diagnostic: `${execution.stderr}\n${output}` } : {}) });
    if (marker) console.log(marker);
    if (execution.status !== 0 || !marker) failure = `netem ${profile.name} failed; see report.json`;
  }
  if (failure) throw new Error(failure);
} catch (error) {
  failure = error.message;
  throw error;
} finally {
  let cleanupError;
  try { if (created) admin('ip', ['netns', 'delete', name]); }
  catch (error) { cleanupError = error; }
  try { fs.rmSync(root, { recursive: true, force: true }); }
  catch (error) { cleanupError ??= error; }
  const report = path.resolve(process.env.SODECK_NETEM_REPORT || 'artifacts/network-netem/report.json');
  fs.mkdirSync(path.dirname(report), { recursive: true });
  fs.writeFileSync(report, JSON.stringify({ topology: 'all fixture TCP links on private namespace loopback',
    environment, completed: !failure && !cleanupError, failure: failure ?? cleanupError?.message ?? null,
    resourcesCleaned: !cleanupError, reports }, null, 2));
  if (cleanupError) throw cleanupError;
}
