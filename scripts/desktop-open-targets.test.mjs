import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {assertDelivered} from './fixtures/desktop-open-targets.mjs';

test('native opener accepts directory aliases and rejects wrong, duplicate or changed targets', () => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-open-targets-'));
  try {
    const real=path.join(root,'real'),alias=path.join(root,'alias');fs.mkdirSync(real);
    fs.symlinkSync(real,alias,process.platform==='win32'?'junction':'dir');
    const directory=path.join(real,'中文 😀 目录 #%');fs.mkdirSync(directory);
    const viaAlias=path.join(alias,path.basename(directory));
    const url='https://example.invalid/?q=%23%25#once',hex=s=>Buffer.from(s).toString('hex');
    assertDelivered([hex(url),hex(viaAlias)],url,directory);
    const other=path.join(root,path.basename(directory));fs.mkdirSync(other);
    for(const received of [[hex(url),hex(other)],[hex(url),hex(url)],[hex(url+'changed'),hex(directory)],[hex(url),hex(directory),hex(directory)]]) {
      assert.throws(()=>assertDelivered(received,url,directory));
    }
  } finally {
    assert.equal(path.dirname(root),os.tmpdir());assert(path.basename(root).startsWith('sodeck-open-targets-'));
    fs.rmSync(root,{recursive:true,force:true});
  }
});
