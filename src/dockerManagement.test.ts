import { describe, expect, it } from "vitest";
import {
  DockerManagementStore,
  type DockerManagementApi,
} from "./dockerManagement";
import type { DockerFrame } from "./dockerApi";
import { dockerMatches, normalizeDockerSnapshot } from "./dockerPresentation";
const frame = (revision: number): DockerFrame => ({
  subscriptionId: "sub",
  revision,
  engine: {
    token: "token",
    generation: "monitor",
    engineGeneration: 1,
    endpoint: "unix:///docker.sock",
    daemonId: "engine",
  },
  operations: [],
  detail: null,
  error: null,
});
function fixture() {
  let receive!: (frame: DockerFrame) => void;
  const interests: (string | null)[] = [],
    removed: string[] = [],
    acks: number[] = [];
  const api: DockerManagementApi = {
    subscribe: async (_, callback) => {
      receive = callback;
      callback(frame(1));
      return "sub";
    },
    unsubscribe: async (id) => {
      removed.push(id);
    },
    interest: async (_, target) => {
      interests.push(target);
    },
    ack: async (_, revision) => {
      acks.push(revision);
    },
    refresh: async () => {},
  };
  return {
    api,
    send: (value: DockerFrame) => receive(value),
    interests,
    removed,
    acks,
  };
}
describe("Docker management subscriptions", () => {
  it("accepts the initial Channel frame before subscribe resolves and rejects older revisions", async () => {
    const f = fixture(),
      store = new DockerManagementStore("session", f.api);
    await store.start();
    expect(store.getState().revision).toBe(1);
    f.send({ ...frame(3), error: "latest" });
    f.send({ ...frame(2), error: "old" });
    expect(store.getState().error).toBe("latest");
    expect(f.acks).toEqual([1, 3, 2]);
    store.stop();
    f.send(frame(4));
    expect(store.getState().revision).toBe(3);
    expect(f.removed).toEqual(["sub"]);
  });
  it("detaches late subscription results without replaying any operation", async () => {
    const f = fixture();
    let resolve!: (id: string) => void;
    f.api.subscribe = () =>
      new Promise((r) => {
        resolve = r;
      });
    const store = new DockerManagementStore("session", f.api),
      start = store.start();
    store.stop();
    resolve("late");
    await start;
    expect(f.removed).toEqual(["late"]);
    expect(f.interests).toEqual([]);
  });
  it("coalesces detail changes while an interest request is in flight", async () => {
    const f = fixture(),
      store = new DockerManagementStore("session", f.api);
    await store.start();
    let resolve!: () => void;
    f.api.interest = async (_, target) => {
      f.interests.push(target);
      if (target === "a")
        await new Promise<void>((r) => {
          resolve = r;
        });
    };
    store.setTarget("a");
    store.setTarget("b");
    store.setTarget(null);
    resolve();
    await new Promise((r) => setTimeout(r, 0));
    expect(f.interests).toEqual([null, "a", null]);
  });
});
it("attention uses known unhealthy IDs and does not mistake unknown health for healthy", () => {
  const c = {
    id: "a",
    name: "web",
    image: "app",
    state: "running",
    status: "Up",
  };
  expect(dockerMatches(c, "", "attention", ["a"])).toBe(true);
  expect(dockerMatches(c, "", "attention")).toBe(false);
  expect(dockerMatches({ ...c, state: "created" }, "", "stopped")).toBe(true);
  expect(normalizeDockerSnapshot({ filter: "attention" }).filter).toBe(
    "attention",
  );
});
