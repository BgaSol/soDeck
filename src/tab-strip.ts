import { icon } from './ui';
import { updateDOM } from './dom-update';

export const tabControls = (label: string) => `<div class="tab-controls" data-tauri-drag-region="false"><span class="tab-arrows" hidden><button class="sd-icon" data-tab-action="back" aria-label="向左滚动${label}">${icon('chevron-left')}</button><button class="sd-icon" data-tab-action="forward" aria-label="向右滚动${label}">${icon('chevron-right')}</button></span><button class="tab-picker-button sd-icon" data-tab-action="pick" aria-label="${label}列表" aria-haspopup="dialog" aria-expanded="false" hidden>${icon('list')}<span class="tab-count"></span></button></div>`;

/** Position only; selection and close lifecycles belong to the two existing hosts. */
export class TabStrip {
  private events = new AbortController();
  private observer: ResizeObserver;
  private motion = matchMedia('(prefers-reduced-motion: reduce)');
  private frame = 0;
  private target?: number;
  private lastFrame = 0;
  private lastWheel = 0;
  private directWheel = false;
  private active?: string | null;
  private activeVisible = false;
  private width = 0;
  private edge?: { direction: number; speed: number; since: number };
  private suspended = false;
  constructor(readonly viewport: HTMLElement, private controls: HTMLElement, private changed: () => void = () => {}) {
    this.observer = new ResizeObserver(() => this.measure());
    this.observer.observe(viewport); this.observer.observe(controls);
    const signal = this.events.signal;
    viewport.addEventListener('wheel', event => this.wheel(event), { passive: false, signal });
    viewport.addEventListener('scroll', () => { if(viewport.clientWidth===this.width)this.activeVisible = this.visible(this.active); this.buttons(); this.changed(); }, { passive: true, signal });
    controls.addEventListener('click', event => {
      const action = (event.target as Element).closest<HTMLElement>('[data-tab-action]')?.dataset.tabAction;
      if (action === 'back' || action === 'forward') this.scroll((action === 'back' ? -1 : 1) * Math.max(100, viewport.clientWidth * .75));
    }, { signal });
    this.motion.addEventListener('change', () => { if (this.target !== undefined) this.viewport.scrollLeft = this.target; this.stop(); }, { signal });
    document.addEventListener('visibilitychange', () => { if (document.hidden) this.stop(); }, { signal });
  }
  private max() { return Math.max(0, this.viewport.scrollWidth - this.viewport.clientWidth); }
  private clamp(value: number) { return Math.max(0, Math.min(this.max(), value)); }
  private tab(id?: string | null) { return id ? this.viewport.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(id)}"]`) : null; }
  private visible(id?: string | null) {
    const tab = this.tab(id); if (!tab) return false;
    const r = tab.getBoundingClientRect(), v = this.viewport.getBoundingClientRect();
    return r.left >= v.left - 1 && r.right <= v.right + 1;
  }
  update(active: string | null, reveal = true) {
    const previous = this.active; this.active = active;
    this.measure();
    if (previous !== active && reveal) this.reveal(active);
    this.activeVisible = this.visible(active);
  }
  private measure() {
    if (this.suspended || !this.viewport.isConnected || !this.viewport.clientWidth) { this.stop(); return; }
    const wasVisible = this.activeVisible;
    const arrows = this.controls.querySelector<HTMLElement>('.tab-arrows')!;
    const count = this.viewport.querySelectorAll('[data-tab-id]').length;
    this.controls.hidden = count === 0;
    const picker = this.controls.querySelector<HTMLButtonElement>('[data-tab-action="pick"]')!;
    picker.hidden = count === 0;
    const countNode = picker.querySelector('.tab-count')!;
    if (countNode.textContent !== String(count)) countNode.textContent = String(count);
    // Include the arrows' occupied width when deciding whether they are needed.
    const baseline = this.viewport.clientWidth + (arrows.hidden ? 0 : arrows.getBoundingClientRect().width);
    this.viewport.style.setProperty('--tab-width', `${Math.max(1, baseline - 4)}px`);
    const overflow = this.viewport.scrollWidth > baseline + 1;
    if (arrows.hidden === overflow) arrows.hidden = !overflow;
    const width = this.viewport.clientWidth;
    this.viewport.style.setProperty('--tab-width', `${Math.max(1, width - 4)}px`);
    if (this.target !== undefined) this.target = this.clamp(this.target);
    if (this.width && width !== this.width && wasVisible) this.reveal(this.active, false);
    this.width = width; this.activeVisible=this.visible(this.active); this.buttons();
  }
  private buttons() {
    const arrows = this.controls.querySelector<HTMLElement>('.tab-arrows'); if (!arrows) return;
    updateDOM(arrows, `<button class="sd-icon" data-tab-action="back" aria-label="向左滚动标签" ${this.viewport.scrollLeft <= 1 ? 'disabled' : ''}>${icon('chevron-left')}</button><button class="sd-icon" data-tab-action="forward" aria-label="向右滚动标签" ${this.viewport.scrollLeft >= this.max() - 1 ? 'disabled' : ''}>${icon('chevron-right')}</button>`);
  }
  scroll(distance: number, smooth = true) {
    this.edge = undefined;
    const current = this.viewport.scrollLeft;
    const base = this.target !== undefined && Math.sign(this.target - current) === Math.sign(distance) ? this.target : current;
    this.move(base + distance, smooth);
  }
  private move(value: number, smooth: boolean) {
    const target = this.clamp(value);
    if (!smooth || this.motion.matches) { this.stop(); this.viewport.scrollLeft = target; this.buttons(); return; }
    this.target = target; this.schedule();
  }
  reveal(id?: string | null, smooth = true) {
    const tab = this.tab(id); if (!tab || this.suspended || !this.viewport.clientWidth) return;
    const r = tab.getBoundingClientRect(), v = this.viewport.getBoundingClientRect();
    if (r.left < v.left) this.move(this.viewport.scrollLeft + r.left - v.left, smooth);
    else if (r.right > v.right) this.move(this.viewport.scrollLeft + r.right - v.right, smooth);
  }
  private wheel(event: WheelEvent) {
    if (event.ctrlKey || this.max() <= 1) return;
    if (event.deltaMode === WheelEvent.DOM_DELTA_PIXEL && (event.deltaX || event.shiftKey)) { this.stop(); return; }
    const now = performance.now(), delta = event.deltaY || event.deltaX;
    if (!delta) return;
    const gap = now - this.lastWheel;
    if (gap > 160) this.directWheel = false;
    // WheelEvent has no device identity; keep a continuous pixel gesture direct.
    if (event.deltaMode === 0 && (Math.abs(delta) < 40 || !Number.isInteger(delta) || gap < 40 && Math.abs(delta) < 80)) this.directWheel = true;
    this.lastWheel = now; event.preventDefault();
    const distance = delta * (event.deltaMode === 1 ? 32 : event.deltaMode === 2 ? this.viewport.clientWidth : 1);
    if (this.directWheel && event.deltaMode === 0) { this.stop(); this.viewport.scrollLeft = this.clamp(this.viewport.scrollLeft + distance); }
    else this.scroll(distance);
  }
  drag(point?: { x: number; y: number }) {
    if (!point || this.suspended) { this.edge = undefined; if (this.target === undefined) this.stop(); return; }
    const r = this.viewport.getBoundingClientRect();
    if (point.x < r.left || point.x >= r.right || point.y < r.top || point.y >= r.bottom) { this.drag(); return; }
    const band = Math.min(24, r.width / 3), left = point.x - r.left, right = r.right - point.x;
    const direction = left < band ? -1 : right < band ? 1 : 0;
    if (!direction || direction < 0 && this.viewport.scrollLeft <= 0 || direction > 0 && this.viewport.scrollLeft >= this.max()) { this.drag(); return; }
    this.target = undefined;
    this.edge = { direction, speed: 80 + 360 * (1 - Math.min(left, right) / band), since: this.edge?.direction === direction ? this.edge.since : performance.now() };
    this.schedule();
  }
  private schedule() { if (!this.frame && !this.suspended) { this.lastFrame = performance.now(); this.frame = requestAnimationFrame(time => this.tick(time)); } }
  private tick(time: number) {
    this.frame = 0;
    if (this.suspended || document.hidden || !this.viewport.isConnected || !this.viewport.clientWidth) { this.stop(); return; }
    const dt = Math.min(64, Math.max(1, time - this.lastFrame)); this.lastFrame = time;
    if (this.edge) {
      if (time - this.edge.since >= 120) {
        const current = this.viewport.scrollLeft;
        this.viewport.scrollLeft = this.clamp(current + this.edge.direction * this.edge.speed * dt / 1000);
        if (this.viewport.scrollLeft === current) this.edge = undefined;
      }
    } else if (this.target !== undefined) {
      this.target = this.clamp(this.target);
      const distance = this.target - this.viewport.scrollLeft;
      if (Math.abs(distance) <= 1) { this.viewport.scrollLeft = this.target; this.target = undefined; }
      else this.viewport.scrollLeft += distance * (1 - Math.exp(-dt / 40));
    }
    if (this.edge || this.target !== undefined) this.frame = requestAnimationFrame(next => this.tick(next));
    this.buttons();
  }
  stop() { cancelAnimationFrame(this.frame); this.frame = 0; this.target = undefined; this.edge = undefined; }
  pause(value: boolean) { this.suspended = value; if (value) this.stop(); else this.measure(); }
  dispose() { this.stop(); this.events.abort(); this.observer.disconnect(); }
}
