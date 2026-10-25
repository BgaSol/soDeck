import {rpc,pluginPages,pluginRpc,until} from './harness.mjs';

// Test setup uses registered commands. Authentication-specific tests drive the
// actual dialog; these helpers cancel the failed command before manual setup.
export async function openTerminal(page,serverId,operationId=crypto.randomUUID()){
  await rpc(page,'workspaces.open',{serverId});
  try{return await rpc(page,'commands.execute',{commandId:'terminal.new',workspaceId:serverId,operationId});}
  catch(error){if(['HOST_KEY_REQUIRED','HOST_KEY_CHANGED','AUTH_REQUIRED','AUTH_FAILED'].includes(error.code))await rpc(page,'workbench.operation.cancel',{operationId});throw error;}
}
export async function invokePlugin(app,command,args={}){
  const page=await until(async()=>{for(const page of await pluginPages(app)){try{if(await page.evaluate(()=>window.__SODECK_CONTEXT__?.viewType)==='terminal.session')return page;}catch{}}});
  return pluginRpc(page,'plugin.invoke',{command,args});
}
