// One client per Surface; resource ownership is always checked again by the core.
(() => {
  const channel = 'sodeck-ui-v2';
  let sequence = 0, surface, parentOrigin, released = false;
  const pending = new Map();
  class ApiError extends Error {
    constructor(error) {
      super(error.message);
      this.name = 'SoDeckApiError';
      this.code = error.code;
      this.data = error.data;
    }
  }
  const failure = (code, message, dispatch = 'notSent') => new ApiError({code, message, data: {dispatch}});
  const post = message => parent.postMessage({channel, surfaceId: surface, ...message}, parentOrigin);
  const closePending = () => {
    for (const request of pending.values()) { clearTimeout(request.timeout); request.reject(failure('surfaceClosed', '页面挂载已结束', 'unknown')); }
    pending.clear();
  };
  window.sodeck = {
    channel,
    request(method, params) {
      if (!surface || released) return Promise.reject(failure('surfaceClosed', '页面尚未挂载或已经结束'));
      if (pending.size >= 64) return Promise.reject(failure('resourceBusy', '页面请求队列已满'));
      const id = String(++sequence);
      return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => { pending.delete(id); reject(failure('requestTimeout', '组件交互超时，结果待确认', 'unknown')); }, method === 'data' ? 45000 : 8000);
        pending.set(id, {resolve, reject, timeout});
        try { post({type: 'request', id, method, params}); }
        catch { clearTimeout(timeout); pending.delete(id); reject(failure('transportUnavailable', '无法发送页面请求', 'unknown')); }
      });
    },
    prepared(flush = false, localFiles = false) { post({type: 'prepared', flush, localFiles}); },
    flushed(id, error) { post({type: 'flushed', id, ...(error ? {error: String(error.message ?? error)} : {})}); },
    interaction() { if (surface && !released) post({type: 'interaction'}); }
  };
  addEventListener('message', event => {
    if (event.source !== parent || event.data?.channel !== channel) return;
    const message = event.data;
    if (message.type === 'mount') {
      if (surface) closePending();
      surface = message.surfaceId;
      parentOrigin = event.origin === 'null' ? '*' : event.origin;
      released = false;
    }
    if (message.surfaceId !== surface || (parentOrigin !== '*' && event.origin !== parentOrigin)) return;
    if (message.type === 'response') {
      const request = pending.get(message.id);
      if (!request) return;
      clearTimeout(request.timeout); pending.delete(message.id);
      message.error ? request.reject(new ApiError(message.error)) : request.resolve(message.data);
      return;
    }
    if (message.type === 'unmount') { released = true; closePending(); }
    dispatchEvent(new CustomEvent('sodeck:' + message.type, {detail: message}));
  });
  const ready = () => parent.postMessage({channel, type: 'ready'}, '*');
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready, {once: true}); else ready();
})();
