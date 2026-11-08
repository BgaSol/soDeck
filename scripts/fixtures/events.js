// Browser fixtures have no native event bus. Keep real core status recovery,
// while satisfying Tauri's listener registration and cleanup contract.
export function installFixtureEvents() {
  const listeners = new Map();
  let sequence = 0;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event, id) => listeners.delete(id),
  };
  window.__TAURI_INTERNALS__.invoke = async (command, args) => {
    if (command === 'plugin:event|listen') {
      const id = ++sequence;
      listeners.set(id, args.handler);
      return id;
    }
    if (command === 'plugin:event|unlisten') return;
    return window.adapter(command, args);
  };
}
