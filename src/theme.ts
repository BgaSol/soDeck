/** Additive UI v1 snapshot: old names remain available to existing plugins. */
export function themeTokens() {
  const css = getComputedStyle(document.documentElement);
  const names = ["canvas", "panel", "subtle", "text", "muted", "border", "accent", "accent-bg", "hover", "good", "warning", "error", "shadow", "file-tree-bg", "status-bg", "folder-color", "folder-fill", "tree-selection-border",
    "sd-stroke-structure", "sd-stroke-control", "sd-icon-stroke", "sd-control-weight", "sd-title-weight", "sd-radius-control", "sd-tab-height", "sd-status-height", "sd-hit-size", "sd-navigation", "sd-tools", "sd-rail", "sd-primary", "sd-primary-text", "sd-primary-hover", "sd-pressed", "sd-success-bg", "sd-error-bg", "sd-scrim", "sd-tab-selected", "sd-selection", "sd-disabled-bg"];
  names.push(...["background", "foreground", "cursor", "selection", "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"].map(name => `terminal-${name}`));
  return { ...Object.fromEntries(names.map(name => [name, css.getPropertyValue(`--${name}`).trim()])), "color-scheme": css.colorScheme };
}
