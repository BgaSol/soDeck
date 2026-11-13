import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
export async function packageDomAssets(directory) {
  const { build } = await import('vite');
  const output = path.resolve(directory, 'ui');
  await build({ root: repository, configFile: false, logLevel: 'warn', build: {
    outDir: output, emptyOutDir: false, sourcemap: false,
    lib: { entry: path.join(repository, 'src/dom-update.ts'), name: 'SoDeckDOM', formats: ['iife'], fileName: () => 'dom-update.js' },
  } });
  const licenses = path.join(output, 'licenses'); fs.mkdirSync(licenses, { recursive: true });
  fs.copyFileSync(path.join(path.dirname(require.resolve('morphdom/package.json')), 'LICENSE'), path.join(licenses, 'morphdom.LICENSE'));
}
