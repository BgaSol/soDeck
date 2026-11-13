import fs from 'node:fs';
import path from 'node:path';
import {build} from 'vite';
if(JSON.parse(fs.readFileSync('node_modules/monaco-editor/package.json','utf8')).version!=='0.57.0')throw new Error('Verify the file editor adapter before updating Monaco');
await build({configFile:false,base:'./',build:{outDir:'dist',emptyOutDir:true,lib:{entry:{editor:path.resolve('src/editor.ts'),index:path.resolve('src/page.js'),open:path.resolve('src/open.js')},formats:['es'],fileName:(_format,name)=>name+'.js',cssFileName:'editor'},rollupOptions:{output:{assetFileNames:'[name][extname]'}}}});
fs.copyFileSync('node_modules/monaco-editor/LICENSE','dist/monaco.LICENSE');
for(const [source,name]of [['monaco-editor/ThirdPartyNotices.txt','ThirdPartyNotices.txt'],['dompurify/LICENSE','dompurify.LICENSE'],['dompurify/LICENSE-MPL','dompurify.LICENSE-MPL'],['marked/LICENSE.md','marked.LICENSE.md']])fs.copyFileSync('node_modules/'+source,'dist/'+name);
