import morphdom from "morphdom";

// Keys belong to entities or to controls within an entity, never to a label.
function nodeKey(node: Node): string | undefined {
  if (!(node instanceof Element)) return;
  const entity = ["data-dom-key", "data-group", "data-view", "data-tab-id", "data-select", "data-split"];
  for (const attr of entity) if (node.hasAttribute(attr)) return `${attr}:${node.getAttribute(attr)}`;
  const scope = node.parentElement?.closest("[data-dom-scope],[data-group],[data-view],[data-tab-id],[data-select]");
  const prefix = scope ? nodeKey(scope) ?? scope.getAttribute("data-dom-scope") : "";
  if (node.id) return `${prefix}/id:${node.id}:${node.tagName}:${node instanceof HTMLInputElement ? node.type : ""}`;
  for (const attr of ["data-scroll", "data-persist", "data-network-server", "data-setting-row", "data-section"]) {
    if (node.hasAttribute(attr)) return `${prefix}/${attr}:${node.getAttribute(attr)}`;
  }
  for (const attr of ["data-component-action", "data-workspace-action", "data-tab-action", "data-tab-picker-action", "data-action"]) {
    if (node.hasAttribute(attr)) return `${prefix}/${attr}:${node.getAttribute(attr)}:${node.getAttribute("data-key") ?? node.getAttribute("data-id") ?? ""}`;
  }
}

/** Update only DOM owned by the caller. Child controllers are updated separately. */
export function updateDOM(element: HTMLElement, html: string) {
  const next = element.cloneNode(false) as HTMLElement;
  next.innerHTML = html;
  let selection: { node: HTMLInputElement | HTMLTextAreaElement; start: number; end: number; direction: "forward" | "backward" | "none" } | undefined;
  morphdom(element, next, {
    childrenOnly: true,
    getNodeKey: nodeKey,
    onBeforeElUpdated(from, to) {
      if (from instanceof HTMLDetailsElement) to.toggleAttribute("open", from.open);
      if (from === document.activeElement && (from instanceof HTMLInputElement || from instanceof HTMLTextAreaElement) && from.selectionStart !== null && from.selectionEnd !== null && from.value !== (to as HTMLInputElement).value) {
        selection = { node: from, start: from.selectionStart, end: from.selectionEnd, direction: from.selectionDirection ?? "none" };
      }
      return true;
    },
  });
  if (selection && document.activeElement === selection.node && selection.node.selectionStart !== null) selection.node.setSelectionRange(selection.start, selection.end, selection.direction);
}

/** A controller owns its press queue and disposes it with its DOM. */
export class DomRegion {
  private events = new AbortController();
  private pointer?: number;
  private pending = new Map<HTMLElement, string>();
  private timer?: ReturnType<typeof setTimeout>;
  private disposed = false;
  constructor(private root: HTMLElement, private updated?: () => void) {
    const signal = this.events.signal;
    root.addEventListener("pointerdown", event => {
      if (event.button === 0 && (event.target as Element).closest("button,summary,[role=button],[role=tab]")) this.pointer = event.pointerId;
    }, { capture: true, signal });
    root.addEventListener("click", () => { this.pointer = undefined; }, { capture: true, signal });
    const release = (event?: PointerEvent) => {
      if (this.pointer === undefined || event && event.pointerId !== this.pointer) return;
      clearTimeout(this.timer);
      this.timer = setTimeout(() => {
        this.pointer = undefined;
        const pending = [...this.pending]; this.pending.clear();
        if (this.disposed || !root.isConnected) return;
        for (const [element, html] of pending) if (root.contains(element)) updateDOM(element, html);
        if (pending.length) this.updated?.();
      }, 0);
    };
    for (const name of ["pointerup", "pointercancel", "lostpointercapture"] as const) root.ownerDocument.addEventListener(name, release, { capture: true, signal });
    root.ownerDocument.defaultView?.addEventListener("blur", () => release(), { signal });
  }
  update(element: HTMLElement, html: string) {
    if (this.disposed) return;
    if (this.pointer !== undefined) { this.pending.set(element, html); return; }
    this.pending.delete(element);
    updateDOM(element, html);
  }
  dispose() { this.disposed = true; this.events.abort(); clearTimeout(this.timer); this.pending.clear(); this.pointer = undefined; }
}
