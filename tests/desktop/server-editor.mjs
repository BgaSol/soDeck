import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdir,readFile,writeFile,access,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {root,launch,rpc,until,stop,pluginPages} from './harness.mjs';

// Actual Tauri/WebView2, DPAPI and loopback SSH. CDP drives only this isolated
// runtime; no global keyboard, clipboard or user server is accessed.
const output=join(root,'.qa/t007-native',String(Date.now()));
await mkdir(output,{recursive:true});const checks=[];
const python=process.env.SODECK_TEST_PYTHON??'C:/Users/bgaso/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/python.exe';
const fixture=spawn(python,['-u',join(root,'tests/core/ssh_fixture.py'),join(output,'fixture')],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,PYTHONPATH:join(root,'.local/python')}});
let stdout='',stderr='',app;fixture.stdout.on('data',d=>stdout+=d);fixture.stderr.on('data',d=>stderr+=d);
const events=async()=>readFile(join(output,'fixture/events.jsonl'),'utf8').catch(()=> '');
const password='isolated-test-password';
try{
  await until(()=>stdout.includes('\n'));const config=JSON.parse(stdout.split('\n')[0]);
  const exeHash=createHash('sha256').update(await readFile(join(root,'target/debug/sodeck-desktop.exe'))).digest('hex');
  const data=join(output,'app');app=await launch({data,shellPort:9583,pluginPort:9584});
  await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  await app.page.getByRole('button',{name:'添加服务器',exact:true}).click();
  await app.page.getByLabel('显示名称').fill('预设密码 · 隔离测试');await app.page.getByLabel('主机地址').fill('127.0.0.1');
  await app.page.getByLabel('端口',{exact:true}).fill(String(config.port));await app.page.getByLabel('用户名').fill('test-user');
  assert.equal(await app.page.getByLabel('在此设备上记住密码').isChecked(),true);
  await app.page.getByLabel(/^登录密码/).fill(password);await app.page.screenshot({path:join(root,'.qa/t007/native-editor.png')});
  await app.page.getByRole('button',{name:'保存服务器'}).click();await until(()=>app.page.getByRole('dialog').count().then(n=>n===0));
  const server=(await rpc(app.page,'servers.list')).servers[0];assert.equal(server.hasSecret,true);
  assert.ok(!(await events()).includes('password-auth'));
  const secretPath=join(data,'core/secrets',server.id+'.dpapi');
  // The core owns the DPAPI directory, never the profile/config JSON.
  const candidates=[secretPath,join(data,'secrets',server.id+'.dpapi')];
  const encrypted=await until(async()=>{for(const path of candidates){try{return {path,bytes:await readFile(path)};}catch{}}});
  assert.ok(!encrypted.bytes.includes(Buffer.from(password)));
  checks.push('SE-01: real server form stores default-remembered password as DPAPI ciphertext; saving performs no SSH authentication');
  await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  await app.page.screenshot({path:join(root,'.qa/t007/native-picker.png')});await app.page.keyboard.press('Escape');
  await app.page.getByRole('button',{name:'新建终端',exact:true}).first().click();
  await app.page.getByRole('heading',{name:'确认服务器身份'}).waitFor();assert.ok(!(await events()).includes('password-auth'));
  await app.page.getByRole('button',{name:'信任并连接',exact:true}).click();
  await until(async()=>{for(const p of await pluginPages(app))if((await p.locator('.xterm-rows').innerText()).includes('fixture-ready'))return true;});
  assert.ok((await events()).includes('"accepted": true'));
  const audit=await rpc(app.page,'calls.list',{limit:200});assert.ok(!JSON.stringify(audit).includes(password));
  const database=await readFile(join(data,'core/core.sqlite'));assert.ok(!database.includes(Buffer.from(password)));
  checks.push('SE-01/04: first host trust still precedes authentication; real SSH accepts preset password with no repeated password dialog; audit/config contain no plaintext password');
  await stop(app);assert.equal(app.process.exitCode,0);app=undefined;
  app=await launch({data,shellPort:9585,pluginPort:9586});assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).hasSecret,true);
  await app.page.getByRole('button',{name:'新建终端',exact:true}).first().click();
  await until(async()=>{const resources=await rpc(app.page,'resources.list');return resources.ptys.length===1;});
  assert.equal(await app.page.getByRole('heading',{name:'连接认证'}).count(),0);
  checks.push('SE-05: normal restart decrypts saved credential and creates a fresh SSH PTY without a password prompt');
  await app.page.getByRole('button',{name:'打开服务器',exact:true}).first().click();
  await app.page.getByRole('button',{name:'编辑 '+server.name,exact:true}).click();
  assert.equal(await app.page.getByLabel(/^登录密码/).inputValue(),'');
  await app.page.getByLabel(/^登录密码/).fill(password);await app.page.getByLabel('在此设备上记住密码').uncheck();
  await app.page.getByRole('button',{name:'保存服务器'}).click();await until(()=>app.page.getByRole('dialog').count().then(n=>n===0));
  assert.equal((await rpc(app.page,'servers.get',{serverId:server.id})).hasSecret,false);
  assert.equal(await access(encrypted.path).then(()=>true,()=>false),false);
  await stop(app);assert.equal(app.process.exitCode,0);app=undefined;
  app=await launch({data,shellPort:9587,pluginPort:9588});
  await app.page.getByRole('button',{name:'新建终端',exact:true}).first().click();await app.page.getByRole('heading',{name:'连接认证',exact:true}).waitFor();
  await app.page.getByRole('button',{name:'取消',exact:true}).click();
  checks.push('SE-02/05: editor never reads back a saved password; replacing it with an unremembered credential removes DPAPI storage and restart asks for authentication again');
  assert.deepEqual(app.errors,[]);await stop(app);assert.equal(app.process.exitCode,0);app=undefined;
  await writeFile(join(output,'results.json'),JSON.stringify({passed:true,scope:'Windows Tauri/WebView2 + DPAPI + loopback SSH; CDP UI, not physical input or Linux shell',exeHash,checks},null,2));
  console.log(JSON.stringify({passed:true,output,checks},null,2));
}catch(error){await writeFile(join(output,'failure.json'),JSON.stringify({checks,error:String(error),stack:error.stack},null,2));console.error(error);process.exitCode=1;}
finally{await stop(app);fixture.kill();await writeFile(join(output,'fixture.log'),stderr);for(const key of ['client-key','client-key-encrypted','client-openssh','client-openssh-encrypted','client-ed25519','client-ed25519-encrypted'])await rm(join(output,'fixture',key),{force:true});}
