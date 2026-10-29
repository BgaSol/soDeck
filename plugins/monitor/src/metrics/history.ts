import type {Point} from '../protocol';

/** Fixed two-second buckets preserve ten minutes even under manual refresh. */
export class History {
  private series = new Map<string, Point[]>();
  add(id: string, time: number, value: number | null) {
    if (!Number.isFinite(time) || (value !== null && (!Number.isFinite(value) || value < 0))) return;
    let points = this.series.get(id);
    if (!points) {points = []; this.series.set(id, points);}
    const last = points.at(-1);
    if (last && time < last.time) return;
    if (last && Math.floor(last.time / 2000) === Math.floor(time / 2000)) points[points.length - 1] = {time, value};
    else points.push({time, value});
    while (points.length > 301 || (points[0] && points[0].time < time - 600000)) points.shift();
  }
  gap(time: number) {for (const id of this.series.keys()) this.add(id, time, null);}
  read(id: string, now: number, seconds: number): Point[] {
    return (this.series.get(id) ?? []).filter(p => p.time >= now - seconds * 1000).map(p => ({time: p.time - now, value: p.value}));
  }
  retain(ids: Set<string>) {for (const id of this.series.keys()) if (!ids.has(id)) this.series.delete(id);}
  bytes() {let total = 0; for (const [id, points] of this.series) total += 128 + id.length * 2 + points.length * 32; return total;}
  clear() {this.series.clear();}
}
