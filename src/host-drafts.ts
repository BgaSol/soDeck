import { core, type Snapshot, type Manifest } from "./core";
import type { ViewState } from "./views";
import type { CloseDraft } from "./close";
import { networkAction, networkDirty } from "./update-network";

type Source = NonNullable<Manifest["updateSource"]>;
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const normalized = (source?: Source) => ({ repository: source?.repository.trim() ?? "", tagPrefix: source?.tagPrefix.trim() ?? "" });

/** Draft lifetime belongs to the host, including pages currently hidden. */
export class HostDrafts {
  private baselines = new Map<string, Source | null>();
  private composing = new Set<EventTarget>();
  private events = new AbortController();
  private waiting = new Set<() => void>();
  constructor(private snapshot: () => Snapshot | undefined, private state: ViewState, private render: () => void) {
    const signal = this.events.signal;
    document.addEventListener("compositionstart", event => { if (event.target) this.composing.add(event.target); }, { capture: true, signal });
    document.addEventListener("compositionend", event => {
      if (event.target) this.composing.delete(event.target);
      // The final input event follows compositionend in the same browser task.
      setTimeout(() => { if (!this.composing.size) for (const done of [...this.waiting]) done(); }, 0);
    }, { capture: true, signal });
  }
  async flushInputs() {
    if (!this.composing.size) return;
    await new Promise<void>((resolve, reject) => {
      const done = () => { clearTimeout(timer); this.waiting.delete(done); resolve(); };
      const timer = setTimeout(() => { this.waiting.delete(done); reject(new Error("输入法仍在组合，请完成输入后重新关闭")); }, 8000);
      this.waiting.add(done);
    });
  }
  dispose() { this.events.abort(); this.composing.clear(); for (const done of [...this.waiting]) done(); }
  input(id: string, field: string, value: string) {
    const s = this.snapshot(); if (!s || this.state.busy.has(`source:${id}`)) return;
    const saved = s.plugins.find(p => p.manifest.id === id)?.manifest.updateSource;
    if (!this.baselines.has(id)) this.baselines.set(id, saved ? structuredClone(saved) : null);
    const draft = this.state.sourceDrafts[id] ?? { repository: saved?.repository ?? "", tagPrefix: saved?.tagPrefix ?? "" };
    if (field === "repository") draft.repository = value; else draft.tagPrefix = value;
    if (same(normalized(draft), normalized(this.baselines.get(id) ?? undefined))) { delete this.state.sourceDrafts[id]; this.baselines.delete(id); }
    else this.state.sourceDrafts[id] = draft;
  }
  discardSource(id: string) { if (this.state.busy.has(`source:${id}`)) return; delete this.state.sourceDrafts[id]; this.baselines.delete(id); }
  async saveSource(id: string, clear = false) {
    const s = this.snapshot(), key = `source:${id}`; if (!s) throw new Error("尚未读取应用状态");
    if (this.state.busy.has(key)) throw new Error("来源正在保存，请等待原保存结果");
    const plugin = s.plugins.find(p => p.manifest.id === id); if (!plugin) throw new Error("插件记录不存在，草稿已保留");
    const source = clear ? null : normalized(this.state.sourceDrafts[id] ?? plugin.manifest.updateSource);
    const baseline = this.baselines.has(id) ? this.baselines.get(id)! : plugin.manifest.updateSource ?? null;
    this.state.busy.add(key); this.render();
    try {
      await core.updateSource(id, source, { source: baseline });
      // Refresh can replace the snapshot while this write is in flight.
      const current = this.snapshot()?.plugins.find(p => p.manifest.id === id);
      if (current) current.manifest.updateSource = source ?? undefined;
      delete this.state.sourceDrafts[id]; this.baselines.delete(id);
    } finally { this.state.busy.delete(key); this.render(); }
  }
  async closeInputs(decision: "collect" | "save" | "discard"): Promise<CloseDraft[]> {
    const s = this.snapshot(); if (!s) return [];
    const busy = this.state.busy.has("network-save") || [...this.state.busy].some(key => key.startsWith("source:"));
    if (decision !== "collect" && busy) throw new Error("宿主设置正在保存，请等待原保存结果后重新关闭");
    if (decision === "save") {
      if (networkDirty(s, this.state)) {
        await networkAction("network-save", "", s, this.state, this.render);
        if (this.state.networkDraft) throw new Error(this.state.networkError || "下载网络设置尚未保存");
        const current = this.snapshot(); if (current) current.preferences.updateNetwork = s.preferences.updateNetwork;
      }
      for (const id of Object.keys(this.state.sourceDrafts)) await this.saveSource(id);
    } else if (decision === "discard") {
      this.state.networkDraft = undefined; this.state.networkResults = undefined;
      this.state.networkGeneration = (this.state.networkGeneration ?? 0) + 1;
      this.state.sourceDrafts = {}; this.baselines.clear(); this.render();
    }
    return [
      ...(networkDirty(s, this.state) || this.state.busy.has("network-save") ? [{ id: "update-network", title: "下载与网络设置", dirty: networkDirty(s, this.state), busy: this.state.busy.has("network-save") }] : []),
      ...Object.keys(this.state.sourceDrafts).map(id => ({ id: `source:${id}`, title: `${s.plugins.find(p => p.manifest.id === id)?.manifest.name ?? id} 更新来源`, dirty: true, busy: this.state.busy.has(`source:${id}`) })),
    ];
  }
}
