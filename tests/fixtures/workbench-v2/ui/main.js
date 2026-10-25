// Test fixture only. It contributes no terminal business names or SSH models.
const pending=new Map();let sequence=0;
const call=(method,params={})=>new Promise((resolve,reject)=>{
  const requestId=`fixture-${++sequence}`;pending.set(requestId,{resolve,reject});
  window.ipc.postMessage(JSON.stringify({apiVersion:1,requestId,method,params}));
});
window.__sodeckReceive=response=>{
  const item=pending.get(response.requestId);if(!item)return;pending.delete(response.requestId);
  response.error?item.reject(response.error):item.resolve(response.result);
};
window.__sodeckNotify=async notification=>{
  if(notification.type==='view.prepareMove'){
    await call('view.snapshotChunk',{operationId:notification.payload.operationId,index:0,total:1,data:JSON.stringify({schema:1,fixture:true})});
  }
};
void (async()=>{
  const state=await call('view.initialize');
  if(state.holdReady){window.__fixtureHeldReady=true;return;}
  await call('view.ready');
  document.getElementById('status').textContent=`夹具已就绪：${window.__SODECK_CONTEXT__.viewType}`;
})();
