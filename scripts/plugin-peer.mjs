import { spawn } from "node:child_process";
import readline from "node:readline";

// Test-only RPC peer. Register process completion before sending shutdown.
export function pluginPeer(executable, core, { args = [], timeout = 10000 } = {}) {
  const child = spawn(executable, args, { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  let sequence = 0, stderr = "", failure;
  const pending = new Map();
  const fail = error => { failure ??= error; for (const p of pending.values()) { clearTimeout(p.timer); p.reject(error); } pending.clear(); };
  child.stderr.on("data", data => { stderr = (stderr + data).slice(-16000); });
  child.stdin.on("error", fail);
  const exited = new Promise(resolve => {
    child.once("error", error => { fail(error); resolve({ error }); });
    child.once("close", (code, signal) => { fail(new Error(`Plugin exited ${code}/${signal}: ${stderr}`)); resolve({ code, signal }); });
  });
  const send = frame => child.stdin.write(JSON.stringify({ jsonrpc: "2.0", ...frame }) + "\n");
  const lines = readline.createInterface({ input: child.stdout });
  lines.on("line", async line => {
    try {
      const frame = JSON.parse(line);
      if (frame.method) {
        try { const result = await core(frame.method, frame.params); if (frame.id !== undefined) send({ id: frame.id, result }); }
        catch (error) { if (frame.id !== undefined) send({ id: frame.id, error: { code: error.code ?? -32000, message: error.message, data: error.data } }); }
      } else {
        const item = pending.get(frame.id);
        if (!item) return;
        pending.delete(frame.id); clearTimeout(item.timer);
        frame.error ? item.reject(new Error(frame.error.message)) : item.resolve(frame.result);
      }
    } catch (error) { fail(error); child.kill(); }
  });
  async function waitExit() {
    let timer;
    try { return await Promise.race([exited, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Plugin exit timed out")), timeout); })]); }
    finally { clearTimeout(timer); }
  }
  return {
    child,
    request(method, params = {}) {
      if (failure) return Promise.reject(failure);
      return new Promise((resolve, reject) => {
        const id = String(++sequence), timer = setTimeout(() => { pending.delete(id); reject(new Error(`RPC timeout: ${method}`)); }, timeout);
        pending.set(id, { resolve, reject, timer }); send({ id, method, params });
      });
    },
    async close() {
      try {
        if (child.exitCode !== null || child.signalCode !== null || failure) throw failure ?? new Error("Plugin exited before shutdown");
        await this.request("shutdown");
        const result = await waitExit();
        if (result.error || result.code !== 0 || result.signal) throw result.error ?? new Error(`Plugin shutdown failed: ${result.code}/${result.signal}`);
      } finally { if (child.exitCode === null && child.signalCode === null) { child.kill(); await waitExit(); } lines.close(); }
    },
    async kill() { if (child.exitCode === null && child.signalCode === null) child.kill(); await waitExit(); lines.close(); },
  };
}
