import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
