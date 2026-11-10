import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { recordTrace } from './ui-trace.mjs';

test('UI trace is CI-only, saves failures once and discards successful traces', async () => {
  const original = process.env.SODECK_CI_ARTIFACTS;
  try {
    delete process.env.SODECK_CI_ARTIFACTS;
    await (await recordTrace({}, 'disabled'))(true);
    process.env.SODECK_CI_ARTIFACTS = 'artifacts/ci/test';
    for (const failed of [true, false]) {
      const calls = [];
      const finish = await recordTrace({ tracing: { start: async options => calls.push(options), stop: async options => calls.push(options) } }, 'workspace');
      await finish(failed); await finish(false);
      assert.equal(calls.length, 2);
      assert.deepEqual(calls[0], { screenshots: true, snapshots: true });
      assert.deepEqual(calls[1], failed ? { path: path.join('artifacts/ci/test', 'workspace-trace.zip') } : {});
    }
  } finally {
    if (original === undefined) delete process.env.SODECK_CI_ARTIFACTS;
    else process.env.SODECK_CI_ARTIFACTS = original;
  }
});
