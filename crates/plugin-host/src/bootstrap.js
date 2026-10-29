(() => {
  'use strict';
  const pending = new Map(), timers = new Map(), listeners = new Set();
  let sequence = 0, timerSequence = 0, state = 'new';
  let binaryVersion = 0;
  let diagnosticContextVersion = 0, observationCreditBytes = 0;
  let consoleCounts={trace:0,debug:0,info:0,warn:0,error:0,fatal:0},consoleAt=0;
  const flushConsole=()=>{
    if(!Object.values(consoleCounts).some(n=>n>0)||pending.size>=31)return Promise.resolve();
    const counts=consoleCounts;consoleCounts={trace:0,debug:0,info:0,warn:0,error:0,fatal:0};
    return globalThis.sodeck.call('observability.console.summary',{...counts,contentNotCollected:true}).catch(()=>{for(const key of Object.keys(counts))consoleCounts[key]=Math.min(1000000,consoleCounts[key]+counts[key]);});
  };
  const binaryListeners = new Set();
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
    clock: Object.freeze({monotonicMs: () => __nativeNow()}),
    get diagnosticContextVersion() { return diagnosticContextVersion; },
    get observationCreditBytes() { return observationCreditBytes; },
    binary: Object.freeze({
      get version() { return binaryVersion; },
      send(bytes) { return binaryVersion === 1 && state !== 'stopped' && bytes instanceof Uint8Array && __nativeBinarySend(bytes); },
      onFrame(listener) {
        if (typeof listener !== 'function' || binaryListeners.size >= 8) throw new Error('Invalid binary listener');
        binaryListeners.add(listener); return () => binaryListeners.delete(listener);
      }
    }),
    onNotification(listener) {
      if (typeof listener !== 'function' || listeners.size >= 128) throw new Error('Invalid or excessive listeners');
      listeners.add(listener); return () => listeners.delete(listener);
    },
    call(method, params = {}, metadata) {
      if (state === 'stopped') return Promise.reject({code:'PLUGIN_STOPPING',message:'Plugin is stopped'});
      if (typeof method !== 'string' || pending.size >= 32) return Promise.reject({code:'CAPACITY',message:'Plugin has too many pending calls'});
      const id = `core-${++sequence}`;
      return new Promise((resolve,reject) => {
        pending.set(id,{resolve,reject});
        try { send({kind:'core',id,method,params,...(diagnosticContextVersion===1&&metadata!==undefined?{metadata}:{})}); }
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
    if(diagnosticContextVersion&&now-consoleAt>=10000){consoleAt=now;flushConsole();}
  };
  // stdout is a private framed channel, never a console stream.
  globalThis.console = Object.freeze(Object.fromEntries(['log','trace','info','warn','error','debug'].map(level=>[level,()=>{const key=level==='log'?'info':level;consoleCounts[key]=Math.min(1000000,consoleCounts[key]+1);}])));
  globalThis.__receive = text => {
    const frame = JSON.parse(text);
    if (frame.kind === 'notification') {
      if (state !== 'active') return;
      for (const listener of listeners) { try { listener({type:frame.type,payload:frame.payload}); } catch (_) {} }
      return;
    }
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
        binaryVersion = frame.binaryVersion === 1 ? 1 : 0;
        diagnosticContextVersion = frame.diagnosticContextVersion === 1 ? 1 : 0;
        observationCreditBytes = Math.min(262144,Math.max(0,Number(frame.context?.observationCreditBytes)||0));
        await plugin.prepare?.(frame.context);
        state = 'activating';
        await plugin.activate?.(frame.context);
        state = 'active'; return {ready:true,...(binaryVersion?{binaryVersion}:{}),...(diagnosticContextVersion?{diagnosticContextVersion}:{})};
      }
      if (frame.kind === 'stop') {
        state = 'stopping'; await plugin.stop?.(); await Promise.race([flushConsole(),new Promise(resolve=>setTimeout(resolve,250))]); listeners.clear(); binaryListeners.clear(); state = 'stopped'; return {stopped:true};
      }
      if (frame.kind !== 'invoke' || state !== 'active') throw new Error('Plugin is not active');
      const invocation=Object.freeze(frame.metadata?{metadata:Object.freeze({diagnostics:Object.freeze({...frame.metadata.diagnostics})})}:{});
      return await plugin.invoke(frame.command, frame.params ?? {}, invocation);
    })().then(value => success(frame.id,value),error => failure(frame.id,error));
  };
  globalThis.__receiveBinary = bytes => {
    if(state === 'stopped') return;
    if (binaryVersion !== 1 || !['active','stopping'].includes(state)) throw new Error('Unexpected binary stream');
    for (const listener of binaryListeners) listener(bytes);
  };
})();
