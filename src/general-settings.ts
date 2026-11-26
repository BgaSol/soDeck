import { invoke } from '@tauri-apps/api/core';
import { core } from './core';

export interface StartupState {
  enabled: boolean;
  state: 'enabled' | 'disabled' | 'blocked' | 'stale' | 'unknown' | 'unavailable';
  reason: string | null;
}
interface PreferenceChange<T extends string> { saving: boolean; requested?: T; error: string }
export interface GeneralSettingsState {
  theme: PreferenceChange<string>;
  close: PreferenceChange<'background' | 'exit'>;
  startup: { status?: StartupState; reading: boolean; saving: boolean; requested: boolean; error: string; readError: string; feedback: string };
}

/** OS reads follow the visible general page; preference values remain owned by the core. */
export class GeneralSettings {
  readonly state: GeneralSettingsState = {
    theme: { saving: false, error: '' },
    close: { saving: false, error: '' },
    startup: { reading: false, saving: false, requested: false, error: '', readError: '', feedback: '' },
  };
  private active = false;
  private disposed = false;
  private revision = 0;
  private readAgain = false;
  constructor(private changed: () => void, private refreshApplication: () => Promise<unknown>) {
    window.addEventListener('focus', this.focus);
  }
  private focus = () => { void this.refresh(); };
  private notify() { if (this.active && !this.disposed) this.changed(); }
  mount(active: boolean) {
    if (this.disposed || active === this.active) return;
    this.active = active;
    if (active) void this.refresh();
    else { ++this.revision; this.readAgain = false; }
  }
  refresh = async () => {
    if (this.disposed || !this.active) return;
    const startup = this.state.startup;
    if (startup.reading || startup.saving) { this.readAgain = true; return; }
    startup.reading = true;
    const revision = this.revision;
    this.notify();
    try {
      const status = await invoke<StartupState>('login_startup_state');
      if (!this.disposed && this.active && revision === this.revision) { startup.status = status; startup.readError = ''; }
    } catch (error) {
      if (!this.disposed && this.active && revision === this.revision) { startup.readError = String(error); startup.status = undefined; }
    } finally {
      startup.reading = false;
      this.finishRead();
    }
  };
  private finishRead() {
    if (this.disposed || !this.active) return;
    if (this.readAgain) { this.readAgain = false; void this.refresh(); }
    else this.notify();
  }
  retry() { this.state.startup.error = ''; return this.refresh(); }
  setTheme(theme: string) { return this.savePreference(this.state.theme, theme, () => core.theme(theme)); }
  setCloseBehavior(value: string) {
    if (value !== 'background' && value !== 'exit') return Promise.resolve();
    return this.savePreference(this.state.close, value, () => core.closeBehavior(value));
  }
  private async savePreference<T extends string>(field: PreferenceChange<T>, value: T, write: () => Promise<void>) {
    if (this.disposed || !this.active || field.saving) return;
    field.saving = true; field.requested = value; field.error = ''; this.notify();
    try { await write(); }
    catch (error) { field.error = String(error); }
    finally {
      try { if (!this.disposed) await this.refreshApplication(); }
      finally { field.saving = false; delete field.requested; this.notify(); }
    }
  }
  updateLoginStartup() { return this.state.startup.status?.state === 'stale' ? this.setLoginStartup(true) : Promise.resolve(); }
  async setLoginStartup(enabled: boolean) {
    const startup = this.state.startup;
    if (this.disposed || !this.active || startup.saving || startup.reading || !startup.status || ['unknown', 'unavailable'].includes(startup.status.state)) return;
    startup.requested = enabled; startup.saving = true; startup.error = ''; startup.readError = ''; startup.feedback = '';
    ++this.revision; this.notify();
    try {
      const status = await invoke<StartupState>('set_login_startup', { enabled });
      if (!this.disposed) {
        startup.status = status;
        startup.feedback = status.state === 'enabled' ? '已开启，下次登录系统时生效。' : status.state === 'disabled' ? '已关闭，不影响当前运行。' : '';
      }
    } catch (error) {
      if (!this.disposed) {
        startup.error = String(error);
        // A failed write may still have changed OS registration; never infer the effective value.
        startup.status = undefined;
        if (this.active) {
          try { startup.status = await invoke<StartupState>('login_startup_state'); }
          catch (readError) { startup.readError = String(readError); }
        }
      }
    } finally { startup.saving = false; this.finishRead(); }
  }
  dispose() { this.disposed = true; ++this.revision; window.removeEventListener('focus', this.focus); }
}
