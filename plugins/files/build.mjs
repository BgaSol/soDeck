import fs from 'node:fs';
import path from 'node:path';
import {build} from 'vite';
if(JSON.parse(fs.readFileSync('node_modules/monaco-editor/package.json','utf8')).version!=='0.57.0')throw new Error('Verify the file editor adapter before updating Monaco');
await build({configFile:false,base:'./',build:{outDir:'ui/vendor',emptyOutDir:true,lib:{entry:path.resolve('editor-src/editor.ts'),formats:['es'],fileName:()=> 'editor.js',cssFileName:'editor'},rollupOptions:{output:{assetFileNames:'[name][extname]'}}}});
fs.copyFileSync('node_modules/monaco-editor/LICENSE','ui/vendor/monaco.LICENSE');
for(const [source,name]of [['monaco-editor/ThirdPartyNotices.txt','ThirdPartyNotices.txt'],['dompurify/LICENSE','dompurify.LICENSE'],['dompurify/LICENSE-MPL','dompurify.LICENSE-MPL'],['marked/LICENSE.md','marked.LICENSE.md']])fs.copyFileSync('node_modules/'+source,'ui/vendor/'+name);
