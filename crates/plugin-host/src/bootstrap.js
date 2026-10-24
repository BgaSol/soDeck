(() => {
  'use strict';
  const pending = new Map(), timers = new Map();
  let sequence = 0, timerSequence = 0, state = 'new';
  const send = frame => {
    if (!__nativeSend(JSON.stringify(frame))) throw new Error('Plugin wire queue is full');
  };
  const failure = (id, error) => send({kind:'result',id,error:{
    code: typeof error?.code === 'string' ? error.code : 'PLUGIN_ERROR',
    message: String(error?.message ?? error).slice(0, 2048),
    ...(error?.operationId ? {operationId:error.operationId}:{}),
    ...(error?.phase ? {phase:error.phase}:{}),
    ...(error?.details ? {details:error.details}:{})
  }});
  const success = (id, result) => send({kind:'result',id,result:result ?? null});
  globalThis.sodeck = Object.freeze({
    call(method, params = {}) {
      if (state === 'stopped') return Promise.reject({code:'PLUGIN_STOPPING',message:'Plugin is stopped'});
      if (typeof method !== 'string' || pending.size >= 32) return Promise.reject({code:'CAPACITY',message:'Plugin has too many pending calls'});
      const id = `core-${++sequence}`;
      return new Promise((resolve,reject) => {
        pending.set(id,{resolve,reject});
        try { send({kind:'core',id,method,params}); }
        catch(error) { pending.delete(id); reject(error); }
      });
    }
  });
  globalThis.setTimeout = (callback, delay = 0) => {
    if (typeof callback !== 'function' || timers.size >= 1024) throw new Error('Invalid or excessive timers');
    const id = ++timerSequence;
    timers.set(id,{callback,at:__nativeNow()+Math.max(0,Math.min(Number(delay)||0,2147483647))}); return id;
  };
  globalThis.clearTimeout = id => timers.delete(id);
  globalThis.__tick = () => {
    const now = __nativeNow();
    for (const [id,timer] of timers) if (timer.at <= now) { timers.delete(id); timer.callback(); }
  };
  // stdout is a private framed channel, never a console stream.
  globalThis.console = Object.freeze(Object.fromEntries(['log','info','warn','error','debug'].map(level=>[level,()=>{}])));
  globalThis.__receive = text => {
    const frame = JSON.parse(text);
    if (frame.kind === 'result') {
      const item = pending.get(frame.id); if (!item) return;
      pending.delete(frame.id); frame.error ? item.reject(frame.error) : item.resolve(frame.result); return;
    }
    (async () => {
      const plugin = globalThis.sodeckPlugin;
      if (!plugin || typeof plugin.invoke !== 'function') throw new Error('Worker must define sodeckPlugin.invoke');
      if (frame.kind === 'initialize') {
        if (state !== 'new') throw new Error('Plugin already initialized');
        state = 'preparing';
        await plugin.prepare?.(frame.context);
        state = 'activating';
        await plugin.activate?.(frame.context);
        state = 'active'; return {ready:true};
      }
      if (frame.kind === 'stop') {
        state = 'stopping'; await plugin.stop?.(); state = 'stopped'; return {stopped:true};
      }
      if (frame.kind !== 'invoke' || state !== 'active') throw new Error('Plugin is not active');
      return await plugin.invoke(frame.command, frame.params ?? {});
    })().then(value => success(frame.id,value),error => failure(frame.id,error));
  };
})();
