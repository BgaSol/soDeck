import { describe, expect, it } from "vitest";
import {
  createInitialDraft,
  draftEquals,
  validateCreateDraft,
  validateRestartPolicy,
} from "./createContainerDraft";
const valid = () => ({ ...createInitialDraft(), name: "web" });
const image = "sha256:" + "a".repeat(64);
describe("container draft validation", () => {
  it("preserves environment values, equals signs, command spaces and source line numbers", () => {
    const d = {
      ...valid(),
      environmentText: "\nTOKEN= a=b  \n\nBAD",
      commandText: " echo \n--name=x y",
    };
    const invalid = validateCreateDraft(d, image);
    expect(invalid.ok).toBe(false);
    if (!invalid.ok)
      expect(invalid.issues[0]).toMatchObject({
        field: "environmentText",
        line: 4,
      });
    d.environmentText = "TOKEN= a=b  ";
    const result = validateCreateDraft(d, image);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.specification.environment).toEqual([
        { key: "TOKEN", value: " a=b  " },
      ]);
      expect(result.specification.command).toEqual([" echo ", "--name=x y"]);
    }
  });
  it("rejects duplicate variable keys and counts UTF-8 bytes", () => {
    const d = {
      ...valid(),
      environmentText: "X=1\nX=2",
      commandText: "汉".repeat(2731),
    };
    const result = validateCreateDraft(d, image);
    if (result.ok) throw Error("expected errors");
    expect(result.issues.map((i) => i.field)).toEqual([
      "environmentText",
      "commandText",
    ]);
  });
  it("keeps field identity after deleting another dynamic row", () => {
    const d = valid();
    d.ports = [
      {
        rowId: "first",
        hostIp: "127.0.0.1",
        hostPort: "8080",
        containerPort: "80",
        protocol: "tcp",
      },
      {
        rowId: "last",
        hostIp: "::1",
        hostPort: "",
        containerPort: "80",
        protocol: "tcp",
      },
    ];
    d.ports.shift();
    const r = validateCreateDraft(d, image);
    if (r.ok) throw Error("expected error");
    expect(r.issues[0].field).toBe("ports.last.hostPort");
  });
  it("rejects invalid addresses, ranges and duplicate bindings", () => {
    const d = valid();
    d.ports = [
      {
        rowId: "a",
        hostIp: "127.0.0.1",
        hostPort: "80",
        containerPort: "80",
        protocol: "tcp",
      },
      {
        rowId: "b",
        hostIp: "127.0.0.1",
        hostPort: "080",
        containerPort: "65536",
        protocol: "tcp",
      },
      {
        rowId: "c",
        hostIp: "999.0.0.1",
        hostPort: "80",
        containerPort: "80",
        protocol: "tcp",
      },
    ];
    const r = validateCreateDraft(d, image);
    if (r.ok) throw Error("expected error");
    expect(r.issues.map((i) => i.field)).toEqual([
      "ports.b.containerPort",
      "ports.b.hostPort",
      "ports.c.hostIp",
    ]);
  });
  it("rejects duplicate and control-character mount paths", () => {
    const d = valid();
    d.mounts = [
      {
        rowId: "a",
        kind: "bind",
        source: "/tmp",
        destination: "/data",
        readOnly: false,
      },
      {
        rowId: "b",
        kind: "volume",
        source: "data",
        destination: "/data",
        readOnly: true,
      },
      {
        rowId: "c",
        kind: "bind",
        source: "/bad\u0085path",
        destination: "/ok",
        readOnly: false,
      },
    ];
    const r = validateCreateDraft(d, image);
    if (r.ok) throw Error("expected error");
    expect(r.issues.map((i) => i.field)).toEqual([
      "mounts.b.destination",
      "mounts.c.source",
    ]);
  });
  it("only submits retry counts for on-failure and rejects a temporary empty value", () => {
    expect(validateRestartPolicy("on-failure", "")).toBeTruthy();
    expect(validateRestartPolicy("on-failure", "100001")).toBeTruthy();
    expect(validateRestartPolicy("on-failure", "0")).toBeNull();
    const d = { ...valid(), maximumRetryCount: "8" };
    const r = validateCreateDraft(d, image);
    if (!r.ok) throw Error("expected valid");
    expect(r.specification.maximumRetryCount).toBe(0);
  });
  it("compares meaningful edits without generated row ids", () => {
    const a = valid(),
      b = valid();
    a.ports = [
      {
        rowId: "a",
        hostIp: "::1",
        hostPort: "80",
        containerPort: "80",
        protocol: "tcp",
      },
    ];
    b.ports = [{ ...a.ports[0], rowId: "b" }];
    expect(draftEquals(a, b)).toBe(true);
    b.ports[0].hostPort = "81";
    expect(draftEquals(a, b)).toBe(false);
  });
});
