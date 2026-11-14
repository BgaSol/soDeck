const encoder = new TextEncoder();
const decode = bytes => new TextDecoder('utf-8').decode(bytes);
const pattern = query => new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu');
const join = parts => { const bytes = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let at = 0; for (const p of parts) { bytes.set(p, at); at += p.length; } return bytes; };
export function timestamp(value) {
  const m = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)$/.exec(value);
  if (!m) return null;
  const ms = Date.parse(m[1] + m[3]);
  return Number.isFinite(ms) ? BigInt(ms) * 1000000n + BigInt((m[2] || '').padEnd(9, '0')) : null;
}
export function level(text) {
  let value = /^\s*(?:\[\d[^\]]*\]\s*)?(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL)\b/i.exec(text)?.[1] || /^\s*\[(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL)\]/i.exec(text)?.[1];
  if (!value && text.trimStart().startsWith('{')) { try { const json = JSON.parse(text); value = json.level || json.severity; } catch {} }
  value = String(value || '').toUpperCase();
  return ({ WARNING: 'WARN', CRITICAL: 'FATAL', ERR: 'ERROR', INFORMATION: 'INFO' })[value] || (['TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR', 'FATAL'].includes(value) ? value : '');
}
export class LogModel {
  constructor(limits = {}) { this.limits = { recentRows: 50000, recentBytes: 16 * 1024 * 1024, historyRows: 200000, historyBytes: 64 * 1024 * 1024, ...limits }; this.reset(0); }
  reset(generation) {
    this.generation = generation; this.sources = new Map(); this.recent = []; this.history = []; this.sizes = { recent: 0, history: 0 };
    this.ids = { recent: new Set(), history: new Set() }; this.fixedIds = new Set();
    this.fixed = null; this.fixedSerial = 0; this.serial = 0; this.conditions = { query: '', levels: [], sources: [], only: false };
    this.expanded = new Set(); this.revision = 0; this.gap = false; this.cache = null; this.exporting = null;
  }
  changed() { this.revision++; this.cache = null; }
  register(resource, combined = false) { if (!this.sources.has(resource.id)) this.sources.set(resource.id, { resource, combined, envelope: '', offset: 0, envelopeStart: 0, channels: new Map(), skip: false }); }
  resetSource(id, offset, gap = false, pinned = false) {
    const s = this.sources.get(id); if (!s) return;
    s.envelope = ''; s.offset = offset; s.envelopeStart = offset; s.channels.clear(); s.skip = offset > 0; s.pinned=pinned;
    if (gap) { this.gap = true; this.changed(); }
  }
  append(id, bytes, end) {
    const s = this.sources.get(id); if (!s) return;
    if (end - bytes.length !== s.offset) throw new Error('日志块偏移不连续，需重新订阅');
    s.offset = end; s.envelope += decode(bytes); // Core envelopes are ASCII JSON with base64 payloads.
    let cut;
    while ((cut = s.envelope.indexOf('\n')) >= 0) {
      const line = s.envelope.slice(0, cut); s.envelope = s.envelope.slice(cut + 1);
      const start = s.envelopeStart; s.envelopeStart += line.length + 1;
      let e; try { e = JSON.parse(line); } catch { this.gap = true; continue; }
      if (e.v !== 1 || !['stdout', 'stderr'].includes(e.source)) { this.gap = true; continue; }
      const bytes = Uint8Array.from(atob(e.dataBase64), c => c.charCodeAt(0));
      let state = s.channels.get(e.source);
      if (!state) { state = { parts: [], size: 0, start, rawIndex: 0, skip: s.skip, oversized: false }; s.channels.set(e.source, state); }
      let from = 0;
      for (let i = 0; i <= bytes.length; i++) {
        if (i !== bytes.length && bytes[i] !== 10) continue;
        const until = i === bytes.length ? i : i + 1; if (until === from) continue;
        if (!state.size && !state.oversized) { state.start = start; state.rawIndex = from; }
        const part = bytes.slice(from, until);
        if (state.size + part.length <= 1024 * 1024 && !state.oversized) { state.parts.push(part); state.size += part.length; } else state.oversized = true;
        if (i !== bytes.length) {
          if(state.skip&&s.pinned&&timestamp(/^\S+/.exec(decode(join(state.parts)))?.[0]||'')!==null)state.skip=false;
          if (!state.skip) this.add(s, e.source, join(state.parts), state, s.envelopeStart, e.receivedAt, state.oversized);
          if (state.skip || state.oversized) this.gap = true;
          state.parts = []; state.size = 0; state.skip = false; state.oversized = false;
        }
        from = until;
      }
      state.end = s.envelopeStart; state.receivedAt = e.receivedAt;
    }
    this.trim('recent'); this.trim('history'); this.changed();
  }
  add(s, channel, bytes, state, end, receivedAt, incomplete = false) {
    const content = decode(bytes).replace(/\r?\n$/, ''), prefix = /^(\S+)\s/.exec(content), ns = prefix ? timestamp(prefix[1]) : null, r = s.resource;
    if (ns !== null && (ns < BigInt(r.start) * 1000000000n || (r.end != null && ns >= BigInt(r.end) * 1000000000n))) return;
    const text = ns === null ? content : content.slice(prefix[0].length), id = `${r.id}:${channel}:${state.start}:${state.rawIndex}`, kind = r.end == null ? 'recent' : 'history';
    if (this.ids[kind].has(id) || this.fixedIds.has(id)) return;
    const row = { id, resourceId: r.id, text, bytes, time: ns === null ? null : prefix[1], ns: ns ?? BigInt(Math.trunc(receivedAt)) * 1000000n, receivedAt,
      source: s.combined ? 'combined' : channel, level: level(text), start: state.start, end, incomplete, serial: ++this.serial, size: bytes.length + text.length * 2 + 240 };
    this[kind].push(row); this.ids[kind].add(id); this.sizes[kind] += row.size;
    if (this.fixed && kind === 'history') { this.fixed.push(row); this.fixedIds.add(id); }
  }
  end(id) {
    const s = this.sources.get(id); if (!s) return;
    for (const [channel, state] of s.channels) { if (state.size && !state.skip) this.add(s, channel, join(state.parts), state, state.end, state.receivedAt, state.oversized); state.parts = []; state.size = 0; }
    if (s.resource.tail) {
      const rows = this.history.filter(r => r.resourceId === id);
      let remove=Math.max(0,rows.length-2000),bytes=rows.slice(remove).reduce((n,r)=>n+r.bytes.length,0);while(bytes>2*1024*1024)bytes-=rows[remove++].bytes.length;
      if (remove) { const removed = new Set(rows.slice(0, remove).map(r => r.id)); this.history = this.history.filter(r => !removed.has(r.id)); this.sizes.history = this.history.reduce((n, r) => n + r.size, 0); for(const id of removed) { this.ids.history.delete(id); this.fixedIds.delete(id); } if (this.fixed) this.fixed = this.fixed.filter(r => !removed.has(r.id)); }
    }
    this.trim('history'); this.changed();
  }
  trim(kind) {
    let n = 0; while (this[kind].length - n > this.limits[kind + 'Rows'] || this.sizes[kind] > this.limits[kind + 'Bytes']) { const row = this[kind][n++]; this.sizes[kind] -= row.size; this.ids[kind].delete(row.id); this.gap = true; }
    if (n) this[kind].splice(0, n);
    if (this.fixed && kind === 'history') {
      let bytes = this.fixed.reduce((n, r) => n + r.size, 0), remove = 0;
      while (this.fixed.length - remove > this.limits.historyRows || bytes > this.limits.historyBytes) { const row = this.fixed[remove++]; bytes -= row.size; this.fixedIds.delete(row.id); this.gap = true; }
      if (remove) this.fixed.splice(0, remove);
    }
  }
  anchor() { if (!this.fixed) { this.fixed = this.all().slice(); this.fixedIds = new Set(this.fixed.map(r => r.id)); this.fixedSerial = this.serial; this.changed(); } return this.pinRanges(); }
  restore(ranges) { this.fixed = this.all().filter(row => ranges.some(r => r.id === row.resourceId && row.start >= r.start && row.end <= r.end)); this.fixedIds = new Set(this.fixed.map(r => r.id)); this.fixedSerial = this.serial; this.changed(); }
  latest() { this.fixed = null; this.fixedIds.clear(); this.expanded.clear(); this.changed(); }
  all() {
    if (this.cache?.all) return this.cache.all;
    const rows = (this.fixed || [...this.history, ...this.recent]).slice().sort((a, b) => a.ns < b.ns ? -1 : a.ns > b.ns ? 1 : a.serial - b.serial);
    if (!this.fixed && this.recent.length && rows.length > this.limits.recentRows) rows.splice(0, rows.length - this.limits.recentRows);
    this.cache = { all: rows }; return rows;
  }
  pinRanges() {
    const ranges = new Map(); for (const row of this.all()) { const r = ranges.get(row.resourceId) || { id: row.resourceId, start: row.start, end: row.end }; r.start = Math.min(r.start, row.start); r.end = Math.max(r.end, row.end); ranges.set(row.resourceId, r); } return [...ranges.values()];
  }
  query(conditions) { this.conditions = { query: String(conditions.query || '').slice(0, 1024), levels: conditions.levels || [], sources: conditions.sources || [], only: !!conditions.only }; this.expanded.clear(); this.changed(); }
  projection() {
    const all = this.all(); if (this.cache.projection) return this.cache.projection;
    const c = this.conditions, regex = c.query ? pattern(c.query) : null, rows = []; let total = 0;
    for (let index = 0; index < all.length; index++) {
      const row = all[index]; let count=0; if (regex) { regex.lastIndex = 0; while (regex.exec(row.text)) count++; }
      const filtered = (!c.levels.length || c.levels.includes(row.level || 'unknown')) && (!c.sources.length || c.sources.includes(row.source)); if (filtered) total += count;
      if ((filtered && (!c.only || count)) || this.expanded.has(row.id)) rows.push({ row, ordinal: index, count, context: !filtered || (c.only && !count) });
    }
    return this.cache.projection = { all, rows, total };
  }
  context(id) { const all = this.all(), at = all.findIndex(r => r.id === id); if (at < 0) return; for (const row of all.slice(Math.max(0, at - 10), at + 11)) this.expanded.add(row.id); this.changed(); }
  layout() { return { revision: this.revision, ids: this.projection().rows.map(r => r.row.id) }; }
  locate(id) { return this.projection().rows.findIndex(r => r.row.id === id); }
  display(row, matches = [], context = false) { return { id: row.id, text: row.text.slice(0, 8192), length: row.text.length, time: row.time, receivedAt: row.receivedAt, source: row.source, level: row.level, matches: matches.filter(([a]) => a < 8192), context, incomplete: row.incomplete }; }
  highlights(row) { if(!this.conditions.query)return [];const regex=pattern(this.conditions.query),matches=[];let found;while((found=regex.exec(row.text))&&found.index<8192)matches.push([found.index,found.index+found[0].length]);return matches; }
  view(start = 0, count = 300) {
    const p = this.projection(), day=row=>new Date(row.time||row.receivedAt).toLocaleDateString('zh-CN'),crossDay=p.rows.length>1&&day(p.rows[0].row)!==day(p.rows.at(-1).row);return { generation: this.generation, revision: this.revision, total: p.rows.length, matches: p.total, gap: this.gap, levels: [...new Set(p.all.map(r => r.level).filter(Boolean))],
      rows: p.rows.slice(start, start + Math.min(count, 500)).map((r, i) => ({ ...this.display(r.row, this.highlights(r.row), r.context), index: start + i,day:crossDay&&(!p.rows[start+i-1]||day(p.rows[start+i-1].row)!==day(r.row))?day(r.row):null })), ranges: this.pinRanges(), first: p.all[0]?.time, last: p.all.at(-1)?.time,
      newCount: this.fixed ? this.recent.filter(r => r.serial > this.fixedSerial).length : 0 };
  }
  detail(id) { const row = this.all().find(r => r.id === id); return row ? { ...this.display(row), text: row.text, raw: decode(row.bytes), resourceId: row.resourceId, start: row.start, end: row.end } : null; }
  navigate(id, direction = 1) { const p = this.projection(); let index = p.rows.findIndex(r => r.row.id === id); if (index < 0) index = direction > 0 ? -1 : 0; for (let n = 0; n < p.rows.length; n++) { index = (index + direction + p.rows.length) % p.rows.length; if (p.rows[index].count) return { index, id: p.rows[index].row.id }; } return null; }
  summary(height = 600) {
    const p = this.projection(), n = Math.max(1, Math.min(1200, height)), bins = Array.from({ length: n }, () => ({ width: 0, indent: 32, error: 0, warn: 0, matches: 0, index: 0, target: null }));
    for (let i = 0; i < p.rows.length; i++) { const { row, count } = p.rows[i], b = bins[Math.min(n - 1, Math.floor(i / Math.max(1, p.rows.length) * n))]; b.width = Math.max(b.width, Math.min(32, row.text.length / 4)); b.indent = Math.min(b.indent, /^\s*/.exec(row.text)[0].length / 2); const error = ['ERROR', 'FATAL'].includes(row.level); b.error += error ? 1 : 0; b.warn += row.level === 'WARN' ? 1 : 0; b.matches += count; if (!b.target || count || error) { b.index = i; b.target = row.id; } }
    return { generation: this.generation, revision: this.revision, total: p.rows.length, bins };
  }
  exportBegin(id, header, context = false) {
    if (this.exporting) throw new Error('已有导出正在进行');
    let rows; if (id) { const all = this.all(), at = all.findIndex(r => r.id === id); if (at < 0) throw new Error('记录已不在当前窗口'); rows = context ? all.slice(Math.max(0, at - 10), at + 11) : [all[at]]; } else rows = this.projection().rows.map(r => r.row);
    if (rows.some(r => r.incomplete)) throw new Error('当前范围含不完整长记录，请缩小时间范围重新读取后导出');
    const chunks = [encoder.encode(header + '\n\n'), ...rows.map(r => r.bytes)], length = chunks.reduce((n, b) => n + b.length, 0);
    this.exporting = { chunks, part: 0, at: 0, offset: 0, length }; return { length };
  }
  exportChunk() {
    const s = this.exporting; if (!s) throw new Error('导出窗口未固定'); const offset = s.offset, output = new Uint8Array(Math.min(16384, s.length - offset)); let wrote = 0;
    while (wrote < output.length) { const p = s.chunks[s.part], amount = Math.min(output.length - wrote, p.length - s.at); output.set(p.subarray(s.at, s.at + amount), wrote); wrote += amount; s.at += amount; if (s.at === p.length) { s.part++; s.at = 0; } }
    s.offset += wrote; return { offset, bytes: output, done: s.offset === s.length };
  }
  exportEnd() { this.exporting = null; }
}
if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  const model = new LogModel(); self.onmessage = event => {
    const m = event.data; try {
      const args = { reset: [m.generation], register: [m.resource, m.combined], resetSource: [m.id, m.offset, m.gap, m.pinned], append: [m.id, new Uint8Array(m.bytes), m.end], end: [m.id], anchor: [], restore: [m.ranges], latest: [], query: [m.conditions], context: [m.id], layout: [], locate: [m.id], view: [m.start, m.count], detail: [m.id], navigate: [m.id, m.direction], summary: [m.height], exportBegin: [m.id, m.header, m.context], exportChunk: [], exportEnd: [] };
      if (!(m.type in args)) throw new Error('未知日志索引请求'); const data = model[m.type](...args[m.type]); postMessage({ requestId: m.requestId, generation: model.generation, data }, data?.bytes ? [data.bytes.buffer] : []);
    } catch (error) { postMessage({ requestId: m.requestId, error: String(error.message || error) }); }
  };
}
