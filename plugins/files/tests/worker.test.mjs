import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
const bundle = await build({
  entryPoints: [fileURLToPath(new URL("../src/worker.ts", import.meta.url))],
  bundle: true,
  write: false,
  format: "iife",
  platform: "neutral",
  define: { __FILES_VERSION__: '"0.1.0"' },
});
const attr = {
  type: "file",
  size: "9007199254740993",
  mtime: null,
  atime: null,
  permissions: null,
  uid: null,
  gid: null,
};
async function fixture() {
  const records = [],
    timers = new Map();
  let sequence = 0;
  const core = {
    onNotification(){return ()=>{};},
    async call(method, p) {
      records.push({ method, p });
      switch (method) {
        case "storage.get":
          return { showHidden: true };
        case "storage.set":
          return { saved: true };
        case "servers.get":
          return { revision: 1 };
        case "connections.observe":
          return {
            state: "active",
            revision: 1,
            connection: { connectionId: "c", generation: 1 },
          };
        case "connections.acquireExisting":
          return { leaseId: "lease", connectionId: "c", generation: 1 };
        case "views.bindResources":
          return { bound: true, cursor:null };
        case "remote.operations.forget":
          return { forgotten: true };
        case "files.openDirectory":
          return { directoryId: "dir", cursor: 0 };
        case "files.stat":
          return { ...attr, type: "directory", size: null };
        case "files.readDirectory":
          return {
            entries: [
              {
                name: { bytesBase64: Buffer.from("huge").toString("base64") },
                attributes: attr,
              },
            ],
            nextCursor: 1,
            eof: true,
          };
        case "files.openRead":
          throw { code: "NOT_FOUND", message: "procfs unavailable" };
        case "connections.release":
          return { released: true };
        case "operations.cancel":
          return { canceled: true };
        case "files.closeDirectory":
        case "files.close":
        case "exec.close":
          return { closed: true };
        default:
          throw Error(method);
      }
    },
  };
  const context = {
    sodeck: core,
    console,
    setTimeout: (fn, ms) => {
      const id = ++sequence;
      timers.set(id, fn);
      return id;
    },
    clearTimeout: (id) => timers.delete(id),
  };
  vm.runInNewContext(bundle.outputFiles[0].text, context);
  const worker = context.sodeckPlugin;
  await worker.prepare({});
  await worker.activate({});
  return { worker, records, timers };
}
test("initialization does not read; visibility acquires a separate lease and procfs failure preserves files", async () => {
  const f = await fixture();
  await f.worker.invoke("lifecycle.initialize", {
    viewId: "v",
    workspaceId: "s",
    workspaceGeneration: "g",
    serverId: "s",
  });
  assert.equal(
    f.records.some((r) => r.method.startsWith("connections.")),
    false,
  );
  await f.worker.invoke("lifecycle.presentation", {
    viewId: "v",
    visible: true,
    revision: 1,
  });
  for (let i = 0; i < 100; i++) await new Promise(r=>setImmediate(r));
  const s = await f.worker.invoke("files.snapshot", { viewId: "v" });
  assert.ok(s.rows.some((r) => r.size === "9007199254740993"),JSON.stringify(s));
  assert.equal(s.mountError, "procfs unavailable");
  assert.equal(
    f.records.filter((r) => r.method === "connections.acquireExisting").length,
    1,
  );
  await f.worker.invoke("lifecycle.presentation", {
    viewId: "v",
    visible: false,
    revision: 2,
  });
  assert.equal(
    f.records.filter((r) => r.method === "connections.release").length,
    0,
  );
  await f.worker.invoke("lifecycle.close", { viewId: "v" });
  assert.equal(
    f.records.filter((r) => r.method === "connections.release").length,
    1,
  );
  await assert.rejects(
    f.worker.invoke("files.snapshot", { viewId: "v" }),
    (e) => e.code === "STALE_WORKSPACE",
  );
  await f.worker.stop();
});
