// T-018 changes only the file package. Preserve the verified dev.14 host and other packages.
import {readFile,writeFile,mkdir,access} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {resolve,join,dirname,sep} from 'node:path';
const root=resolve(import.meta.dirname,'..'),baseline=join(root,'.local/dev/SoDeck-v2-T017-r2'),output=resolve(process.argv[2]??join(root,'.local/dev/SoDeck-v2-T018'));
if(!output.startsWith(join(root,'.local/dev')+sep))throw Error('Delivery must stay in workspace .local/dev');
try{await access(output);throw Error('T018 delivery already exists; preserve it and its data');}catch(e){if(e.code!=='ENOENT')throw e;}
const base=JSON.parse(await readFile(join(baseline,'build.json'),'utf8'));
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const hashes={},sources=new Map(Object.keys(base.hashes).map(path=>[path,join(baseline,path)]));
sources.set('resources/files.sodeck-plugin',join(root,'resources/files.sodeck-plugin'));
sources.set('README.md',join(root,'docs/tasks/T-018-acceptance.md'));
sources.set('RESULTS.md',join(root,'docs/tasks/T-018-results.md'));
const bytes=new Map();
for(const [path,source] of sources){
 const content=await readFile(source);if(source===join(baseline,path)&&sha(content)!==base.hashes[path])throw Error('Baseline hash mismatch: '+path);
 bytes.set(path,content);hashes[path]=sha(content);
}
assertVersion();
function assertVersion(){if(base.version!=='2.0.0-dev.14')throw Error('Unexpected baseline');}
for(const [path,content] of bytes){await mkdir(dirname(join(output,path)),{recursive:true});await writeFile(join(output,path),content);}
const plugins=base.plugins.map(p=>({...p,...(p.pluginId==='sodeck.files'?{version:'0.6.0'}:{}),sha256:hashes['resources/'+p.path]}));
await writeFile(join(output,'build.json'),JSON.stringify({deliveryTask:'T018',version:base.version,buildProfile:'release',published:false,pluginApiVersion:base.pluginApiVersion,hostPublicUiVersion:base.publicUiVersion,filesPublicUiVersion:'1.4.0',plugins,builtAt:new Date().toISOString(),baseline:{task:base.deliveryTask,sourceCommit:base.sourceCommit,sourceFingerprint:base.sourceFingerprint},sourceDirty:true,composition:'Verified dev.14 executable and original terminal/network packages; files 0.6.0 rebuilt from T018 workspace',evidence:'RESULTS.md',hashes},null,2)+'\n');
const quote=s=>"'"+s.replaceAll("'","''")+"'";
execFileSync('powershell.exe',['-NoProfile','-Command',`Compress-Archive -LiteralPath ${quote(output)} -DestinationPath ${quote(output+'.zip')} -CompressionLevel Optimal`],{windowsHide:true});
console.log(JSON.stringify({output,archive:output+'.zip',files:Object.keys(hashes).length,plugins}));
