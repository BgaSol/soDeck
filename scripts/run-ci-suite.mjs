import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export async function runCommand(suite, executable, args, root = 'artifacts/ci') {
  if (!/^[a-z][a-z0-9:-]*$/.test(suite)) throw new Error('Invalid CI suite name');
  const directory = path.resolve(root, suite.replaceAll(':', '-'));
  fs.mkdirSync(directory, { recursive: true });
  const log = fs.createWriteStream(path.join(directory, 'output.log'));
  const started = Date.now();
  const report = { suite, sha: process.env.GITHUB_SHA ?? null, platform: process.platform,
    attempt: process.env.GITHUB_RUN_ATTEMPT ?? null, startedAt: new Date(started).toISOString() };
  const writeReport = result => fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify({
    ...report, ...result, elapsedMs: Date.now() - started,
  }, null, 2) + '\n');
  writeReport({ status: 'running' });
  const child = spawn(executable, args, { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, SODECK_CI_ARTIFACTS: directory } });
  child.stdout.on('data', data => { process.stdout.write(data); log.write(data); });
  child.stderr.on('data', data => { process.stderr.write(data); log.write(data); });
  const result = await new Promise(resolve => {
    child.once('error', error => resolve({ status: 'failure', code: 1, error: error.message }));
    child.once('close', (code, signal) => resolve({ status: code === 0 ? 'success' : 'failure', code: code ?? 1, signal }));
  });
  await new Promise(resolve => log.end(resolve));
  writeReport(result);
  return result.code;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const suite = process.argv[2];
  const scripts = JSON.parse(fs.readFileSync('package.json', 'utf8')).scripts;
  if (!Object.hasOwn(scripts, suite) || suite === 'ci:test' || !process.env.npm_execpath) throw new Error('Run npm run ci:test -- <existing test script>');
  process.exitCode = await runCommand(suite, process.execPath, [process.env.npm_execpath, 'run', suite]);
  if (process.env.GITHUB_STEP_SUMMARY) {
    const directory = `artifacts/ci/${suite.replaceAll(':', '-')}`;
    const result = JSON.parse(fs.readFileSync(`${directory}/result.json`, 'utf8'));
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n- ${suite}: **${result.status}**, ${(result.elapsedMs / 1000).toFixed(1)}s; log/result: ${directory}\n`);
  }
}
