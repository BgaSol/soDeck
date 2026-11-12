import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Workspaces, type DropTarget } from "./workspaces";
import { core, type Snapshot, type Message } from "./core";
import * as views from "./views";
import { selectedUpdates, updateCount, CORE_ID, renderUpdatesSettings } from "./updates";
import { networkAction, networkInput } from "./update-network";
import { button as b, escape as e, icon, replace } from "./ui";
import { parseLocalTime } from "./datetime";
import "./style.css";
import "./text-menu.js";
import { ServerManager } from "./servers";
import { ConnectionsUi } from "./connections";
import { ServerPicker } from "./server-picker";
import { ComponentSettings, type SettingsSnapshot } from "./settings";

const win = getCurrentWindow();
await invoke("close_register_host");
window.installTextMenu({
  read: async valid => { const text = await invoke<string>("clipboard_text", { operation: "read" }); return valid() ? text : null; },
  write: async (text, valid) => { if (!valid()) return false; await invoke("clipboard_text", { operation: "write", text }); return valid(); },
  notify: error => toast(error, undefined, true),
});
const root = document.querySelector<HTMLDivElement>("#app")!;
root.innerHTML = `<header class="topbar" data-tauri-drag-region="deep"><button id="catalog-tab" role="tab" aria-selected="true">${icon("grid")}<span>服务器管理</span></button><div class="server-tabs" id="server-tabs" role="tablist" aria-label="服务器工作区"></div><button id="new-workspace" class="sd-icon" aria-label="选择服务器" title="选择服务器" aria-haspopup="dialog" aria-expanded="false">${icon("plus")}</button><div id="drag-region" class="drag-region" aria-hidden="true"></div><nav class="global-tools" aria-label="应用工具"><button data-action="status" id="status-button" aria-label="应用状态" aria-expanded="false"><span class="health-dot"></span><span id="health">正在启动</span></button>${b("messages", '<span id="message-count">消息</span>', "bell", "icon-label", 'id="messages-button" aria-label="消息中心"')}${b("settings", "设置", "settings", "icon-label", 'id="settings-button"')}</nav><div id="window-controls" class="window-controls">${b("minimize", "", "minus", "", 'aria-label="最小化"')}${b("maximize", "", "maximize", "", 'id="maximize-button" aria-label="最大化或还原"')}${b("close-window", "", "close", "close-window", 'aria-label="关闭窗口"')}</div></header><main class="workspace" id="server-workspace"><div id="server-catalog"></div><div id="workspace-content" hidden></div></main><div id="overlay" hidden></div><div id="toasts" class="toasts" aria-live="polite"></div>`;
const servers = win.label === "main" ? new ServerManager(document.querySelector<HTMLElement>("#server-catalog")!, text => toast(text)) : undefined;
const workspaces = new Workspaces(document.querySelector("#server-tabs")!, document.querySelector("#workspace-content")!, document.querySelector("#server-catalog")!, text => toast(text, undefined, true));
const connections = new ConnectionsUi(win.label, views => { servers?.setConnections(views); picker.setConnections(views); }, text => toast(text, undefined, true));
const picker = new ServerPicker(document.querySelector<HTMLButtonElement>("#new-workspace")!, id => connections.connect(id), text => toast(text, undefined, true));
async function restart(install: boolean) {
  if (!servers) { await invoke("request_main_action", { action: install ? "install" : "restart" }); return; }
  await workspaces.restart(install);
}
const overlay = document.querySelector<HTMLDivElement>("#overlay")!;
const componentSettings = new ComponentSettings(() => { if (layer === "settings") renderLayer(); });
workspaces.setCloseParticipants(async (decision,operationId)=>[...await (servers?.closeInputs(decision)??Promise.resolve([])),...await componentSettings.closeInputs(decision,operationId)],blocked=>servers?.setCloseBlocked(blocked));
let snapshot: Snapshot | undefined;
let layer = "";
let returnFocus: HTMLElement | null = null;
let lastError = "";
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
let refreshing = false;
let refreshAgain = false;
let logGeneration = 0;
const state: views.ViewState = {
  officialPlugins: [],
  officialFetchedAt: 0,
  officialLoading: false,
  officialError: "",
  section: "appearance",
  plugin: "",
  messages: [],
  unread: false,
  selectedMessage: "",
  logs: [],
  filter: { pluginId: "", level: "", query: "" },
  paused: false,
  error: "",
  busy: new Set(),
  updates: {},
  updateOverview: { checking: false, busy: false, lastCheckedAt: 0, nextCheckAt: 0 },
  excludedUpdates: new Set(),
  pluginTab: null,
  sourceDrafts: {},
};

function applyTheme() {
  const theme = snapshot?.preferences.theme ?? "system";
  document.documentElement.dataset.theme =
    theme === "system"
      ? matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;
}
matchMedia("(prefers-color-scheme: dark)").addEventListener(
  "change",
  applyTheme,
);

function renderTop() {
  const count = snapshot ? updateCount(snapshot, state) : 0;
  const pending = snapshot?.restartRequired || state.updates[CORE_ID]?.phase === "ready";
  const settingsButton = document.querySelector<HTMLButtonElement>("#settings-button")!;
  settingsButton.innerHTML = `${icon("settings")}设置${count ? `<span class="settings-update-count">${count}</span>` : pending ? '<span class="settings-update-pending">待重启</span>' : ""}`;
  const label = snapshot ? views.health(snapshot) : "状态不可用";
  document.querySelector("#health")!.textContent = label;
  document.querySelector("#status-button .health-dot")!.className =
    `health-dot ${label === "正常" ? "good" : "warning"}`;
  document.querySelector("#message-count")!.textContent = snapshot?.unreadCount
    ? `消息 ${snapshot.unreadCount > 99 ? "99+" : snapshot.unreadCount}`
    : "消息";
  if (snapshot) {
    document.body.classList.toggle("macos", snapshot.os === "macos");
    document.querySelector<HTMLElement>("#window-controls")!.hidden =
      snapshot.os === "macos";
  }
}

function content() {
  if (!snapshot)
    return `<div class="error-box">无法读取应用状态：${e(lastError)}<br>${b("refresh", "重新读取")}</div>`;
  if (layer === "status") return views.status(snapshot);
  if (layer === "settings") return views.settings(snapshot, state, componentSettings.navigation(state.section === "components"));
  if (layer === "logs") return views.logs(snapshot, state);
  return views.messages(state);
}
function renderLayer() {
  const target = document.querySelector<HTMLElement>("#layer-content");
  if (target) {
    const hadPluginDetails = !!target.querySelector("#back-plugins");
    const retained = layer === "settings" && state.section === "components" ? target.querySelector<HTMLElement>("#component-settings-page") : null;
    const retainedFocus = retained?.contains(document.activeElement) ? document.activeElement as HTMLElement : null;
    const expanded=[...target.querySelectorAll<HTMLDetailsElement>('details[data-network-server][open]')].map(d=>d.dataset.networkServer);
    if (layer === "settings" && state.section === "updates") renderUpdatesSettings(target, content());
    else replace(target, content());
    const placeholder = target.querySelector<HTMLElement>("#component-settings-page");
    if (retained && placeholder && retained !== placeholder) placeholder.replaceWith(retained);
    if (retainedFocus?.isConnected) retainedFocus.focus({preventScroll: true});
    if (!hadPluginDetails && document.activeElement === document.body)
      target.querySelector<HTMLElement>("#back-plugins")?.focus({preventScroll: true});
    componentSettings.mount(target.querySelector("#component-settings-page"), layer === "settings" && state.section === "components");
    for(const key of expanded)target.querySelector<HTMLDetailsElement>(`details[data-network-server="${CSS.escape(key!)}"]`)?.setAttribute('open','');
  }
}
async function showLayer(next: string) {
  if (layer === next && next === "status") {
    closeLayer();
    return;
  }
  if (!layer) returnFocus = document.activeElement as HTMLElement;
  layer = next;
  overlay.hidden = false;
  overlay.className = next === "status" ? "popover-backdrop" : "modal-backdrop";
  const title = (
    {
      status: "应用状态",
      settings: "设置",
      logs: "应用日志",
      messages: "消息中心",
    } as Record<string, string>
  )[next];
  overlay.innerHTML = `<section class="layer ${next === "status" ? "status-popover" : next === "settings" ? "settings-dialog" : "wide-dialog"}" role="dialog" aria-modal="${next !== "status"}" aria-labelledby="layer-title"><header class="layer-heading"><h2 id="layer-title">${title}</h2>${b("close-layer", "", "close", "icon-button", 'aria-label="关闭"')}</header><div id="layer-content" class="layer-content ${next}-content"></div></section>`;
  document.querySelector<HTMLElement>(".workspace")!.inert = next !== "status";
  document
    .querySelector("#status-button")!
    .setAttribute("aria-expanded", String(next === "status"));
  renderLayer();
  overlay.querySelector<HTMLElement>("button, input, select")?.focus();
  if (next === "messages") await refreshMessages();
  if (next === "logs") {
    state.paused = false;
    await refreshLogs();
  }
  if (next === "settings" && state.section === "updates") void core.checkUpdates(true).catch(() => {});
  if (next === "settings") void componentSettings.catalog();
  if (next === "settings" && state.section === "plugins") void loadOfficialPlugins();
  scheduleRefresh();
}
function closeLayer() {
  componentSettings.mount(null, false);
  layer = "";
  overlay.hidden = true;
  overlay.innerHTML = "";
  document.querySelector<HTMLElement>(".workspace")!.inert = false;
  document
    .querySelector("#status-button")!
    .setAttribute("aria-expanded", "false");
  (returnFocus?.isConnected ? returnFocus : document.querySelector<HTMLElement>("#settings-button"))?.focus();
  returnFocus = null;
}
overlay.addEventListener("click", (event) => {
  if (event.target === overlay) closeLayer();
});
document.addEventListener("keydown", (event) => {
  if (!layer) return;
  if (event.key === "Escape") {
    event.preventDefault();
    closeLayer();
    return;
  }
  if (event.key !== "Tab" || layer === "status") return;
  const nodes = [
    ...overlay.querySelectorAll<HTMLElement>(
      'button,input,select,textarea,a[href],summary,[tabindex]',
    ),
  ].filter((n) => n.tabIndex >= 0 && !n.matches(":disabled") && !n.closest("[inert]") && n.getClientRects().length && getComputedStyle(n).visibility !== "hidden")
    .sort((a, b) => (a.tabIndex || Infinity) - (b.tabIndex || Infinity));
  if (!nodes.length) {
    event.preventDefault();
    const dialog = overlay.querySelector<HTMLElement>('[role="dialog"]')!;
    dialog.tabIndex = -1;
    dialog.focus();
    return;
  }
  const index = nodes.indexOf(document.activeElement as HTMLElement);
  if (
    (event.shiftKey && index <= 0) ||
    (!event.shiftKey && (index === nodes.length - 1 || index === -1))
  ) {
    event.preventDefault();
    nodes[event.shiftKey ? nodes.length - 1 : 0]?.focus();
  }
});

function toast(text: string, message?: Message, error = false) {
  const holder = document.querySelector("#toasts")!;
  const node = document.createElement(message ? "button" : "div");
  node.className = `toast ${error ? "toast-error" : ""}`;
  node.textContent = text;
  if (message)
    node.addEventListener("click", () => {
      state.selectedMessage = message.id;
      void action(async () => {
        await core.markRead(message.id);
        await showLayer("messages");
      });
      node.remove();
    });
  holder.append(node);
  while (holder.children.length > 3) holder.firstElementChild?.remove();
  setTimeout(() => node.remove(), 5000);
}
async function refresh() {
  if (refreshing) {
    refreshAgain = true;
    return;
  }
  refreshing = true;
  try {
    const networkRevision = snapshot?.preferences.updateNetwork?.revision;
    [snapshot, state.updates, state.updateOverview] = await Promise.all([core.snapshot(), core.updates(), core.updateOverview()]);
    if (networkRevision !== undefined && networkRevision !== snapshot.preferences.updateNetwork?.revision) {
      state.networkGeneration = (state.networkGeneration ?? 0) + 1;
      state.networkResults = undefined;
    }
    snapshot.network=await core.network();
    lastError = "";
    applyTheme();
    renderTop();
    if (layer === "settings") void componentSettings.catalog();
    if (layer === "status" || layer === "settings") renderLayer();
    if (layer === "logs" && !state.paused) await refreshLogs();
    if (layer === "messages") await refreshMessages();
  } catch (error) {
    snapshot = undefined;
    lastError = String(error);
    renderTop();
    renderLayer();
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      scheduleRefresh();
    }
  }
}
function scheduleRefresh() {
  if (refreshTimer) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = undefined;
    void refresh();
  }, 200);
}
async function refreshLogs() {
  const generation = ++logGeneration;
  const logs = await core.logs({ ...state.filter });
  if (generation !== logGeneration) return;
  state.logs = logs;
  const results = document.querySelector<HTMLElement>("#log-results");
  if (layer === "logs" && results) {
    const top = results.scrollTop;
    replace(results, views.logResults(logs));
    results.scrollTop = top;
  }
}
async function refreshMessages() {
  state.messages = await core.messages();
  if (layer === "messages") renderLayer();
}
async function action(run: () => Promise<unknown>) {
  try {
    await run();
  } catch (error) {
    toast(String(error), undefined, true);
  } finally {
    scheduleRefresh();
  }
}
async function pluginAction(key: string, run: () => Promise<unknown>) {
  if (state.busy.has(key)) return;
  state.busy.add(key);
  state.error = "";
  renderLayer();
  try {
    await run();
  } catch (error) {
    state.error = String(error);
    toast(state.error, undefined, true);
  } finally {
    state.busy.delete(key);
    await refresh();
  }
}
async function loadOfficialPlugins(force = false) {
  if (state.officialLoading || (!force && state.officialFetchedAt && Date.now() - state.officialFetchedAt < 60 * 60 * 1000)) return;
  state.officialLoading = true;
  state.officialError = "";
  if (layer === "settings") renderLayer();
  try {
    const catalog = await core.officialPlugins(force);
    state.officialPlugins = catalog.plugins;
    state.officialFetchedAt = catalog.fetchedAt;
    state.officialError = catalog.error ?? "";
  } catch (error) {
    state.officialError = String(error);
  } finally {
    state.officialLoading = false;
    if (layer === "settings") renderLayer();
  }
}
const restartFromUpdate = () => restart(state.updates["sodeck.core"]?.phase === "ready");
const copy = (text: string) =>
  navigator.clipboard.writeText(text).then(() => toast("已复制"));

root.addEventListener("click", (event) => {
  const link = (event.target as Element).closest<HTMLAnchorElement>("a[data-update-link]");
  if (link) {
    event.preventDefault();
    void action(() => invoke("open_update_link", { url: link.href }));
    return;
  }
  const target = (event.target as Element).closest<HTMLButtonElement>(
    "button[data-action]",
  );
  if (!target || target.disabled) return;
  const name = target.dataset.action!;
  const id = target.dataset.id ?? "";
  void action(async () => {
    if (snapshot && await networkAction(name, id, snapshot, state, renderLayer)) {
      await refresh();
      if (name === "network-add") document.getElementById(`mirror-name-${(state.networkDraft?.customMirrors.length ?? 1) - 1}`)?.focus();
      if (name === "network-delete") document.getElementById("network-add")?.focus({ preventScroll: true });
      if (name === "network-up" || name === "network-down") document.getElementById(`mirror-name-${Number(id) + (name === "network-up" ? -1 : 1)}`)?.focus({ preventScroll: true });
      if (name === "network-save" || name === "network-reload") document.getElementById("update-network-summary")?.focus({ preventScroll: true });
      return;
    }
    switch (name) {
      case "status":
      case "settings":
      case "messages":
        await showLayer(name);
        break;
      case "logs":
        state.filter.pluginId = "";
        await showLayer("logs");
        break;
      case "close-layer":
        closeLayer();
        break;
      case "refresh":
        await refresh();
        break;
      case "section":
        state.section = id;
        if (id === "plugins") state.plugin = "";
        renderLayer();
        if (id === "updates") void core.checkUpdates(true).catch(() => {});
        if (id === "plugins") void loadOfficialPlugins();
        break;
      case "manage-plugins":
        state.section = "plugins";
        state.plugin = "";
        state.pluginTab = null;
        await showLayer("settings");
        break;
      case "component-settings":
        state.section = "components";
        componentSettings.select(id);
        break;
      case "plugin-component-settings": {
        await componentSettings.catalog();
        const entry = componentSettings.entries.find(p => p.pluginId === id);
        if (entry) { state.section = "components"; componentSettings.select(`${entry.pluginId}/${entry.pageId}`); }
        break;
      }
      case "save-component-settings":
      case "defaults-component-settings":
      case "discard-component-settings":
      case "reload-component-settings":
      case "retry-component-settings":
        await componentSettings.action(target.dataset.action!);
        break;
      case "plugin-tab":
        state.pluginTab = id === "official" ? "official" : "installed";
        state.plugin = "";
        renderLayer();
        document.getElementById(`plugins-${state.pluginTab}`)?.focus();
        if (state.pluginTab === "official") void loadOfficialPlugins();
        break;
      case "refresh-official-plugins":
        await loadOfficialPlugins(true);
        break;
      case "plugin-updates":
        state.section = "updates";
        renderLayer();
        overlay.querySelector<HTMLElement>(`[data-update-id="${CSS.escape(id)}"]`)?.focus();
        void core.checkUpdates(true).catch(() => {});
        break;
      case "select-plugin":
        state.plugin = id;
        renderLayer();
        overlay.querySelector<HTMLElement>('[data-action="back-plugins"]')?.focus();
        break;
      case "back-plugins": {
        const previous = state.plugin;
        state.plugin = "";
        renderLayer();
        (overlay.querySelector<HTMLElement>(`[data-action="select-plugin"][data-id="${CSS.escape(previous)}"]`) ?? overlay.querySelector<HTMLElement>('.plugin-tabs [aria-pressed="true"]'))?.focus();
        break;
      }
      case "choose-plugin": {
        const directory = await open({
          directory: true,
          multiple: false,
          title: "选择已解压的插件目录",
        });
        if (typeof directory === "string")
          await pluginAction("choose", async () => {
            state.plugin = await core.add(directory);
            state.pluginTab = "installed";
          });
        if (typeof directory === "string") overlay.querySelector<HTMLElement>('#back-plugins')?.focus();
        break;
      }
      case "toggle-plugin": {
        const p = snapshot?.plugins.find((p) => p.manifest.id === id);
        if (p) await pluginAction(id, () => core.enabled(id, !p.enabled));
        break;
      }
      case "install-official-plugin":
        state.pluginTab = "official";
        await pluginAction(`install:${id}`, () => core.installOfficial(id));
        if (layer === "settings" && state.section === "plugins" && !state.plugin && state.pluginTab === "official" && document.activeElement === document.body)
          document.getElementById(`official-action-${id}`)?.focus({preventScroll: true});
        break;
      case "check-update":
        await pluginAction(`update:${id}`, () => core.checkUpdate(id));
        break;
      case "download-update":
        await pluginAction(`update:${id}`, () => core.downloadUpdates([id]));
        break;
      case "check-all-updates":
        await pluginAction("updates", () => core.checkUpdates());
        break;
      case "download-all-updates":
        if (snapshot) await pluginAction("downloads", () => core.downloadUpdates(selectedUpdates(snapshot!, state)));
        break;
      case "open-update-release":
        await invoke("open_update_release");
        break;
      case "usage-help":
        await invoke("open_usage_help");
        break;
      case "restart-app":
        await pluginAction("restart", async () => { await restartFromUpdate(); });
        break;
      case "install-core":
        await pluginAction("restart", async () => { await restart(true); });
        break;
      case "save-update-source": {
        const p = snapshot?.plugins.find((p) => p.manifest.id === id);
        const source = state.sourceDrafts[id] ?? p?.manifest.updateSource;
        if (source) await pluginAction(id, () => core.updateSource(id, { repository: source.repository.trim(), tagPrefix: source.tagPrefix.trim() }));
        break;
      }
      case "clear-update-source":
        await pluginAction(id, async () => { await core.updateSource(id, null); delete state.sourceDrafts[id]; });
        break;
      case "forget-plugin":
        await pluginAction(id, async () => {
          await core.forget(id);
          state.plugin = "";
        });
        break;
      case "plugin-logs":
        state.filter.pluginId = id;
        await showLayer("logs");
        break;
      case "locate-plugin":
        state.plugin = id;
        state.pluginTab = "installed";
        state.section = "plugins";
        await showLayer("settings");
        break;
      case "theme":
        await core.theme(id);
        break;
      case "pause-logs":
        state.paused = !state.paused;
        if (state.paused) ++logGeneration;
        renderLayer();
        if (!state.paused) await refreshLogs();
        break;
      case "clear-log-filters":
        clearTimeout(queryTimer);
        ++logGeneration;
        state.filter = { pluginId: "", level: "", query: "" };
        renderLayer();
        if (!state.paused) await refreshLogs();
        break;
      case "copy-logs-content":
        await copy(state.logs.map((l) => JSON.stringify(l)).join("\n"));
        break;
      case "all-messages":
      case "unread-messages":
        state.unread = name === "unread-messages";
        renderLayer();
        break;
      case "select-message":
        state.selectedMessage = id;
        await core.markRead(id);
        await refreshMessages();
        break;
      case "read-all":
        await core.markRead(null);
        await refreshMessages();
        break;
      case "copy-data":
        if (snapshot) await copy(snapshot.dataDirectory);
        break;
      case "copy-logs":
        if (snapshot) await copy(snapshot.logDirectory);
        break;
      case "copy-status":
        if (snapshot) await copy(JSON.stringify(snapshot, null, 2));
        break;
      case "minimize":
        await win.minimize();
        break;
      case "maximize":
        await win.toggleMaximize();
        break;
      case "close-window":
        await win.close();
        break;
    }
  });
});
let queryTimer: ReturnType<typeof setTimeout>;
root.addEventListener("input", (event) => {
  const input = event.target as HTMLInputElement;
  if (snapshot && networkInput(input, snapshot, state)) { if (!(event as InputEvent).isComposing) renderLayer(); return; }
  if (input.dataset.source && state.plugin) {
    const saved = snapshot?.plugins.find(p => p.manifest.id === state.plugin)?.manifest.updateSource;
    const draft = state.sourceDrafts[state.plugin] ?? { repository: saved?.repository ?? "", tagPrefix: saved?.tagPrefix ?? "" };
    if (input.dataset.source === "repository") draft.repository = input.value;
    else draft.tagPrefix = input.value;
    state.sourceDrafts[state.plugin] = draft;
    return;
  }
  if (input.id !== "log-query") return;
  ++logGeneration;
  state.filter.query = input.value;
  clearTimeout(queryTimer);
  queryTimer = setTimeout(() => { if (!state.paused) void action(refreshLogs); }, 250);
});
root.addEventListener("change", (event) => {
  const input = event.target as HTMLInputElement;
  if (snapshot && networkInput(input, snapshot, state)) { renderLayer(); return; }
  if (input.dataset.updateSelect) { input.checked ? state.excludedUpdates.delete(input.dataset.updateSelect) : state.excludedUpdates.add(input.dataset.updateSelect); renderLayer(); return; }
  if (input.id === "automatic-updates") { void action(() => core.automaticUpdates(input.checked)); return; }
  if (input.id === "log-source") state.filter.pluginId = input.value;
  else if (input.id === "log-level") state.filter.level = input.value;
  else if (input.id === "log-since")
    state.filter.since = parseLocalTime(input.value);
  else if (input.id === "log-until")
    state.filter.until = parseLocalTime(input.value);
  else return;
  ++logGeneration;
  if (!state.paused) void action(refreshLogs);
});
let hostDisposed=false;
let statusTimer:ReturnType<typeof setInterval>|undefined;
const hostListeners:UnlistenFn[]=[];
async function subscribe<T>(name:string,callback:EventCallback<T>) {
  const unlisten=await listen<T>(name,callback);
  if(hostDisposed)unlisten();else hostListeners.push(unlisten);
}
window.addEventListener("pagehide",()=>{hostDisposed=true;clearInterval(statusTimer);hostListeners.forEach(fn=>fn());picker.dispose();workspaces.dispose();},{once:true});
async function start() {
  await subscribe("core-changed", scheduleRefresh);
  await subscribe<SettingsSnapshot>("core-settings-changed", event => componentSettings.changed(event.payload));
  await subscribe<Parameters<ComponentSettings["applied"]>[0]>("core-settings-application", event => componentSettings.applied(event.payload));
  await subscribe("server-catalog-changed", () => { void servers?.start(); void picker.refresh(); });
  await subscribe("connections-changed", () => { void connections.refresh().catch(error => toast(String(error), undefined, true)); scheduleRefresh(); });
  await subscribe("workspaces-changed", () => void workspaces.refresh().catch(error => toast(String(error), undefined, true)));
  await subscribe("workspace-request-close", () => void action(() => workspaces.closeWindow()));
  await subscribe<DropTarget>("workspace-drop-preview", event => workspaces.previewDrop(event.payload));
  await subscribe<string>("core-main-action", event => { if (servers) void action(() => event.payload==="exit"?workspaces.exit():restart(event.payload === "install")); });
  await subscribe("core-request-close", () => {
    void action(()=>workspaces.exit());
  });
  await subscribe<Message>("core-message", (event) => {
    toast(event.payload.title, event.payload);
    scheduleRefresh();
  });
  await subscribe<string>("core-exit-error", (event) =>
    toast(`退出未完成：${event.payload}`, undefined, true),
  );
  const unresize=await win.onResized(async () => {
    document.querySelector("#maximize-button")!.innerHTML = icon(
      (await win.isMaximized()) ? "restore" : "maximize",
    );
  });
  if(hostDisposed)unresize();else hostListeners.push(unresize);
  const [, catalogReady] = await Promise.all([refresh(), servers?.start() ?? Promise.resolve(true), workspaces.start(), connections.refresh()]);
  if (snapshot && catalogReady) await core.ready();
  if(hostDisposed)return;
  statusTimer=setInterval(() => {
    if (layer === "status") scheduleRefresh();
  }, 1000);
}
void start().catch((error) => {
  lastError = String(error);
  renderTop();
  toast(lastError, undefined, true);
});
