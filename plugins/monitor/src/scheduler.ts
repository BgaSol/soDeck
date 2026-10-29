export type Lane = 'fast' | 'slow';
interface Task {owner: string; lane: Lane; run: () => Promise<void>}
/** One coalesced intention per owner/lane; independent quotas isolate slow mounts. */
export class Scheduler {
  private waiting = new Map<string, (active: boolean) => void>();
  private admitted = new Set<string>();
  private queue = new Map<string, Task>();
  private running = new Set<string>();
  private active: Record<Lane, number> = {fast: 0, slow: 0};
  private held = new Map<string,Lane>();
  private transientBytes = 0;
  private charge(lane:Lane) {return lane==='fast'?1310720:2621440;}
  want(owner: string, notify: (active: boolean) => void) {
    if (!this.waiting.has(owner)) this.waiting.set(owner, notify);
    this.admit();
    return this.admitted.has(owner);
  }
  drop(owner: string) {
    this.waiting.delete(owner); this.admitted.delete(owner);
    for (const [key, task] of this.queue) if (task.owner === owner) this.queue.delete(key);
    this.admit();
  }
  private admit() {
    for (const [owner, notify] of this.waiting) {
      if (this.admitted.size >= 8) break;
      if (!this.admitted.has(owner)) {this.admitted.add(owner); notify(true);}
    }
  }
  enqueue(owner: string, lane: Lane, run: () => Promise<void>) {
    const key = owner + ':' + lane;
    if (!this.admitted.has(owner) || this.running.has(key) || this.held.has(key) || this.queue.has(key)) return false;
    this.queue.set(key, {owner, lane, run}); this.drain(); return true;
  }
  private drain() {
    for (const [key, task] of this.queue) {
      if (this.active[task.lane] >= (task.lane === 'fast' ? 3 : 1)) continue;
      if(this.transientBytes+this.charge(task.lane)>4194304)continue;
      this.queue.delete(key); this.running.add(key); this.active[task.lane]++;this.transientBytes+=this.charge(task.lane);
      void Promise.resolve().then(task.run).catch(() => {}).finally(() => {
        this.running.delete(key);if(!this.held.has(key)){this.active[task.lane]--;this.transientBytes-=this.charge(task.lane);}this.drain();
      });
    }
  }
  hold(owner:string,lane:Lane){const key=owner+':'+lane;if(this.running.has(key))this.held.set(key,lane);}
  releaseHeld(owner:string){for(const [key,lane]of this.held)if(key.startsWith(owner+':')){this.held.delete(key);if(!this.running.has(key)){this.active[lane]--;this.transientBytes-=this.charge(lane);}}this.drain();}
  counts() {return {targets: this.admitted.size, waiting: this.waiting.size - this.admitted.size, fast: this.active.fast, slow: this.active.slow, queued: this.queue.size, transientBytes:this.transientBytes, cleanupSlots:this.held.size};}
}
