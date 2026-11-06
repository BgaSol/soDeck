import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Workspaces, type DropTarget } from "./workspaces";
import { core, type Snapshot, type Message } from "./core";
import * as views from "./views";
import { button as b, escape as e, icon, replace } from "./ui";
import "./style.css";
import { ServerManager } from "./servers";
import { ConnectionsUi } from "./connections";
import { ServerPicker } from "./server-picker";

const win = getCurrentWindow();
const root = document.querySelector<HTMLDivElement>("#app")!;
root.innerHTML = `<header class="topbar" data-tauri-drag-region="deep"><button id="catalog-tab" role="tab" aria-selected="true">${icon("grid")}<span>服务器管理</span></button><div class="server-tabs" id="server-tabs" role="tablist" aria-label="服务器工作区"></div><button id="new-workspace" class="sd-icon" aria-label="选择服务器" title="选择服务器" aria-haspopup="dialog" aria-expanded="false">${icon("plus")}</button><div id="drag-region" class="drag-region" aria-hidden="true"></div><nav class="global-tools" aria-label="应用工具"><button data-action="status" id="status-button" aria-label="应用状态" aria-expanded="false"><span class="health-dot"></span><span id="health">正在启动</span></button>${b("messages", '<span id="message-count">消息</span>', "bell", "icon-label", 'id="messages-button" aria-label="消息中心"')}${b("settings", "设置", "settings", "icon-label", 'id="settings-button"')}</nav><div id="window-controls" class="window-controls">${b("minimize", "", "minus", "", 'aria-label="最小化"')}${b("maximize", "", "maximize", "", 'id="maximize-button" aria-label="最大化或还原"')}${b("close-window", "", "close", "close-window", 'aria-label="关闭窗口"')}</div></header><main class="workspace" id="server-workspace"><div id="server-catalog"></div><div id="workspace-content" hidden></div></main><div id="overlay" hidden></div><div id="toasts" class="toasts" aria-live="polite"></div>`;
const servers = win.label === "main" ? new ServerManager(document.querySelector<HTMLElement>("#server-catalog")!, text => toast(text)) : undefined;
const workspaces = new Workspaces(document.querySelector("#server-tabs")!, document.querySelector("#workspace-content")!, document.querySelector("#server-catalog")!, text => toast(text, undefined, true));
const connections = new ConnectionsUi(win.label, views => { servers?.setConnections(views); picker.setConnections(views); }, text => toast(text, undefined, true));
const picker = new ServerPicker(document.querySelector<HTMLButtonElement>("#new-workspace")!, id => connections.connect(id), text => toast(text, undefined, true));
async function restart(install: boolean) {
  if (!servers) { await invoke("request_main_action", { action: install ? "install" : "restart" }); return; }
  if (await servers.allowExit() && await workspaces.allowExit()) await core.restart(install);
}
const overlay = document.querySelector<HTMLDivElement>("#overlay")!;
let snapshot: Snapshot | undefined;
let layer = "";
let returnFocus: HTMLElement | null = null;
let lastError = "";
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
let refreshing = false;
let refreshAgain = false;
let exitPrompt = false;
let logGeneration = 0;
const state: views.ViewState = {
  officialPlugins: [],
  section: "plugins",
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
  if (layer === "settings") return views.settings(snapshot, state);
  if (layer === "logs") return views.logs(snapshot, state);
  return views.messages(state);
}
function renderLayer() {
  const target = document.querySelector<HTMLElement>("#layer-content");
  if (target) {
    const expanded=[...target.querySelectorAll<HTMLDetailsElement>('details[data-network-server][open]')].map(d=>d.dataset.networkServer);
    replace(target, content());
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
  scheduleRefresh();
}
function closeLayer() {
  layer = "";
  overlay.hidden = true;
  overlay.innerHTML = "";
  document.querySelector<HTMLElement>(".workspace")!.inert = false;
  document
    .querySelector("#status-button")!
    .setAttribute("aria-expanded", "false");
  returnFocus?.focus();
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
      'button:not(:disabled),input:not(:disabled),select:not(:disabled),[tabindex="0"]',
    ),
  ].filter((n) => n.getClientRects().length);
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
    [snapshot, state.updates] = await Promise.all([core.snapshot(), core.updates()]);
    snapshot.network=await core.network();
    lastError = "";
    applyTheme();
    renderTop();
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
const restartFromUpdate = () => restart(state.updates["sodeck.core"]?.phase === "ready");
const copy = (text: string) =>
  navigator.clipboard.writeText(text).then(() => toast("已复制"));

root.addEventListener("click", (event) => {
  const target = (event.target as Element).closest<HTMLButtonElement>(
    "button[data-action]",
  );
  if (!target || target.disabled) return;
  const name = target.dataset.action!;
  const id = target.dataset.id ?? "";
  void action(async () => {
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
        renderLayer();
        break;
      case "select-plugin":
        state.plugin = id;
        renderLayer();
        break;
      case "back-plugins":
        state.plugin = "";
        renderLayer();
        break;
      case "choose-plugin": {
        const directory = await open({
          directory: true,
          multiple: false,
          title: "选择已解压的插件目录",
        });
        if (typeof directory === "string")
          await pluginAction("choose", async () => {
            state.plugin = await core.add(directory);
          });
        break;
      }
      case "toggle-plugin": {
        const p = snapshot?.plugins.find((p) => p.manifest.id === id);
        if (p) await pluginAction(id, () => core.enabled(id, !p.enabled));
        break;
      }
      case "install-official-plugin":
        await pluginAction(`install:${id}`, async () => { state.plugin = await core.installOfficial(id); });
        break;
      case "check-update":
        await pluginAction(`update:${id}`, () => core.checkUpdate(id));
        break;
      case "download-update":
      case "update-restart":
        await pluginAction(`update:${id}`, async () => {
          const restart = await core.download(id);
          if (name === "update-restart" && restart) await restartFromUpdate();
        });
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
        state.section = "plugins";
        await showLayer("settings");
        break;
      case "theme":
        await core.theme(id);
        break;
      case "pause-logs":
        state.paused = !state.paused;
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
  if (input.dataset.source && state.plugin) {
    const saved = snapshot?.plugins.find(p => p.manifest.id === state.plugin)?.manifest.updateSource;
    const draft = state.sourceDrafts[state.plugin] ?? { repository: saved?.repository ?? "", tagPrefix: saved?.tagPrefix ?? "" };
    if (input.dataset.source === "repository") draft.repository = input.value;
    else draft.tagPrefix = input.value;
    state.sourceDrafts[state.plugin] = draft;
    return;
  }
  if (input.id !== "log-query") return;
  state.filter.query = input.value;
  clearTimeout(queryTimer);
  queryTimer = setTimeout(() => void action(refreshLogs), 250);
});
root.addEventListener("change", (event) => {
  const input = event.target as HTMLInputElement;
  if (input.id === "log-source") state.filter.pluginId = input.value;
  else if (input.id === "log-level") state.filter.level = input.value;
  else if (input.id === "log-since")
    state.filter.since = input.value
      ? new Date(input.value).getTime()
      : undefined;
  else if (input.id === "log-until")
    state.filter.until = input.value
      ? new Date(input.value).getTime()
      : undefined;
  else return;
  void action(refreshLogs);
});
async function start() {
  state.officialPlugins = await core.officialPlugins();
  await listen("core-changed", scheduleRefresh);
  await listen("server-catalog-changed", () => { void servers?.start(); void picker.refresh(); });
  await listen("connections-changed", () => { void connections.refresh().catch(error => toast(String(error), undefined, true)); scheduleRefresh(); });
  await listen("workspaces-changed", () => void workspaces.refresh().catch(error => toast(String(error), undefined, true)));
  await listen("workspace-request-close", () => void action(() => workspaces.closeWindow()));
  await listen<DropTarget>("workspace-drop-preview", event => workspaces.previewDrop(event.payload));
  await listen<string>("core-main-action", event => { if (servers) void action(() => restart(event.payload === "install")); });
  await listen("core-request-close", () => {
    if (exitPrompt) return;
    exitPrompt = true;
    void action(async () => {
      try { if (servers && await servers.allowExit() && await workspaces.allowExit()) await core.close(); }
      finally { exitPrompt = false; }
    });
  });
  await listen<Message>("core-message", (event) => {
    toast(event.payload.title, event.payload);
    scheduleRefresh();
  });
  await listen<string>("core-exit-error", (event) =>
    toast(`退出未完成：${event.payload}`, undefined, true),
  );
  await win.onResized(async () => {
    document.querySelector("#maximize-button")!.innerHTML = icon(
      (await win.isMaximized()) ? "restore" : "maximize",
    );
  });
  const [, catalogReady] = await Promise.all([refresh(), servers?.start() ?? Promise.resolve(true), workspaces.start(), connections.refresh()]);
  if (snapshot && catalogReady) await core.ready();
  setInterval(() => {
    if (layer === "status") scheduleRefresh();
  }, 1000);
}
void start().catch((error) => {
  lastError = String(error);
  renderTop();
  toast(lastError, undefined, true);
});
