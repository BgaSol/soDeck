import {createComponentWorkbench} from './component-workbench-fixture.mjs';
import {testPluginDirectory} from './test-plugin-directory.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {spawnSync} from 'node:child_process';

assert(process.env.SODECK_TEST_SSH_PORT,'Run through the isolated Docker Engine fixture');
const build=spawnSync('cargo',['build','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--bin','catalog-fixture'],{windowsHide:true,stdio:'inherit'});assert.equal(build.status,0);
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-directory-follow-'));
const packages=Object.fromEntries(['terminal','files','system-status','docker'].map(name=>{const root=testPluginDirectory(name),manifest=JSON.parse(fs.readFileSync(path.join(root,'plugin.json')));return [manifest.id,root];}));
const harness=await createComponentWorkbench(directory,packages),{call,pageFor,wait,errors}=harness;
try{
  for(const root of Object.values(packages))await call('fixture_load_plugin',{directory:root});
  const catalog=await call('server_catalog'),saved=await call('change_catalog',{revision:catalog.revision,change:{op:'saveServer',draft:{name:'目录发布联调',host:'127.0.0.1',port:Number(process.env.SODECK_TEST_SSH_PORT),username:process.env.SODECK_TEST_SSH_USER,authentication:{type:'privateKey',privateKeyPath:process.env.SODECK_TEST_SSH_KEY,secret:{action:'clear'}},route:[]}}});
  await call('connection',{command:{op:'start',serverId:saved.selectedId}});let connection;
  await wait(async()=>{[connection]=await call('connection',{command:{op:'snapshot'}});if(connection.question)await call('connection',{command:{op:'answer',attemptId:connection.attemptId,questionId:connection.question.id,answer:{kind:'hostKey',accept:true}}});if(connection.state==='failed')throw new Error(JSON.stringify(connection.error));return connection.state==='connected';},'isolated SSH');
  await call('connection',{command:{op:'focus',context:connection.context}});
  await call('component_ui',{window:'main',operation:'action',args:{context:connection.context,action:{op:'open',key:'sodeck.docker:browser',requestId:'directory-docker'}}});
  const page=await pageFor('main'),files=page.frameLocator('iframe[title^="文件 ·"]'),docker=page.frameLocator('iframe[title^="Docker ·"]'),terminal=page.frameLocator('iframe[title^="终端 ·"]');
  await terminal.locator('body[data-ready=true]').waitFor();await docker.locator('#state[data-state=ready]').waitFor();
  const selected=()=>files.getByRole('textbox',{name:'远端路径'}).inputValue(),expectPath=target=>wait(async()=>await selected()===target,'directory '+target);
  await terminal.locator('.xterm-helper-textarea').focus();await page.keyboard.type('cd /srv/sodeck-terminal');await page.keyboard.press('Enter');await expectPath('/srv/sodeck-terminal');
  const group=docker.locator('[data-tree-key="group:container:sodeck-test"]');await group.waitFor();
  const follow=async()=>{await group.click({button:'right'});await docker.getByRole('menuitem',{name:'跟随',exact:true}).click();await expectPath('/srv/sodeck-compose');};
  await follow();
  await files.getByRole('textbox',{name:'远端路径'}).fill('/srv/sodeck-manual');await files.getByRole('textbox',{name:'远端路径'}).press('Enter');await expectPath('/srv/sodeck-manual');
  await follow(); // A repeated same-path user publication must be new.
  await terminal.locator('.xterm-screen').click({position:{x:15,y:15}});await expectPath('/srv/sodeck-terminal');
  assert.deepEqual(errors,[]);fs.mkdirSync('artifacts/directory-follow',{recursive:true});await page.screenshot({path:'artifacts/directory-follow/compose-terminal.png'});
  fs.writeFileSync('artifacts/directory-follow/result.json',JSON.stringify({passed:true,actualCore:true,actualIndependentPlugins:Object.keys(packages),ssh:true,sftp:true,engine:'isolated',composeLabelDirectory:true,terminalAutomaticDirectory:true,repeatedSameDirectory:true,userReactivation:true},null,2)+'\n');
  console.log('Actual core/PTY/SFTP/isolated Docker Engine: Compose directory publication, repeated same-path follow, manual browsing and terminal reactivation passed.');
}finally{await harness.close();assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-directory-follow-'));fs.rmSync(directory,{recursive:true,force:true});}
