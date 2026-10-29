import {build as viteBuild} from 'vite';
import {build as esbuild} from 'esbuild';
import {readFile,readdir} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {resolve,relative,join} from 'node:path';
import {root,hash,json,node} from './common.mjs';
const normalized=path=>relative(root,resolve(path)).replaceAll('\\','/');
async function tree(path){const files=[];for(const e of await readdir(path,{withFileTypes:true})){if(['target','node_modules','dist','.git','gen','tests','example','examples','benches','docs'].includes(e.name)||/\.(md|test\.mjs)$/.test(e.name))continue;const p=join(path,e.name);if(e.isSymbolicLink())throw Error('Release inputs cannot contain links');if(e.isDirectory())files.push(...await tree(p));else files.push(p);}return files;}
function dependencyLock(lock,files){
  const used=new Set();for(const file of files){const normalizedFile=normalized(file);const matches=Object.keys(lock.packages).filter(p=>p.startsWith('node_modules/')&&(normalizedFile===p||normalizedFile.startsWith(p+'/'))).sort((a,b)=>b.length-a.length);if(matches[0])used.add(matches[0]);}
  const resolvePackage=(name,from='')=>{let at=from;while(true){const candidate=(at?at+'/':'')+'node_modules/'+name;if(lock.packages[candidate]&&existsSync(join(root,candidate)))return candidate;if(!at)return;at=at.slice(0,Math.max(0,at.lastIndexOf('/node_modules/')));}};
  const buildTools=new Set();const visit=path=>{if(!path||buildTools.has(path))return;buildTools.add(path);const p=lock.packages[path];for(const name of Object.keys({...p.dependencies,...p.optionalDependencies}))visit(resolvePackage(name,path));};
  for(const name of ['vite','@vitejs/plugin-react','esbuild','@tauri-apps/cli'])visit(resolvePackage(name));
  for(const path of buildTools)used.add(path);
  // Resolution records include transitive packages actually present in bundled modules.
  return [...used].sort().map(path=>{const p=lock.packages[path];return {path,version:p.version,integrity:p.integrity,resolved:p.resolved,link:p.link};});
}
export async function fingerprint(component,config){
  const path=resolve(root,component.path),manifest=component.kind==='plugin'?await json(join(path,'manifest.json')):undefined;
  const packageJson=await json(join(path,'package.json'));
  if(manifest&&(manifest.pluginId!==component.id||manifest.version!==packageJson.version))throw Error('Plugin manifest/package version mismatch: '+component.id);
  const modules=new Set(),outputs=[];
  const envNames=['TERMINAL_PLUGIN_VERSION','FILES_PLUGIN_VERSION','NETWORK_PLUGIN_VERSION','MONITOR_PLUGIN_VERSION'];const old=envNames.map(n=>process.env[n]);envNames.forEach(n=>process.env[n]=packageJson.version);
  try{
    const result=await viteBuild({root:path,logLevel:'error',build:{write:false},plugins:[{name:'sodeck-release-inputs',generateBundle(_,bundle){for(const item of Object.values(bundle)){if(item.type==='chunk')for(const id of Object.keys(item.modules))if(!id.startsWith('\0'))modules.add(id.split('?')[0]);}}}]});
    for(const output of Array.isArray(result)?result:[result])for(const f of output.output??[])outputs.push({name:f.type==='chunk'?f.name:f.names?.[0]??f.name??f.fileName,bytes:hash(f.type==='chunk'?f.code:f.source)});
  }finally{envNames.forEach((n,i)=>old[i]===undefined?delete process.env[n]:process.env[n]=old[i]);}
  if(component.kind==='plugin'){
    const worker=await esbuild({entryPoints:[join(path,'src/worker.ts')],bundle:true,write:false,metafile:true,format:'iife',platform:'neutral',target:'es2022',define:Object.fromEntries(['TERMINAL','FILES','NETWORK','MONITOR'].map(n=>[`__${n}_VERSION__`,JSON.stringify(packageJson.version)]))});
    Object.keys(worker.metafile.inputs).forEach(p=>modules.add(resolve(root,p)));outputs.push(...worker.outputFiles.map(f=>({name:'worker',bytes:hash(f.contents)})));
  }
  const inputs=new Set([join(path,'package.json'),join(path,'vite.config.ts'),...await tree(join(root,'scripts/release'))]);
  if(manifest){inputs.add(join(path,'manifest.json'));if(manifest.contributes?.settings)inputs.add(join(path,manifest.contributes.settings.path));if(manifest.observability)inputs.add(join(path,manifest.observability.path));inputs.add(join(root,'scripts/pack-plugin.mjs'));}
  let cargo=[];
  if(component.kind==='core') {
    for(const folder of ['apps/desktop/src-tauri','crates/core','crates/plugin-manager','crates/plugin-host','crates/communication','crates/updater','contracts','themes','assets/brand','third-party'])for(const f of await tree(join(root,folder)))inputs.add(f);
    inputs.add(join(root,'Cargo.toml'));
    // Cargo metadata resolves the desktop graph; unrelated workspace crates do not enter the fingerprint.
    const metadata=JSON.parse(node(['scripts/cargo.mjs','metadata','--format-version','1','--locked','--filter-platform','x86_64-pc-windows-msvc'],{capture:true}));
    const nodes=new Map(metadata.resolve.nodes.map(n=>[n.id,n])),ids=new Set();const visit=id=>{if(ids.has(id))return;ids.add(id);for(const d of nodes.get(id)?.deps??[])if(d.dep_kinds.some(k=>k.kind!=='dev'))visit(d.pkg);};visit(metadata.packages.find(p=>p.name==='sodeck-desktop').id);visit(metadata.packages.find(p=>p.name==='sodeck-plugin-host').id);
    const locked=(await readFile(join(root,'Cargo.lock'),'utf8')).split('[[package]]').slice(1).map(s=>Object.fromEntries([...s.matchAll(/^(name|version|source|checksum) = "([^"]+)"/gm)].map(m=>[m[1],m[2]])));
    cargo=metadata.packages.filter(p=>ids.has(p.id)).map(p=>({name:p.name,version:p.version,source:p.source,checksum:locked.find(l=>l.name===p.name&&l.version===p.version&&(l.source??null)===p.source)?.checksum,features:nodes.get(p.id)?.features})).sort((a,b)=>a.name.localeCompare(b.name)||a.version.localeCompare(b.version));
  }
  const files=[];for(const file of [...inputs].sort()){const name=normalized(file);if(name.endsWith('.pdb'))continue;files.push({path:name,sha256:hash(await readFile(file))});}
  const lock=dependencyLock(await json(join(root,'package-lock.json')),modules);
  const evidence={schemaVersion:1,componentId:component.id,version:packageJson.version,outputs:outputs.sort((a,b)=>a.name.localeCompare(b.name)||a.bytes.localeCompare(b.bytes)),files,dependencies:lock,cargo,toolchain:await json(join(root,'release/components.json')),publicConfig:config};
  return {...evidence,inputFingerprint:hash(JSON.stringify(evidence))};
}
