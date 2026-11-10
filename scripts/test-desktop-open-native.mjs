// Changes default associations only on disposable Actions runners or explicitly isolated desktops.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';

assert(process.env.GITHUB_ACTIONS === 'true' || process.env.SODECK_TEST_OPEN_ISOLATED === '1', 'Use a disposable runner or explicitly isolated desktop');
const [executable, ...launcherArgs] = process.argv.slice(2);
assert(executable, 'Provide the actual package executable or launcher');
const fixtureOnly = executable === '--handler-only';
assert(!fixtureOnly || process.platform === 'darwin', 'Handler-only preflight is for the macOS fixture');
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-native-open-')));
const log = path.join(root, 'opened.txt'); fs.writeFileSync(log, '');
const directory = path.join(root, '中文 😀 目录 #%'); fs.mkdirSync(directory);
const scheme = process.platform === 'darwin' ? 'http' : 'https';
const url = `${scheme}://example.invalid/sodeck-${path.basename(root)}?q=%E4%B8%AD%E6%96%87&literal=%23%25#once`;
const env = {...process.env};
for (const key of Object.keys(env)) if (key.startsWith('SODECK_TEST_') || key === 'SODECK_VERIFY_PERSISTED') delete env[key];
let restore = () => {}, report, failed = false;
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {env, encoding:'utf8', windowsHide:true, timeout:30000, ...options});
  assert.equal(result.status, 0, `${command}: ${result.error?.message || result.stderr || result.stdout}`);
  return result.stdout?.trim() || '';
}
async function probe(name, target = directory, expectFailure = false) {
  fs.writeFileSync(log, '');
  const config = path.join(root, 'probe.json');
  fs.writeFileSync(config, JSON.stringify({directory:target, url, expect_failure:expectFailure}));
  if (fixtureOnly) {
    run('/usr/bin/open',[url]);run('/usr/bin/open',[target]);
    report = {fixtureOnly:true,target:'macos-handler'};
  } else {
    run(process.execPath, ['scripts/smoke.mjs', executable, ...launcherArgs], {env:{...env, SODECK_TEST_DATA_DIR:path.join(root,name), SODECK_TEST_DESKTOP_OPEN:config}, stdio:'inherit'});
    report = JSON.parse(fs.readFileSync(path.join(root,name,'smoke-result.json')));
    assert.equal(report.desktopOpenNative, true);
  }
  if (!expectFailure) {
    const wanted = [url, target].map(s => Buffer.from(s).toString('hex')).sort();
    const deadline = Date.now()+5000;
    while (fs.readFileSync(log,'utf8').trim().split('\n').filter(Boolean).length < wanted.length && Date.now()<deadline) await delay(50);
    await delay(350);
    assert.deepEqual(fs.readFileSync(log,'utf8').trim().split('\n').filter(Boolean).sort(), wanted, `${name}: targets must reach their defaults exactly once`);
  } else assert.equal(fs.readFileSync(log,'utf8'), '', 'Failed launchers must not deliver targets');
}

try {
  if (process.platform === 'win32') {
    const handler = path.join(root,'handler.exe');
    run('rustc', ['scripts/fixtures/desktop-open-handler.rs', '-o', handler], {env:{...env,SODECK_OPEN_HANDLER_LOG:log}});
    const classes = ['http','https','Folder','Directory'];
    for (const scheme of ['http','https']) {
      const choice = spawnSync('reg',['query',`HKCU\\Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\${scheme}\\UserChoice`,'/v','ProgId'],{encoding:'utf8',windowsHide:true});
      const id = choice.stdout?.match(/ProgId\s+REG_SZ\s+(\S+)/)?.[1];
      if (id && !classes.includes(id)) classes.push(id);
    }
    const keys = classes.map(name=>'HKCU\\Software\\Classes\\'+name);
    const saved = keys.map((key,i)=>{
      const file=path.join(root,`registry-${i}.reg`);
      const result=spawnSync('reg',['export',key,file,'/y'],{windowsHide:true,encoding:'utf8'});
      return {key,file,existed:result.status===0};
    });
    restore=()=>{for(const {key,file,existed} of saved){spawnSync('reg',['delete',key,'/f'],{windowsHide:true});if(existed)run('reg',['import',file]);}};
    const configure = command => {
      for (const key of keys) {
        if (/http/.test(key)) run('reg',['add',key,'/v','URL Protocol','/d','','/f']);
        run('reg',['add',key+'\\shell','/ve','/d','open','/f']);
        run('reg',['add',key+'\\shell\\open\\command','/ve','/d',`"${command}" "%1"`,'/f']);
        run('reg',['add',key+'\\shell\\open\\command','/v','DelegateExecute','/d','','/f']);
      }
    };
    configure(handler);
    await probe('defaults');
    configure(path.join(root,'missing-handler.exe'));
    await probe('launcher-failure',directory,true);
  } else if (process.platform === 'darwin') {
    const app=path.join(root,'Handler.app'),contents=path.join(app,'Contents'),bin=path.join(contents,'MacOS');fs.mkdirSync(bin,{recursive:true});
    const handler=path.join(bin,'handler'),id='com.sodeck.open-test.'+process.pid;
    run('clang',['-fobjc-arc','-fblocks','-framework','AppKit','-framework','CoreServices',`-DSODECK_OPEN_HANDLER_LOG=${JSON.stringify(log)}`,'scripts/fixtures/desktop-open-handler.m','-o',handler]);
    fs.writeFileSync(path.join(contents,'Info.plist'),`<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${id}</string><key>CFBundleVersion</key><string>1</string><key>CFBundleExecutable</key><string>handler</string><key>CFBundleName</key><string>SoDeck Open Test</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/><key>CFBundleURLTypes</key><array><dict><key>CFBundleTypeRole</key><string>Viewer</string><key>CFBundleURLName</key><string>${id}.web</string><key>CFBundleURLSchemes</key><array><string>http</string></array></dict></array><key>CFBundleDocumentTypes</key><array><dict><key>CFBundleTypeName</key><string>Folder</string><key>CFBundleTypeRole</key><string>Viewer</string><key>LSHandlerRank</key><string>Owner</string><key>LSItemContentTypes</key><array><string>public.folder</string></array></dict></array></dict></plist>`);
    const previous=JSON.parse(run(handler,['associations']));
    restore=()=>run(handler,['set',previous[0],'',previous[2]]);
    run(handler,['register',app]);run(handler,['set',id,'',id]);
    await probe('defaults');
    // macOS confirms /usr/bin/open's handoff; app exit after acceptance is outside that contract.
  } else {
    env.XDG_DATA_HOME=path.join(root,'data-home');env.XDG_CONFIG_HOME=path.join(root,'config-home');env.XDG_CURRENT_DESKTOP='GNOME';
    const apps=path.join(env.XDG_DATA_HOME,'applications');fs.mkdirSync(apps,{recursive:true});fs.mkdirSync(env.XDG_CONFIG_HOME);
    const handler=path.join(root,'handler.mjs');
    fs.writeFileSync(handler,`import fs from 'node:fs';import {fileURLToPath} from 'node:url';for(const arg of process.argv.slice(2)){const target=arg.startsWith('file:')?fileURLToPath(arg):arg;fs.appendFileSync(${JSON.stringify(log)},Buffer.from(target).toString('hex')+'\\n');}`);
    fs.writeFileSync(path.join(apps,'sodeck-open-test.desktop'),`[Desktop Entry]\nType=Application\nName=SoDeck Open Test\nExec="${process.execPath}" "${handler}" %u\nMimeType=x-scheme-handler/http;x-scheme-handler/https;inode/directory;\nNoDisplay=true\n`);
    run('update-desktop-database',[apps]);
    for(const mime of ['x-scheme-handler/http','x-scheme-handler/https','inode/directory'])run('xdg-mime',['default','sodeck-open-test.desktop',mime]);
    await probe('defaults');
    const failing=path.join(root,'failing');fs.mkdirSync(failing);
    for(const program of ['xdg-open','gio','gnome-open','kde-open6','kde-open5','kde-open'])fs.writeFileSync(path.join(failing,program),'#!/bin/sh\nexit 1\n',{mode:0o755});
    env.PATH=failing+path.delimiter+process.env.PATH;
    await probe('launcher-failure',directory,true);
    env.PATH=process.env.PATH;
  }
  fs.mkdirSync('artifacts/native-desktop-open',{recursive:true});
  const variant=launcherArgs.some(a=>a.endsWith('.AppImage')) || executable.endsWith('.AppImage')?'appimage':'installed';
  fs.writeFileSync(`artifacts/native-desktop-open/${report.target}-${variant}.json`,JSON.stringify({...report,actualCore:!fixtureOnly,controlledScheme:scheme,defaultHandlers:true,targetsExactlyOnce:true,invalidTargetsRejected:!fixtureOnly,launcherFailure:process.platform!=='darwin',variant},null,2)+'\n');
  console.log(fixtureOnly ? 'macOS default-handler fixture preflight passed.' : 'Actual package opened exact URL/directory once through system defaults, rejected invalid targets and exited.');
} catch (error) {
  failed = true;
  throw error;
} finally {
  try { restore(); } catch (error) { if (failed) console.error('Restoring isolated associations:',error); else throw error; }
  assert.equal(path.dirname(root),fs.realpathSync(os.tmpdir()));assert(path.basename(root).startsWith('sodeck-native-open-'));
  fs.rmSync(root,{recursive:true,force:true,maxRetries:8,retryDelay:250});
}
