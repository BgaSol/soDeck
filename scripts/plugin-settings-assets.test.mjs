import {test} from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {packageSettingsAssets} from './plugin-settings-assets.mjs';
test('font-consuming plugins independently package complete fonts, metadata and licenses',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-font-package-'));
  try{
    for(const plugin of ['terminal','files','network-proxy']){
      const metadata=JSON.parse(fs.readFileSync(`plugins/${plugin}/release.json`,'utf8')),directory=path.join(root,plugin);fs.mkdirSync(directory);
      packageSettingsAssets(metadata,directory);
      if(plugin==='network-proxy'){assert(!fs.existsSync(path.join(directory,'ui/fonts')));continue;}
      const files=fs.readdirSync(path.join(directory,'ui/fonts'));assert.equal(files.filter(f=>f.endsWith('.woff2')).length,6);assert.equal(files.filter(f=>f.endsWith('.LICENSE.txt')).length,3);
      assert(fs.readFileSync(path.join(directory,'ui/font-loader.js'),'utf8').includes('FontFace'));
      for(const font of files.filter(f=>f.endsWith('.woff2')))assert.equal(fs.readFileSync(path.join(directory,'ui/fonts',font)).subarray(0,4).toString(),'wOF2');
    }
    const metadata=JSON.parse(fs.readFileSync('plugins/terminal/release.json','utf8'));metadata.settings.pages[0].groups[0].fields[0].presets[0].regular='../outside.woff2';
    assert.throws(()=>packageSettingsAssets(metadata,root),/Invalid font asset/);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
