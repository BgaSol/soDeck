import type { TerminalPlatform, TerminalClipboardPreferences } from "./types";
export const INPUT_LIMIT = 1024 * 1024;
export const PREVIEW_THRESHOLD = 16 * 1024;
const encoder = new TextEncoder();
export const normalizeLineEndings = (text: string) =>
  text.replace(/\r?\n/g, "\r");
export const byteLength = (text: string) => encoder.encode(text).length;
export const removeBlockedControls = (text: string) =>
  text.replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "");
export const removeTrailingLineBreaks = (text: string) =>
  text.replace(/[\r\n]+$/, "");
export function analyzePaste(text: string) {
  const utf8Bytes = byteLength(text);
  const controlCount = text.length - removeBlockedControls(text).length;
  const hasLineBreak = /[\r\n]/.test(text);
  return {
    utf8Bytes,
    normalizedBytes: byteLength(normalizeLineEndings(text)),
    lineCount: text.split(/\r\n|[\r\n]/).length,
    hasLineBreak,
    hasTrailingLineBreak: /[\r\n]$/.test(text),
    controlCount,
    needsPreview:
      hasLineBreak || controlCount > 0 || utf8Bytes > PREVIEW_THRESHOLD,
  };
}
export function finalPasteSize(text: string, bracketed: boolean) {
  return byteLength(normalizeLineEndings(text)) + (bracketed ? 12 : 0);
}
export function convenientKeys(
  platform: TerminalPlatform,
  settings: TerminalClipboardPreferences,
) {
  return (
    platform !== "macos" &&
    (settings.shortcutMode === "convenient" ||
      (settings.shortcutMode === "platform" && platform === "windows"))
  );
}
export function shortcutLabels(
  platform: TerminalPlatform,
  settings: TerminalClipboardPreferences,
) {
  return platform === "macos"
    ? { copy: "⌘C", paste: "⌘V" }
    : convenientKeys(platform, settings)
      ? { copy: "Ctrl+C", paste: "Ctrl+V" }
      : { copy: "Ctrl+Shift+C", paste: "Ctrl+Shift+V" };
}
export type KeyAction =
  "copy" | "paste" | "clearSelection" | "menu" | "passthrough";
export function resolveKeyAction(
  event: Pick<
    KeyboardEvent,
    "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "isComposing"
  >,
  platform: TerminalPlatform,
  settings: TerminalClipboardPreferences,
  hasSelection: boolean,
): KeyAction {
  if (event.isComposing) return "passthrough";
  const { ctrlKey: ctrl, metaKey: meta, altKey: alt, shiftKey: shift } = event;
  const key = event.key.toLowerCase();
  if (alt) return "passthrough";
  if (!ctrl && !meta && (key === "contextmenu" || (key === "f10" && shift)))
    return "menu";
  if (!ctrl && !meta && !shift && key === "escape" && hasSelection)
    return "clearSelection";
  if (platform === "macos") {
    if (meta && !ctrl && !shift && key === "c") return "copy";
    if (meta && !ctrl && !shift && key === "v") return "paste";
    return "passthrough";
  }
  if (meta) return "passthrough";
  if (shift && !ctrl && key === "insert") return "paste";
  if (
    ctrl &&
    key === "c" &&
    (shift || (hasSelection && convenientKeys(platform, settings)))
  )
    return "copy";
  if (ctrl && key === "v" && (shift || convenientKeys(platform, settings)))
    return "paste";
  return "passthrough";
}
