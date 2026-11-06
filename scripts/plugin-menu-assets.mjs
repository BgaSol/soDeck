import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
export async function packageMenuAssets(directory) {
  const { build } = await import('vite');
  const output = path.resolve(directory, 'ui');
  await build({ root: repository, configFile: false, logLevel: 'warn', build: {
    outDir: output, emptyOutDir: false, sourcemap: false,
    lib: { entry: path.join(repository, 'src/text-menu.js'), name: 'SoDeckTextMenu', formats: ['iife'], fileName: () => 'text-menu.js' },
  } });
  const licenses = path.join(output, 'licenses'); fs.mkdirSync(licenses, { recursive: true });
  for (const name of ['dom', 'core', 'utils']) {
    const packageRoot = path.dirname(require.resolve(`@floating-ui/${name}/package.json`));
    fs.copyFileSync(path.join(packageRoot, 'LICENSE'), path.join(licenses, `floating-ui-${name}.LICENSE`));
  }
}
