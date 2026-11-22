import {test} from 'node:test';
import assert from 'node:assert/strict';
import {packageIcon, resourceIcon, catalogIcon} from '../src/plugin-icons.ts';
test('icon names resolve within their owning package and use a generic fallback for missing assets',()=>{
  globalThis.window={__TAURI_INTERNALS__:{convertFileSrc:()=> 'http://sodeck-plugin.localhost/'}};
  try {
    const first={id:'any.first',version:'1.0.0',icon:'shape',icons:{shape:'assets/a.svg'}};
    const second={...first,id:'any.second'};
    assert.deepEqual(packageIcon(first),{url:'http://sodeck-plugin.localhost/any.first/icons/1.0.0/shape'});
    assert.notDeepEqual(packageIcon(first),packageIcon(second));
    assert.notDeepEqual(packageIcon(first),packageIcon({...first,version:'1.0.1'}));
    assert.equal(packageIcon({id:'old',version:'1.0.0',icon:'bot'}),'plug');
    assert.equal(resourceIcon(null),'plug');
    assert.equal(catalogIcon({icon:'bot',icons:{bot:'missing.svg'}}),'plug');
    assert.deepEqual(catalogIcon({icon:'bot',iconData:'data:image/svg+xml;base64,PHN2Zy8+'}),{url:'data:image/svg+xml;base64,PHN2Zy8+'});
  }finally{delete globalThis.window;}
});
