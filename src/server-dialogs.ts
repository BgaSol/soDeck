import {enterMotion, exitMotion} from './motion.js';

/** Native dialog lifecycle shared by directory editors and connection sharing. */
export class ServerDialogs {
  private entries: {dialog: HTMLDialogElement; source: Element | null}[] = [];
  create(label: string, size: string, cancel: () => void) {
    const dialog = document.createElement('dialog');
    dialog.id = `catalog-dialog-${crypto.randomUUID()}`;
    dialog.className = `catalog-dialog ${size}`;
    dialog.setAttribute('aria-label', label);
    dialog.addEventListener('cancel', event => {event.preventDefault(); cancel();});
    this.entries.push({dialog, source: document.activeElement});
    document.body.append(dialog);
    return dialog;
  }
  show(dialog: HTMLDialogElement) {dialog.showModal(); enterMotion(dialog);}
  dismiss(dialog: HTMLDialogElement, restore = true) {
    const index = this.entries.findIndex(entry => entry.dialog === dialog);
    if (index < 0) return;
    const [entry] = this.entries.splice(index, 1);
    exitMotion(dialog); dialog.close(); dialog.remove();
    if (restore && entry.source instanceof HTMLElement && entry.source.isConnected) entry.source.focus({preventScroll: true});
  }
  block(blocked: boolean) {for (const {dialog} of this.entries) dialog.inert = blocked;}
  dispose() {for (const {dialog} of [...this.entries].reverse()) this.dismiss(dialog, false);}
}
