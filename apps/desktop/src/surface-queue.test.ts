import { describe, expect, it } from "vitest";
import { SurfaceQueue, type SurfaceLayout } from "./surface-queue";

const layout = (visible: boolean, width: number): SurfaceLayout => ({ viewId: "v1", bounds: { x: 0, y: 0, width, height: 200 }, visible });
describe("native surface presentation", () => {
  it("serializes an old-container hide before a new-container show", async () => {
    const calls: SurfaceLayout[] = []; const releases: (() => void)[] = [];
    const queue = new SurfaceQueue(value => { calls.push(value); return new Promise(resolve => releases.push(() => resolve(null))); });
    const hidden = queue.submit(layout(false, 0)); const shown = queue.submit(layout(true, 600));
    expect(calls.map(call => call.visible)).toEqual([false]);
    releases.shift()!(); await hidden;
    expect(calls.map(call => call.visible)).toEqual([false, true]);
    releases.shift()!(); await shown;
  });
  it("bounds resize buffering to one pending latest value", async () => {
    const calls: SurfaceLayout[] = []; let release!: () => void;
    const queue = new SurfaceQueue(value => { calls.push(value); return calls.length === 1 ? new Promise(resolve => { release = () => resolve(null); }) : Promise.resolve(null); });
    const first = queue.submit(layout(true, 500)); const intermediate = queue.submit(layout(true, 550)); const last = queue.submit(layout(true, 700));
    await intermediate; expect(calls).toHaveLength(1);
    release(); await Promise.all([first, last]);
    expect(calls.map(call => call.bounds.width)).toEqual([500, 700]);
  });
  it("continues cleanup after an earlier layout fails", async () => {
    let attempts = 0; const queue = new SurfaceQueue(async () => { if (++attempts === 1) throw new Error("view busy"); });
    const failure = queue.submit(layout(true, 300)).catch(error => error.message);
    const cleanup = queue.submit(layout(false, 0));
    expect(await failure).toBe("view busy"); await cleanup; expect(attempts).toBe(2);
  });
});
