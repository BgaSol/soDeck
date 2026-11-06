import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const repository = fileURLToPath(new URL('../',import.meta.url));
export function packageSettingsAssets(metadata, directory) {
  if(!metadata.settings)return;
  const json=JSON.stringify(metadata.settings);
  if(Buffer.byteLength(json)>48*1024)throw new Error('Settings declaration exceeds 48 KiB');
  const fonts=metadata.settings.pages.flatMap(p=>p.groups.flatMap(g=>g.fields.flatMap(f=>f.presets??[])));
  if(!fonts.length)return;
  const sources=JSON.parse(fs.readFileSync(path.join(repository,'plugins/shared/fonts/sources.json'),'utf8'));
  const required=new Set(['sources.json']);
  for(const font of fonts) {
    for(const asset of [font.regular,font.bold]) {
      if(!/^ui\/fonts\/[a-z0-9-]+\.woff2$/.test(asset))throw new Error(`Invalid font asset path: ${asset}`);
      required.add(path.basename(asset));
    }
    required.add(`${font.id}.LICENSE.txt`);
  }
  const target=path.join(directory,'ui/fonts');fs.mkdirSync(target,{recursive:true});
  for(const file of required) {
    const source=path.join(repository,'plugins/shared/fonts',file),bytes=fs.readFileSync(source);
    if(file!=='sources.json') {
      const expected=sources.files.find(v=>v.file===file);
      if(!expected||createHash('sha256').update(bytes).digest('hex')!==expected.sha256)throw new Error(`Font asset checksum mismatch: ${file}`);
    }
    fs.copyFileSync(source,path.join(target,file));
  }
  fs.copyFileSync(path.join(repository,'src/font-loader.js'),path.join(directory,'ui/font-loader.js'));
  fs.copyFileSync(path.join(repository,'plugins/shared/font-presets.js'),path.join(directory,'ui/font-presets.js'));
}
