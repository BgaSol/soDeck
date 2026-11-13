import {test} from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {fileEditorSettings} from './plugin-file-editor-assets.mjs';
import {packageSettingsAssets} from './plugin-settings-assets.mjs';
import {execFileSync} from 'node:child_process';
test('Git preserves pinned font asset bytes with either checkout newline policy',()=>{
  const sources=JSON.parse(fs.readFileSync('plugins/shared/fonts/sources.json','utf8'));
  for(const asset of sources.files){
    const file=`plugins/shared/fonts/${asset.file}`;
    const options={encoding:'utf8',windowsHide:true,timeout:10_000};
    const raw=execFileSync('git',['hash-object','--no-filters','--',file],options).trim();
    for(const autocrlf of ['false','true']){
      const filtered=execFileSync('git',['-c',`core.autocrlf=${autocrlf}`,'hash-object',`--path=${file}`,'--',file],options).trim();
      assert.equal(filtered,raw,`${asset.file} must retain upstream bytes with core.autocrlf=${autocrlf}`);
    }
  }
});
test('font-consuming plugins independently package complete fonts, metadata and licenses',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-font-package-'));
  try{
    for(const plugin of ['terminal','files','network-proxy']){
      const metadata=JSON.parse(fs.readFileSync(`plugins/${plugin}/release.json`,'utf8')),directory=path.join(root,plugin);fs.mkdirSync(directory);
      packageSettingsAssets(fileEditorSettings(metadata),directory);
      if(plugin==='network-proxy'){assert(!fs.existsSync(path.join(directory,'ui/fonts')));continue;}
      const files=fs.readdirSync(path.join(directory,'ui/fonts'));assert.equal(files.filter(f=>f.endsWith('.woff2')).length,6);assert.equal(files.filter(f=>f.endsWith('.LICENSE.txt')).length,3);
      assert(fs.readFileSync(path.join(directory,'ui/font-loader.js'),'utf8').includes('FontFace'));
      for(const font of files.filter(f=>f.endsWith('.woff2')))assert.equal(fs.readFileSync(path.join(directory,'ui/fonts',font)).subarray(0,4).toString(),'wOF2');
    }
    const metadata=JSON.parse(fs.readFileSync('plugins/terminal/release.json','utf8'));metadata.settings.pages.flatMap(p=>p.groups).flatMap(g=>g.fields).find(f=>f.type==='font').presets[0].regular='../outside.woff2';
    assert.throws(()=>packageSettingsAssets(metadata,root),/Invalid font asset/);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
