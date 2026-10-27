import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { relative } from 'node:path';
import { boundaryErrors } from './dependency-boundaries.mjs';
const root = fileURLToPath(new URL('../', import.meta.url));
async function walk(path) {
  const result = [];
  for (const entry of await readdir(path, { withFileTypes: true })) {
    if (['node_modules', 'dist', 'target'].includes(entry.name)) continue;
    const next = join(path, entry.name);
    if (entry.isDirectory()) result.push(...await walk(next));
    else if (/\.(ts|tsx|js|mjs|rs)$/.test(entry.name)) result.push(next);
  }
  return result;
}
const failures = [];
for (const path of await walk(join(root, 'plugins'))) {
  const text = await readFile(path, 'utf8');
  for(const error of boundaryErrors(relative(root,path),text))failures.push(`${path}: ${error}`);
}
for (const base of ['crates/core','crates/communication','packages/sdk','packages/ui']) for (const path of await walk(join(root, base))) {
  for(const error of boundaryErrors(relative(root,path),await readFile(path,'utf8')))failures.push(`${path}: ${error}`);
}
if (failures.length) throw new Error(`Dependency boundary violation:\n${failures.join('\n')}`);
console.log('Core/plugin dependency boundaries passed.');
