import { expect, it } from "vitest";
import {
  definitelyRejected,
  previewExpired,
  submissionRequest,
} from "./dockerActionFlow";
import type { DockerPreview } from "../dockerApi";
it("freezes the reviewed action and treats the expiration boundary as expired", () => {
  const p = {
    id: "p",
    engine: { token: "engine" },
    action: { type: "rename", name: "before" },
    targetId: "target",
    expiresAt: 1000,
  } as DockerPreview;
  const r = submissionRequest(p);
  (p.action as { name: string }).name = "after";
  expect(r.action).toEqual({ type: "rename", name: "before" });
  expect(r.previewId).toBe("p");
  expect(previewExpired(p, 999)).toBe(false);
  expect(previewExpired(p, 1000)).toBe(true);
});
it("keeps transport and unknown failures uncertain", () => {
  expect(definitelyRejected("timeout")).toBe(false);
  expect(definitelyRejected("ipc")).toBe(false);
  expect(definitelyRejected("conflict")).toBe(true);
});
