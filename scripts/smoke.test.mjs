import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

test("basic package mode requires its own fresh report and never accepts a nonzero exit", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-basic-smoke-test-"));
  try {
    const launcher = path.join(root, "launcher.mjs");
    fs.writeFileSync(launcher, `import fs from 'node:fs';import path from 'node:path';const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];if(!args.includes('--package-only'))throw Error('Missing basic mode');fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:false,packageNative:process.env.REPORT==='valid'}));process.exit(Number(process.env.EXIT_CODE||0));`);
    const env = { ...process.env, SODECK_PACKAGE_ONLY: "1" };
    const run = () => spawnSync(process.execPath, [fileURLToPath(new URL('./smoke.mjs', import.meta.url)), process.execPath, launcher], { env, encoding: "utf8" });
    assert.notEqual(run().status, 0);
    env.REPORT = "valid"; assert.equal(run().status, 0);
    env.EXIT_CODE = "7"; assert.notEqual(run().status, 0);
  } finally { assert.equal(path.dirname(root), os.tmpdir()); fs.rmSync(root, { recursive: true, force: true }); }
});

test("native opener smoke requires its own report and dispatches the probe once", () => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-open-smoke-test-'));
  try {
    const launcher=path.join(root,'launcher.mjs');
    fs.writeFileSync(launcher,`import fs from 'node:fs';import path from 'node:path';const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];if(args.includes('--verify-persisted'))throw new Error('Open probe must run once');if(!path.isAbsolute(args[args.indexOf('--desktop-open-test')+1]))throw new Error('Absolute probe required');fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:true,desktopOpenNative:process.env.WRITE_OPEN==='1'}));`);
    const env={...process.env,SODECK_TEST_DESKTOP_OPEN:path.join(root,'probe.json')};
    for(const key of ['SODECK_TEST_DATA_DIR','SODECK_VERIFY_PERSISTED','SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD','SODECK_TEST_NETWORK_PLUGIN','SODECK_EXPECT_CORE_VERSION','SODECK_EXPECT_TARGET'])delete env[key];
    const run=()=>spawnSync(process.execPath,[fileURLToPath(new URL('./smoke.mjs',import.meta.url)),process.execPath,launcher],{env,encoding:'utf8'});
    assert.notEqual(run().status,0);
    env.WRITE_OPEN='1';const result=run();assert.equal(result.status,0,result.stderr);
  } finally { fs.rmSync(root,{recursive:true,force:true}); }
});

test("Windows package cleanup waits for transient runtime file locks", { skip: process.platform !== "win32" }, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-smoke-lock-test-"));
  let smoke, locker, temporary;
  try {
    const launcher = path.join(root, "launcher.mjs"), recorded = path.join(root, "directory.txt"), ready = path.join(root, "ready");
    fs.writeFileSync(launcher, `
      import fs from 'node:fs';import path from 'node:path';import {setTimeout as delay} from 'node:timers/promises';
      const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];
      fs.writeFileSync(process.env.SODECK_TEST_DIRECTORY_REPORT,data);
      const deadline=Date.now()+10000;
      while(!fs.existsSync(process.env.SODECK_TEST_LOCK_READY)){if(Date.now()>deadline)throw new Error('Lock setup timed out');await delay(10);}
      fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:true}));
    `);
    const env = { ...process.env, SODECK_TEST_DIRECTORY_REPORT: recorded, SODECK_TEST_LOCK_READY: ready };
    for (const name of ['SODECK_TEST_DATA_DIR','SODECK_VERIFY_PERSISTED','SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD','SODECK_TEST_NETWORK_PLUGIN','SODECK_EXPECT_CORE_VERSION','SODECK_EXPECT_TARGET']) delete env[name];
    smoke = spawn(process.execPath, [fileURLToPath(new URL('./smoke.mjs', import.meta.url)), process.execPath, launcher], {env,windowsHide:true});
    let errors='';smoke.stderr.on('data',data=>errors+=data);smoke.stdout.resume();
    const finished=new Promise((resolve,reject)=>{smoke.on('error',reject);smoke.on('close',resolve);});
    const deadline=Date.now()+10000;
    while(!fs.existsSync(recorded)){assert(Date.now()<deadline,'The launcher must report its temporary directory');await delay(10);}
    temporary=fs.readFileSync(recorded,'utf8');
    locker=spawn('powershell.exe',['-NoProfile','-Command',"$ErrorActionPreference='Stop';$stream=[System.IO.File]::Open($env:SODECK_TEST_LOCK_FILE,'OpenOrCreate','ReadWrite','None');[Console]::WriteLine('ready');Start-Sleep -Milliseconds 1800;$stream.Dispose()"],{windowsHide:true,env:{...process.env,SODECK_TEST_LOCK_FILE:path.join(temporary,'runtime.lock')}});
    locker.stderr.on('data',data=>errors+=data);
    await new Promise((resolve,reject)=>{locker.on('error',reject);locker.stdout.once('data',resolve);locker.once('exit',code=>reject(new Error('Lock holder exited: '+code)));});
    fs.writeFileSync(ready,'ready');
    assert.equal(await finished,0,errors);
    assert.equal(fs.existsSync(temporary), false, 'The owned temporary directory must actually be removed');
  } finally {
    for(const child of [smoke,locker])if(child&&child.exitCode===null){const stopped=new Promise(resolve=>child.once('close',resolve));child.kill();await stopped;}
    if(temporary){assert.equal(path.dirname(path.resolve(temporary)),path.resolve(os.tmpdir()));assert(path.basename(temporary).startsWith('sodeck-package-'));fs.rmSync(temporary,{recursive:true,force:true,maxRetries:8,retryDelay:200});}
    fs.rmSync(root, { recursive:true,force:true,maxRetries:8,retryDelay:200 });
  }
});

test("package validation paths survive a launcher changing its working directory", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-launcher-test-"));
  try {
    const appDirectory = path.join(root, "mounted-app");
    fs.mkdirSync(appDirectory);
    fs.writeFileSync(path.join(root, "update.AppImage"), "signed package");
    fs.writeFileSync(path.join(root, "ssh-validation.json"), "isolated ssh config");
    const launcher = path.join(appDirectory, "launcher.mjs");
    fs.writeFileSync(launcher, `
      import fs from "node:fs";
      import path from "node:path";
      import assert from "node:assert/strict";
      import { fileURLToPath } from "node:url";
      const args = process.argv.slice(2);
      const value = flag => args[args.indexOf(flag) + 1];
      process.chdir(path.dirname(fileURLToPath(import.meta.url)));
      assert.equal(fs.readFileSync(value("--update-bundle-test"), "utf8"), "signed package");
      assert.equal(fs.readFileSync(value("--ssh-test-config"), "utf8"), "isolated ssh config");
      fs.writeFileSync(path.join(value("--test-data-dir"), "smoke-result.json"), '{"completed":true,"workspaceNative":true,"sshNative":true}');
    `);
    const env = { ...process.env, SODECK_TEST_UPDATE_BUNDLE: "update.AppImage", SODECK_TEST_SSH_CONFIG: "ssh-validation.json" };
    delete env.SODECK_TEST_DATA_DIR;
    delete env.SODECK_VERIFY_PERSISTED;
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./smoke.mjs", import.meta.url)), process.execPath, launcher], { cwd: root, env, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("network package validation rejects a generic report and clears stale failure reports on retry", () => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),"sodeck-network-smoke-test-"));
  try {
    const launcher=path.join(root,"launcher.mjs"),data=path.join(root,"data");fs.mkdirSync(data);
    fs.writeFileSync(launcher, `
      import fs from 'node:fs';import path from 'node:path';
      const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];
      fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:true}));
      if(process.env.WRITE_NETWORK_REPORT==='1')fs.writeFileSync(path.join(data,'network-smoke-result.json'),JSON.stringify({completed:true}));
    `);
    const env={...process.env,SODECK_TEST_DATA_DIR:data,SODECK_VERIFY_PERSISTED:'1',SODECK_TEST_NETWORK_PLUGIN:'isolated-test-package'};
    for(const name of ['SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD'])delete env[name];
    const run=()=>spawnSync(process.execPath,[fileURLToPath(new URL('./smoke.mjs',import.meta.url)),process.execPath,launcher],{env,encoding:'utf8'});
    // A report left by a previous run must not substitute for this run's result.
    fs.writeFileSync(path.join(data,'network-smoke-result.json'),'{"completed":true}');
    assert.notEqual(run().status,0);
    fs.writeFileSync(path.join(data,'smoke-error.txt'),'previous failed attempt');
    env.WRITE_NETWORK_REPORT='1';const result=run();assert.equal(result.status,0,result.stderr);
    assert.equal(fs.existsSync(path.join(data,'smoke-error.txt')),false);
  } finally {fs.rmSync(root,{recursive:true,force:true});}
});

test("native package validation requires the selected runtime version and target, including after reopening", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "sodeck-native-identity-test-"));
  try {
    const launcher = path.join(root, "launcher.mjs");
    fs.writeFileSync(launcher, `
      import fs from 'node:fs';import path from 'node:path';
      const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];
      const version=args.includes('--verify-persisted')&&process.env.WRONG_REOPEN==='1'?'0.9.2':process.env.ACTUAL_VERSION;
      fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:true,version,target:process.env.ACTUAL_TARGET}));
    `);
    const env = { ...process.env, SODECK_EXPECT_CORE_VERSION: '0.9.1', SODECK_EXPECT_TARGET: 'x86_64-unknown-linux-gnu', ACTUAL_VERSION: '0.9.1', ACTUAL_TARGET: 'x86_64-unknown-linux-gnu' };
    for (const name of ['SODECK_TEST_DATA_DIR','SODECK_VERIFY_PERSISTED','SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD','SODECK_TEST_NETWORK_PLUGIN']) delete env[name];
    const run = () => spawnSync(process.execPath, [fileURLToPath(new URL('./smoke.mjs', import.meta.url)), process.execPath, launcher], {env, encoding:'utf8'});
    assert.equal(run().status, 0);
    for (const [key,value] of [['ACTUAL_VERSION','0.9.2'],['ACTUAL_VERSION',''],['ACTUAL_TARGET','aarch64-unknown-linux-gnu'],['WRONG_REOPEN','1']]) {
      const saved=env[key];env[key]=value;const result=run();assert.notEqual(result.status,0);assert.match(result.stderr,/version\/target mismatch/);
      if(saved===undefined)delete env[key];else env[key]=saved;
    }
  } finally { fs.rmSync(root, { recursive:true, force:true }); }
});

test("native files smoke does not replay a changed provider and rejects stale/text-only reports", () => {
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-file-smoke-test-'));
  try{
    const launcher=path.join(root,'launcher.mjs'),data=path.join(root,'data');fs.mkdirSync(data);
    fs.writeFileSync(launcher,`import fs from 'node:fs';import path from 'node:path';const args=process.argv.slice(2),data=args[args.indexOf('--test-data-dir')+1];if(args.includes('--verify-persisted'))throw new Error('File probe cannot be replayed after changing its provider to text');fs.writeFileSync(path.join(data,'smoke-result.json'),JSON.stringify({completed:true,workspaceNative:true,clipboardNative:true,desktopFilesNative:process.env.WRITE_FILES==='1'}));`);
    const env={...process.env,SODECK_TEST_DATA_DIR:data,SODECK_TEST_DESKTOP_FILES:path.join(root,'probe.json')};
    delete env.SODECK_VERIFY_PERSISTED;
    for(const key of ['SODECK_TEST_SSH_CONFIG','SODECK_TEST_UPDATE_BUNDLE','SODECK_TEST_CLIPBOARD','SODECK_TEST_NETWORK_PLUGIN','SODECK_EXPECT_CORE_VERSION','SODECK_EXPECT_TARGET'])delete env[key];
    const run=()=>spawnSync(process.execPath,[fileURLToPath(new URL('./smoke.mjs',import.meta.url)),process.execPath,launcher],{env,encoding:'utf8'});
    fs.writeFileSync(path.join(data,'smoke-result.json'),'{"completed":true,"desktopFilesNative":true}');
    assert.notEqual(run().status,0);
    env.WRITE_FILES='1';const result=run();assert.equal(result.status,0,result.stderr);
  }finally{fs.rmSync(root,{recursive:true,force:true});}
});
