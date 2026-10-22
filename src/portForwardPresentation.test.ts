import { describe, expect, it } from "vitest";
import { emptyForward, type ForwardView } from "./portForwardApi";
import {
  draftSettings,
  forwardDraft,
  updateTargetPort,
  validateForwardDraft,
} from "./portForwardDraft";
import { forwardPresentation } from "./portForwardPresentation";
import type { Server } from "./types";

const server = {
  id: "s",
  name: "新服务器名称",
  host: "new.host",
  port: 22,
  jumpServerId: null,
} as Server;

describe("forwarding form", () => {
  it("keeps explicit auto mode separate from an empty fixed port and preserves a manual port", () => {
    let draft = forwardDraft(emptyForward("s"));
    expect(draft.targetHost).toBe("127.0.0.1");
    expect(draft.targetPort).toBe("");
    expect(validateForwardDraft(draft, ["s"]).localPort).toBeTruthy();
    draft = updateTargetPort(draft, "5432");
    expect(draft.localPort).toBe("5432");
    draft = updateTargetPort(
      { ...draft, localPort: "15432", localPortEdited: true, portMode: "auto" },
      "6543",
    );
    expect(draftSettings(draft).listenPort).toBeNull();
    expect(draft.localPort).toBe("15432");
    expect(draftSettings({ ...draft, portMode: "fixed" }).listenPort).toBe(
      15432,
    );
  });
  it("validates numeric strings before conversion and accepts bracketed IPv6", () => {
    const draft = {
      ...forwardDraft(emptyForward("s")),
      name: "数据库",
      targetPort: "5432",
      portMode: "auto" as const,
    };
    for (const targetPort of ["", "0", "65536", "1.5", "1e3", " 80"])
      expect(
        validateForwardDraft({ ...draft, targetPort }, ["s"]).targetPort,
      ).toBeTruthy();
    for (const targetHost of [
      "http://db.local",
      "db.local:80",
      "db.local/path",
      "bad host",
      "[bad:ipv6]",
      "[::1]:80",
    ])
      expect(
        validateForwardDraft({ ...draft, targetHost }, ["s"]).targetHost,
      ).toBeTruthy();
    const ipv6 = { ...draft, targetHost: "[2001:db8::1]" };
    expect(validateForwardDraft(ipv6, ["s"])).toEqual({});
    expect(draftSettings(ipv6).service).toMatchObject({
      kind: "portForward",
      targetHost: "2001:db8::1",
    });
    expect(validateForwardDraft(draft, []).serverId).toBeTruthy();
  });
});

describe("effective forwarding configuration", () => {
  it("keeps running search, address and route on one snapshot, then switches to saved settings when stopped", () => {
    const rule = emptyForward("s");
    rule.name = "数据库";
    rule.settings.service.targetHost = "new.db";
    rule.settings.service.targetPort = 5432;
    const view = {
      rule,
      pendingChanges: true,
      serverError: null,
      runtime: {
        phase: "ready",
        actualAddress: "[::1]:43000",
        settings: {
          ...rule.settings,
          service: { ...rule.settings.service, targetHost: "old.db" },
          listenAddress: "::1",
        },
        connection: {
          hops: [{ server: { ...server, name: "原服务器", host: "old.host" } }],
        },
      },
    } as unknown as ForwardView;
    const running = forwardPresentation(view, [server]);
    expect(running.local).toBe("[::1]:43000");
    expect(running.target).toBe("old.db:5432");
    expect(running.search).toContain("old.host");
    expect(running.search).not.toContain("new.db");
    expect(running.copyable).toBe(true);
    view.runtime!.phase = "reconnecting";
    expect(forwardPresentation(view, [server]).copyable).toBe(false);
    view.runtime!.phase = "stopped";
    const stopped = forwardPresentation(view, [server]);
    expect(stopped.local).toBe("启动后自动分配");
    expect(stopped.target).toBe("new.db:5432");
    expect(stopped.hops[0].host).toBe("new.host");
    expect(stopped.copyable).toBe(false);
  });
});
