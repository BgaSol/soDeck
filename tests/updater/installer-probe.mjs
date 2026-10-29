// G1 native mechanism probe. Full SoDeck lifecycle is verified separately after integration.
import {spawn} from 'node:child_process';
import {mkdir,readFile,writeFile,readdir,copyFile,stat} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {resolve,join,delimiter} from 'node:path';
import {homedir} from 'node:os';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';

const root=resolve(import.meta.dirname,'../..');
const stamp=String(Date.now()),output=join(root,'.qa/t029/probe',stamp);
const identifier=`com.sodeck.v2.qa.probe.${stamp}`,name=`SoDeck Update QA ${stamp}`;
const data=join(process.env.APPDATA,identifier),install=join(output,'install');
const cli=join(root,'node_modules/@tauri-apps/cli/tauri.js');
const environment={...process.env,PATH:[join(homedir(),'.cargo/bin'),process.env.PATH].join(delimiter),CI:'true'};
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function run(exe,args,{cwd=root,env=environment,log='command',timeout=900000}={}){
  let text='';const child=spawn(exe,args,{cwd,env,windowsHide:true,stdio:['ignore','pipe','pipe']});
  child.stdout.on('data',b=>text+=b);child.stderr.on('data',b=>text+=b);
  const timer=setTimeout(()=>child.kill(),timeout);
  const code=await new Promise((res,rej)=>{child.on('error',rej);child.on('exit',res);});clearTimeout(timer);
  await writeFile(join(output,log+'.log'),text);
  if(code!==0)throw Error(`${log} failed (${code}): ${text.slice(-3500)}`);
  return text;
}
async function wait(check,timeout=60000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{const r=await check();if(r)return r;}catch(e){last=e;}await delay(100);}throw last??Error('Probe timeout');}
async function readResult(){return JSON.parse(await readFile(join(data,'result.json'),'utf8'));}
await mkdir(output,{recursive:true});await mkdir(data,{recursive:true});
await writeFile(join(output,'context.json'),JSON.stringify({identifier,name,output,data,install},null,2));
console.log(JSON.stringify({stage:'buildProbe',output}));
const key=join(output,'updater.key');
await run(process.execPath,[cli,'signer','generate','--ci','--write-keys',key],{log:'generate-test-key'});
const publicKey=(await readFile(key+'.pub','utf8')).trim();
const signingEnv={...environment,TAURI_SIGNING_PRIVATE_KEY:await readFile(key,'utf8'),TAURI_SIGNING_PRIVATE_KEY_PASSWORD:''};
const builds={};
for(const [label,version] of [['A','0.1.0'],['B','0.1.1']]){
  const config=join(output,`${label}.json`);
  await writeFile(config,JSON.stringify({productName:name,identifier,version,plugins:{updater:{pubkey:publicKey}}}));
  await run(process.execPath,[cli,'build','--config',config],{cwd:join(root,'tests/updater/installer-probe'),env:signingEnv,log:`build-${label}`});
  const dir=join(root,'target/release/bundle/nsis');
  const file=(await readdir(dir)).find(f=>f.startsWith(name+'_')&&f.includes(`_${version}_`)&&f.endsWith('.exe'));
  if(!file)throw Error('Missing NSIS probe installer');
  const dest=join(output,`${label}-setup.exe`);await copyFile(join(dir,file),dest);
  await copyFile(join(dir,file+'.sig'),dest+'.sig');builds[label]={version,path:dest};
  console.log(JSON.stringify({stage:'built',label,version,bytes:(await stat(dest)).size}));
}
await mkdir(install,{recursive:true});
await run(builds.A.path,['/S',`/D=${install}`],{log:'install-A',timeout:120000});
const executable=join(install,'sodeck-update-probe.exe');assert.ok(existsSync(executable));
const signature=(await readFile(builds.B.path+'.sig','utf8')).trim();
const request={fromVersion:'0.1.0',publicKey,installer:builds.B.path,action:'verify',metadata:{version:'0.1.1',url:'https://github.com/BgaSol/soDeck/releases/download/qa/probe.exe',signature}};
const results=[];
async function invoke(label,req){await writeFile(join(data,'request.json'),JSON.stringify(req));await run(executable,[],{log:label,timeout:30000}).catch(async e=>{const result=await readResult();if(result.stage!=='failed')throw e;});return readResult();}
const good=await invoke('verify-signed-version',request);assert.equal(good.stage,'verified');results.push({case:'cached-verify',...good});
const wrong=await invoke('reject-wrong-version',{...request,metadata:{...request.metadata,version:'0.1.2'}});assert.equal(wrong.stage,'failed');assert.match(wrong.error,/version/i);results.push({case:'wrong-version',...wrong});
await run(process.execPath,[cli,'signer','sign','--private-key-path',key,builds.B.path],{log:'sign-unversioned'});
const legacy=await invoke('reject-missing-version',{...request,metadata:{...request.metadata,signature:(await readFile(builds.B.path+'.sig','utf8')).trim()}});assert.equal(legacy.stage,'failed');assert.match(legacy.error,/version/i);results.push({case:'missing-version',...legacy});
const tampered=join(output,'tampered.exe'),bytes=await readFile(builds.B.path);bytes[bytes.length-1]^=1;await writeFile(tampered,bytes);
const corrupt=await invoke('reject-corrupt-package',{...request,installer:tampered});assert.equal(corrupt.stage,'failed');results.push({case:'changed-bytes',...corrupt});
await writeFile(join(data,'preserved.txt'),'isolated user data');const digest=createHash('sha256').update(await readFile(join(data,'preserved.txt'))).digest('hex');
await invoke('update-A-to-B',{...request,action:'install'});
const reopened=await wait(async()=>{const r=await readResult();return r.stage==='reopened'&&r.version==='0.1.1'?r:null;},120000);
assert.equal(createHash('sha256').update(await readFile(join(data,'preserved.txt'))).digest('hex'),digest);
assert.ok(JSON.parse(await readFile(join(data,'handoff.json'),'utf8')).beforeExit);results.push({case:'native-A-to-B',...reopened,dataPreserved:true});
const uninstall=(await readdir(install)).find(f=>/uninstall.*\.exe$/i.test(f));assert.ok(uninstall);
await run(join(install,uninstall),['/S'],{log:'uninstall-probe',timeout:120000});
assert.equal(await readFile(join(data,'preserved.txt'),'utf8'),'isolated user data');
const result={passed:true,kind:'native-isolated-mechanism-probe',fullApplicationLifecycle:false,cliVersion:'2.12.4',updaterVersion:'2.14.1',output,data,results};
await writeFile(join(output,'result.json'),JSON.stringify(result,null,2));await writeFile(join(root,'.qa/t029/probe-latest.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
