import fs from 'node:fs';
fs.mkdirSync('ui/vendor',{recursive:true});
fs.cpSync('ui-src','ui',{recursive:true});
for(const [pkg,file,name] of [
  ['@xterm/xterm','lib/xterm.js','xterm.js'], ['@xterm/xterm','css/xterm.css','xterm.css'],
  ['@xterm/addon-fit','lib/addon-fit.js','fit.js'], ['@xterm/addon-serialize','lib/addon-serialize.js','serialize.js']
]){fs.copyFileSync(`node_modules/${pkg}/${file}`,`ui/vendor/${name}`);const license=fs.existsSync(`node_modules/${pkg}/LICENSE`)?`node_modules/${pkg}/LICENSE`:`node_modules/@xterm/xterm/LICENSE`;fs.copyFileSync(license,`ui/vendor/${name}.LICENSE`);}
