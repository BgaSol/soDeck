import { core, type Snapshot, type UpdateNetwork } from "./core";
import type { ViewState } from "./views";
import { button as b, escape as e } from "./ui";

const defaults = (): UpdateNetwork => ({ revision: 0, fallbackEnabled: true, disabledBuiltins: [], customMirrors: [] });
const saved = (s: Snapshot) => s.preferences.updateNetwork ?? defaults();
const draft = (s: Snapshot, v: ViewState) => v.networkDraft ??= structuredClone(saved(s));
function mirrorKeys(p: UpdateNetwork, v: ViewState) {
  if (!v.networkMirrorKeys || !v.networkDraft && v.networkMirrorKeys.revision !== p.revision) v.networkMirrorKeys = { revision: p.revision, keys: [] };
  while (v.networkMirrorKeys.keys.length < p.customMirrors.length) v.networkMirrorKeys.keys.push(crypto.randomUUID());
  return v.networkMirrorKeys.keys;
}
export function networkDirty(s: Snapshot, v: ViewState) {
  const comparable = (p: UpdateNetwork) => JSON.stringify({ fallback: p.fallbackEnabled, disabled: [...p.disabledBuiltins].sort(), mirrors: p.customMirrors });
  return !!v.networkDraft && comparable(v.networkDraft) !== comparable(saved(s));
}

export function updateNetworkPanel(s: Snapshot, v: ViewState) {
  const p = v.networkDraft ?? saved(s), saving = v.busy.has("network-save");
  const keys = mirrorKeys(p, v);
  const test = (id: string) => b("network-test", v.networkTesting === id ? "测试中…" : "测试", undefined, "text-button", `data-id="${e(id)}" id="network-test-${e(id)}" ${v.networkTesting ? "disabled" : ""}`);
  return `<details class="update-network" data-persist="update-network"><summary id="update-network-summary">下载与网络<span>${p.fallbackEnabled ? "自动切换备用地址" : "仅使用原始地址"}${v.networkDraft ? " · 未保存" : ""}</span></summary><div class="update-network-body"><p class="network-intro">优先访问 GitHub；连接失败后自动尝试备用地址。应用更新、插件列表和安装包都使用此设置。</p><label class="network-toggle"><input id="network-fallback" type="checkbox" ${p.fallbackEnabled ? "checked" : ""} ${saving ? "disabled" : ""}>GitHub 不通时使用加速地址</label><div class="network-direct"><span>GitHub 原始地址</span>${test("direct")}</div><fieldset ${saving ? "disabled" : ""}><legend>自定义地址</legend><p class="network-intro">按列表顺序尝试，近期可用的地址会优先使用。地址格式为 HTTPS 转发前缀。</p>${p.customMirrors.map((m, i) => `<div class="network-custom" data-mirror-index="${i}" data-dom-key="mirror:${keys[i]}"><label class="network-toggle"><input type="checkbox" id="mirror-enabled-${i}" data-dom-key="mirror:${keys[i]}:enabled" data-network-field="enabled" data-index="${i}" ${m.enabled ? "checked" : ""}>启用地址 ${i + 1}</label><label>名称<input id="mirror-name-${i}" data-dom-key="mirror:${keys[i]}:name" data-network-field="name" data-index="${i}" value="${e(m.name)}" maxlength="64" placeholder="例如：公司下载节点"></label><label class="network-prefix">加速地址<input id="mirror-prefix-${i}" data-dom-key="mirror:${keys[i]}:prefix" data-network-field="prefix" data-index="${i}" value="${e(m.prefix)}" maxlength="2048" spellcheck="false" placeholder="https://example.com/"></label><div class="network-row-actions">${test(`custom-${i}`)}${b("network-up", "上移", undefined, "text-button", `data-id="${i}" ${i === 0 ? "disabled" : ""}`)}${b("network-down", "下移", undefined, "text-button", `data-id="${i}" ${i === p.customMirrors.length - 1 ? "disabled" : ""}`)}${b("network-delete", "删除", undefined, "text-button", `data-id="${i}"`)}</div></div>`).join("") || '<p class="network-intro">尚未添加，默认使用下方内置地址。</p>'}${b("network-add", "添加地址", "plus", "quiet", `id="network-add" ${p.customMirrors.length >= 10 ? "disabled" : ""}`)}<p class="network-intro">最多 10 个；保存时会补齐末尾的斜线并检查重复地址。</p></fieldset><fieldset ${saving ? "disabled" : ""}><legend>内置地址</legend>${(s.updateMirrors ?? []).map(m => `<div class="network-builtin"><label class="network-toggle"><input id="mirror-builtin-${e(m.id)}" type="checkbox" data-builtin="${e(m.id)}" ${p.disabledBuiltins.includes(m.id) ? "" : "checked"}><span>${e(m.prefix)}${!m.supportsApi ? '<small>仅清单与安装包</small>' : ""}</span></label>${test(m.id)}</div>`).join("")}</fieldset>${v.networkResults ? `<div class="network-diagnostic" role="status"><b>测试结果：${e(v.networkResults.endpoint)}</b><p class="network-intro">仅测试该地址。首段读取成功不代表完整安装包已通过校验。</p><ul>${v.networkResults.results.map(r => `<li><span class="${r.ok ? "update-good" : "update-error"}">${r.ok ? "可访问" : "失败"}</span> · ${e(r.name)} · ${r.elapsedMs} ms${r.error ? `<p>${e(r.error)}</p>` : ""}</li>`).join("")}</ul></div>` : ""}${v.networkError ? `<p class="error-box" role="alert">${e(v.networkError)}</p>` : ""}${v.networkNotice ? `<p class="network-notice" role="status">${e(v.networkNotice)}</p>` : ""}<div class="network-save actions">${b("network-save", saving ? "保存中…" : "保存设置", undefined, "primary", `id="network-save" ${!v.networkDraft || saving ? "disabled" : ""}`)}${v.networkDraft ? b("network-reload", "重新读取已保存设置", undefined, "text-button", saving ? "disabled" : "") : ""}</div><p class="network-intro">保存后用于下一次请求；正在进行的任务继续使用原设置。第三方节点可能限流或缓存旧信息，可随时停用。</p></div></details>`;
}

export function networkInput(input: HTMLInputElement, s: Snapshot, v: ViewState) {
  if (input.id === "network-fallback") draft(s, v).fallbackEnabled = input.checked;
  else if (input.dataset.builtin) {
    const p = draft(s, v), id = input.dataset.builtin;
    p.disabledBuiltins = p.disabledBuiltins.filter(x => x !== id);
    if (!input.checked) p.disabledBuiltins.push(id);
  } else if (input.dataset.networkField) {
    const m = draft(s, v).customMirrors[Number(input.dataset.index)];
    if (!m) return false;
    if (input.dataset.networkField === "enabled") m.enabled = input.checked;
    else if (input.dataset.networkField === "name") m.name = input.value;
    else m.prefix = input.value;
  } else return false;
  v.networkGeneration = (v.networkGeneration ?? 0) + 1;
  v.networkResults = undefined;
  v.networkError = ""; v.networkNotice = "";
  if (!networkDirty(s, v)) v.networkDraft = undefined;
  return true;
}

export async function networkAction(action: string, id: string, s: Snapshot, v: ViewState, render: () => void) {
  if (!action.startsWith("network-")) return false;
  if (v.busy.has("network-save")) return true;
  v.networkError = ""; v.networkNotice = "";
  const index = Number(id);
  if (action === "network-test") {
    if (v.networkTesting) return true;
    const p = v.networkDraft ?? saved(s);
    const prefix = id === "direct" ? null : id.startsWith("custom-") ? p.customMirrors[Number(id.slice(7))]?.prefix : s.updateMirrors?.find(m => m.id === id)?.prefix;
    if (prefix === undefined) return true;
    const endpoint = prefix ?? "GitHub 原始地址";
    const generation = v.networkGeneration ?? 0;
    v.networkTesting = id; v.networkResults = undefined; render();
    try {
      const results = await core.testMirror(prefix);
      if (generation === (v.networkGeneration ?? 0)) v.networkResults = { endpoint, results };
    }
    catch (error) { if (generation === (v.networkGeneration ?? 0)) v.networkError = String(error); }
    finally { v.networkTesting = undefined; render(); }
  } else if (action === "network-save") {
    if (!v.networkDraft) return true;
    v.networkGeneration = (v.networkGeneration ?? 0) + 1;
    v.busy.add("network-save"); render();
    try {
      const preferences = await core.updateNetwork(structuredClone(v.networkDraft));
      s.preferences.updateNetwork = preferences;
      if (v.networkMirrorKeys) v.networkMirrorKeys.revision = preferences.revision;
      v.networkDraft = undefined;
      v.networkNotice = "已保存，新的更新任务将使用此设置。";
    } catch (error) { v.networkError = String(error); }
    finally { v.busy.delete("network-save"); }
  } else if (action === "network-reload") {
    v.networkGeneration = (v.networkGeneration ?? 0) + 1;
    v.networkDraft = undefined;
    v.networkMirrorKeys = undefined;
    v.networkResults = undefined;
  } else {
    v.networkGeneration = (v.networkGeneration ?? 0) + 1;
    const p = draft(s, v), mirrors = p.customMirrors, keys = mirrorKeys(p, v);
    if (action === "network-add" && mirrors.length < 10) mirrors.push({ name: "", prefix: "", enabled: true });
    else if (action === "network-delete" && mirrors[index]) { mirrors.splice(index, 1); keys.splice(index, 1); }
    else if ((action === "network-up" || action === "network-down") && mirrors[index]) {
      const destination = index + (action === "network-up" ? -1 : 1);
      if (mirrors[destination]) { [mirrors[index], mirrors[destination]] = [mirrors[destination], mirrors[index]]; [keys[index], keys[destination]] = [keys[destination], keys[index]]; }
    }
    v.networkResults = undefined;
  }
  return true;
}
