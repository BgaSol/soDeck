import assert from 'node:assert/strict';
import fs from 'node:fs';
import {chromium} from 'playwright';
import themes from '../packages/ui/themes.json' with {type:'json'};
import {tokenNames} from '../packages/ui/token-names.ts';
import {readTestThemeTokens} from './test-theme-tokens.mjs';

const browser=await chromium.launch({headless:true});
try {
 const snapshots=await readTestThemeTokens(browser);
 for(const theme of themes){const tokens=snapshots[theme.id];assert(tokens,theme.id);for(const name of tokenNames)assert(tokens[name],`${theme.id}: ${name}`);assert.equal(tokens['color-scheme'],theme.dark?'dark':'light');assert.equal(tokens['sd-tab-height'],'36px');assert.equal(tokens['sd-status-height'],'32px');assert.equal(tokens['sd-hit-size'],'28px');}
 assert.equal(new Set(tokenNames).size,tokenNames.length);
 const shared=fs.readFileSync('packages/ui/shared.css','utf8');
 assert(!/\.pathbar|\.tree-menu|\.server-tab|\.settings-nav|\.topbar/.test(shared));
 assert(!fs.existsSync('src/ui.css')&&!fs.existsSync('src/themes.css'));
 console.log('Registered themes provide every published token and preserve plugin geometry.');
}finally{await browser.close();}
