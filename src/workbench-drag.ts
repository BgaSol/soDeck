export type Point = { x: number; y: number };
export type DropTarget = { kind: 'strip'; windowId: string; index: number } | { kind: 'group'; windowId: string; groupId: string; edge: string; index: number } | { kind: 'dock'; windowId: string; workspaceId: string; side: string; beforeToolKey?: string | null } | { kind: 'outside'; x: number; y: number } | { kind: 'invalid' };
export type DropPreview = DropTarget & { point?: Point | null };
export type DragEvent = { revision: number; token: string } & ({ kind: 'update'; target: DropTarget; point?: Point | null } | { kind: 'end' });
type Call = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export interface DragSource {
  workspaceId: string;
  scope: { kind: 'host' | 'view' | 'group' | 'tool'; id?: string };
  trigger: HTMLElement;
  label: string;
  threshold: number;
  valid: () => boolean;
  start?: () => void;
  prepare?: () => Promise<unknown>;
  confirm?: (token: string, operation: () => Promise<unknown>) => Promise<unknown>;
  local?: { preview: (point?: Point) => void; commit: () => Promise<unknown> };
}
interface Gesture {
  token: string;
  source: DragSource;
  pointer: number;
  origin: Point;
  point: Point;
  offset: Point;
  phase: 'pressed' | 'starting' | 'dragging' | 'finishing';
  starting?: Promise<unknown>;
  ending?: Promise<void>;
  timer?: ReturnType<typeof setTimeout>;
  ghost?: HTMLElement;
}

/** Owns only workbench drag input and preview lifetime; moving views stays with the core. */
export class WorkbenchDrag {
  private gesture?: Gesture;
  private events = new AbortController();
  private revision = 0;
  private previewToken?: string;
  private suppressedPointer?: number;
  private disposed = false;
  private visible = true;
  constructor(private call: Call, private notify: (message: string) => void, private render: (preview: DropPreview) => void) {
    const signal = this.events.signal;
    document.addEventListener('pointerdown', () => { this.suppressedPointer = undefined; }, { capture: true, signal });
    document.addEventListener('click', event => {
      if (event.detail && this.suppressedPointer === event.pointerId) {
        this.suppressedPointer = undefined; event.preventDefault(); event.stopImmediatePropagation();
      }
    }, { capture: true, signal });
    document.addEventListener('pointermove', this.move, { capture: true, signal });
    document.addEventListener('pointerup', event => {
      if (event.pointerId !== this.gesture?.pointer) return;
      this.gesture.point = { x: event.clientX, y: event.clientY };
      if (this.gesture.phase === 'dragging') this.gesture.source.local?.preview(this.gesture.point);
      void this.finish(false);
    }, { capture: true, signal });
    for (const name of ['pointercancel', 'lostpointercapture'] as const) document.addEventListener(name, event => {
      if (event.pointerId === this.gesture?.pointer && this.gesture.phase !== 'finishing') this.cancel();
    }, { capture: true, signal });
    document.addEventListener('keydown', event => {
      if (event.key === 'Escape' && this.gesture?.phase !== 'finishing' && this.gesture) { event.preventDefault(); this.cancel(); }
    }, { capture: true, signal });
    window.addEventListener('blur', () => this.cancel(), { signal });
    document.addEventListener('visibilitychange', () => { if (document.hidden) { this.cancel(); this.clearPreview(); } }, { signal });
    window.addEventListener('pagehide', () => this.dispose(), { signal });
  }
  get busy() { return Boolean(this.gesture); }
  get source() { return this.gesture?.source; }
  begin(event: PointerEvent, source: DragSource) {
    if (this.busy || this.disposed || !this.visible || event.button !== 0 || !event.isPrimary || !(event.buttons & 1) || !source.valid()) return;
    const rect = source.trigger.getBoundingClientRect();
    const g: Gesture = { token: crypto.randomUUID(), source, pointer: event.pointerId, origin: { x: event.clientX, y: event.clientY }, point: { x: event.clientX, y: event.clientY }, offset: { x: event.clientX - rect.left, y: event.clientY - rect.top }, phase: 'pressed' };
    this.gesture = g;
    try { source.trigger.setPointerCapture(g.pointer); } catch { this.cancel(); }
  }
  validate() {
    if (this.gesture && this.gesture.phase !== 'finishing' && (!this.gesture.source.trigger.isConnected || !this.gesture.source.valid())) this.cancel();
  }
  private move = (event: PointerEvent) => {
    const g = this.gesture;
    if (!g || g.pointer !== event.pointerId || g.phase === 'finishing') return;
    if (!(event.buttons & 1) || !event.isPrimary) { this.cancel(); return; }
    if (!g.source.trigger.isConnected || !g.source.valid()) { this.cancel(); return; }
    g.point = { x: event.clientX, y: event.clientY };
    if (g.phase === 'pressed' && Math.hypot(event.clientX - g.origin.x, event.clientY - g.origin.y) >= g.source.threshold) {
      event.preventDefault(); g.phase = 'starting'; this.clearPreview(); g.source.start?.();
      window.getSelection()?.removeAllRanges(); document.body.classList.add('workbench-dragging');
      const ghost = document.createElement('div'); ghost.className = g.source.scope.kind === 'host' ? 'workspace-drag-ghost' : 'workbench-drag-ghost'; ghost.textContent = g.source.label; document.body.append(ghost); g.ghost = ghost;
      if (g.source.local) g.phase = 'dragging';
      else {
        g.starting = this.call('workspace_drag_start', { token: g.token, id: g.source.workspaceId, scope: g.source.scope, offsetX: g.offset.x, offsetY: g.offset.y });
        void g.starting.then(() => { if (g.phase === 'starting') { g.phase = 'dragging'; void this.sample(g); } }).catch(() => { if (g.phase !== 'finishing') void this.cancel(); });
      }
    }
    if (g.ghost) { event.preventDefault(); g.ghost.style.left = `${event.clientX + 14}px`; g.ghost.style.top = `${event.clientY + 14}px`; }
    if (g.phase === 'dragging') g.source.local?.preview(g.point);
  };
  private async sample(g: Gesture) {
    if (g.phase !== 'dragging' || this.disposed) return;
    try { await this.call('workspace_drag_sample', { token: g.token, localPoint: g.point }); }
    catch (error) { if (g.phase === 'dragging') { this.notify(String(error)); this.cancel(); } }
    if (g.phase === 'dragging' && !this.disposed) g.timer = setTimeout(() => void this.sample(g), 32);
  }
  cancel() { return this.finish(true); }
  private finish(cancel: boolean): Promise<void> {
    const g = this.gesture;
    if (!g) return Promise.resolve();
    if (g.ending) return g.ending;
    const started = g.phase !== 'pressed'; g.phase = 'finishing';
    clearTimeout(g.timer); g.ghost?.remove(); document.body.classList.remove('workbench-dragging');
    g.source.local?.preview(); this.clearPreview();
    if (started) this.suppressedPointer = g.pointer;
    if (g.source.trigger.hasPointerCapture(g.pointer)) g.source.trigger.releasePointerCapture(g.pointer);
    const point = { ...g.point };
    g.ending = (async () => {
      try {
        if (!started) return;
        if (g.source.local) { if (!cancel && g.source.valid()) await g.source.local.commit(); return; }
        await g.starting;
        if (cancel || !g.source.valid() || this.disposed) return;
        const released = await this.call<DragEvent | null>('workspace_drag_release', { token: g.token, localPoint: point });
        if (released) this.receive(released);
        await g.source.prepare?.();
        if (!g.source.valid() || this.disposed) return;
        const operation = async () => { const end = await this.call<DragEvent | null>('workspace_drag_finish', { token: g.token, cancel: false }); if (end) this.receive(end); };
        if (g.source.confirm) await g.source.confirm(g.token, operation); else await operation();
      } catch (error) { this.notify(String(error)); }
      finally {
        if (started && !g.source.local) await this.call<DragEvent | null>('workspace_drag_finish', { token: g.token, cancel: true }).then(end => { if (end) this.receive(end); }).catch(error => { if (!this.disposed) this.notify(`拖拽清理失败：${String(error)}`); });
        if (this.gesture === g) this.gesture = undefined;
      }
    })();
    return g.ending;
  }
  receive(event: DragEvent) {
    if (this.disposed || event.revision <= this.revision) return;
    this.revision = event.revision;
    if (event.kind === 'end') { if (this.previewToken === event.token) this.clearPreview(); return; }
    if (!this.visible || document.hidden || this.gesture?.source.local || this.gesture?.token === event.token && this.gesture.phase === 'finishing') return;
    this.previewToken = event.token; this.render({ ...event.target, point: event.point });
  }
  private clearPreview() { this.previewToken = undefined; this.render({ kind: 'invalid' }); }
  setVisible(visible: boolean) { this.visible = visible; if (!visible) { void this.cancel(); this.clearPreview(); } }
  dispose() { if (this.disposed) return; this.disposed = true; void this.cancel(); this.clearPreview(); this.events.abort(); }
}
