import type {Catalog} from './server-api';
import {sharingApi, type ShareSummary} from './server-sharing-api';
import {sharingView} from './server-sharing-views';
import {ServerDialogs} from './server-dialogs';
import {updateDOM} from './dom-update';

const limit = 8 * 1024 * 1024;
const errorMessage = (error: unknown) => typeof error === 'object' && error && 'message' in error ? String(error.message) : String(error);
interface Session {dialog: HTMLDialogElement; importing: boolean; code: string; summary?: ShareSummary; busy: string; error: string; copied: boolean; generation: number; timer?: ReturnType<typeof setTimeout>}
export class ServerSharing {
  private current?: Session;
  constructor(private dialogs: ServerDialogs, private catalog: () => Catalog | undefined, private imported: (catalog: Catalog, ids: string[]) => void, private notify: (text: string) => void, private api = sharingApi) {}
  get submitting() {return this.current?.busy === 'import';}
  private render(s: Session) {if (this.current === s) updateDOM(s.dialog, sharingView(s.importing, s.code, s.summary, s.busy, s.error, s.copied));}
  private open(importing: boolean) {
    this.close();
    const dialog = this.dialogs.create(importing ? '导入连接' : '分享连接', 'endpoint-dialog share-dialog', () => this.close());
    const s: Session = {dialog, importing, code: '', busy: '', error: '', copied: false, generation: 0};
    this.current = s;
    dialog.addEventListener('input', event => {
      if (!(event.target instanceof HTMLTextAreaElement) || !s.importing || this.submitting) return;
      s.code = event.target.value; s.summary = undefined; s.error = ''; ++s.generation;
      clearTimeout(s.timer);
      this.render(s);
      s.timer = setTimeout(() => {void this.preview(s);}, 250);
    });
    dialog.addEventListener('click', event => {
      const action = (event.target as Element).closest<HTMLElement>('[data-share-action]')?.dataset.shareAction;
      if (action === 'close') this.close();
      else if (action === 'preview') void this.preview(s);
      else if (action === 'copy') void this.copy(s);
      else if (action === 'import') void this.commit(s);
    });
    this.render(s); this.dialogs.show(dialog);
    dialog.querySelector<HTMLTextAreaElement>('textarea')?.focus();
    return s;
  }
  import() {this.open(true);}
  async export(serverId?: string) {
    const catalog = this.catalog(); if (!catalog) return;
    const s = this.open(false); s.busy = 'export'; this.render(s);
    try {const result = await this.api.export(catalog.revision, serverId); if (this.current !== s) return; s.code = result.code; s.summary = result.summary;}
    catch (error) {if (this.current === s) s.error = errorMessage(error);}
    finally {s.busy = ''; this.render(s);}
  }
  private async preview(s: Session) {
    if (this.current !== s || this.submitting) return;
    clearTimeout(s.timer); const generation = ++s.generation; const code = s.code;
    s.summary = undefined; s.error = ''; s.busy = '';
    if (!code.trim()) {this.render(s); return;}
    if (new TextEncoder().encode(code).length > limit) {s.error = '分享内容超过 8 MiB，请改为逐台分享'; this.render(s); return;}
    s.busy = 'preview'; this.render(s);
    try {const summary = await this.api.preview(code); if (this.current !== s || generation !== s.generation) return; s.summary = summary;}
    catch (error) {if (this.current === s && generation === s.generation) s.error = errorMessage(error);}
    finally {if (generation === s.generation) {s.busy = ''; this.render(s);}}
  }
  catalogChanged() {
    const s = this.current;
    if (s?.importing && s.summary && !this.submitting && s.summary.catalogRevision !== this.catalog()?.revision) {
      s.summary = undefined; s.error = '目录已更新，请重新预览后再导入'; ++s.generation; this.render(s);
    }
  }
  private async copy(s: Session) {
    if (this.current !== s || s.busy || !s.code) return;
    s.busy = 'copy'; s.error = ''; this.render(s);
    try {await this.api.copy(s.code); s.copied = true;}
    catch (error) {s.error = `${errorMessage(error)}。可全选分享码后手动复制。`;}
    finally {s.busy = ''; this.render(s);}
  }
  private async commit(s: Session) {
    if (this.current !== s || s.busy || !s.summary) return;
    if (s.summary.catalogRevision !== this.catalog()?.revision) {this.catalogChanged(); return;}
    s.busy = 'import'; s.error = ''; this.render(s);
    try {
      const result = await this.api.import(s.summary.catalogRevision, s.code);
      if (this.current !== s) return;
      s.busy = ''; this.close(); this.imported(result.catalog, result.targetIds);
      this.notify(`已导入 ${result.summary.targetCount} 台目标服务器及所需中转资料`);
    } catch (error) {s.summary = undefined; s.error = `${errorMessage(error)}；如结果未能返回，请刷新目录核对，勿直接重复导入。`;}
    finally {s.busy = ''; this.render(s);}
  }
  close(force = false) {
    const s = this.current; if (!s || this.submitting && !force) return;
    this.current = undefined; ++s.generation; clearTimeout(s.timer); s.code = ''; s.summary = undefined;
    this.dialogs.dismiss(s.dialog);
  }
}
