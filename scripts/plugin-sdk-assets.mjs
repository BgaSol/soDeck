import fs from 'node:fs';
import path from 'node:path';

export function packageSdkAssets(directory) {
  const target = path.join(directory, 'ui/sdk');
  fs.mkdirSync(target, {recursive: true});
  fs.copyFileSync(new URL('../packages/plugin-sdk/index.d.ts',import.meta.url),path.join(target,'index.d.ts'));
  for (const file of ['index.js', 'state.js', 'clipboard.js']) {
    fs.copyFileSync(new URL('../packages/plugin-sdk/src/' + file, import.meta.url), path.join(target, file));
  }
}
