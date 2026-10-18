import { expect, it, vi } from "vitest";
import { emptyFrame, NetBirdStore } from "./netbirdStore";
import type { Frame, Interest } from "./netbirdApi";
function fixture() {
  let receive!: (frame: Frame) => void;
  const api = {
    subscribe: vi.fn(
      async (_: string, __: Interest, callback: (frame: Frame) => void) => {
        receive = callback;
        callback({ ...emptyFrame(), subscriptionId: "sub", revision: 1 });
        return "sub";
      },
    ),
    unsubscribe: vi.fn(async (_: string) => {}),
    interest: vi.fn(async (_: string, __: Interest) => {}),
    ack: vi.fn(async (_: string, __: number) => {}),
    refresh: vi.fn(async (_: string) => {}),
  };
  return { api, send: (frame: Frame) => receive(frame) };
}
it("accepts frames before subscribe resolves, rejects older revisions, and ignores late epochs", async () => {
  const f = fixture(),
    store = new NetBirdStore("session", f.api);
  await store.start();
  expect(store.getState().revision).toBe(1);
  const frame = { ...emptyFrame(), subscriptionId: "sub", revision: 3 };
  f.send(frame);
  f.send({ ...frame, revision: 2 });
  expect(store.getState().revision).toBe(3);
  expect(f.api.ack.mock.calls.map((c) => c[1])).toEqual([1, 3, 2]);
  store.stop();
  f.send({ ...frame, revision: 4 });
  expect(store.getState().revision).toBe(3);
  expect(f.api.unsubscribe).toHaveBeenCalledWith("sub");
});
it("does not poll until requested and serializes the latest interest", async () => {
  const f = fixture(),
    store = new NetBirdStore("session", f.api);
  await store.start();
  expect(f.api.subscribe.mock.calls[0][1]).toBe("none");
  let release!: () => void;
  f.api.interest.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  store.setInterest("visible");
  store.setInterest("background");
  store.setInterest("none");
  release();
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(f.api.interest.mock.calls.map((c) => c[1])).toEqual([
    "none",
    "visible",
    "none",
  ]);
  store.stop();
});
it("unsubscribes a late result after stopping", async () => {
  const f = fixture();
  let resolve!: (id: string) => void;
  f.api.subscribe.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const store = new NetBirdStore("s", f.api),
    start = store.start();
  store.stop();
  resolve("late");
  await start;
  expect(f.api.unsubscribe).toHaveBeenCalledWith("late");
});
it("coalesces refresh requests and preserves the snapshot on failure", async () => {
  const f = fixture(),
    store = new NetBirdStore("s", f.api);
  await store.start();
  const frame = emptyFrame();
  frame.subscriptionId = "sub";
  frame.revision = 2;
  frame.snapshot.updatedAt = 1;
  frame.snapshot.connection = "connected";
  f.send(frame);
  let reject!: (error: Error) => void;
  f.api.refresh.mockImplementation(
    () =>
      new Promise((_, r) => {
        reject = r;
      }),
  );
  const a = store.refresh(),
    b = store.refresh();
  expect(a).toBe(b);
  reject(new Error("network error"));
  await expect(a).rejects.toThrow("network error");
  expect(store.getState().snapshot.connection).toBe("connected");
  expect(store.getState().snapshot.collectionStatus).toBe("stale");
  store.stop();
});
