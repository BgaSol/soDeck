import type {Counter, Rate} from '../protocol';
interface Baseline {first: bigint; second: bigint; time: number; source: string}
export class Counters {
  private previous = new Map<string, Baseline>();
  update(rows: Counter[], time: number, source: string, maximumGapMs: number): Rate[] {
    const next = new Map<string, Baseline>();
    const result = rows.map(row => {
      const current = {first: BigInt(row.first), second: BigInt(row.second), time, source};
      const old = this.previous.get(row.id), dt = old ? time - old.time : 0;
      const valid = old && old.source === source && dt > 0 && dt <= maximumGapMs && current.first >= old.first && current.second >= old.second;
      next.set(row.id, current);
      const rate = (difference: bigint) => {const n = Number(difference) * 1000 / dt; return Number.isFinite(n) && n <= Number.MAX_SAFE_INTEGER ? n : null;};
      return {...row, first: valid ? rate(current.first - old.first) : null, second: valid ? rate(current.second - old.second) : null};
    });
    this.previous = next;
    return result;
  }
  clear() {this.previous.clear();}
}
