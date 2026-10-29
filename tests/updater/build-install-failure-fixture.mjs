// Instrument only the QA copy at the real Tauri installation call boundary.
import assert from 'node:assert/strict';
import {readFile,writeFile,copyFile} from 'node:fs/promises';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';
const {source,out,root}=JSON.parse(await readFile('.qa/t029/release-local/fixture-context.json','utf8'));
for(const path of ['package.json','package-lock.json','Cargo.lock','apps/desktop/package.json','apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/tauri.conf.json'])await copyFile(join(root,path),join(source,path));
for(const name of ['terminal','files','network-proxy','monitor'])for(const path of ['package.json','manifest.json','src/worker.ts','index.html'])await copyFile(join(root,'plugins',name,path),join(source,'plugins',name,path));
const installer=join(source,'apps/desktop/src-tauri/src/updates/installer.rs'),original=await readFile(join(root,'apps/desktop/src-tauri/src/updates/installer.rs'),'utf8'),fault=join(out,'installer-temp-fault');await writeFile(fault,'QA staging fault');
const call='update.install(&bytes).map_err',replacement=`{let old_tmp=std::env::var_os("TMP");let old_temp=std::env::var_os("TEMP");unsafe {std::env::set_var("TMP",r#"${fault}"#);std::env::set_var("TEMP",r#"${fault}"#);}let installed=update.install(&bytes);unsafe {if let Some(v)=old_tmp{std::env::set_var("TMP",v)}else{std::env::remove_var("TMP")};if let Some(v)=old_temp{std::env::set_var("TEMP",v)}else{std::env::remove_var("TEMP")};}installed}.map_err`;
assert.equal(original.split(call).length,2);await writeFile(installer,original.replace(call,replacement));
const env={...process.env,CI:'true',OPENSSL_SRC_PERL:join(root,'.local/build-tools/strawberry/perl/bin/perl.exe'),SODECK_V2_RELEASE_KEY_FILE:join(out,'catalog-private.pem'),SODECK_V2_UPDATER_PRIVATE_KEY:await readFile(join(out,'updater.key'),'utf8')};
try{for(const [name,args]of [['plan',['scripts/release/plan.mjs','--tag','sodeck-v2/preview/000001','--config',join(out,'config.json'),'--out',join(out,'failure-A')]],['build',['scripts/release/build.mjs','--plan',join(out,'failure-A/plan.json')]]]){const r=spawnSync(process.execPath,args,{cwd:source,env,windowsHide:true,encoding:'utf8',maxBuffer:64*1024*1024});await writeFile(join(out,`failure-A-${name}.log`),r.stdout+r.stderr);assert.equal(r.status,0,(r.stderr??'').slice(-2000));}console.log('QA failure-A built; root implementation unchanged');}
finally{await writeFile(installer,original);}
