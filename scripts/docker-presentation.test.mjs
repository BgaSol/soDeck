import assert from 'node:assert/strict';
import fs from 'node:fs';
import {test} from 'node:test';

const metadata=JSON.parse(fs.readFileSync(new URL('../plugins/docker/release.json',import.meta.url),'utf8'));
// The icon renderer shares a module with browser event handlers.
globalThis.document={addEventListener(){}};
globalThis.window={addEventListener(){}};
const {icon}=await import('../src/ui.ts');
delete globalThis.document;
delete globalThis.window;

test('Docker starts pinned on the right and opens on demand',()=>{
  const browser=metadata.ui.views.find(view=>view.id==='browser');
  assert.equal(browser.defaultSide,'right');
  assert.equal(browser.defaultPinned,true);
  assert.equal(browser.openOnWorkspace,'none');
});

test('Docker entry points use a whale and every declared icon renders',()=>{
  assert.equal(metadata.icon,'docker');
  assert.equal(metadata.ui.views.find(view=>view.id==='browser').icon,'docker');
  assert.equal(metadata.settings.pages[0].icon,'docker');
  for(const name of [metadata.icon,...metadata.ui.views.map(view=>view.icon),...metadata.settings.pages.map(page=>page.icon)]){
    assert.notEqual(icon(name),icon('info'),`${name} must not fall back to the information icon`);
    assert.match(icon(name),/aria-hidden="true"/);
  }
});
