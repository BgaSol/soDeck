import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";
import type { Window } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Workspaces, type DropPreview } from "./workspaces";
import { core, type Snapshot, type Message } from "./core";
import * as views from "./views";
import { selectedUpdates, updateCount, CORE_ID } from "./updates";
import { networkAction, networkInput } from "./update-network";
import { button as b, escape as e, icon } from "./ui";
import { updateDOM } from "./dom-update";
import { parseLocalTime } from "./datetime";


import { HostDrafts } from "./host-drafts";
import { ServerManager } from "./servers";
import { ConnectionsUi } from "./connections";
import { ServerPicker } from "./server-picker";
import { ComponentSettings, type SettingsSnapshot } from "./settings";

import { Shell, type ConsolePage } from './shell';
export async function startConsole(shell:Shell, workspaces:Workspaces, servers:ServerManager|undefined, connections:ConnectionsUi, picker:ServerPicker|undefined, win:Window) {
const root=shell.root;
async function restart(install: boolean) {
  if (!servers) { await invoke("request_main_action", { action: install ? "install" : "restart" }); return; }
  await workspaces.restart(install);
}
const pageHost = shell.page;
const componentSettings = new ComponentSettings(() => { if (["settings","plugins","updates"].includes(currentPage)) renderPage(); });
workspaces.setCloseParticipants(async (decision,operationId)=>{await hostDrafts.flushInputs();return [...await (servers?.closeInputs(decision)??Promise.resolve([])),...await componentSettings.closeInputs(decision,operationId),...await hostDrafts.closeInputs(decision)];},blocked=>servers?.setCloseBlocked(blocked));
let snapshot: Snapshot | undefined;
let currentPage = 'connections';
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

const hostDrafts = new HostDrafts(() => snapshot, state, renderPage);


function renderTop() {
  const count = snapshot ? updateCount(snapshot, state) : 0;
  const pending = snapshot?.restartRequired || state.updates[CORE_ID]?.phase === "ready";
  const settingsButton = document.querySelector<HTMLButtonElement>("#settings-button")!;
  settingsButton.innerHTML = `${icon("settings")}设置${count ? `<span class="settings-update-count">${count}</span>` : pending ? '<span class="settings-update-pending">待重启</span>' : ""}`;
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
  if (currentPage === "status") return views.status(snapshot);
  if (["settings", "plugins", "updates"].includes(currentPage)) return views.settings(snapshot, state, componentSettings.navigation(state.section === "components"));
  if (currentPage === "logs") return views.logs(snapshot, state);
  return views.messages(state);
}
function renderPage() {
  const error = document.querySelector<HTMLElement>("#layer-error");
  if (error) { error.textContent = lastError ? `${lastError}；保留上次读取结果。` : ""; error.hidden = !lastError || !snapshot; }
  const target = document.querySelector<HTMLElement>("#layer-content");
  if (target) {
    const hadPluginDetails = !!target.querySelector("#back-plugins");
    if (["settings", "plugins", "updates"].includes(currentPage) && snapshot) {
      const next = document.createElement("div"); next.innerHTML = content();
      if (!target.querySelector(".settings-main")) target.innerHTML = '<aside class="settings-nav" aria-label="设置导航"></aside><div class="settings-main"></div>';
      const navigation=target.querySelector<HTMLElement>(".settings-nav")!;
      navigation.hidden=currentPage!=="settings";
      updateDOM(navigation,next.querySelector(".settings-nav")!.innerHTML);
      const main = target.querySelector<HTMLElement>(".settings-main")!;
      const identity = state.section === "components" ? `components:${componentSettings.selected}` : state.section === "plugins" ? `plugins:${state.plugin}` : state.section;
      if (main.dataset.page !== identity) {
        componentSettings.mount(null, false);
        main.innerHTML = next.querySelector(".settings-main")!.innerHTML;
        main.dataset.page = identity;
      } else if (state.section !== "components") updateDOM(main, next.querySelector(".settings-main")!.innerHTML);
    } else updateDOM(target, content());
    if (!hadPluginDetails && document.activeElement === document.body)
      target.querySelector<HTMLElement>("#back-plugins")?.focus({preventScroll: true});
    componentSettings.mount(target.querySelector("#component-settings-page"), ["settings","plugins","updates"].includes(currentPage) && state.section === "components");
  }
}
async function showPage(next: string) {
  shell.navigate(next as ConsolePage);
}
async function enterPage(next: ConsolePage) {
  currentPage=next;
  if(next === 'connections') {componentSettings.mount(null,false);return;}
  if(next==='plugins')state.plugin='';
  if(['plugins','updates'].includes(next))state.section=next;
  else if(next==='settings'&&['plugins','updates'].includes(state.section))state.section='appearance';
  const target=pageHost.querySelector<HTMLElement>('#layer-content')!;
  target.className='layer-content '+next+'-content';
  renderPage();
  if(next==='messages')await refreshMessages();
  if(next==='logs'){state.paused=false;await refreshLogs();}
  if(next==='updates')void core.checkUpdates(true).catch(()=>{});
  if(['settings','plugins','updates'].includes(next))void componentSettings.catalog();
  if(next==='plugins')void loadOfficialPlugins();
  scheduleRefresh();
}
shell.onPage=page=>void action(()=>enterPage(page));
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
        await showPage("messages");
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
    const result = await shell.read();
    if (!result || hostDisposed) return;
    const {updates, overview} = result;
    snapshot = result.snapshot;
    if (updates.status === "fulfilled") state.updates = updates.value;
    if (overview.status === "fulfilled") state.updateOverview = overview.value;
    lastError = shell.error;
    if (networkRevision !== undefined && networkRevision !== snapshot?.preferences.updateNetwork?.revision) {
      state.networkGeneration = (state.networkGeneration ?? 0) + 1;
      state.networkResults = undefined;
    }
    renderTop();
    if (["settings","plugins","updates"].includes(currentPage)) void componentSettings.catalog();
    if (currentPage !== "connections") renderPage();
    if (currentPage === "logs" && !state.paused) await refreshLogs();
    if (currentPage === "messages") await refreshMessages();
  } catch (error) {
    lastError = String(error);
    renderTop();
    renderPage();
  } finally {
    refreshing = false;
    if (refreshAgain) {
      refreshAgain = false;
      scheduleRefresh();
    }
  }
}
function scheduleRefresh() {
  if (refreshTimer || hostDisposed) return;
  refreshTimer = setTimeout(() => {
    refreshTimer = undefined;
    void refresh();
  }, 200);
}
async function refreshLogs() {
  const generation = ++logGeneration;
  const logs = await core.logs({ ...state.filter });
  if (hostDisposed || generation !== logGeneration) return;
  state.logs = logs;
  const results = document.querySelector<HTMLElement>("#log-results");
  if (currentPage === "logs" && results) {
    const top = results.scrollTop;
    updateDOM(results, views.logResults(logs));
    results.scrollTop = top;
  }
}
async function refreshMessages() {
  state.messages = await core.messages();
  if (currentPage === "messages") renderPage();
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
  renderPage();
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
  if (["settings","plugins","updates"].includes(currentPage)) renderPage();
  try {
    const catalog = await core.officialPlugins(force);
    state.officialPlugins = catalog.plugins;
    state.officialFetchedAt = catalog.fetchedAt;
    state.officialError = catalog.error ?? "";
  } catch (error) {
    state.officialError = String(error);
  } finally {
    state.officialLoading = false;
    if (["settings","plugins","updates"].includes(currentPage)) renderPage();
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
    if (snapshot && await networkAction(name, id, snapshot, state, renderPage)) {
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
        await showPage(name);
        break;
      case "logs":
        state.filter.pluginId = "";
        await showPage("logs");
        break;
      case "close-page":
        shell.showWorkbench();
        break;
      case "refresh":
        await refresh();
        break;
      case "section":
        state.section = id;
        if (["plugins","updates"].includes(id)) {await showPage(id);break;}
        renderPage();
        break;
      case "manage-plugins":
        state.section = "plugins";
        state.plugin = "";
        state.pluginTab = null;
        await showPage("settings");
        break;
      case "component-settings":
        state.section = "components";
        await showPage("settings");
        componentSettings.select(id);
        break;
      case "plugin-component-settings": {
        await componentSettings.catalog();
        const entry = componentSettings.entries.find(p => p.pluginId === id);
        if (entry) { state.section = "components"; await showPage("settings"); componentSettings.select(`${entry.pluginId}/${entry.pageId}`); }
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
        renderPage();
        document.getElementById(`plugins-${state.pluginTab}`)?.focus();
        if (state.pluginTab === "official") void loadOfficialPlugins();
        break;
      case "refresh-official-plugins":
        await loadOfficialPlugins(true);
        break;
      case "plugin-updates":
        state.section = "updates";
        await showPage("updates");
        pageHost.querySelector<HTMLElement>(`[data-update-id="${CSS.escape(id)}"]`)?.focus();
        void core.checkUpdates(true).catch(() => {});
        break;
      case "select-plugin":
        state.plugin = id;
        renderPage();
        pageHost.querySelector<HTMLElement>('[data-action="back-plugins"]')?.focus();
        break;
      case "back-plugins": {
        const previous = state.plugin;
        state.plugin = "";
        renderPage();
        (pageHost.querySelector<HTMLElement>(`[data-action="select-plugin"][data-id="${CSS.escape(previous)}"]`) ?? pageHost.querySelector<HTMLElement>('.plugin-tabs [aria-pressed="true"]'))?.focus();
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
        if (typeof directory === "string") pageHost.querySelector<HTMLElement>('#back-plugins')?.focus();
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
        if (["settings","plugins","updates"].includes(currentPage) && state.section === "plugins" && !state.plugin && state.pluginTab === "official" && document.activeElement === document.body)
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
        if (source) await pluginAction(id, () => hostDrafts.saveSource(id));
        break;
      }
      case "reload-update-source":
        hostDrafts.discardSource(id); renderPage(); break;
      case "clear-update-source":
        await pluginAction(id, async () => { await hostDrafts.saveSource(id, true); });
        break;
      case "forget-plugin":
        await pluginAction(id, async () => {
          await core.forget(id);
          state.plugin = "";
        });
        break;
      case "plugin-logs":
        state.filter.pluginId = id;
        await showPage("logs");
        break;
      case "locate-plugin":
        state.plugin = id;
        state.pluginTab = "installed";
        state.section = "plugins";
        await showPage("settings");
        break;
      case "theme":
        await core.theme(id);
        break;
      case "pause-logs":
        state.paused = !state.paused;
        if (state.paused) ++logGeneration;
        renderPage();
        if (!state.paused) await refreshLogs();
        break;
      case "clear-log-filters":
        clearTimeout(queryTimer);
        ++logGeneration;
        state.filter = { pluginId: "", level: "", query: "" };
        renderPage();
        if (!state.paused) await refreshLogs();
        break;
      case "copy-logs-content":
        await copy(state.logs.map((l) => JSON.stringify(l)).join("\n"));
        break;
      case "all-messages":
      case "unread-messages":
        state.unread = name === "unread-messages";
        renderPage();
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
  if (snapshot && networkInput(input, snapshot, state)) { if (!(event as InputEvent).isComposing) renderPage(); return; }
  if (input.dataset.source && state.plugin) {
    hostDrafts.input(state.plugin, input.dataset.source, input.value);
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
  if (snapshot && networkInput(input, snapshot, state)) { renderPage(); return; }
  if (input.dataset.updateSelect) { input.checked ? state.excludedUpdates.delete(input.dataset.updateSelect) : state.excludedUpdates.add(input.dataset.updateSelect); renderPage(); return; }
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
window.addEventListener("pagehide",()=>{hostDisposed=true;clearTimeout(refreshTimer);clearTimeout(queryTimer);servers?.dispose();componentSettings.dispose();hostDrafts.dispose();clearInterval(statusTimer);hostListeners.forEach(fn=>fn());picker?.dispose();workspaces.dispose();shell.dispose();},{once:true});
async function start() {
  await subscribe("core-changed", scheduleRefresh);
  await subscribe<SettingsSnapshot>("core-settings-changed", event => componentSettings.changed(event.payload));
  await subscribe<Parameters<ComponentSettings["applied"]>[0]>("core-settings-application", event => componentSettings.applied(event.payload));
  await subscribe("server-catalog-changed", () => { void servers?.start(); void picker?.refresh(); });
  await subscribe("connections-changed", () => { void connections.refresh().catch(error => toast(String(error), undefined, true)); scheduleRefresh(); });
  await subscribe("workspaces-changed", () => void workspaces.refresh().catch(error => toast(String(error), undefined, true)));
  await subscribe("workspace-request-close", () => void action(() => workspaces.closeWindow()));
  await subscribe<{token:string}>("workspace-drop-settle", event => { void workspaces.settleDrop(event.payload.token).catch(error => toast(String(error))); });
  await subscribe<DropPreview>("workspace-drop-preview", event => workspaces.previewDrop(event.payload));
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
    scheduleRefresh();
  }, 1000);
}
await start();
}
