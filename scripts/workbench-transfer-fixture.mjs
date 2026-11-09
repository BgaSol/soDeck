// Real core transfer and shipped plugin UIs; browser pages replace native windows.
import {randomUUID} from 'node:crypto';

export async function transferWorkbench(call,{window,id,target,scope={kind:'host'}},{sourcePage,openTarget}) {
  const movement=await call('workbench_fixture_prepare',{window,id,target,scope,requestId:randomUUID()});
  const targetPage=await openTarget();
  const refresh=async()=>{await sourcePage.evaluate(()=>manager.refresh());await targetPage.evaluate(()=>manager.refresh());};
  const snapshot=()=>call('component_ui',{window:target,operation:'snapshot',args:{}});
  const prepared=(await snapshot()).transfers.find(t=>t.id===movement.token).viewIds;
  const deadline=Date.now()+20000;
  const wait=async predicate=>{while(!await predicate()){if(Date.now()>deadline)throw Error('Shipped plugin transfer did not settle');await refresh();await new Promise(resolve=>setTimeout(resolve,30));}};
  try {
    await refresh();
    await wait(()=>targetPage.evaluate(ids=>ids.every(id=>[...manager.components.shells.values()].some(shell=>shell.surfaces.get(id)?.mounted)),prepared));
    await call('workbench_fixture_drain',{movement});await refresh();
    await wait(async()=>{try {await call('workbench_fixture_commit',{movement});return true;}catch(error){if(!String(error).includes('目标页面尚未恢复'))throw error;return false;}});
    await refresh();return targetPage;
  } catch(error) {
    const state=await snapshot();console.error('Shipped plugin transfer',JSON.stringify(state.transfers));throw error;
  }
}
