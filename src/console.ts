import { enterMotion, exitMotion, captureMotion, settleMotion } from './motion.js';
import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";
import type { Window } from "@tauri-apps/api/window";
import {DesktopUi, chooseLocalPath} from "./desktop";
import {GeneralSettings} from './general-settings';
import * as settingsViews from './settings-views';
import { invoke } from "@tauri-apps/api/core";
import { Workspaces, type DropPreview } from "./workspaces";
import { core, type Snapshot, type Message } from "./core";
import * as views from "./views";
import { selectedUpdates, updateNotice } from "./update-model";
import { networkAction, networkInput } from "./update-network";
import { button as b, escape as e, icon } from "./ui";
import { updateDOM } from "./dom-update";
import { LogsPage } from './logs';
import { MessagesPage } from './messages';
import type { StatusViewState } from "./status";


import { HostDrafts } from "./host-drafts";
import { ServerManager } from "./servers";
import { ConnectionsUi } from "./connections";
import { ServerPicker } from "./server-picker";
import { ComponentSettings, type SettingsSnapshot } from "./settings";

import { Shell, type ConsolePage } from './shell';
export async function startConsole(shell:Shell, workspaces:Workspaces, servers:ServerManager|undefined, connections:ConnectionsUi, picker:ServerPicker|undefined, win:Window) {
const root=shell.root;
async function restart(install: boolean) {
  await workspaces.restart(install);
}
const pageHost = shell.page;
const componentSettings = new ComponentSettings(() => { if (["settings","plugins","updates"].includes(currentPage)) renderPage(); });
const generalSettings=new GeneralSettings(()=>{if(shell.visibleConsolePage==='settings'&&settingsSection==='general')renderPage();},refresh);
workspaces.setCloseParticipants(async (decision,operationId)=>{await hostDrafts.flushInputs();return [...await (servers?.closeInputs(decision)??Promise.resolve([])),...await componentSettings.closeInputs(decision,operationId),...await hostDrafts.closeInputs(decision)];},blocked=>servers?.setCloseBlocked(blocked));
let snapshot: Snapshot | undefined;
let currentPage:ConsolePage = 'connections';
let settingsSection:settingsViews.SettingsSection = 'general';
let lastError = "";
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
let refreshing = false;
let refreshAgain = false;
const statusView: StatusViewState = { paused: false, onlyErrors: false, updatedAt: 0, networkStale: false };
let pausedStatus: Snapshot | undefined;
const state: views.ViewState = {
  officialPlugins: [],
  officialFetchedAt: 0,
  officialLoading: false,
  officialError: "",
  plugin: "",
  error: "",
  busy: new Set(),
  updates: {},
  updateOverview: { checking: false, busy: false, lastCheckedAt: 0, nextCheckAt: 0 },
  excludedUpdates: new Set(),
  pluginTab: null,
  sourceDrafts: {},
};

const hostDrafts = new HostDrafts(() => snapshot, state, renderPage);
const logsPage = new LogsPage();
const messagesPage = new MessagesPage(id => {
  state.plugin=id;state.pluginTab='installed';shell.navigate('plugins');
},scheduleRefresh);


function renderTop() {
  const {count, pending, label} = snapshot ? updateNotice(snapshot, state) : {count: 0, pending: false, label: ""};
  const notice = root.querySelector<HTMLElement>("#updates-button")!;
  notice.hidden = !count && !pending;
  notice.setAttribute("aria-label", label || "软件更新");
  notice.title = label;
  root.querySelector("#update-notice-label")!.textContent = label;
  const updatesBadge = root.querySelector<HTMLElement>('#console-update-count')!;
  updatesBadge.textContent = count ? String(count) : pending ? '待重启' : '';
  updatesBadge.hidden = !count && !pending;
  const messagesBadge = root.querySelector<HTMLElement>('#console-message-count')!;
  messagesBadge.textContent = snapshot?.unreadCount ? snapshot.unreadCount > 99 ? '99+' : String(snapshot.unreadCount) : '';
  messagesBadge.hidden = !snapshot?.unreadCount;
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
  if (currentPage === "status") return views.status(pausedStatus ?? snapshot, statusView);
  if (currentPage === "settings") return settingsViews.settings(snapshot, settingsSection,generalSettings.state);
  if (currentPage === "plugins" || currentPage === "updates") return views.maintenance(snapshot, state, currentPage);
  return '';
}
function syncSettingsActivity() {
  generalSettings.mount(shell.visibleConsolePage==='settings'&&settingsSection==='general');
  logsPage.setActive(shell.visibleConsolePage==='logs');messagesPage.setActive(shell.visibleConsolePage==='messages');
}
function renderPage(navigating = false) {
  shell.updateSettingsNavigation(settingsViews.settingsNavigation(currentPage === "settings" ? settingsSection : null,componentSettings.navigation(currentPage === "settings" && settingsSection === "components")));
  const error = document.querySelector<HTMLElement>("#layer-error");
  if (error) { error.textContent = lastError ? `${lastError}；保留上次读取结果。` : ""; error.hidden = !lastError || !snapshot; }
  const target = document.querySelector<HTMLElement>("#layer-content");
  if (target) {
    const activity=currentPage==='logs'||currentPage==='messages';
    if(activity){
      componentSettings.mount(null,false);
      if(target.dataset.page!==currentPage){logsPage.mount(null);messagesPage.mount(null);target.innerHTML='';target.dataset.page=currentPage;(currentPage==='logs'?logsPage:messagesPage).mount(target);}
      const context={plugins:snapshot?.plugins,storageErrors:snapshot?.storageErrors??[]};
      logsPage.setContext(context);messagesPage.setContext(context);syncSettingsActivity();return;
    }
    logsPage.mount(null);messagesPage.mount(null);
    const hadPluginDetails = !!target.querySelector("#back-plugins");
    if (["settings", "plugins", "updates"].includes(currentPage) && snapshot) {
      const component=currentPage === "settings" && settingsSection === "components";
      const identity = currentPage === "settings" ? `settings:${component ? componentSettings.selected : settingsSection}` : currentPage === "plugins" ? `plugins:${state.plugin}` : "updates";
      if (target.dataset.page !== identity) {
        componentSettings.mount(null, false);
        target.innerHTML = content();
        const back=target.dataset.page?.startsWith('plugins:')&&identity==='plugins:';
        target.dataset.page = identity;if(!navigating)enterMotion(target,back?'back':'x');
      } else if (!component) updateDOM(target, content());
    } else {componentSettings.mount(null,false);delete target.dataset.page;updateDOM(target, content(), !navigating);}
    if (!hadPluginDetails && document.activeElement === document.body)
      target.querySelector<HTMLElement>("#back-plugins")?.focus({preventScroll: true});
    componentSettings.mount(target.querySelector("#component-settings-page"), currentPage === "settings" && settingsSection === "components");
  }
  syncSettingsActivity();
}
async function showPage(next: ConsolePage) {
  shell.navigate(next);
}
async function enterPage(next: ConsolePage) {
  const changed=currentPage!==next;currentPage=next;
  if(next === 'connections') {componentSettings.mount(null,false);logsPage.mount(null);messagesPage.mount(null);delete pageHost.querySelector<HTMLElement>('#layer-content')!.dataset.page;syncSettingsActivity();return;}
  const target=pageHost.querySelector<HTMLElement>('#layer-content')!;
  target.className='layer-content '+next+'-content';
  renderPage(changed);
  if(['settings','plugins','updates'].includes(next))void componentSettings.catalog();
  if(next==='plugins')void loadOfficialPlugins();
  scheduleRefresh();
}
shell.onPage=page=>void action(()=>enterPage(page));
shell.onConsoleVisibility=syncSettingsActivity;
function openSettings(section:settingsViews.SettingsSection) {
  settingsSection=section;
  shell.navigate('settings');
}
function openComponentSettings(key:string) {
  settingsSection="components";
  componentSettings.select(key);
  openSettings('components');
}
function toast(text: string, message?: Message, error = false) {
  const holder = document.querySelector("#toasts")!;
  const node = document.createElement(message ? "button" : "div");
  node.dataset.motion='item';
  node.className = `toast ${error ? "toast-error" : ""}`;
  node.textContent = text;
  if (message)
    node.addEventListener("click", () => {
      messagesPage.open(message.id);shell.navigate('messages');
      remove();
    });
  const remove=()=>{if(!node.isConnected)return;const before=captureMotion(holder);exitMotion(node);node.remove();settleMotion(holder,before);};
  const before=captureMotion(holder);holder.append(node);
  while (holder.children.length > 3) {exitMotion(holder.firstElementChild!);holder.firstElementChild?.remove();}
  settleMotion(holder,before);
  setTimeout(remove, 5000);
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
    const {updates} = result;
    snapshot = result.snapshot;
    if (!statusView.paused) { statusView.updatedAt = Date.now(); statusView.networkStale = shell.networkStale; }
    if (updates.status === "fulfilled") {
      state.updates = updates.value.items; state.updateOverview = updates.value.overview;
      state.officialPreparation = updates.value.officialPreparation;
      const catalog = state.officialPreparation?.catalog;
      if (catalog && !state.officialFetchedAt) {
        state.officialPlugins = catalog.plugins; state.officialFetchedAt = catalog.fetchedAt;
        state.officialError = catalog.error ?? "";
      }
    }
    lastError = shell.error;
    if (snapshot) {
      const initial = shell.takeStartupPage(snapshot);
      if (initial) { state.pluginTab = "official"; shell.navigate(initial); }
    }
    if (networkRevision !== undefined && networkRevision !== snapshot?.preferences.updateNetwork?.revision) {
      state.networkGeneration = (state.networkGeneration ?? 0) + 1;
      state.networkResults = undefined;
    }
    renderTop();
    if (["settings","plugins","updates"].includes(currentPage)) void componentSettings.catalog();
    if (currentPage !== "connections") renderPage();
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
    void logsPage.refresh();void messagesPage.refresh();
    void refresh();
  }, 200);
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
  if (!force && state.officialPreparation) return;
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
      case "updates":
      case "settings":
      case "messages":
        await showPage(name);
        break;
      case "logs":
        logsPage.openSource('');
        await showPage("logs");
        break;
      case "pause-status":
        statusView.paused = !statusView.paused;
        pausedStatus = statusView.paused ? structuredClone(snapshot) : undefined;
        if (!statusView.paused) await refresh();
        renderPage();
        break;
      case "clear-status-filter":
        statusView.onlyErrors = false;
        renderPage();
        document.getElementById("status-errors")?.focus();
        break;
      case "status-about":
        openSettings('about');
        break;
      case "connections":
        await showPage("connections");
        break;
      case "refresh":
        await refresh();
        break;
      case "section":
        if(settingsViews.isBuiltinSettingsSection(id))openSettings(id);
        break;
      case "manage-plugins":
        state.plugin = "";
        await showPage("plugins");
        break;
      case "component-settings":
        openComponentSettings(id);
        break;
      case "plugin-component-settings": {
        const revision=shell.navigationRevision;
        await componentSettings.catalog();
        if(revision!==shell.navigationRevision)break;
        const entry = componentSettings.entries.find(p => p.pluginId === id);
        if (entry) openComponentSettings(`${entry.pluginId}/${entry.pageId}`);
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
        await showPage("updates");
        pageHost.querySelector<HTMLElement>(`[data-update-id="${CSS.escape(id)}"]`)?.focus();
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
        const directory = await chooseLocalPath("pluginDirectory");
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
        if (currentPage === "plugins" && !state.plugin && state.pluginTab === "official" && document.activeElement === document.body)
          document.getElementById(`official-action-${id}`)?.focus({preventScroll: true});
        break;
      case "retry-official-preparation":
        await pluginAction("official-preparation", () => core.retryOfficialPreparation());
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
        logsPage.openSource(id);
        await showPage("logs");
        break;
      case "locate-plugin":
        state.plugin = id;
        state.pluginTab = "installed";
        await showPage("plugins");
        break;
      case "theme":
        await generalSettings.setTheme(id);
        break;
      case "copy-data":
        if (snapshot) await copy(snapshot.dataDirectory);
        break;
      case "login-startup-retry":
        await generalSettings.retry();
        break;
      case "login-startup-update":
        await generalSettings.updateLoginStartup();
        break;
      case "copy-logs":
        if (snapshot) await copy(snapshot.logDirectory);
        break;
      case "copy-status":
        if (snapshot) await copy(JSON.stringify(currentPage === "status" ? pausedStatus ?? snapshot : snapshot, null, 2));
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
root.addEventListener("input", (event) => {
  const input = event.target as HTMLInputElement;
  if (snapshot && networkInput(input, snapshot, state)) { if (!(event as InputEvent).isComposing) renderPage(); return; }
  if (input.dataset.source && state.plugin) {
    hostDrafts.input(state.plugin, input.dataset.source, input.value);
    return;
  }
});
root.addEventListener("change", (event) => {
  const input = event.target as HTMLInputElement;
  if (input.id === "status-errors") { statusView.onlyErrors = input.checked; renderPage(); return; }
  if (snapshot && networkInput(input, snapshot, state)) { renderPage(); return; }
  if (input.dataset.updateSelect) { input.checked ? state.excludedUpdates.delete(input.dataset.updateSelect) : state.excludedUpdates.add(input.dataset.updateSelect); renderPage(); return; }
  if (input.id === "automatic-updates") { void action(() => core.automaticUpdates(input.checked)); return; }
  if(input.id==='login-startup'){void action(()=>generalSettings.setLoginStartup(input.checked));return;}
  if(input.id==='close-behavior'){void action(()=>generalSettings.setCloseBehavior(input.value));return;}
});
let hostDisposed=false;
let statusTimer:ReturnType<typeof setInterval>|undefined;
const desktop=new DesktopUi(win,shell,target=>{if(target==='settings')openSettings('general');else shell.navigate(target);},async()=>{await hostDrafts.flushInputs();return workspaces.canBackground();},message=>toast(message,undefined,true));
const hostListeners:UnlistenFn[]=[];
async function subscribe<T>(name:string,callback:EventCallback<T>) {
  const unlisten=await listen<T>(name,callback);
  if(hostDisposed)unlisten();else hostListeners.push(unlisten);
}
window.addEventListener("pagehide",()=>{hostDisposed=true;logsPage.dispose();messagesPage.dispose();generalSettings.dispose();desktop.dispose();clearTimeout(refreshTimer);servers?.dispose();componentSettings.dispose();hostDrafts.dispose();clearInterval(statusTimer);hostListeners.forEach(fn=>fn());picker?.dispose();workspaces.dispose();shell.dispose();},{once:true});
async function start() {
  await desktop.listen();
  await desktop.initialize();
  await subscribe("core-changed", scheduleRefresh);
  await subscribe("messages-changed", scheduleRefresh);
  await subscribe<SettingsSnapshot>("core-settings-changed", event => componentSettings.changed(event.payload));
  await subscribe<Parameters<ComponentSettings["applied"]>[0]>("core-settings-application", event => componentSettings.applied(event.payload));
  await subscribe("server-catalog-changed", () => { void servers?.start(); void picker?.refresh(); });
  await subscribe("connections-changed", () => { void connections.refresh().catch(error => toast(String(error), undefined, true)); scheduleRefresh(); });
  await subscribe("workspaces-changed", () => void workspaces.refresh().catch(error => toast(String(error), undefined, true)));
  await subscribe("workspace-request-close", () => void action(() => workspaces.closeWindow()));
  await subscribe<{token:string}>("workspace-drop-settle", event => { void workspaces.settleDrop(event.payload.token).catch(error => toast(String(error))); });
  await subscribe<DropPreview>("workspace-drop-preview", event => workspaces.previewDrop(event.payload));
  await subscribe<Message>("core-message", (event) => {
    toast(event.payload.title, event.payload);
    scheduleRefresh();
  });
  await subscribe("login-startup-changed",()=>{void generalSettings.refresh();});
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
  await desktop.ready();
  if (snapshot && catalogReady) await core.ready();
  if(hostDisposed)return;
  statusTimer=setInterval(() => {
    scheduleRefresh();
  }, 1000);
}
await start();
}
