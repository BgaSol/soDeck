import test from "node:test";
import assert from "node:assert/strict";
import { pluginPeer } from "./plugin-peer.mjs";

function fixture(body) {
  return pluginPeer(process.execPath, () => { throw new Error("Unexpected fixture request"); }, { timeout: 800, args: ["-e", `require('readline').createInterface({input:process.stdin}).on('line',line=>{const f=JSON.parse(line);${body}});`] });
}
test("shutdown observes immediate exit and rejects nonzero, missing reply and hanging processes", async () => {
  const good = fixture("console.log(JSON.stringify({id:f.id,result:{}}));if(f.method==='shutdown')process.exit(0);");
  await good.request("initialize"); await good.close();
  for (const body of ["process.exit(7);", "if(f.method==='shutdown'){console.log(JSON.stringify({id:f.id,result:{}}));setInterval(()=>{},100);}", "process.stdout.write('bad json\\n');"]) {
    const peer = fixture(body);
    await assert.rejects(peer.close());
    assert(peer.child.exitCode !== null || peer.child.signalCode !== null);
  }
});
