import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { desktop } from "../api";
export interface TextClipboard {
  readText(): Promise<string>;
  writeText(text: string): Promise<void>;
}
export const textClipboard: TextClipboard = {
  readText: () => (desktop ? readText() : navigator.clipboard.readText()),
  writeText: (text) =>
    desktop ? writeText(text) : navigator.clipboard.writeText(text),
};
// Register before fetching a historical range: slow producers cannot overwrite newer copies.
export function createCopyQueue(clipboard: TextClipboard) {
  let tail = Promise.resolve();
  let count = 0;
  return (
    text: string | (() => Promise<string>),
    valid: () => boolean = () => true,
  ): Promise<boolean> => {
    if (count >= 8)
      return Promise.reject(new Error("复制操作繁忙，请稍后重试"));
    count++;
    // Attach rejection handling immediately, even while an earlier write is pending.
    const content = Promise.resolve()
      .then(() => (typeof text === "string" ? text : text()))
      .then(
        (value) => ({ value }),
        (error) => ({ error }),
      );
    const result = tail.then(async () => {
      const outcome = await content;
      if ("error" in outcome) throw outcome.error;
      if (!valid() || !outcome.value) return false;
      await clipboard.writeText(outcome.value);
      return true;
    });
    tail = result
      .then(
        () => {},
        () => {},
      )
      .finally(() => {
        count--;
      });
    return result;
  };
}
export const copyText = createCopyQueue(textClipboard);
