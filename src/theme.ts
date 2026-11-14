import { tokenNames } from '../packages/ui/token-names.ts';
export { default as themes } from '../packages/ui/themes.json' with {type:'json'};
/** Additive UI v1 snapshot: old names remain available to existing plugins. */
export function themeTokens(names = tokenNames) {
  const css = getComputedStyle(document.documentElement);
  return { ...Object.fromEntries(names.map(name => [name, css.getPropertyValue(`--${name}`).trim()])), "color-scheme": css.colorScheme };
}

let preference = 'system';
const system = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : undefined;
export function applyTheme(next = preference) {
  preference = next;
  const resolved = next === 'system' ? system?.matches ? 'dark' : 'light' : next;
  if (document.documentElement.dataset.theme !== resolved) document.documentElement.dataset.theme = resolved;
}
system?.addEventListener('change', () => applyTheme());
export function subscribeTheme(change: () => void) {
  const observer = new MutationObserver(change);
  observer.observe(document.documentElement, {attributes:true,attributeFilter:['data-theme','style']});
  return () => observer.disconnect();
}
