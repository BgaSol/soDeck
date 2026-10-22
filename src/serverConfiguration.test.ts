import { describe, expect, it } from "vitest";
import {
  configurationFields,
  duplicateDraft,
  createServerName,
  duplicatePasswordPolicy,
  needsDirectAddressReview,
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
  it("previews a unique create name without renaming an update or a custom name", () => {
    expect(createServerName(original, original, [original])).toBe(
      "Web-01 - 副本",
    );
    expect(
      createServerName(original, original, [
        original,
        { ...original, name: "Web-01 - 副本" },
      ]),
    ).toBe("Web-01 - 副本 2");
    expect(
      createServerName({ ...original, name: "Other" }, original, [original]),
    ).toBe("Other");
    expect(original.name).toBe("Web-01");
  });
  it("maps password intent without inheriting for other authentication methods", () => {
    expect(
      duplicatePasswordPolicy(original, original, { type: "keep" }).type,
    ).toBe("inherit");
    expect(
      duplicatePasswordPolicy(original, original, { type: "clear" }).type,
    ).toBe("clear");
    expect(
      duplicatePasswordPolicy(original, original, {
        type: "set",
        value: "replacement",
      }),
    ).toEqual({ type: "set", value: "replacement" });
    expect(
      duplicatePasswordPolicy(
        original,
        { ...original, authType: "key" },
        { type: "keep" },
      ).type,
    ).toBe("clear");
    expect(
      duplicatePasswordPolicy(
        { ...original, hasSavedPassword: false },
        original,
        { type: "keep" },
      ).type,
    ).toBe("clear");
  });
  it("reminds about unchanged direct endpoints while preserving the visible draft", () => {
    const draft = { ...original, host: "other.internal" };
    expect(needsDirectAddressReview(draft, original)).toBe(true);
    expect(draft.directHost).toBe("10.0.0.8");
    expect(
      needsDirectAddressReview(
        { ...draft, directHost: "other.internal" },
        original,
      ),
    ).toBe(false);
    expect(needsDirectAddressReview(original, original)).toBe(false);
  });
  it("matches multiple keywords across name, host, user and tags with AND semantics", () => {
    expect(filterServers([original], "生产 deploy host", "生产")).toHaveLength(
      1,
    );
    expect(filterServers([original], "生产 missing", "")).toHaveLength(0);
    expect(filterServers([original], "  ", "")).toHaveLength(1);
  });
});
