import { describe, expect, it } from "vitest";
import { dockerCanAttempt, emptyDocker } from "./serverMonitoring";

describe("Docker operation availability", () => {
  it("retains a verified target only for a classified timeout", () => {
    const docker = emptyDocker();
    expect(dockerCanAttempt(docker)).toBe(false);
    docker.engine = {
      value: {
        state: "running",
        endpoint: "unix:///docker.sock",
        message: "ok",
      },
      status: "ready",
      updatedAt: 1,
      error: null,
    };
    expect(dockerCanAttempt(docker)).toBe(true);
    docker.engine.status = "stale";
    for (const kind of [
      null,
      "network",
      "permission",
      "authentication",
      "hostKey",
      "disconnected",
    ]) {
      docker.engineErrorKind = kind;
      expect(dockerCanAttempt(docker)).toBe(false);
    }
    docker.engineErrorKind = "timeout";
    expect(dockerCanAttempt(docker)).toBe(true);
    docker.engine.value!.state = "permission";
    expect(dockerCanAttempt(docker)).toBe(false);
    docker.engine.value = null;
    expect(dockerCanAttempt(docker)).toBe(false);
  });
  it("does not let list freshness revoke a reachable engine", () => {
    const docker = emptyDocker();
    docker.engine = {
      value: {
        state: "running",
        endpoint: "unix:///docker.sock",
        message: "ok",
      },
      status: "ready",
      updatedAt: 1,
      error: null,
    };
    docker.containers.status = "stale";
    docker.containers.error = "远端资源查询超时";
    expect(dockerCanAttempt(docker)).toBe(true);
  });
});
