export const escape = (value: unknown) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
export const date = (value: number) =>
  new Date(value).toLocaleString("zh-CN", { hour12: false });
export const statusText = {
  unloaded: "未加载",
  starting: "正在加载",
  running: "运行中",
  stopping: "正在卸载",
  failed: "失败",
};
export function icon(name: string) {
  const paths: Record<string, string> = {
    plus: '<path d="M12 5v14M5 12h14"/>',
    chevron: '<path d="m6 9 6 6 6-6"/>',
    more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    terminal: '<path d="m4 6 6 6-6 6m9 0h7"/>',
    server: '<rect x="3" y="3" width="18" height="7" rx="2"/><rect x="3" y="14" width="18" height="7" rx="2"/><path d="M7 6.5h.01M7 17.5h.01M12 6.5h5M12 17.5h5"/>',
    network: '<circle cx="12" cy="5" r="3"/><circle cx="5" cy="19" r="3"/><circle cx="19" cy="19" r="3"/><path d="M12 8v5M5 16v-3h14v3"/>',
    search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
    grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
    settings:
      '<path d="m9 3-1 3-3 1-1 4 2 2v3l3 2 3-1 3 1 3-2v-3l2-2-1-4-3-1-1-3z"/><circle cx="12" cy="11" r="3"/>',
    bell: '<path d="M5 17h14l-2-3V9a5 5 0 0 0-10 0v5zM10 21h4"/>',
    close: '<path d="m6 6 12 12M6 18 18 6"/>',
    minus: '<path d="M5 12h14"/>',
    maximize: '<rect x="5" y="5" width="14" height="14" rx="1"/>',
    restore: '<path d="M8 5h11v11M5 8h11v11H5z"/>',
    plug: '<path d="M8 3v5m8-5v5M6 8h12v3a6 6 0 0 1-12 0zm6 9v4"/>',
    folder: '<path d="M3 7V5h7l2 2h9v12H3z"/>',
    arrow: '<path d="M5 12h14m-5-5 5 5-5 5"/>',
    log: '<path d="M7 3h10l3 3v15H4V3zM8 10h8M8 14h8M8 18h5"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1 1m12 12 1 1M5 19l1-1M18 6l1-1"/>',
    info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6m0-11v1"/>',
    copy: '<rect x="8" y="8" width="12" height="13" rx="2"/><path d="M16 8V3H3v13h5"/>',
    check: '<path d="m5 12 4 4L19 6"/>',
    download: '<path d="M12 3v12m-4-4 4 4 4-4M4 16v5h16v-5"/>',
    restart: '<path d="M20 7v5h-5M20 12a8 8 0 1 0-2 5M20 7v5"/>',
  };
  return `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">${paths[name] ?? paths.info}</svg>`;
}
export const button = (
  action: string,
  label: string,
  name?: string,
  cls = "",
  attrs = "",
) =>
  `<button type="button" data-action="${action}" class="${cls}" ${attrs}>${name ? icon(name) : ""}${label}</button>`;
export const empty = (title: string, body: string, name = "plug") =>
  `<div class="empty"><div class="empty-icon">${icon(name)}</div><h3>${title}</h3><p>${body}</p></div>`;
const rendered = new WeakMap<HTMLElement, { input: string; output: string }>();
export function replace(element: HTMLElement, html: string) {
  const previous = rendered.get(element);
  if (element.innerHTML === html || (previous?.input === html && previous.output === element.innerHTML)) return;
  const active = document.activeElement as HTMLInputElement | null;
  const id = active && element.contains(active) ? active.id : "";
  const focusedDetails = active?.matches("summary") && element.contains(active)
    ? active.closest<HTMLDetailsElement>("details[data-persist]")?.dataset.persist
    : undefined;
  const selection =
    active && "selectionStart" in active
      ? [active.selectionStart, active.selectionEnd]
      : null;
  const scroll = [
    ...element.querySelectorAll<HTMLElement>("[data-scroll]"),
  ].map((e) => [e.dataset.scroll, e.scrollTop] as const);
  const expanded = [...element.querySelectorAll<HTMLDetailsElement>('details[data-persist][open]')].map(d => d.dataset.persist!);
  element.innerHTML = html;
  for (const key of expanded) element.querySelector(`details[data-persist="${CSS.escape(key)}"]`)?.setAttribute("open", "");
  for (const [key, top] of scroll)
    element
      .querySelector<HTMLElement>(`[data-scroll="${key}"]`)
      ?.scrollTo(0, top);
  if (focusedDetails)
    element.querySelector<HTMLElement>(`details[data-persist="${CSS.escape(focusedDetails)}"] > summary`)?.focus({ preventScroll: true });
  if (id) {
    const target = document.getElementById(id) as HTMLInputElement | null;
    target?.focus({ preventScroll: true });
    if (selection && target && ["text", "search"].includes(target.type))
      target.setSelectionRange(selection[0], selection[1]);
  }
  rendered.set(element, { input: html, output: element.innerHTML });
}

// Workspace chrome has stable nodes; plugin frames and editable content keep
// their separate lifetime and rendering rules.
function chromeKey(node: Node): string | null {
  if (!(node instanceof Element)) return null;
  for (const attr of ["data-view", "data-component-action", "data-workspace-action", "data-id"]) {
    if (node.hasAttribute(attr)) return `${attr}:${node.getAttribute(attr)}:${node.getAttribute("data-key") ?? node.getAttribute("data-id") ?? ""}`;
  }
  return null;
}
function patchChildren(parent: Node, source: Node) {
  const old = [...parent.childNodes], used = new Set<Node>();
  [...source.childNodes].forEach((next, index) => {
    const key = chromeKey(next);
    let current: Node | undefined = key ? old.find(node => !used.has(node) && chromeKey(node) === key) : old[index];
    if (!current || used.has(current) || (!key && chromeKey(current)) || current.nodeType !== next.nodeType || current.nodeName !== next.nodeName) current = next.cloneNode(true);
    else if (current instanceof Element && next instanceof Element) {
      for (const attr of [...current.attributes]) if (!next.hasAttribute(attr.name)) current.removeAttribute(attr.name);
      for (const attr of [...next.attributes]) if (current.getAttribute(attr.name) !== attr.value) current.setAttribute(attr.name, attr.value);
      patchChildren(current, next);
    } else if (current.nodeValue !== next.nodeValue) current.nodeValue = next.nodeValue;
    used.add(current);
    if (parent.childNodes[index] !== current) parent.insertBefore(current, parent.childNodes[index] ?? null);
  });
  for (const node of old) if (!used.has(node)) parent.removeChild(node);
}
let chromePress: { button: Element; pointerId: number } | undefined;
const pendingChrome = new Map<HTMLElement, string>();
document.addEventListener("pointerdown", event => {
  const button = (event.target as Element).closest?.("[data-component-action], [data-workspace-action]");
  if (event.button === 0 && button) chromePress = { button, pointerId: event.pointerId };
}, true);
// The browser has committed the click target; action handlers may now update
// disabled/selected state immediately without interrupting the press.
document.addEventListener("click", () => { chromePress = undefined; }, true);
const releaseChrome = (event?: PointerEvent) => {
  if (!chromePress || (event && event.pointerId !== chromePress.pointerId)) return;
  const press = chromePress;
  // Complete pointerup and click before a queued layout moves the target.
  setTimeout(() => {
    if (chromePress && chromePress !== press) return;
    chromePress = undefined;
    const pending = [...pendingChrome]; pendingChrome.clear();
    for (const [element, html] of pending) if (element.isConnected) patchChrome(element, html);
  }, 0);
};
document.addEventListener("pointerup", releaseChrome, true);
document.addEventListener("pointercancel", releaseChrome, true);
window.addEventListener("blur", () => releaseChrome());
export function patchChrome(element: HTMLElement, html: string) {
  if (chromePress && element.contains(chromePress.button)) { pendingChrome.set(element, html); return; }
  pendingChrome.delete(element);
  const template = document.createElement("template"); template.innerHTML = html;
  const active = document.activeElement as HTMLElement | null;
  const hadFocus = active && element.contains(active);
  patchChildren(element, template.content);
  if (hadFocus && active.isConnected && document.activeElement !== active) active.focus({ preventScroll: true });
}
