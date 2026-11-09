// Run only on an explicitly isolated desktop: this test replaces its clipboard.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
const [provider, executable] = process.argv.slice(2);
assert(provider && executable && fs.existsSync(provider) && fs.existsSync(executable), 'Provide same-architecture clipboard fixture and actual core package');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-native-files-'));
let owner, application;
try {
  const directory = path.join(root, '中文 目录 #%'); fs.mkdirSync(directory);
  const file = path.join(directory, '中文 😀 %.txt'); fs.writeFileSync(file, 'native file input\n');
  const paths = [file, directory];
  const config = path.join(root, 'paths.json'); fs.writeFileSync(config, JSON.stringify(paths));
  owner = spawn(provider, [config], {windowsHide:true, stdio:['pipe','pipe','pipe']});
  let ready = 0, diagnostics = '', buffer = '';
  owner.stdout.on('data', data => {
    buffer += data;
    while(buffer.includes('\n')) { const split=buffer.indexOf('\n'); if(buffer.slice(0,split).trim()==='ready')ready++; buffer=buffer.slice(split+1); }
  });
  owner.stderr.on('data', data=>diagnostics+=data);
  owner.on('error', error=>diagnostics+=error.message);
  async function until(predicate, message) {
    const deadline = Date.now()+15000;
    while (!predicate()) { assert(owner.exitCode===null && Date.now()<deadline, `${message}: ${diagnostics}`); await delay(20); }
  }
  await until(()=>ready>0,'Clipboard provider startup');
  const marker = path.join(root,'change'), probe = path.join(root,'probe.json');
  fs.writeFileSync(probe,JSON.stringify({paths,change_marker:marker}));
  const env={...process.env,SODECK_TEST_DESKTOP_FILES:probe,SODECK_TEST_DATA_DIR:path.join(root,'data'),SODECK_VERIFY_PERSISTED:'1'};
  // Plugin/SSH integration is verified separately by the existing package smoke.
  for(const key of ['SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD','SODECK_TEST_TERMINAL_PLUGIN','SODECK_TEST_FILES_PLUGIN','SODECK_TEST_NETWORK_PLUGIN','SODECK_TEST_SYSTEM_STATUS_PLUGIN'])delete env[key];
  const formats=process.platform==='linux'?['files','gnome']:['files'];
  let report;
  for(const format of formats){
    fs.rmSync(marker,{force:true});
    const reset=ready;owner.stdin.write(`${format}\n`);await until(()=>ready>reset,'Publishing file format');
    env.SODECK_TEST_DATA_DIR=path.join(root,format);
    application=spawn(process.execPath,['scripts/smoke.mjs',executable],{env,windowsHide:true,stdio:'inherit'});
    const finished=new Promise((resolve,reject)=>{application.once('error',reject);application.once('exit',resolve);});
    await until(()=>fs.existsSync(marker)||application.exitCode!==null,'Core reading actual file formats');
    assert(fs.existsSync(marker),'Core must read the independent file list before changing it');
    const previous=ready;owner.stdin.write('text\n');await until(()=>ready>previous,'Clipboard change');
    assert.equal(await finished,0,'Actual package file clipboard/source-change/shutdown verification');
    report=JSON.parse(fs.readFileSync(path.join(env.SODECK_TEST_DATA_DIR,'smoke-result.json'),'utf8'));
    assert.equal(report.desktopFilesNative,true);
  }
  fs.mkdirSync('artifacts/native-desktop-files',{recursive:true});
  fs.writeFileSync(`artifacts/native-desktop-files/${report.target}.json`,JSON.stringify({...report,independentProvider:true,formats},null,2)+'\n');
  console.log('Actual file clipboard, native paths, file-to-text revision and shutdown passed.');
}finally{
  if(application?.exitCode===null){application.kill();await new Promise(resolve=>application.once('exit',resolve));}
  if(owner?.exitCode===null){owner.stdin.end('quit\n');await new Promise(resolve=>{owner.once('exit',resolve);setTimeout(()=>{if(owner.exitCode===null)owner.kill();},3000);});}
  assert.equal(path.dirname(root),os.tmpdir());assert(path.basename(root).startsWith('sodeck-native-files-'));
  fs.rmSync(root,{recursive:true,force:true,maxRetries:8,retryDelay:250});
}
