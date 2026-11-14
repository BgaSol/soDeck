import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {packageStyles} from '../packages/ui/package.mjs';

test('independent UI builds carry shared controls and frozen legacy styles without host imports',()=>{
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-shared-ui-'));
 try {
  packageStyles(directory);
  const css=fs.readFileSync(path.join(directory,'ui/shared.css'),'utf8');
  assert.equal(css,['shared.css','legacy.css'].map(file=>fs.readFileSync(`packages/ui/${file}`,'utf8')).join('\n'));
  assert(css.includes('.tree-menu')&&css.includes('.sd-primary'));
  assert(!css.includes('@import')&&!css.includes('.topbar'));
 }finally{fs.rmSync(directory,{recursive:true,force:true});}
});
