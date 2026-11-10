import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runCommand } from './run-ci-suite.mjs';

test('CI reports preserve success, failure and unavailable executable without false passes', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-ci-report-'));
  try {
    for (const code of [0, 7]) {
      const suite = `test:${code}`;
      assert.equal(await runCommand(suite, process.execPath, ['-e', `console.log('stdout'); console.error('stderr'); process.exit(${code})`], root), code);
      const directory = path.join(root, `test-${code}`);
      const report = JSON.parse(fs.readFileSync(path.join(directory, 'result.json'), 'utf8'));
      assert.equal(report.status, code === 0 ? 'success' : 'failure');
      assert.equal(report.code, code);
      assert(report.elapsedMs >= 0);
      assert.match(fs.readFileSync(path.join(directory, 'output.log'), 'utf8'), /stdout/);
      assert.match(fs.readFileSync(path.join(directory, 'output.log'), 'utf8'), /stderr/);
    }
    assert.equal(await runCommand('missing', path.join(root, 'missing-executable'), [], root), 1);
    assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'missing/result.json'), 'utf8')).status, 'failure');
    await assert.rejects(runCommand('../escape', process.execPath, [], root), /Invalid/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
