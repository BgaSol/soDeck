import { describe, expect, it } from "vitest";
import { resolveWorkspaceLayout, workspaceMetrics } from "./workspaceSizing";
import { defaultTools } from "./workspaceTools";
const both = {
  ...defaultTools,
  servicesOpen: true,
  lastSide: "services" as const,
  compactPane: "services" as const,
};
describe("workspace tool layout", () => {
  it("docks both tools when they fit and gives priority to the latest side otherwise", () => {
    expect(resolveWorkspaceLayout({ width: 1440, tools: both })).toMatchObject({
      mode: "docked",
      visible: { files: true, terminal: true, services: true },
    });
    for (const width of [1280, 1024])
      expect(resolveWorkspaceLayout({ width, tools: both })).toMatchObject({
        visible: { files: false, terminal: true, services: true },
        services: { placement: "docked", width: 360 },
      });
    expect(resolveWorkspaceLayout({ width: 860, tools: both })).toMatchObject({
      mode: "overlay",
      services: { placement: "overlay" },
      terminalVisible: true,
    });
    expect(resolveWorkspaceLayout({ width: 650, tools: both }).visible).toEqual(
      { files: false, terminal: false, services: true },
    );
  });
  it("leaves terminal space, bounds overlays, and displays exactly one compact pane", () => {
    for (const scale of [1, 1.25, 1.5])
      for (let width = 260; width <= 2100; width += 13)
        for (const filesOpen of [false, true])
          for (const servicesOpen of [false, true]) {
            const m = workspaceMetrics(scale);
            const result = resolveWorkspaceLayout({
              width,
              metrics: m,
              tools: { ...both, filesOpen, servicesOpen },
              preferred: { fileWidth: 520, serviceWidth: 480 },
            });
            const { files, services } = result;
            if (result.mode === "single")
              expect(
                Object.values(result.visible).filter(Boolean),
              ).toHaveLength(1);
            else {
              const occupied =
                2 * m.rail +
                (files.placement === "docked" ? files.width + m.separator : 0) +
                (services.placement === "docked"
                  ? services.width + m.separator
                  : 0);
              expect(width - occupied).toBeGreaterThanOrEqual(
                m.terminal - 0.01,
              );
            }
            for (const panel of [files, services])
              if (panel.placement === "overlay")
                expect(panel.width).toBeLessThanOrEqual(width - 2 * m.rail);
          }
  });
  it("compresses rendered width without mutating preferences and recovers it on widening", () => {
    const preferred = { fileWidth: 520, serviceWidth: 480 };
    expect(
      resolveWorkspaceLayout({ width: 1024, tools: both, preferred }).services
        .width,
    ).toBe(366);
    expect(
      resolveWorkspaceLayout({ width: 1800, tools: both, preferred }),
    ).toMatchObject({ files: { width: 520 }, services: { width: 480 } });
    expect(preferred).toEqual({ fileWidth: 520, serviceWidth: 480 });
  });
  it("initializes narrow windows in the terminal and falls back from a closed compact tool", () => {
    expect(
      resolveWorkspaceLayout({ width: 0, tools: defaultTools }).visible,
    ).toEqual({ files: false, terminal: true, services: false });
    expect(
      resolveWorkspaceLayout({
        width: 650,
        tools: { ...both, servicesOpen: false },
      }).terminalVisible,
    ).toBe(true);
  });
  it("uses the other requested side if the latest side was explicitly closed", () => {
    expect(
      resolveWorkspaceLayout({
        width: 1024,
        tools: { ...both, servicesOpen: false },
      }).visible,
    ).toEqual({ files: true, terminal: true, services: false });
  });
});
