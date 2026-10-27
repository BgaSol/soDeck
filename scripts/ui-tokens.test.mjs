import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {validateTokens,generate} from './generate-ui-tokens.mjs';
import {styleErrors} from './check-ui-boundaries.mjs';
const schema=JSON.parse(await readFile(new URL('../packages/ui/src/tokens/schema.json',import.meta.url)));
const foundation=JSON.parse(await readFile(new URL('../packages/ui/src/tokens/foundation.json',import.meta.url)));
test('token schema rejects missing, unknown, negative, unit and CSS injection',()=>{
 for(const changed of [{...foundation,'space-1':'-4px'},{...foundation,'space-1':'4em'},{...foundation,'space-1':'url(https://example.test)'},{...foundation,unknown:1},{...foundation,'line-height':Infinity}])assert.throws(()=>validateTokens(changed,schema.foundation,schema.properties));
 const missing={...foundation};delete missing['space-1'];assert.throws(()=>validateTokens(missing,schema.foundation,schema.properties));
});
test('generation is deterministic and checked-in outputs match',async()=>{const result=await generate();assert.deepEqual(result,await generate());for(const[file,text]of Object.entries(result))assert.equal(await readFile(new URL('../'+file,import.meta.url),'utf8'),text);});
test('style boundary detects private skins, branches and runtime injection',()=>{
 for(const source of ['.file-row{background:var(--selected)}','[data-theme=dark] .row{display:flex}','.row{color:#ff00ff}',"@import '../../../apps/desktop/src/styles.css';"])assert.ok(styleErrors('plugins/files/src/files.css',source).length);
 for(const source of ['const ui=<div style={{color:"red"}}/>','const ui=<button/>','document.createElement("style")',"if (theme.id === 'dopamine') render();",'const theme=document.documentElement.dataset.theme;'])assert.ok(styleErrors('plugins/files/src/view.tsx',source).length);
 assert.deepEqual(styleErrors('plugins/files/src/view.tsx','const ui=<TreeRow style={{height:rowHeight}}/>'),[]);
 assert.deepEqual(styleErrors('plugins/files/src/files.css','.tree-rows{position:absolute;top:0}'),[]);
});

test('catalog and supplemental surfaces reject unknown profiles, absent roles and references',async()=>{
 const read=async path=>JSON.parse(await readFile(new URL('../'+path,import.meta.url),'utf8'));
 for(const change of [v=>{v.themes[0].profile='unregistered';},v=>{v.defaultThemeId='unknown';},v=>{v.themes[0].mode='dark';}])await assert.rejects(generate(async path=>{const v=await read(path);if(path==='themes/catalog.json')change(v);return v;}));
 await assert.rejects(generate(async path=>{const v=await read(path);if(path==='themes/surfaces/dopamine.json')v.navigation='$missing';return v;}));
 await assert.rejects(generate(async path=>{const v=await read(path);if(path==='themes/surfaces/dopamine.json')delete v.tool;return v;}));
 await assert.rejects(generate(async path=>{const v=await read(path);if(path==='themes/surfaces/dopamine.json')delete v.capacityWarning;return v;}));
});

test('capacity surfaces keep body, secondary and selected text readable in each theme',async()=>{
 const luminance=hex=>{const c=hex.slice(1).match(/../g).slice(0,3).map(v=>parseInt(v,16)/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return .2126*c[0]+.7152*c[1]+.0722*c[2];};
 const contrast=(a,b)=>{const x=luminance(a),y=luminance(b);return (Math.max(x,y)+.05)/(Math.min(x,y)+.05);};
 for(const id of ['soft-light','soft-dark','dopamine']){
  const {colors}=JSON.parse(await readFile(new URL(`../themes/${id}.json`,import.meta.url)));
  const surfaces=JSON.parse(await readFile(new URL(`../themes/surfaces/${id}.json`,import.meta.url)));
  for(const role of ['capacityNormal','capacityWarning','capacityCritical']){
   for(const text of ['text','secondary'])assert.ok(contrast(colors[text],surfaces[role])>=4.5,`${id} ${text} on ${role}`);
   assert.ok(contrast(colors.focus,surfaces[role])>=3,`${id} focus on ${role}`);
  }
 }
});
