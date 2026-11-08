// Deliberately framework independent. The core binds the sender to a view and mount token.
(() => {
  let sequence = 0, surface, parentOrigin;
  const pending = new Map();
  window.sodeck = {
    request(method, params) {
      if (!surface) return Promise.reject(new Error('界面尚未挂载'));
      const id = String(++sequence);
      return new Promise((resolve,reject) => {
        const timeout = setTimeout(() => { pending.delete(id); reject(new Error('组件交互超时')); }, 8000);
        pending.set(id,{resolve,reject,timeout});
        parent.postMessage({channel:'sodeck-ui-v1',type:'request',surfaceId:surface,id,method,params},parentOrigin);
      });
    },
    mounted() { parent.postMessage({channel:'sodeck-ui-v1',type:'mounted',surfaceId:surface,flush:true},parentOrigin); },
    flushed(id,error) { parent.postMessage({channel:'sodeck-ui-v1',type:'flushed',surfaceId:surface,id,error},parentOrigin); }
  };
  addEventListener('message',event => {
    if(event.source!==parent || event.data?.channel!=='sodeck-ui-v1') return;
    const message=event.data;
    if(message.type==='mount') { surface=message.surfaceId; parentOrigin=event.origin==='null'?'*':event.origin; }
    if(message.surfaceId!==surface) return;
    if(message.type==='response') { const p=pending.get(message.id); if(!p)return; clearTimeout(p.timeout);pending.delete(message.id);message.error?p.reject(new Error(message.error)):p.resolve(message.data); }
    else dispatchEvent(new CustomEvent('sodeck:'+message.type,{detail:message}));
  });
  parent.postMessage({channel:'sodeck-ui-v1',type:'ready'},'*');
})();
