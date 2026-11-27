import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {compatiblePluginApi,currentPluginApi} from './plugin-api-version.mjs';

test('plugin packages use the same canonical major.minor rule as the protocol',()=>{
  assert.equal(currentPluginApi(),'2.2');
  assert(compatiblePluginApi('2.2','2.1'));
  assert(compatiblePluginApi('2.2','2.0'));
  for(const value of ['1','1.0','2','2.00','2.1','2.0.0','3.0','2.-1','','4294967296.0'])assert(!compatiblePluginApi('2.0',value),value);
});

test('plugin and workspace fixture manifests match the shared SDK handshake',()=>{
  const manifests=['scripts/fixtures/workspace-ui/manifest.json',...['terminal','files','network-proxy','system-status','docker','mcp'].map(name=>`plugins/${name}/release.json`)];
  for(const file of manifests)assert.equal(JSON.parse(fs.readFileSync(new URL('../'+file,import.meta.url),'utf8')).apiVersion,currentPluginApi(),file);
});
