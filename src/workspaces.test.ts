import { describe, expect, it } from "vitest";
import {
  closeWorkspace,
  emptyWorkspaces,
  openWorkspace,
  tabTitle,
} from "./workspaces";
import { emptyServer } from "./types";

describe("session workspaces", () => {
  const server = {
    ...emptyServer(),
    id: "web",
    name: "Web-01",
    username: "ops",
  };
  it("keeps one top-level tab per saved server even for a new-terminal intent", () => {
    const first = openWorkspace(emptyWorkspaces(), server, true, "first");
    const other = openWorkspace(
      first,
      { ...server, id: "other" },
      false,
      "other",
    );
    const entered = openWorkspace(other, server, true, "unused");
    expect(entered.tabs.map((t) => t.id)).toEqual(["first", "other"]);
    expect(entered.activeId).toBe("first");
    expect(entered.tabs.map(tabTitle)).toEqual(["Web-01", "Web-01"]);
  });
  it("keeps a zero-terminal workspace and treats different config IDs independently", () => {
    const first = openWorkspace(emptyWorkspaces(), server);
    expect(first.tabs[0].terminals).toEqual([]);
    expect(openWorkspace(first, server).activeId).toBe(first.activeId);
    const second = openWorkspace(first, { ...server, id: "other" });
    expect(second.tabs).toHaveLength(2);
  });
  it("closes only the selected server and chooses a surviving neighbour", () => {
    const first = openWorkspace(emptyWorkspaces(), server);
    const second = openWorkspace(first, { ...server, id: "db" });
    const third = openWorkspace(second, { ...server, id: "cache" });
    const backgroundClosed = closeWorkspace(third, second.activeId);
    expect(backgroundClosed.activeId).toBe(third.activeId);
    expect(backgroundClosed.tabs.map((t) => t.id)).toEqual([
      first.activeId,
      third.activeId,
    ]);
    const activeClosed = closeWorkspace(backgroundClosed, third.activeId);
    expect(activeClosed.activeId).toBe(first.activeId);
    expect(closeWorkspace(activeClosed, first.activeId)).toEqual(
      emptyWorkspaces(),
    );
  });
});
