// Builds genuine terminal A/B packages from isolated source copies, never edits live source.
import {cp,mkdir,readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {build as bundle} from 'esbuild';
const root=resolve('.'),out=resolve('.qa/t023/packages',String(Date.now()));await mkdir(out,{recursive:true});
const hash=b=>createHash('sha256').update(b).digest('hex'),deltas=[];
async function replace(dir,path,from,to){const file=join(dir,path),before=(await readFile(file,'utf8')).replaceAll('\r\n','\n');if(!before.includes(from))throw Error('Missing fixture source anchor: '+path);const after=before.replace(from,to);await writeFile(file,after);deltas.push({path,from,to,beforeSha256:hash(before),afterSha256:hash(after)});}
const result={root:out,packages:{}};
for(const [kind,version] of [['A','0.11.0-rc.1'],['B','0.11.0-rc.2']]){
  const source=join(out,kind);await mkdir(source);
  for(const name of ['src','settings','diagnostics','index.html','vite.config.ts','manifest.json','package.json'])await cp(join(root,'plugins/terminal',name),join(source,name),{recursive:true});
  if(kind==='A'){
    const descriptor=JSON.parse(await readFile(join(source,'settings/settings.json'),'utf8'));
    descriptor.pages[0].sections=descriptor.pages[0].sections.map(s=>({...s,fields:s.fields.filter(f=>f.key!=='shellIntegration')})).filter(s=>s.fields.length);
    await writeFile(join(source,'settings/settings.json'),JSON.stringify(descriptor,null,2));
    await replace(source,'src/worker.ts','shellIntegration=next.shellIntegration;','if(snapshot.values.some(e=>e.key===\'shellIntegration\'))shellIntegration=next.shellIntegration;');
    await replace(source,'src/worker.ts','await settingsProvider.initialize();',"await settingsProvider.initialize();const legacy=await runtime.sodeck.call('storage.get',{key:'shellIntegration'});shellIntegration=legacy!==false;");
    await replace(source,'src/worker.ts','case "settings.get":',"case 'settings.shellIntegration': {await runtime.sodeck.call('storage.set',{key:'shellIntegration',value:params.enabled===true});shellIntegration=params.enabled===true;return {saved:true};}\n      case \"settings.get\":");
    await replace(source,'src/ui-observation.ts',"import {observationTransport}","import {core,observationTransport}");
    await replace(source,'src/ui-observation.ts',"const key=command===", "if(command==='settings.shellIntegration')return core.call('plugin.invoke',{command,args});\n  const key=command===");
    await replace(source,'src/terminal-view.tsx','setShellIntegration(next.shellIntegration);',"if(snapshot.values.some(e=>e.key==='shellIntegration'))setShellIntegration(next.shellIntegration);");
    await replace(source,'src/terminal-view.tsx','await changeSetting(command,args,settingsRef.current);',"await changeSetting(command,args,settingsRef.current);if(command==='settings.shellIntegration')setShellIntegration(args.enabled===true);");
  }
  execFileSync(process.execPath,[join(root,'node_modules/vite/bin/vite.js'),'build'],{cwd:source,env:{...process.env,TERMINAL_PLUGIN_VERSION:version},windowsHide:true,stdio:'pipe'});
  const dist=join(source,'dist');await bundle({entryPoints:[join(source,'src/worker.ts')],outfile:join(dist,'worker/main.js'),bundle:true,format:'iife',platform:'neutral',target:'es2022',define:{__TERMINAL_VERSION__:JSON.stringify(version)}});
  for(const name of ['manifest.json','settings','diagnostics'])await cp(join(source,name),join(dist,name),{recursive:true});
  const packagePath=join(out,kind+'.sodeck-plugin');execFileSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','terminal','--source',dist,'--version',version,'--out',packagePath],{cwd:root,windowsHide:true,stdio:'pipe'});
  result.packages[kind]={path:packagePath,version,sha256:hash(await readFile(packagePath))};
}
const fault=join(out,'fault');await cp(join(out,'B/dist'),fault,{recursive:true});
await writeFile(join(fault,'worker/main.js'),(await readFile(join(fault,'worker/main.js'),'utf8'))+'\nglobalThis.sodeckPlugin.prepare=async()=>{throw Error("T023_ACTIVATION_FAILURE");};\n');
const faultPath=join(out,'fault.sodeck-plugin');execFileSync(process.execPath,['scripts/pack-plugin.mjs','--plugin','terminal','--source',fault,'--version','0.11.0-rc.3','--out',faultPath],{cwd:root,windowsHide:true,stdio:'pipe'});result.packages.fault={path:faultPath,version:'0.11.0-rc.3',sha256:hash(await readFile(faultPath))};
await writeFile(join(out,'A-source-delta.json'),JSON.stringify(deltas,null,2));await writeFile(resolve('.qa/t023/settings-packages.json'),JSON.stringify(result,null,2));console.log(out);
