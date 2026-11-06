import { autoUpdate, computePosition, flip, hide, offset, shift, size } from '@floating-ui/dom';

/** Positioning only: callers keep menu actions, targets and focus policy. */
export function attachMenuPosition(menu, reference, { kind = 'menu', isValid = () => true, onInvalid = () => {} } = {}) {
  const point = !(reference instanceof Element);
  const context = point ? reference.contextElement : reference;
  const anchor = point ? {
    contextElement: context,
    getBoundingClientRect: () => ({ x: reference.x, y: reference.y, left: reference.x, right: reference.x, top: reference.y, bottom: reference.y, width: 0, height: 0 }),
  } : reference;
  const cleanups = [];
  let disposed = false, sequence = 0, queued = false, settle;
  const ready = new Promise(resolve => { settle = resolve; });
  const alive = () => !disposed && menu.isConnected && !menu.hidden && context?.isConnected &&
    !context.closest('[hidden],[inert]') && context.getClientRects().length > 0 &&
    getComputedStyle(context).visibility === 'visible' && isValid();
  const dispose = () => {
    if (disposed) return;
    disposed = true; sequence++;
    for (const cleanup of cleanups.splice(0)) cleanup();
    settle(false);
  };
  const invalidate = () => { if (!disposed) { dispose(); onInvalid(); } };
  const addCleanup = cleanup => { if (disposed) cleanup(); else cleanups.push(cleanup); };
  Object.assign(menu.style, { position: 'fixed', visibility: 'hidden', minWidth: '0' });
  menu.style.removeProperty('max-width'); menu.style.removeProperty('max-height');
  async function update() {
    if (disposed) return;
    if (!alive()) { invalidate(); return; }
    const current = ++sequence;
    try {
      const result = await computePosition(anchor, menu, {
        strategy: 'fixed', placement: kind === 'submenu' ? 'right-start' : 'bottom-start',
        middleware: [
          offset(kind === 'submenu' ? 7 : point ? 0 : 4),
          flip({ padding: 8, rootBoundary: 'viewport', fallbackPlacements: kind === 'submenu' ? ['left-start', 'bottom-start', 'top-start'] : ['top-start'] }),
          shift({ padding: 8, rootBoundary: 'viewport' }),
          size({ padding: 8, rootBoundary: 'viewport', apply({ availableWidth, availableHeight }) {
            if (sequence !== current || !alive()) return;
            menu.style.maxWidth = `${Math.max(0, availableWidth)}px`;
            menu.style.maxHeight = `min(${Math.max(0, availableHeight)}px, var(--sd-menu-height-limit, 100dvh))`;
          } }),
          ...(point ? [] : [hide({ strategy: 'referenceHidden', rootBoundary: 'viewport' })]),
        ],
      });
      if (disposed || sequence !== current) return;
      if (!alive() || result.middlewareData.hide?.referenceHidden) { invalidate(); return; }
      Object.assign(menu.style, { left: `${result.x}px`, top: `${result.y}px`, visibility: 'visible' });
      menu.dataset.placement = result.placement;
      settle(true);
    } catch (error) {
      if (!disposed && sequence === current) { console.error('Menu positioning failed', error); invalidate(); }
    }
  }
  const schedule = () => {
    if (queued || disposed) return;
    queued = true;
    queueMicrotask(() => { queued = false; void update(); });
  };
  const mutations = new MutationObserver(records => {
    if (!alive()) { invalidate(); return; }
    // Our own position writes must not schedule another measurement. Ancestor
    // movement matters for nested submenus and transformed dialog containers.
    if (records.some(record => record.target !== menu && record.target instanceof Element && record.target.contains(context))) schedule();
  });
  mutations.observe(menu.ownerDocument.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ['hidden', 'inert', 'style', 'class'] });
  addCleanup(() => mutations.disconnect());
  addCleanup(autoUpdate(anchor, menu, schedule));
  // A resize can deliver a scroll event queued before this menu opened. Only
  // a subsequent change in the source's scroll chain invalidates a point.
  const scrollPositions = new Map();
  if (point) {
    for (let node = context; node; node = node.parentElement) scrollPositions.set(node, [node.scrollLeft, node.scrollTop]);
    const root = menu.ownerDocument.scrollingElement;
    if (root) scrollPositions.set(menu.ownerDocument, [root.scrollLeft, root.scrollTop]);
  }
  const scroll = event => {
    if (!point || menu.contains(event.target)) return;
    const before = scrollPositions.get(event.target);
    const node = event.target === menu.ownerDocument ? menu.ownerDocument.scrollingElement : event.target;
    if (before && (before[0] !== node.scrollLeft || before[1] !== node.scrollTop)) invalidate();
  };
  const window = menu.ownerDocument.defaultView;
  menu.ownerDocument.addEventListener('scroll', scroll, true);
  window.addEventListener('blur', invalidate);
  addCleanup(() => { menu.ownerDocument.removeEventListener('scroll', scroll, true); window.removeEventListener('blur', invalidate); });
  return { ready, update, dispose };
}

// Ordinary plugin scripts receive this same implementation in their bundle.
window.SoDeckMenuPosition = { attachMenuPosition };
