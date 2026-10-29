import assert from 'node:assert/strict';
import {readFile,writeFile,copyFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
const context=JSON.parse(await readFile('.qa/t029/release-local/fixture-context.json','utf8'));
const {source,out,root}=context;
// Bring only this task's final changes into the existing isolated copy.
for(const path of ['apps/desktop/src-tauri/src/updates/mod.rs','apps/desktop/src-tauri/src/updates/service.rs','apps/desktop/src/settings/update-model.ts','scripts/release/inputs.mjs','scripts/release/build.mjs'])await copyFile(join(root,path),join(source,path));
const {fingerprint}=await import(pathToFileURL(join(source,'scripts/release/inputs.mjs')));
const config=JSON.parse(await readFile(join(out,'config.json'),'utf8'));
const components=JSON.parse(await readFile(join(source,'release/components.json'),'utf8')).components;
const collect=async()=>{const values={};for(const c of components)values[c.id]=(await fingerprint(c,config)).inputFingerprint;return values;};
const baseline=await collect(),results=[];
async function scenario(name,changes,expected){
  const old=[];try{for(const [file,transform] of changes){const path=join(source,file),text=await readFile(path,'utf8');old.push([path,text]);await writeFile(path,transform(text));}
    const hashes=await collect(),changed=Object.keys(hashes).filter(id=>hashes[id]!==baseline[id]).sort();assert.deepEqual(changed,[...expected].sort(),name);results.push({name,changed});console.log(JSON.stringify({name,changed}));
  }finally{for(const [path,text]of old)await writeFile(path,text);}
}
const version=text=>{const p=JSON.parse(text);p.version='0.13.2';return JSON.stringify(p,null,2)+'\n';};
await scenario('terminal version and workspace lock only',[
  ['plugins/terminal/package.json',version],['plugins/terminal/manifest.json',version],['package-lock.json',text=>{const p=JSON.parse(text);p.packages['plugins/terminal'].version='0.13.2';return JSON.stringify(p,null,2)+'\n';}]
],['sodeck.terminal']);
await scenario('documentation only',[['DEVELOPMENT.md',s=>s+'\nQA documentation-only change.\n']],[]);
await scenario('unused SDK export',[['packages/sdk/src/index.ts',s=>s+'\nexport const unusedReleaseRegression = () => 42;\n']],[]);
await scenario('used shared button implementation',[['packages/ui/src/components/controls.tsx',s=>{assert.ok(s.includes('sd-button-face'));return s.replace('sd-button-face','sd-button-face-qa');}]],components.map(c=>c.id));
await writeFile(join(out,'input-regression.json'),JSON.stringify({passed:true,source,baseline,results},null,2));
