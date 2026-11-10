import fs from 'node:fs';
import {packageSettingsAssets} from '../../scripts/plugin-settings-assets.mjs';
import {packageMenuAssets} from '../../scripts/plugin-menu-assets.mjs';
for(const [pkg,version]of [['@xterm/xterm','6.0.0'],['@xterm/addon-serialize','0.14.0']]){
  if(JSON.parse(fs.readFileSync(`node_modules/${pkg}/package.json`,'utf8')).version!==version)throw new Error('Update and verify the terminal history/snapshot adapter before changing '+pkg);
}
fs.mkdirSync('ui/vendor',{recursive:true});
fs.cpSync('ui-src','ui',{recursive:true});
fs.copyFileSync('../../src/ui.css','ui/shared.css');
fs.copyFileSync('../shared/clipboard.js','ui/clipboard-bridge.js');
fs.copyFileSync('../shared/state.js','ui/state.js');
await packageMenuAssets('.');
packageSettingsAssets(JSON.parse(fs.readFileSync('release.json','utf8')),'.');
for(const [pkg,file,name] of [
  ['@xterm/xterm','lib/xterm.js','xterm.js'], ['@xterm/xterm','css/xterm.css','xterm.css'],
  ['@xterm/addon-fit','lib/addon-fit.js','fit.js'], ['@xterm/addon-serialize','lib/addon-serialize.js','serialize.js']
]){fs.copyFileSync(`node_modules/${pkg}/${file}`,`ui/vendor/${name}`);const license=fs.existsSync(`node_modules/${pkg}/LICENSE`)?`node_modules/${pkg}/LICENSE`:`node_modules/@xterm/xterm/LICENSE`;fs.copyFileSync(license,`ui/vendor/${name}.LICENSE`);}
