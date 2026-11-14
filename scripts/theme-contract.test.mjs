import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {packageStyles} from '../packages/ui/package.mjs';
import {chromium} from 'playwright';
import themes from '../packages/ui/themes.json' with {type:'json'};
import {tokenNames} from '../packages/ui/token-names.ts';
import {readTestThemeTokens} from './test-theme-tokens.mjs';

test('registered themes provide every published token and preserve plugin geometry',async()=>{
 const browser=await chromium.launch({headless:true});
 try {
  const snapshots=await readTestThemeTokens(browser);
  for(const theme of themes){const tokens=snapshots[theme.id];assert(tokens,theme.id);for(const name of tokenNames)assert(tokens[name],`${theme.id}: ${name}`);assert.equal(tokens['color-scheme'],theme.dark?'dark':'light');assert.equal(tokens['sd-tab-height'],'36px');assert.equal(tokens['sd-status-height'],'32px');assert.equal(tokens['sd-hit-size'],'28px');}
  assert.equal(new Set(tokenNames).size,tokenNames.length);
  const shared=fs.readFileSync('packages/ui/shared.css','utf8');
  assert(!/\.pathbar|\.tree-menu|\.server-tab|\.settings-nav|\.topbar/.test(shared));
  assert(!fs.existsSync('src/ui.css')&&!fs.existsSync('src/themes.css'));
 }finally{await browser.close();}
});

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
