import assert from 'node:assert/strict';
import {readFile,writeFile,copyFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {join} from 'node:path';
const {source,out,root}=JSON.parse(await readFile('.qa/t029/release-local/fixture-context.json','utf8'));
assert.equal(JSON.parse(await readFile(join(out,'input-regression.json'),'utf8')).passed,true);
const env={...process.env,CI:'true',OPENSSL_SRC_PERL:join(root,'.local/build-tools/strawberry/perl/bin/perl.exe'),SODECK_V2_RELEASE_KEY_FILE:join(out,'catalog-private.pem'),SODECK_V2_UPDATER_PRIVATE_KEY:await readFile(join(out,'updater.key'),'utf8')};
async function run(args,label){let log='';const child=spawn(process.execPath,args,{cwd:source,windowsHide:true,env,stdio:['ignore','pipe','pipe']});child.stdout.on('data',b=>log+=b);child.stderr.on('data',b=>log+=b);const code=await new Promise((res,rej)=>{child.on('error',rej);child.on('exit',res);});await writeFile(join(out,label+'.log'),log);assert.equal(code,0,label+': '+log.slice(-1800));}
async function build(label,seq){console.log(JSON.stringify({stage:'building',label}));await run(['scripts/release/plan.mjs','--tag',`sodeck-v2/preview/${String(seq).padStart(6,'0')}`,'--config',join(out,'config.json'),'--out',join(out,label)],'plan-'+label);await run(['scripts/release/build.mjs','--plan',join(out,label,'plan.json')],'build-'+label);console.log(JSON.stringify({stage:'built',label}));}
for(const path of ['package.json','package-lock.json','Cargo.lock','apps/desktop/package.json','apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/src/startup_config.rs'])await copyFile(join(root,path),join(source,path));
for(const path of ['apps/desktop/src-tauri/src/app.rs','apps/desktop/src-tauri/src/updates/mod.rs','apps/desktop/src-tauri/src/updates/installer.rs','apps/desktop/src-tauri/src/updates/plugin.rs','crates/updater/src/store.rs','crates/updater/src/github.rs','scripts/release/build.mjs','scripts/release/github.mjs'])await copyFile(join(root,path),join(source,path));
for(const name of ['terminal','files','network-proxy','monitor'])for(const path of ['package.json','manifest.json','src/worker.ts','index.html'])await copyFile(join(root,'plugins',name,path),join(source,'plugins',name,path));
await build('A',1);
const jsonChange=async(path,change)=>{const p=join(source,path),v=JSON.parse(await readFile(p,'utf8'));change(v);await writeFile(p,JSON.stringify(v,null,2)+'\n');};
const version='2.0.0-dev.22';for(const path of ['package.json','apps/desktop/package.json','apps/desktop/src-tauri/tauri.conf.json'])await jsonChange(path,v=>v.version=version);
for(const path of ['apps/desktop/src-tauri/Cargo.toml','Cargo.lock']){const p=join(source,path),s=await readFile(p,'utf8');assert.ok(s.includes('version = "2.0.0-dev.21"'));await writeFile(p,s.replace('version = "2.0.0-dev.21"',`version = "${version}"`));}
const plugins={terminal:'0.13.2',files:'0.10.1','network-proxy':'0.6.1',monitor:'0.3.1'};
for(const [name,value]of Object.entries(plugins))for(const file of ['package.json','manifest.json'])await jsonChange(`plugins/${name}/${file}`,v=>v.version=value);
// Instrument only the QA copy. The marker executes in the actual new UI and
// worker while every SSH/SFTP/monitor/tunnel operation remains real.
for(const [name,value]of Object.entries(plugins)){
  const ui=join(source,'plugins',name,'index.html');await writeFile(ui,(await readFile(ui,'utf8')).replace('</head>',`<meta name="t029-release" content="${value}"></head>`));
  const worker=join(source,'plugins',name,'src/worker.ts');await writeFile(worker,(await readFile(worker,'utf8'))+`\nconst releaseRuntime=(globalThis as unknown as {sodeckPlugin:{invoke:(command:string,p:unknown,i:unknown)=>unknown}}).sodeckPlugin;\nconst releaseOriginal=releaseRuntime.invoke.bind(releaseRuntime);\nreleaseRuntime.invoke=(command,p,i)=>command==='t029.release.version'?{version:${JSON.stringify(value)}}:releaseOriginal(command,p,i);\n`);
}
await jsonChange('package-lock.json',v=>{v.version=version;v.packages[''].version=version;v.packages['apps/desktop'].version=version;for(const [name,value]of Object.entries(plugins))v.packages[`plugins/${name}`].version=value;});
await build('B',2);
await writeFile(join(out,'build-fixtures.json'),JSON.stringify({passed:true,source,applicationVersions:['2.0.0-dev.21',version],pluginVersions:plugins},null,2));
