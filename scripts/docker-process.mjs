export { pluginPeer } from "./plugin-peer.mjs";
import { testPluginDirectory } from "./test-plugin-directory.mjs";
export const context={workspaceId:'docker-test',serverId:'server',connectionId:'connection',generation:1};
export const capabilities=['commands.v1','session.exec.v1','ui.workbench.v2','ui.openView.v1','settings.v1'];
export function packageRoot() { return testPluginDirectory("docker"); }
export async function until(read, predicate, timeout=10000){
  const end=Date.now()+timeout;
  while(Date.now()<end){const value=await read();if(predicate(value))return value;await new Promise(resolve=>setTimeout(resolve,25));}
  throw new Error('Timed out waiting for plugin state');
}
