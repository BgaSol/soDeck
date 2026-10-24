export interface SurfaceLayout {
  viewId: string;
  bounds: { x: number; y: number; width: number; height: number };
  visible: boolean;
}
interface Pending { layout: SurfaceLayout; resolve(): void; reject(error: unknown): void }
interface Queue { pending?: Pending }

/** Native layout changes have bounded buffering and cannot race a late hide
 * against the same view's new container. Intermediate resize values coalesce. */
export class SurfaceQueue {
  private readonly queues = new Map<string, Queue>();
  constructor(private readonly present: (layout: SurfaceLayout) => Promise<unknown>) {}
  submit(layout: SurfaceLayout): Promise<void> {
    let queue = this.queues.get(layout.viewId);
    const start = !queue;
    if (!queue) { queue = {}; this.queues.set(layout.viewId, queue); }
    // A replaced layout has no remaining side effect; its caller may finish.
    queue.pending?.resolve();
    const result = new Promise<void>((resolve, reject) => { queue!.pending = { layout, resolve, reject }; });
    if (start) void this.drain(layout.viewId, queue);
    return result;
  }
  private async drain(viewId: string, queue: Queue) {
    while (queue.pending) {
      const next = queue.pending; queue.pending = undefined;
      try { await this.present(next.layout); next.resolve(); }
      catch (error) { next.reject(error); }
    }
    this.queues.delete(viewId);
  }
}
