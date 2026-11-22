import fs from 'node:fs';
import path from 'node:path';

// Mirror the host's package icon URL in browser fixtures using actual package assets.
export function pluginUiFile(root, relative) {
  if (relative.startsWith('icons/')) {
    const [, version, name, extra] = relative.split('/');
    const manifest = JSON.parse(fs.readFileSync(path.join(root, 'plugin.json'), 'utf8'));
    if (extra || version !== manifest.version || !manifest.icons?.[name]) throw new Error('Invalid fixture icon URL: ' + relative);
    relative = manifest.icons[name];
  }
  const filename = path.resolve(root, relative);
  if (!filename.startsWith(path.resolve(root) + path.sep)) throw new Error('Fixture resource outside package');
  return filename;
}

export function fixtureError(error) {
  return typeof error === 'string' ? new Error(error) : Object.assign(new Error(error.message), error);
}
