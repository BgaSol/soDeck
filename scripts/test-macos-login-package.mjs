import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn, spawnSync} from 'node:child_process';

const [app, output]=process.argv.slice(2);
assert(process.platform==='darwin'&&process.env.GITHUB_ACTIONS==='true','Use a clean native macOS CI runner');
const data=path.resolve(output,'login-data');fs.mkdirSync(data,{recursive:true});
const binary=path.join(app,'Contents/MacOS/sodeck');
const env={...process.env,SODECK_TEST_DATA_DIR:data,SODECK_PACKAGE_ONLY:'1',SODECK_TEST_LOGIN_RESTART:'0',SODECK_TEST_MACOS_LOGIN:'1'};
const child=spawn(process.execPath,['scripts/smoke.mjs',binary],{env,stdio:'inherit'});
const completed=new Promise(resolve=>{child.once('error',error=>resolve({error}));child.once('exit',(code,signal)=>resolve({code,signal}));});
const deadline=Date.now()+90000;
let inspected=false,captureError;
while(Date.now()<deadline){
  if(fs.existsSync(path.join(data,'macos-login-enabled.json'))){
    try{
    // dumpbtm is supplemental system diagnostics; API registration assertions remain mandatory.
    const btm=spawnSync('/usr/bin/sudo',['-n','/usr/bin/sfltool','dumpbtm'],{encoding:'utf8',timeout:20000});
    fs.writeFileSync(path.join(output,'login-items.txt'),(btm.stdout??'')+'\n'+(btm.stderr??''));
    fs.writeFileSync(path.join(output,'login-diagnostics.json'),JSON.stringify({status:btm.status,error:btm.error?String(btm.error):null,identified:btm.stdout?.includes(app)||btm.stdout?.includes('com.bgasol.sodeck')||false}));
    const settings=spawnSync('/usr/bin/open',['x-apple.systempreferences:com.apple.LoginItems-Settings.extension'],{encoding:'utf8',timeout:10000});
    await new Promise(resolve=>setTimeout(resolve,5000));
    const screenshot=spawnSync('/usr/sbin/screencapture',['-x',path.join(output,'login-items.png')],{encoding:'utf8',timeout:5000});
    fs.writeFileSync(path.join(data,'macos-login-inspected.json'),JSON.stringify({btm:btm.status,settings:settings.status,screenshot:screenshot.status}));
    }catch(error){captureError=error;fs.writeFileSync(path.join(data,'macos-login-inspected.json'),JSON.stringify({error:String(error)}));}
    inspected=true;break;
  }
  const result=await Promise.race([completed,new Promise(resolve=>setTimeout(()=>resolve(null),100))]);
  if(result)throw Error(`Application exited before registration evidence: ${JSON.stringify(result)}`);
}
assert(inspected,'Main application login registration was not observed');
const result=await completed;assert.equal(result.code,0,JSON.stringify(result));
if(captureError)throw captureError;
const receipt=JSON.parse(fs.readFileSync(path.join(data,'macos-login-service.json'),'utf8'));
assert(receipt.enabled&&receipt.migrated&&receipt.disabled&&receipt.processContinued);
const launcher=path.join(output,'login-event');
const compile=spawnSync('clang',['-fobjc-arc','-framework','AppKit','-framework','Carbon','scripts/fixtures/macos-login-event.m','-o',launcher],{stdio:'inherit'});assert.equal(compile.status,0);
const eventData=path.join(output,'native-event');
const event=spawnSync(process.execPath,['scripts/smoke.mjs',launcher,binary],{env:{...env,SODECK_TEST_DATA_DIR:eventData,SODECK_TEST_MACOS_LOGIN:'0',SODECK_EXPECT_NATIVE_LOGIN:'1'},stdio:'inherit',timeout:120000});assert.equal(event.status,0);
fs.writeFileSync(path.join(output,'macos-login-result.json'),JSON.stringify({...receipt,syntheticNativeEvent:true,realLogoutLogin:false,visualInspection:false},null,2)+'\n');
console.log('Main-app registration, old-file migration, idempotence, running-process preservation and native login Apple Event passed.');
