import fs from 'node:fs';
import path from 'node:path';
export function validateIcons(icons = {}) {
  if (!icons || typeof icons !== 'object' || Array.isArray(icons) || Object.keys(icons).length > 32) throw new Error('Invalid plugin icons mapping');
  for (const [name, asset] of Object.entries(icons)) {
    if (!/^[a-zA-Z0-9-]{1,64}$/.test(name) || typeof asset !== 'string' || asset.length > 256 || !asset.endsWith('.svg') || /[\\:%?#]/.test(asset) || asset.split('/').some(p => !p || p === '.' || p === '..')) throw new Error('Invalid plugin icon name or package path');
  }
}
export function packageIconAssets(metadata, pluginRoot, directory) {
  validateIcons(metadata.icons);
  const root = fs.realpathSync(pluginRoot);
  for (const asset of Object.values(metadata.icons ?? {})) {
    const source = fs.realpathSync(path.join(root, asset)), relative = path.relative(root, source);
    if (relative.startsWith('..') || path.isAbsolute(relative) || !fs.statSync(source).isFile() || fs.statSync(source).size > 32 * 1024) throw new Error(`Plugin icon escapes package or exceeds 32 KiB: ${asset}`);
    const destination = path.join(directory, asset); fs.mkdirSync(path.dirname(destination), {recursive:true}); fs.copyFileSync(source, destination);
  }
}
