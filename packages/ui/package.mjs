import fs from 'node:fs';
import path from 'node:path';

export function packageStyles(directory) {
  fs.mkdirSync(path.join(directory, 'ui'), {recursive:true});
  const css = ['shared.css','legacy.css'].map(file => fs.readFileSync(new URL(file, import.meta.url), 'utf8')).join('\n');
  fs.writeFileSync(path.join(directory, 'ui/shared.css'), css);
}
