import { describe, expect, it } from "vitest";
import {
  configurationFields,
  duplicateDraft,
  updateDuplicateHost,
} from "./serverConfiguration";
import { emptyServer, filterServers } from "./types";

const original = {
  ...emptyServer(),
  id: "source",
  name: "Web-01",
  host: "host.internal",
  username: "Deploy",
  authType: "password" as const,
  hasSavedPassword: true,
  tags: ["生产"],
  directHost: "10.0.0.8",
  directPort: 2222,
};
describe("server configuration duplication", () => {
  it("creates an independent draft and suggests an available name within the limit", () => {
    const source = { ...original, name: "长".repeat(120) };
    const first = duplicateDraft(source, [source]);
    const second = duplicateDraft(source, [source, first]);
    expect(first.name.length).toBe(120);
    expect(second.name.length).toBe(120);
    expect(second.name).toMatch(/副本 2$/);
    expect(first.id).toBe("");
    expect(first.hasSavedPassword).toBe(false);
    first.tags.push("Test");
    expect(source.tags).toEqual(["生产"]);
  });
  it("only sends configuration fields, never source ID, credential indicators or save actions", () => {
    const value = {
      ...original,
      passwordChange: { type: "set", value: "secret" },
    };
    const fields = configurationFields(value);
    expect(fields).not.toHaveProperty("id");
    expect(fields).not.toHaveProperty("hasSavedPassword");
    expect(fields).not.toHaveProperty("passwordChange");
    expect(JSON.stringify(fields)).not.toContain("secret");
  });
  it("clears inherited direct endpoints after host changes, preserves explicitly edited values", () => {
    const draft = duplicateDraft(original, [original]);
    expect(
      updateDuplicateHost(draft, "other.internal", original, false),
    ).toMatchObject({ directHost: null, directPort: 22 });
    expect(
      updateDuplicateHost(draft, "other.internal", original, true),
    ).toMatchObject({ directHost: "10.0.0.8", directPort: 2222 });
    expect(
      updateDuplicateHost(draft, " host.internal ", original, false).directHost,
    ).toBe(original.directHost);
    expect(original.directHost).toBe("10.0.0.8");
  });
  it("matches multiple keywords across name, host, user and tags with AND semantics", () => {
    expect(filterServers([original], "生产 deploy host", "生产")).toHaveLength(
      1,
    );
    expect(filterServers([original], "生产 missing", "")).toHaveLength(0);
    expect(filterServers([original], "  ", "")).toHaveLength(1);
  });
});
