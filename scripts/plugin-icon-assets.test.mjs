import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {packageIconAssets, validateIcons} from './plugin-icon-assets.mjs';
test('plugin-owned icons are copied without a core name or plugin ID registration',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-icons-'));
  try {
    fs.mkdirSync(path.join(root,'plugin/assets'),{recursive:true});
    fs.writeFileSync(path.join(root,'plugin/assets/shape.svg'),'<svg xmlns="http://www.w3.org/2000/svg"/>');
    const metadata={id:'arbitrary.plugin',icons:{'unknown-to-core':'assets/shape.svg'}};
    packageIconAssets(metadata,path.join(root,'plugin'),path.join(root,'package'));
    assert.equal(fs.readFileSync(path.join(root,'package/assets/shape.svg'),'utf8'),'<svg xmlns="http://www.w3.org/2000/svg"/>');
    for(const asset of ['../shape.svg','/shape.svg','assets/a.png','assets/a.svg?x','C:/shape.svg','assets/%2e.svg'])assert.throws(()=>validateIcons({shape:asset}));
    assert.throws(()=>validateIcons(Object.fromEntries(Array.from({length:33},(_,i)=>['n'+i,'a.svg']))));
    assert.throws(()=>packageIconAssets({icons:{missing:'assets/missing.svg'}},path.join(root,'plugin'),path.join(root,'package')));
    fs.writeFileSync(path.join(root,'plugin/assets/shape.svg'),'x'.repeat(32769));
    assert.throws(()=>packageIconAssets(metadata,path.join(root,'plugin'),path.join(root,'package')),/32 KiB/);
    fs.writeFileSync(path.join(root,'outside.svg'),'<svg/>');
    fs.symlinkSync(path.join(root,'outside.svg'),path.join(root,'plugin/assets/link.svg'));
    assert.throws(()=>packageIconAssets({icons:{shape:'assets/link.svg'}},path.join(root,'plugin'),path.join(root,'package')),/escapes/);
  }finally{assert.equal(path.dirname(root),os.tmpdir());fs.rmSync(root,{recursive:true,force:true});}
});
