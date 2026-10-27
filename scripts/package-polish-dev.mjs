// Release-profile, unpublished portable delivery. Copy only an explicit allowlist.
import {mkdir,readFile,writeFile,access} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),base=join(root,'.local/dev');
const task=process.argv[3]??'T015';
if(!['T015','T017','T017R2'].includes(task))throw Error('Unsupported delivery task');
const docTask=task==='T017R2'?'T-017-r2':task.replace('T','T-');
const output=resolve(process.argv[2]??join(base,`SoDeck-v2-${task}`));
if(!output.startsWith(base+sep))throw Error('Delivery must remain inside this workspace .local/dev');
try{await access(output);throw Error('Output already exists; choose a new directory to preserve data and previous artifacts');}catch(e){if(e.code!=='ENOENT')throw e;}
const hashes={},plugins=[],files=[['target/release/sodeck-desktop.exe','sodeck-desktop.exe'],['resources/development-public-key.json','resources/development-public-key.json'],[`docs/tasks/${docTask}-results.md`,'RESULTS.md'],[`docs/tasks/${docTask}-acceptance.md`,'README.md']];
const exe=await readFile(join(root,files[0][0]));const pe=exe.readUInt32LE(0x3c);
if(exe.toString('ascii',pe,pe+4)!=='PE\0\0'||exe.readUInt16LE(pe+24+68)!==2)throw Error('Expected a Windows GUI subsystem executable');
for(const name of ['terminal','files','network-proxy']){
  const manifest=JSON.parse(await readFile(join(root,`plugins/${name}/manifest.json`),'utf8'));
  const path=`resources/${name}.sodeck-plugin`;files.push([path,path]);plugins.push({pluginId:manifest.pluginId,version:manifest.version,path:`${name}.sodeck-plugin`});
}
for(const [name,license] of [['ssh2','LICENSE-MIT'],['ssh2','LICENSE-APACHE'],['ssh2','SODECK-PATCH.md'],['tauri-runtime-wry','LICENSE-MIT'],['tauri-runtime-wry','LICENSE-APACHE-2.0']])files.push([`third-party/${name}/${license}`,`licenses/${name}/${license}`]);
files.push(['node_modules/@noble/hashes/LICENSE','licenses/noble-hashes/LICENSE']);
async function put(path,bytes){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),bytes);hashes[path]=createHash('sha256').update(bytes).digest('hex');}
for(const [source,target] of files)await put(target,await readFile(join(root,source)));
await put('portable.json',Buffer.from(JSON.stringify({schemaVersion:1},null,2)+'\n'));
await put('resources/bundled-plugins.json',Buffer.from(JSON.stringify({schemaVersion:1,publicKeys:'development-public-key.json',plugins:plugins.map(({pluginId,path})=>({pluginId,path}))},null,2)+'\n'));
const git=args=>execFileSync('git',args,{cwd:root,windowsHide:true,maxBuffer:64*1024*1024});
const fingerprint=createHash('sha256').update(git(['diff','--binary','HEAD']));
for(const path of git(['ls-files','--others','--exclude-standard','-z']).toString().split('\0').filter(Boolean).sort()){fingerprint.update(path+'\0');fingerprint.update(await readFile(join(root,path)));}
const version=JSON.parse(await readFile(join(root,'package.json'),'utf8')).version;
const publicUiVersion=JSON.parse(await readFile(join(root,'packages/ui/package.json'),'utf8')).version;
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:task,version,buildProfile:'release',published:false,pluginApiVersion:'4.8.0',publicUiVersion,plugins:plugins.map(p=>({...p,sha256:hashes[`resources/${p.path}`]})),builtAt:new Date().toISOString(),sourceCommit:git(['rev-parse','HEAD']).toString().trim(),sourceDirty:Boolean(git(['status','--porcelain']).length),sourceFingerprint:fingerprint.digest('hex'),evidence:'RESULTS.md',hashes},null,2));
const quote=s=>"'"+s.replaceAll("'","''")+"'";
execFileSync('powershell.exe',['-NoProfile','-Command',`Compress-Archive -LiteralPath ${quote(output)} -DestinationPath ${quote(output+'.zip')} -CompressionLevel Optimal`],{windowsHide:true});
console.log(JSON.stringify({output,archive:output+'.zip',files:Object.keys(hashes).length}));
