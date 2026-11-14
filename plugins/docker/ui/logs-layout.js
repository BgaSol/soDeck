// Measured record heights keep wrapped text anchored without an average-row approximation.
export class LogLayout {
  constructor() { this.ids = []; this.heights = new Map(); this.tree = new Float64Array(1); }
  reset(ids, clear = false) {
    this.ids = ids; if (clear) this.heights.clear();
    const retained = new Set(ids); for (const id of this.heights.keys()) if (!retained.has(id)) this.heights.delete(id);
    this.tree = new Float64Array(ids.length + 1);
    for (let i = 0; i < ids.length; i++) this.add(i, this.heights.get(ids[i]) || 23);
  }
  add(index, delta) { for (let n = index + 1; n < this.tree.length; n += n & -n) this.tree[n] += delta; }
  measure(index, height) { const id = this.ids[index]; if (!id) return; const old = this.heights.get(id) || 23; height = Math.max(23, height); if (Math.abs(height - old) < .5) return; this.heights.set(id, height); this.add(index, height - old); }
  offset(index) { let sum = 0; for (let n = Math.min(index, this.ids.length); n > 0; n -= n & -n) sum += this.tree[n]; return sum; }
  index(offset) {
    let index = 0, sum = 0, bit = 1; while (bit * 2 < this.tree.length) bit *= 2;
    for (; bit; bit >>= 1) { const next = index + bit; if (next < this.tree.length && sum + this.tree[next] <= offset) { index = next; sum += this.tree[next]; } }
    return Math.max(0, Math.min(index, this.ids.length - 1));
  }
  anchor(top) { const index = this.index(top); return { id: this.ids[index], pixel: top - this.offset(index) }; }
  restore(anchor) { const index = this.ids.indexOf(anchor?.id); return index < 0 ? null : this.offset(index) + anchor.pixel; }
}
