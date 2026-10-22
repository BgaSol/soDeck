import { expect, it } from "vitest";
import { WorkspaceConnectionQueue } from "./workspaceConnectionQueue";
it("serializes setup, removes cancelled waiters and releases a cancelled owner", async () => {
  const queue = new WorkspaceConnectionQueue();
  const first = new AbortController(),
    second = new AbortController(),
    third = new AbortController();
  const done = await queue.acquire(first.signal);
  let next = false;
  void queue.acquire(second.signal).then(() => {
    throw new Error("cancelled waiter must never run");
  });
  const pending = queue.acquire(third.signal).then((release) => {
    next = true;
    return release;
  });
  await Promise.resolve();
  expect(next).toBe(false);
  second.abort();
  first.abort();
  const release = await pending;
  expect(next).toBe(true);
  done(); // late duplicate completion must not release the third connection.
  let fourth = false;
  const final = queue.acquire(new AbortController().signal).then((end) => {
    fourth = true;
    end();
  });
  await Promise.resolve();
  expect(fourth).toBe(false);
  release();
  await final;
  expect(fourth).toBe(true);
});
