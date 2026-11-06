import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { loadFont, type FontValue, type FontPreset } from "./font-loader.js";
import { escape as e, button as b, icon } from "./ui";

export interface SettingsEntry { pluginId: string; pluginName: string; pageId: string; title: string; icon?: string; order: number; enabled: boolean; available: boolean; error?: string }
interface Field { key: string; type: string; title: string; description: string; default: unknown; apply: string; min?: number; max?: number; step?: number; unit?: string; maxLength?: number; options: {value: unknown; title: string}[]; presets: FontPreset[]; visibleWhen?: {key: string; equals: unknown} }
export interface SettingsSnapshot { pluginId: string; pageId: string; definitionToken: string; revision: string; version: string; values: Record<string, unknown>; savedValues: Record<string, unknown>; warnings: string[]; application: string; declaration: {title: string; groups: {id: string; title: string; fields: Field[]}[]} }
interface Draft { snapshot: SettingsSnapshot; values: Record<string, unknown>; dirty: boolean; conflict: boolean; saving: boolean; message: string; error: string; fontErrors: Record<string, string>; fontPending: Set<string>; generation: number; request?: {input: unknown; values: string} }
interface ApplicationReport {pluginId:string;pageId:string;definitionToken:string;revision:string;target:string;surfaceId?:string;error?:string}
export interface SettingsSnapshot { applications?: ApplicationReport[] }
export interface SettingsEntry { version: string }
const copy = <T>(value: T): T => structuredClone(value);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
export class ComponentSettings {
  entries: SettingsEntry[] = [];
  selected = "";
  private drafts = new Map<string, Draft>();
  private loading = new Map<string, Promise<void>>();
  private host?: HTMLElement;
  private active = "";
  private catalogPending = false;
  private catalogError = "";
  private applications = new Map<string, Map<string, string>>();
  private reports = new Map<string, ApplicationReport[]>();
  constructor(private render: () => void) {}
  async catalog() {
    if (this.catalogPending) return;
    this.catalogPending = true;
    try {
      const entries = await invoke<SettingsEntry[]>("settings_catalog");
      if (!same(entries, this.entries)) {
        const previous=this.entries;this.entries=entries;this.active="";this.render();
        for(const entry of entries){const key=`${entry.pluginId}/${entry.pageId}`;if(entry.available&&this.drafts.has(key)&&previous.find(p=>`${p.pluginId}/${p.pageId}`===key)?.version!==entry.version)void this.read(key);}
      }
      this.catalogError="";
    }
    catch (error) { this.catalogError = String(error); }
    finally { this.catalogPending = false; }
  }
  navigation() {
    return `<div class="nav-label">组件</div>${this.entries.length ? this.entries.map(p => {
      const key = `${p.pluginId}/${p.pageId}`, draft = this.drafts.get(key);
      return b("component-settings", `${e(p.title)}${draft?.dirty ? ' <span class="settings-draft-dot" title="未保存">●</span>' : ""}`, p.icon || "settings", `nav-item ${this.selected === key ? "active" : ""}`, `data-id="${e(key)}" aria-current="${this.selected === key ? "page" : "false"}"`);
    }).join("") : `<p class="hint settings-nav-hint">安装插件后可配置组件</p>`}`;
  }
  select(key: string) { this.selected = key; this.active = ""; this.render(); }
  mount(host: HTMLElement | null, visible: boolean) {
    this.host = visible ? host ?? undefined : undefined;
    if (!this.host) { this.active = ""; return; }
    if (this.active === this.selected && this.host.dataset.settingsKey === this.selected) return;
    this.active = this.selected;
    this.host.dataset.settingsKey = this.selected;
    const entry = this.entries.find(p => `${p.pluginId}/${p.pageId}` === this.selected);
    if (!entry?.available) { this.host.innerHTML = `<div class="section-heading"><div><h2>${e(entry?.title ?? "组件设置")}</h2><p>${e(entry?.error ?? this.catalogError ?? "请选择组件")}</p></div></div>`; return; }
    const draft = this.drafts.get(this.selected);
    if (draft) this.paint(draft);
    else { this.host.innerHTML = '<p class="hint" role="status">正在读取设置…</p>'; void this.read(this.selected); }
  }
  private async read(key: string, reload = false) {
    if (this.loading.has(key)) return this.loading.get(key);
    const entry = this.entries.find(p => `${p.pluginId}/${p.pageId}` === key); if (!entry?.available) return;
    const job = (async () => {
      try {
        const snapshot = await invoke<SettingsSnapshot>("settings_page", {pluginId: entry.pluginId, pageId: entry.pageId});
        const old = this.drafts.get(key);
        if (!old || reload || !old.dirty) this.drafts.set(key, {snapshot, values: copy(snapshot.values), dirty: false, conflict: false, saving: false, message: "", error: "", fontErrors: {}, fontPending: new Set(), generation: (old?.generation ?? 0) + 1});
        else if (old.snapshot.revision !== snapshot.revision || old.snapshot.definitionToken !== snapshot.definitionToken) old.conflict = true;
        if (this.selected === key && this.host) {
          if(old?.dirty && !reload)this.feedback(old);else this.paint(this.drafts.get(key)!);
        }
        for(const report of snapshot.applications ?? [])this.applied(report);
      } catch (error) { if (this.selected === key && this.host) this.host.innerHTML = `<p class="error-box" role="alert">${e(error)}</p>${b("reload-component-settings","重新读取",undefined,"quiet")}`; }
      finally { this.loading.delete(key); }
    })(); this.loading.set(key, job); return job;
  }
  private field(f: Field, d: Draft) {
    const value = d.values[f.key], id = `setting-${f.key}`, disabled = d.saving ? "disabled" : "";
    const attrs = `id="${id}" data-setting="${e(f.key)}" ${disabled}`;
    let control: string;
    if (f.type === "boolean") control = `<input ${attrs} type="checkbox" role="switch" ${value ? "checked" : ""}>`;
    else if (f.type === "number") control = `<div class="setting-number"><input ${attrs} type="number" value="${e(value)}" min="${f.min}" max="${f.max}" step="${f.step}"><span>${e(f.unit)}</span></div>`;
    else if (f.type === "select") control = `<select ${attrs}>${f.options.map((c,i) => `<option value="${i}" ${same(c.value,value) ? "selected" : ""}>${e(c.title)}</option>`).join("")}</select>`;
    else if (f.type === "font") {
      const font = value as FontValue;
      control = `<div class="setting-font"><select ${attrs}>${f.presets.map(p => `<option value="${e(p.id)}" ${font.source === "preset" && font.id === p.id ? "selected" : ""}>${e(p.title)}</option>`).join("")}<option value="local" ${font.source === "local" ? "selected" : ""}>本机字体</option></select><input data-local-font="${e(f.key)}" aria-label="本机字体名称" placeholder="例如 Cascadia Code" maxlength="128" value="${e(font.family)}" ${font.source === "local" ? "" : "hidden"} ${disabled}><p class="setting-font-sample" data-font-sample="${e(f.key)}">Aa 0O 1Il → ┌─┐ 中文</p><small data-font-status="${e(f.key)}" role="status"></small></div>`;
    } else control = `<input ${attrs} value="${e(value)}" maxlength="${f.maxLength ?? 1024}">`;
    return `<div class="setting-row" data-setting-row="${e(f.key)}" ${f.visibleWhen && !same(d.values[f.visibleWhen.key], f.visibleWhen.equals) ? "hidden" : ""}><div><label for="${id}">${e(f.title)}</label>${f.description ? `<p>${e(f.description)}</p>` : ""}</div>${control}</div>`;
  }
  private paint(d: Draft) {
    const host = this.host; if (!host) return;
    const entry = this.entries.find(p => `${p.pluginId}/${p.pageId}` === this.selected);
    host.innerHTML = `<div class="section-heading"><div><h2>${e(d.snapshot.declaration.title)}</h2><p>同类组件在所有服务器和窗口共享${entry?.enabled ? "" : " · 插件关闭时也可配置"}</p></div><span class="hint">${e(d.snapshot.version)}</span></div><form class="component-settings-form">${d.snapshot.declaration.groups.map(g => `<fieldset><legend>${e(g.title)}</legend>${g.fields.map(f => this.field(f,d)).join("")}</fieldset>`).join("")}<div class="settings-warnings">${d.snapshot.warnings.map(w => `<p>${e(w)}</p>`).join("")}</div><div class="settings-feedback" role="status"></div><div class="settings-save-bar">${b("defaults-component-settings","恢复默认",undefined,"text-button")}${b("discard-component-settings","放弃更改",undefined,"quiet")}${b("save-component-settings","保存",undefined,"")}</div></form>`;
    host.oninput = event => this.input(event, d);
    host.onchange = event => this.input(event, d);
    host.querySelector("form")!.onsubmit = event => {event.preventDefault(); void this.save();};
    d.snapshot.declaration.groups.flatMap(g => g.fields).filter(f => f.type === "font").forEach(f => void this.preview(f,d));
    this.feedback(d);
  }
  private input(event: Event, d: Draft) {
    const input = event.target as HTMLInputElement | HTMLSelectElement;
    const key = input.dataset.setting ?? input.dataset.localFont; if (!key || d.saving) return;
    const field = d.snapshot.declaration.groups.flatMap(g => g.fields).find(f => f.key === key)!;
    if (input.dataset.localFont) d.values[key] = {source: "local", family: input.value};
    else if (field.type === "boolean") d.values[key] = (input as HTMLInputElement).checked;
    else if (field.type === "number") d.values[key] = input.value === "" ? null : Number(input.value);
    else if (field.type === "select") d.values[key] = field.options[Number(input.value)].value;
    else if (field.type === "font") d.values[key] = input.value === "local" ? {source: "local", family: ""} : {source: "preset", id: input.value};
    else d.values[key] = input.value;
    d.dirty = !same(d.values,d.snapshot.values); d.message = ""; d.error = ""; d.request = undefined;
    if (field.type === "font") {
      const local = this.host?.querySelector<HTMLInputElement>(`[data-local-font="${CSS.escape(key)}"]`); if (local) {local.hidden = (d.values[key] as FontValue).source !== "local"; if (!input.dataset.localFont) local.value = "";}
      void this.preview(field,d);
    }
    for (const f of d.snapshot.declaration.groups.flatMap(g => g.fields)) if (f.visibleWhen) this.host?.querySelector<HTMLElement>(`[data-setting-row="${CSS.escape(f.key)}"]`)?.toggleAttribute("hidden", !same(d.values[f.visibleWhen.key],f.visibleWhen.equals));
    this.feedback(d); this.render();
  }
  private async preview(f: Field, d: Draft) {
    const value = copy(d.values[f.key] as FontValue), generation = d.generation;
    const valid = () => d.generation === generation && same(d.values[f.key],value);
    d.fontPending.add(f.key); delete d.fontErrors[f.key]; this.feedback(d);
    if(this.drafts.get(this.selected)===d){const status=this.host?.querySelector(`[data-font-status="${CSS.escape(f.key)}"]`);if(status)status.textContent="正在加载字体…";}
    try {
      const base = new URL(`${d.snapshot.pluginId}/settings/${d.snapshot.definitionToken}/`,convertFileSrc("","sodeck-plugin"));
      const family = await loadFont(value,f.presets,path => new URL(path,base).href);
      if (!valid()) return;
      const sample = this.host?.querySelector<HTMLElement>(`[data-font-sample="${CSS.escape(f.key)}"]`); if (this.drafts.get(this.selected) === d && sample) sample.style.fontFamily = family;
    } catch (error) { if (valid()) d.fontErrors[f.key] = String(error); }
    finally { if (valid()) {d.fontPending.delete(f.key); if (this.drafts.get(this.selected) === d) {const status = this.host?.querySelector(`[data-font-status="${CSS.escape(f.key)}"]`); if (status) status.textContent = d.fontErrors[f.key] ?? "字体已加载 · 预置字体离线可用"; this.feedback(d);} } }
  }
  private feedback(d: Draft) {
    if (!this.host || this.drafts.get(this.selected) !== d) return;
    const node = this.host.querySelector<HTMLElement>(".settings-feedback"); if (!node) return;
    node.classList.toggle("error-box",Boolean(d.error));
    node.innerHTML = d.conflict ? `其他窗口或插件版本已更新，草稿已保留。${b("reload-component-settings","载入最新值",undefined,"text-button")}` : e(d.error || d.message || (d.dirty ? "有未保存的更改" : d.snapshot.application === "newInstance" ? "用于之后新建的规则" : "保存后应用于组件"));
    if(d.error.startsWith("设置已保存，部分组件未应用"))node.insertAdjacentHTML("beforeend",b("retry-component-settings","重试应用",undefined,"text-button"));
    for(const input of this.host.querySelectorAll<HTMLInputElement|HTMLSelectElement>("input,select"))input.disabled=d.saving;
    const save = this.host.querySelector<HTMLButtonElement>('[data-action="save-component-settings"]')!;
    save.disabled = d.saving || !d.dirty || d.conflict || d.fontPending.size > 0 || Object.keys(d.fontErrors).some(key => !same(d.values[key],d.snapshot.values[key]));
    save.textContent = d.saving ? "保存中…" : "保存";
  }
  async action(action: string) {
    const d = this.drafts.get(this.selected);
    if (action === "reload-component-settings") {await this.read(this.selected,true); this.render(); return;}
    if (!d || d.saving) return;
    if (action === "save-component-settings") {await this.save(); return;}
    if (action === "retry-component-settings") {await invoke("retry_settings_apply",{pluginId:d.snapshot.pluginId,pageId:d.snapshot.pageId,revision:d.snapshot.revision}); d.message="设置已保存，正在重试应用…";this.feedback(d);return;}
    if (action === "defaults-component-settings" || action === "discard-component-settings") {
      d.values = action === "defaults-component-settings" ? Object.fromEntries(d.snapshot.declaration.groups.flatMap(g => g.fields).map(f => [f.key,copy(f.default)])) : copy(d.snapshot.values);
      d.dirty=!same(d.values,d.snapshot.values);d.generation++;d.fontErrors={};d.fontPending.clear();d.error="";d.message="";d.request=undefined;this.paint(d);this.render();
    }
  }
  private async save() {
    const key=this.selected,d=this.drafts.get(key);if(!d||d.saving||d.conflict||d.fontPending.size>0||Object.keys(d.fontErrors).some(k=>!same(d.values[k],d.snapshot.values[k]))||!this.host?.querySelector<HTMLFormElement>("form")?.reportValidity())return;
    const values=JSON.stringify(d.values);
    d.request ??= {values,input:{definitionToken:d.snapshot.definitionToken,baseRevision:d.snapshot.revision,requestId:crypto.randomUUID(),values:copy(d.values)}};
    d.saving=true;d.error="";this.feedback(d);
    try {
      const result=await invoke<{persisted:boolean;conflict?:boolean;snapshot:SettingsSnapshot}>("save_settings",{pluginId:d.snapshot.pluginId,pageId:d.snapshot.pageId,input:d.request.input});
      if(result.conflict){d.conflict=true;d.message="保存冲突，草稿已保留";return;}
      d.snapshot=result.snapshot;d.values=copy(result.snapshot.values);d.dirty=false;d.request=undefined;d.message=result.snapshot.application==="newInstance"?"已保存，用于新建规则":result.snapshot.application==="nextInstance"?"已保存，组件启动后使用":"设置已保存，正在应用…";
    }catch(error){d.error=String(error);}
    finally{d.saving=false;if(this.selected===key&&this.host)this.paint(d);for(const report of this.reports.get(key)??[])this.applied(report);this.render();}
  }
  changed(snapshot: SettingsSnapshot) {
    const key=`${snapshot.pluginId}/${snapshot.pageId}`, d=this.drafts.get(key);if(!d||d.saving)return;
    if(d.snapshot.definitionToken===snapshot.definitionToken&&BigInt(d.snapshot.revision)>=BigInt(snapshot.revision))return;
    void this.read(key);
  }
  applied(report: ApplicationReport) {
    const key=`${report.pluginId}/${report.pageId}`,d=this.drafts.get(key);
    const reports=this.reports.get(key)??[];
    if(!reports.includes(report)){reports.push(report);while(reports.length>64)reports.shift();this.reports.set(key,reports);}
    if(!d||d.snapshot.revision!==report.revision||d.snapshot.definitionToken!==report.definitionToken)return;
    const applicationKey=`${key}/${report.definitionToken}/${report.revision}`;
    let targets=this.applications.get(applicationKey);if(!targets){targets=new Map();this.applications.set(applicationKey,targets);while(this.applications.size>64)this.applications.delete(this.applications.keys().next().value!);}targets.set(report.surfaceId??report.target,report.error??"");
    const errors=[...targets.values()].filter(Boolean);
    if(errors.length){d.error=`设置已保存，部分组件未应用：${errors.join("；")}`;if(this.selected===key)this.feedback(d);}
    else{d.error="";d.message=d.snapshot.application==="newInstance"?"已保存，用于新建规则":[...targets.keys()].some(t=>t!=="backend")?"已保存，组件显示已更新": "设置已保存，等待可见组件应用";if(this.selected===key)this.feedback(d);}
  }
}
