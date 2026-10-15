import { productName, version } from "../src-tauri/tauri.conf.json";

// Keep the interface and native bundle on the same product metadata.
export const APP_NAME = productName;
export const APP_VERSION = version;
export const APP_DESCRIPTION = "远程工作台";
export const APP_TAGLINE = `${APP_NAME}，你的远程工作台。`;
export const APP_ICON = new URL(
  "../src-tauri/icons/source.svg",
  import.meta.url,
).href;

export function windowTitle(...parts: string[]) {
  return [...parts, APP_NAME].join(" · ");
}
