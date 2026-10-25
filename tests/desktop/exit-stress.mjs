import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {root, launch, rpc, stop, until, pluginPages} from './harness.mjs';

// Native ownership regression: create/drop window contexts while async IPC
// responses clone/drop their handles. No mouse, external server or debugger.
const output = join(root, '.qa/t-004/exit-stress', String(Date.now()));
await mkdir(output, {recursive: true});
const runs = [];
let app;
try {
  for (let run = 0; run < 3; run++) {
    app = await launch({data: join(output, `run-${run}`), shellPort: 9513, pluginPort: 9514});
    const server = await rpc(app.page, 'servers.create', {
      name: '退出并发回归', host: '127.0.0.1', port: 22,
      username: 'fixture', authType: 'password',
    });
    await rpc(app.page, 'workspaces.open', {serverId: server.id});
    await until(async () => (await pluginPages(app)).length > 0);
    for (let cycle = 0; cycle < 10; cycle++) {
      const moved = await rpc(app.page, 'workspaces.move', {
        workspaceId: server.id, operationId: crypto.randomUUID(),
      });
      assert.equal(moved.committed, true);
      await Promise.all([
        rpc(app.page, 'windows.merge', {
          sourceWindowId: moved.targetWindowId, operationId: crypto.randomUUID(),
        }),
        app.page.evaluate(async () => {
          for (let batch = 0; batch < 10; batch++) {
            const replies = await Promise.all(Array.from({length: 32}, () =>
              window.__TAURI_INTERNALS__.invoke('rpc', {request: {
                apiVersion: 1, requestId: crypto.randomUUID(), method: 'app.info', params: {},
              }})));
            if (replies.some(reply => reply.error)) throw new Error('Concurrent query failed');
          }
        }),
      ]);
    }
    const capacity = await app.page.evaluate(async () => {
      let rejected = 0;
      for (let offset = 0; offset < 10050; offset += 32) {
        const replies = await Promise.all(Array.from({length: Math.min(32, 10050 - offset)}, (_, i) =>
          window.__TAURI_INTERNALS__.invoke('rpc', {request: {
            apiVersion: 1, requestId: `exit-stress-${offset + i}`, method: 'capacity.fixture', params: {},
          }})));
        for (const reply of replies) {
          if (!['METHOD_NOT_FOUND', 'CAPACITY'].includes(reply.error?.code)) {
            throw new Error(`Unexpected stress response: ${JSON.stringify(reply)}`);
          }
          if (reply.error.code === 'CAPACITY') rejected++;
        }
      }
      return rejected;
    });
    assert.ok(capacity > 0, 'Deduplication capacity must be reached');
    await rpc(app.page, 'workspaces.close', {workspaceId: server.id});
    const resources = await rpc(app.page, 'resources.list');
    for (const key of ['ptys', 'leases', 'connections']) assert.equal(resources[key].length, 0);
    await stop(app);
    runs.push({run, capacity, shutdown: app.shutdown});
    assert.equal(app.shutdown.forced, false, 'Exit must not use forced cleanup');
    assert.equal(app.process.exitCode, 0, 'IPC pressure must not corrupt native ownership');
    app = null;
    console.log(`Native exit stress ${run + 1}/3 passed`);
  }
  await writeFile(join(output, 'result.json'), JSON.stringify({passed: true, runs}, null, 2));
  console.log(JSON.stringify({passed: true, output}));
} catch (error) {
  await stop(app);
  await writeFile(join(output, 'result.json'), JSON.stringify({
    passed: false, runs, shutdown: app?.shutdown, error: error.message, stack: error.stack,
  }, null, 2));
  console.error(error);
  process.exitCode = 1;
} finally {
  await stop(app);
}
