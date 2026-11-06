// Explicit maintainer task. Normal builds verify committed assets and never download fonts.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
const sources = [
  {id:'jetbrains-mono',title:'JetBrains Mono',repository:'JetBrains/JetBrainsMono',commit:'19371302b95d218af43299bce79ddbddd0bc364d',directory:'fonts/webfonts',stem:'JetBrainsMono',suffix:'.woff2',license:'OFL.txt'},
  {id:'source-code-pro',title:'Source Code Pro',repository:'adobe-fonts/source-code-pro',commit:'803b7e23ec97ae58b6232ea76519a76d428ba268',directory:'WOFF2/TTF',stem:'SourceCodePro',suffix:'.ttf.woff2',license:'LICENSE.md'},
  {id:'ibm-plex-mono',title:'IBM Plex Mono',repository:'IBM/plex',commit:'48e8676114c04f29033b4b53da4ec579f8846297',directory:'packages/plex-mono/fonts/complete/woff2',stem:'IBMPlexMono',suffix:'.woff2',license:'LICENSE.txt'},
];
const root = 'plugins/shared/fonts';
fs.mkdirSync(root,{recursive:true});
const files = [];
for(const source of sources) {
  for(const [remote, file] of [
    ...['Regular','Bold'].map(weight => [`${source.directory}/${source.stem}-${weight}${source.suffix}`,`${source.id}-${weight.toLowerCase()}.woff2`]),
    [source.license,`${source.id}.LICENSE.txt`],
  ]) {
    const url=`https://raw.githubusercontent.com/${source.repository}/${source.commit}/${remote}`;
    const api=`https://api.github.com/repos/${source.repository}/contents/${remote}?ref=${source.commit}`;
    const response=await fetch(api);if(!response.ok)throw new Error(`${api}: ${response.status}`);
    const data=await response.json();
    if(data.encoding!=='base64')throw new Error('Expected base64 repository asset');
    const bytes=Buffer.from(data.content,'base64');
    if(file.endsWith('.woff2')&&bytes.subarray(0,4).toString()!=='wOF2')throw new Error('Expected a full WOFF2 font');
    fs.writeFileSync(`${root}/${file}`,bytes);
    files.push({file,url,bytes:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex')});
  }
}
fs.writeFileSync(`${root}/sources.json`,JSON.stringify({sources,files},null,2)+'\n');
console.log(`Saved ${files.length} pinned assets (${files.reduce((n,f)=>n+f.bytes,0)} bytes)`);
