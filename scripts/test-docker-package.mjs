import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {packageRoot} from './docker-process.mjs';
const nativeRoot=packageRoot(),stem=path.basename(nativeRoot);
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-package-'));
const tar=process.platform==='win32'?path.join(process.env.SystemRoot,'System32','tar.exe'):'tar';
try{
  const archive=nativeRoot+(process.platform==='win32'?'.zip':'.tar.gz');
  const extracted=spawnSync(tar,['-xf',archive,'-C',directory],{encoding:'utf8',windowsHide:true});assert.equal(extracted.status,0,extracted.stderr);
  const root=path.join(directory,stem),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));
  assert.equal(manifest.id,'sodeck.docker');assert.equal(manifest.ui.apiVersion,'2');assert.equal(manifest.ui.views.length,3);
  const browser=manifest.ui.views.find(view=>view.id==='browser');
  assert.equal(browser.defaultSide,'right');assert.equal(browser.defaultPinned,true);assert.equal(browser.openOnWorkspace,'none');assert.equal(browser.icon,'docker');
  for(const file of ['index.html','style.css','shared.css','bridge.js','view.js','text-menu.js','clipboard-bridge.js']) assert(fs.statSync(path.join(root,'ui',file)).size>0,file);
  const smoke=spawnSync(process.execPath,['scripts/test-docker-process.mjs'],{stdio:'inherit',windowsHide:true,env:{...process.env,SODECK_TEST_DOCKER_PLUGIN:root}});
  assert.equal(smoke.status,0);
  console.log('Docker actual native archive: packaged UI, initialize/create/get/dispose and clean exit passed.');
}finally{assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-package-'));fs.rmSync(directory,{recursive:true,force:true});}
