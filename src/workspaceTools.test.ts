import { describe, expect, it } from "vitest";
import {
  defaultTools,
  normalizeWorkspacePresentation as normalize,
  workspacePresentationReducer as reduce,
} from "./workspaceTools";
describe("workspace presentation", () => {
  it("opens, switches, and closes tools atomically", () => {
    let state = normalize();
    state = reduce(state, {
      type: "toggle-service",
      tool: "docker",
      visible: false,
    });
    expect(state).toMatchObject({
      resources: { tab: "docker" },
      tools: { filesOpen: true, servicesOpen: true, compactPane: "services" },
    });
    state = reduce(state, {
      type: "toggle-service",
      tool: "netbird",
      visible: true,
    });
    expect(state.resources.tab).toBe("netbird");
    expect(state.tools.servicesOpen).toBe(true);
    state = reduce(state, {
      type: "toggle-service",
      tool: "netbird",
      visible: true,
    });
    expect(state.tools.servicesOpen).toBe(false);
    expect(state.tools.compactPane).toBe("terminal");
  });
  it("reveals an automatically hidden requested tool and treats explicit navigation as idempotent", () => {
    let state = reduce(normalize(), { type: "toggle-files", visible: false });
    expect(state.tools.filesOpen).toBe(true);
    state = reduce(state, { type: "open-metric", metric: "memory" });
    expect(reduce(state, { type: "open-metric", metric: "memory" })).toEqual(
      state,
    );
    expect(state).toMatchObject({
      resources: { tab: "monitor", selectedMetric: "memory", detailOpen: true },
      tools: { servicesOpen: true },
    });
    state = reduce(state, { type: "return-to-terminal" });
    expect(state.tools).toMatchObject({
      filesOpen: true,
      servicesOpen: true,
      compactPane: "terminal",
    });
    expect(reduce(state, { type: "reveal-file" }).tools.lastSide).toBe("files");
  });
  it("migrates old snapshots deterministically and honors versioned snapshots", () => {
    expect(normalize({ view: { pane: "files" } }).tools).toMatchObject({
      filesOpen: true,
      servicesOpen: false,
      compactPane: "files",
    });
    expect(normalize({ view: { pane: "services" } }).tools).toMatchObject({
      filesOpen: false,
      servicesOpen: true,
    });
    expect(normalize({ view: { pane: "terminal" } }).tools).toMatchObject({
      filesOpen: true,
      servicesOpen: true,
      compactPane: "terminal",
    });
    expect(
      normalize({
        tools: { ...defaultTools, filesOpen: false },
        view: { pane: "files" },
      }).tools.filesOpen,
    ).toBe(false);
    expect(normalize({ resourceWidth: 400 }).layout).toEqual({
      fileWidth: 400,
      serviceWidth: 320,
    });
    expect(normalize({ tools: { version: 3 } as never }).tools).toEqual(
      defaultTools,
    );
  });
  it("validates stored widths and leaves unrelated preferences and open intent alone", () => {
    expect(
      normalize({ layout: { fileWidth: NaN, serviceWidth: -1 } }).layout,
    ).toEqual({ fileWidth: 280, serviceWidth: 360 });
    let state = reduce(normalize(), {
      type: "set-size",
      side: "fileWidth",
      width: 900,
    });
    expect(state.layout.fileWidth).toBe(520);
    expect(
      reduce(state, {
        type: "set-size",
        side: "serviceWidth",
        width: Infinity,
      }),
    ).toBe(state);
    const tools = state.tools;
    state = reduce(state, {
      type: "replace-resource-preferences",
      next: (p) => ({ ...p, minutes: 60 }),
    });
    expect(state.tools).toBe(tools);
    expect(state.resources.minutes).toBe(60);
  });
});
