import fs from 'node:fs';
import path from 'node:path';

export function packageStyles(directory) {
  fs.mkdirSync(path.join(directory, 'ui'), {recursive:true});
  const css = fs.readFileSync(new URL('shared.css', import.meta.url), 'utf8');
  fs.writeFileSync(path.join(directory, 'ui/shared.css'), css);
}
