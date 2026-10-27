import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {chromium} from 'playwright';
const root=new URL('../assets/brand/',import.meta.url);
const browser=await chromium.launch({headless:true});const assets=[];
try{for(const size of [16,20,24,32,40,48,64,128,256,512]){
 const source=size<=24?'pocket-deck-micro.svg':'pocket-deck.svg';
 const svg=await readFile(new URL(source,root),'utf8');
 const page=await browser.newPage({viewport:{width:size,height:size},deviceScaleFactor:1});
 await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block;width:100vw;height:100vh}</style>${svg}`);
 const png=await page.screenshot({omitBackground:true});await page.close();
 const file=`pocket-deck-${size}.png`;await writeFile(new URL(file,root),png);assets.push({size,file,source,sha256:createHash('sha256').update(png).digest('hex'),png});
}
const entries=assets.filter(a=>a.size<=256),header=Buffer.alloc(6+16*entries.length);header.writeUInt16LE(1,2);header.writeUInt16LE(entries.length,4);let offset=header.length;
entries.forEach((a,i)=>{const at=6+i*16;header[at]=a.size===256?0:a.size;header[at+1]=header[at];header.writeUInt16LE(1,at+4);header.writeUInt16LE(32,at+6);header.writeUInt32LE(a.png.length,at+8);header.writeUInt32LE(offset,at+12);offset+=a.png.length;});
const ico=Buffer.concat([header,...entries.map(a=>a.png)]);await writeFile(new URL('pocket-deck.ico',root),ico);
await mkdir(new URL('../apps/desktop/src-tauri/icons/',import.meta.url),{recursive:true});
await writeFile(new URL('../apps/desktop/src-tauri/icons/icon.ico',import.meta.url),ico);
await writeFile(new URL('../apps/desktop/src-tauri/icons/icon.png',import.meta.url),assets.find(a=>a.size===256).png);
await writeFile(new URL('manifest.json',root),JSON.stringify({name:'Pocket Deck',sources:['pocket-deck.svg','pocket-deck-micro.svg','pocket-deck-monochrome.svg'],exports:assets.map(({png,...a})=>a),icoSha256:createHash('sha256').update(ico).digest('hex')},null,2)+'\n');
}finally{await browser.close();}
