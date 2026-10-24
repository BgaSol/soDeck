import { describe, expect, it, vi } from "vitest";
import { CoreError, createCore, decodeBytes, encodeBytes } from "./index";

describe("core transport", () => {
  it("binds the request ID and retains structured failure details", async () => {
    const client = createCore(async request => ({ requestId: request.requestId, error: { code: "HOST_KEY_UNKNOWN", message: "需要核对主机身份", details: { fingerprint: "SHA256:example" } } }));
    await expect(client.call("connections.acquire")).rejects.toMatchObject({ code: "HOST_KEY_UNKNOWN", details: { fingerprint: "SHA256:example" } });
  });
  it("does not repeat a timed out write", async () => {
    vi.useFakeTimers();
    const transport = vi.fn(() => new Promise<never>(() => {}));
    const failure = createCore(transport).call("pty.write", { bytes: "YQ==" }, 50).catch(error => error);
    await vi.advanceTimersByTimeAsync(51);
    expect(await failure).toBeInstanceOf(CoreError);
    expect(transport).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });
  it("rejects a response for another operation", async () => {
    await expect(createCore(async () => ({ requestId: "other", result: 1 })).call("resources.list")).rejects.toMatchObject({ code: "PROTOCOL_ERROR" });
  });
  it("preserves arbitrary terminal bytes across base64", () => {
    const bytes = new Uint8Array(65536).map((_, index) => index % 256);
    expect(decodeBytes(encodeBytes(bytes))).toEqual(bytes);
  });
});
