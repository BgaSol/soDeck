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
  it("creates an independent tab next to an inactive source and preserves other tabs", () => {
    const first = openWorkspace(emptyWorkspaces(), server, true, "first");
    const other = openWorkspace(
      first,
      { ...server, id: "other", name: "Other" },
      true,
      "other",
    );
    const created = openWorkspace(other, server, true, "new", "first");
    expect(created.tabs.map((tab) => tab.id)).toEqual([
      "first",
      "new",
      "other",
    ]);
    expect(created.activeId).toBe("new");
    expect(created.tabs[0]).toBe(other.tabs[0]);
    expect(created.tabs[1].number).toBe(2);
    expect(created.tabs[1].server).not.toBe(created.tabs[0].server);
  });
  it("focuses an existing tab by default and explicitly opens independent duplicates", () => {
    const first = openWorkspace(emptyWorkspaces(), server);
    const duplicate = openWorkspace(first, server, true);
    expect(duplicate.tabs.map(tabTitle)).toEqual(["Web-01", "Web-01 · 2"]);
    expect(duplicate.tabs[0].id).not.toBe(duplicate.tabs[1].id);
    const focused = openWorkspace(duplicate, server);
    expect(focused.tabs).toHaveLength(2);
    expect(focused.activeId).toBe(first.activeId);
    const edited = { ...server, host: "new-host" };
    expect(openWorkspace(focused, edited).tabs[0].server.host).toBe(
      server.host,
    );
  });
  it("closes only the selected tab and chooses a surviving neighbour", () => {
    const first = openWorkspace(emptyWorkspaces(), server);
    const second = openWorkspace(first, { ...server, id: "db", name: "DB-01" });
    const third = openWorkspace(second, server, true);
    const backgroundClosed = closeWorkspace(third, second.activeId);
    expect(backgroundClosed.activeId).toBe(third.activeId);
    expect(backgroundClosed.tabs.map((tab) => tab.id)).toEqual([
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
