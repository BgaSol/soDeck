import { describe, expect, it } from "vitest";
import {
  analyzePaste,
  finalPasteSize,
  INPUT_LIMIT,
  removeBlockedControls,
  removeTrailingLineBreaks,
  resolveKeyAction,
} from "./policy";
import { defaultPreferences } from "./types";
const key = (name: string, extra = {}) => ({
  key: name,
  ctrlKey: true,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  isComposing: false,
  ...extra,
});
describe("terminal key ownership", () => {
  it("keeps interrupt without a selection and copies with one on Windows", () => {
    expect(
      resolveKeyAction(key("c"), "windows", defaultPreferences, false),
    ).toBe("passthrough");
    expect(
      resolveKeyAction(key("c"), "windows", defaultPreferences, true),
    ).toBe("copy");
    expect(
      resolveKeyAction(key("v"), "windows", defaultPreferences, false),
    ).toBe("paste");
  });
  it("preserves traditional control keys, AltGr, and IME", () => {
    for (const platform of ["linux", "macos"] as const)
      expect(
        resolveKeyAction(key("v"), platform, defaultPreferences, true),
      ).toBe("passthrough");
    expect(
      resolveKeyAction(
        key("v"),
        "windows",
        { ...defaultPreferences, shortcutMode: "traditional" },
        true,
      ),
    ).toBe("passthrough");
    expect(
      resolveKeyAction(
        key("v", { altKey: true }),
        "windows",
        defaultPreferences,
        true,
      ),
    ).toBe("passthrough");
    expect(
      resolveKeyAction(
        key("c", { isComposing: true }),
        "windows",
        defaultPreferences,
        true,
      ),
    ).toBe("passthrough");
  });
  it("handles explicit terminal shortcuts and macOS command keys", () => {
    expect(
      resolveKeyAction(
        key("C", { shiftKey: true }),
        "linux",
        defaultPreferences,
        false,
      ),
    ).toBe("copy");
    expect(
      resolveKeyAction(
        key("v", { ctrlKey: false, metaKey: true }),
        "macos",
        defaultPreferences,
        false,
      ),
    ).toBe("paste");
    expect(
      resolveKeyAction(key("a"), "windows", defaultPreferences, true),
    ).toBe("passthrough");
    expect(
      resolveKeyAction(
        key("Escape", { ctrlKey: false }),
        "windows",
        defaultPreferences,
        true,
      ),
    ).toBe("clearSelection");
  });
});
describe("paste analysis", () => {
  it.each(["echo ok\n", "echo ok\r", "echo ok\r\n", "a\nb", "a\rb"])(
    "previews every line break in %j",
    (text) => {
      expect(analyzePaste(text).needsPreview).toBe(true);
      expect(analyzePaste(text).lineCount).toBe(2);
    },
  );
  it("preserves intentional whitespace and removes only blocked controls explicitly", () => {
    expect(analyzePaste("  \t").needsPreview).toBe(false);
    const text = "a\x1b[200~\x00\t中\r\n\x7f";
    expect(analyzePaste(text).controlCount).toBe(3);
    expect(removeBlockedControls(text)).toBe("a[200~\t中\r\n");
    expect(removeTrailingLineBreaks("  x\r\n\n")).toBe("  x");
  });
  it("uses UTF-8, normalizes CRLF and accounts for bracketed paste overhead", () => {
    expect(finalPasteSize("中😀\r\n", false)).toBe(8);
    expect(finalPasteSize("中😀\r\n", true)).toBe(20);
    expect(finalPasteSize("a".repeat(INPUT_LIMIT - 12), true)).toBe(
      INPUT_LIMIT,
    );
    expect(finalPasteSize("a".repeat(INPUT_LIMIT - 11), true)).toBe(
      INPUT_LIMIT + 1,
    );
    expect(analyzePaste("中".repeat(5500)).needsPreview).toBe(true);
  });
});
