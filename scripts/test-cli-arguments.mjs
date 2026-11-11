import { parseArgs } from 'node:util';

export function parseOpenSshTestArgs(args) {
  const boundary = args.indexOf('--packaged');
  const launcher = boundary === -1 ? null : args.slice(boundary + 1);
  if (launcher && !launcher[0]) throw new Error('--packaged requires the installed executable or launcher command');
  const { values } = parseArgs({ args: boundary === -1 ? args : args.slice(0, boundary), options: Object.fromEntries([
    'terminal', 'files', 'network', 'system-status', 'communication', 'files-ui-only', 'network-ui-only', 'file-editor-native', 'close-native',
  ].map(name => [name, { type: 'boolean' }])) });
  return { flags: values, launcher };
}

export function parseTerminalInputTestArgs(args) {
  const { values } = parseArgs({ args, options: { 'desktop-script': { type: 'string', multiple: true } } });
  const desktopScript = values['desktop-script']?.[0];
  if (desktopScript === '') throw new Error('--desktop-script requires a nonempty path');
  return { desktopScript };
}

export function parseProductionUiTestArgs(args) {
  const { values } = parseArgs({ args, options: { 'updates-only': { type: 'boolean' } } });
  return { updatesOnly: values['updates-only'] ?? false };
}
