// All mutations are confined to a fresh source copy under .qa/t029.
import assert from 'node:assert/strict';
import {cp,mkdir,readFile,writeFile,symlink,access} from 'node:fs/promises';
import {join,resolve,dirname} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {spawnSync} from 'node:child_process';
const root=resolve(import.meta.dirname,'../..'),out=resolve(root,'.qa/t029/release-local'),source=join(out,'source-'+Date.now());
const run=(program,args,options={})=>{const r=spawnSync(program,args,{cwd:source,windowsHide:true,stdio:'inherit',...options});assert.equal(r.status,0,`${program} failed`);};
await assert.rejects(access(source));await mkdir(source,{recursive:true});
const files=spawnSync('git',['ls-files','--cached','--others','--exclude-standard','-z'],{cwd:root,encoding:'utf8',windowsHide:true});assert.equal(files.status,0);
for(const file of new Set(files.stdout.split('\0').filter(Boolean))){if(file.startsWith('.qa/')||file.startsWith('.local/'))continue;if(!await access(join(root,file)).then(()=>true,()=>false))continue;const target=join(source,file);await mkdir(dirname(target),{recursive:true});await cp(join(root,file),target);}
// Cargo serializes access to this cache; installed test copies are independent.
await symlink(join(root,'target'),join(source,'target'),'junction');
run(process.execPath,[join(dirname(process.execPath),'node_modules/npm/bin/npm-cli.js'),'ci']);
await writeFile(join(out,'fixture-context.json'),JSON.stringify({source,out,root},null,2));
console.log(JSON.stringify({stage:'isolated-source-ready',source}));
