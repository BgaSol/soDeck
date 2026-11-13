import { escape as e, icon } from './ui';
import { DomRegion } from './dom-update';
import { attachMenuPosition, type MenuPosition } from './menu-position.js';

export interface TabRecord { id: string; title: string; detail: string; icon?: string; current?: boolean; status?: string; disabled?: boolean; closeLabel?: string }
export class TabPicker {
  private static open?: TabPicker;
  private node = document.createElement('div');
  private input = document.createElement('input');
  private list = document.createElement('div');
  private events = new AbortController();
  private region = new DomRegion(this.node, () => this.update());
  private position: MenuPosition;
  private busy = new Set<string>();
  constructor(private anchor: HTMLElement, label: string, private records: () => TabRecord[], private select: (id: string) => Promise<unknown>, private close: (id: string) => Promise<unknown>, valid: () => boolean, private notify: (error: string) => void) {
    TabPicker.open?.dispose(); TabPicker.open = this;
    document.dispatchEvent(new Event('tab-picker-open'));
    this.node.className = 'tab-picker sd-menu'; this.node.setAttribute('role', 'dialog'); this.node.setAttribute('aria-label', label);
    this.input.type = 'search'; this.input.placeholder = `搜索${label}`; this.input.setAttribute('aria-label', `搜索${label}`);
    this.list.className = 'tab-picker-list'; this.node.append(this.input, this.list); document.body.append(this.node);
    anchor.setAttribute('aria-expanded', 'true');
    const signal = this.events.signal;
    this.input.addEventListener('input', () => this.update(), { signal });
    this.list.addEventListener('click', event => {
      const button = (event.target as Element).closest<HTMLElement>('[data-tab-picker-action]');
      if (button) void this.action(button.dataset.tabPickerAction!, button.dataset.key!);
    }, { signal });
    this.node.addEventListener('keydown', event => {
      if (event.isComposing) return;
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); this.dispose(true); return; }
      const buttons = [...this.list.querySelectorAll<HTMLButtonElement>('[data-tab-picker-action="select"]:not(:disabled)')];
      if (event.key === 'Enter' && event.target === this.input) { event.preventDefault(); buttons[0]?.click(); }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || event.target === this.input && ['Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const index = buttons.indexOf((document.activeElement as Element)?.closest('.tab-picker-row')?.querySelector<HTMLButtonElement>('[data-tab-picker-action="select"]') ?? document.activeElement as HTMLButtonElement);
      buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : index < 0 ? event.key === 'ArrowUp' ? buttons.length - 1 : 0 : (index + (event.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    }, { signal });
    document.addEventListener('pointerdown', event => { if (!this.node.contains(event.target as Node) && !anchor.contains(event.target as Node)) this.dispose(); }, { signal });
    document.addEventListener('focusin', event => { if (!this.node.contains(event.target as Node) && !anchor.contains(event.target as Node)) this.dispose(); }, { signal });
    window.addEventListener('blur', () => this.dispose(), { signal });
    this.position = attachMenuPosition(this.node, anchor, { isValid: () => TabPicker.open === this && valid(), onInvalid: () => this.dispose() });
    this.update(); void this.position.ready.then(ready => { if (ready && TabPicker.open === this) this.input.focus(); });
  }
  update() {
    if (!this.node.isConnected) return;
    this.anchor.setAttribute('aria-expanded','true');
    const query = this.input.value.trim().toLocaleLowerCase(), rows = this.records().filter(r => `${r.title}\n${r.detail}`.toLocaleLowerCase().includes(query));
    const focus = document.activeElement as HTMLElement, oldRows = [...this.list.children], oldIndex = oldRows.indexOf(focus.closest('.tab-picker-row')!);
    this.region.update(this.list, rows.map(r => `<div class="tab-picker-row" data-tab-id="${e(r.id)}"><button data-tab-picker-action="select" data-key="${e(r.id)}" aria-current="${r.current ? 'page' : 'false'}" ${r.disabled || this.busy.has(r.id) ? 'disabled' : ''}>${icon(r.icon || 'file')}<span class="tab-picker-record"><strong>${e(r.title)}</strong><span title="${e(r.detail)}">${e(r.detail)}</span>${r.status ? `<span>${e(r.status)}</span>` : ''}</span>${r.current ? icon('check') : ''}</button>${r.closeLabel ? `<button class="sd-icon" data-tab-picker-action="close" data-key="${e(r.id)}" aria-label="${e(r.closeLabel)} ${e(r.title)}" ${r.disabled || this.busy.has(r.id) ? 'disabled' : ''}>${icon('close')}</button>` : ''}</div>`).join('') || '<p class="tab-picker-empty" role="status">没有匹配的标签</p>');
    if (oldIndex >= 0 && (!focus.isConnected || focus.matches(':disabled'))) (this.list.children[Math.min(oldIndex, this.list.children.length - 1)]?.querySelector<HTMLElement>('button:not(:disabled)') ?? this.input).focus({ preventScroll: true });
    void this.position?.update();
  }
  private async action(action: string, id: string) {
    const record = this.records().find(r => r.id === id); if (!record || record.disabled || this.busy.has(id)) return;
    if (action === 'select') { this.dispose(); try { await this.select(id); } catch (error) { this.notify(String(error)); } return; }
    this.busy.add(id); this.update();
    try { await this.close(id); } catch (error) { this.notify(String(error)); }
    finally { this.busy.delete(id); this.update(); }
  }
  dispose(focus = false) { this.region.dispose(); this.position?.dispose(); this.events.abort(); this.node.remove(); this.anchor.setAttribute('aria-expanded', 'false'); if (TabPicker.open === this) TabPicker.open = undefined; if (focus && this.anchor.isConnected) this.anchor.focus({ preventScroll: true }); }
}
