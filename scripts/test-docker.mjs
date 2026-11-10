// All Docker mutations target a separate daemon. Never mount the host Docker socket.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {packageRoot} from './docker-process.mjs';
const run=(args,options={})=>{
  const r=spawnSync('docker',args,{encoding:'utf8',windowsHide:true,timeout:120000,...options});
  if(r.status!==0)throw new Error('Docker fixture command failed: '+args[0]+' '+(r.stderr||r.error||''));
  return r.stdout.trim();
};
const suffix=randomUUID().slice(0,8),name='sodeck-docker-test-'+suffix,engine=name+'-engine',ssh=name+'-ssh',socket=name+'-socket',image=name+':local';
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-engine-'));
const created=[];let volumeCreated=false,imageBuilt=false;
try{
  const key=spawnSync('ssh-keygen',['-q','-t','ed25519','-N','','-f',path.join(directory,'user')],{windowsHide:true,encoding:'utf8'});assert.equal(key.status,0,key.stderr);
  fs.writeFileSync(path.join(directory,'Dockerfile'),'FROM docker:24.0.9-cli\nARG SODECK_ALPINE_MIRROR=https://dl-cdn.alpinelinux.org/alpine\nRUN printf "%s/v3.20/main\\n%s/v3.20/community\\n" "$SODECK_ALPINE_MIRROR" "$SODECK_ALPINE_MIRROR" > /etc/apk/repositories && apk del openssh-client && apk add --no-cache openssh-server && passwd -d root\n');
  const mirror=process.env.SODECK_TEST_ALPINE_MIRROR,buildArgs=['build','-t',image];
  if(mirror){const url=new URL(mirror);assert(url.protocol==='https:'&&!url.username&&!url.password&&!url.search&&!url.hash);buildArgs.push('--build-arg','SODECK_ALPINE_MIRROR='+mirror.replace(/\/$/,''));}
  run([...buildArgs,directory]);imageBuilt=true;
  run(['volume','create',socket]);volumeCreated=true;
  run(['run','-d','--name',engine,'--label','sodeck.test='+suffix,'--privileged','--mount','type=volume,source='+socket+',target=/var/run',
    'docker:24.0.9-dind','--host=unix:///var/run/docker.sock','--storage-driver=vfs','--iptables=false','--bridge=none']);created.push(engine);
  const inside=args=>run(['exec',engine,'docker','--host','unix:///var/run/docker.sock',...args]);
  let ready=false;
  for(let i=0;i<60;i++){
    const result=spawnSync('docker',['exec',engine,'docker','--host','unix:///var/run/docker.sock','info'],{encoding:'utf8',windowsHide:true,timeout:5000});
    if(result.status===0){ready=true;break;}await new Promise(r=>setTimeout(r,500));
  }
  assert(ready,'isolated Docker daemon unavailable');
  const saved=spawn('docker',['image','save',image],{stdio:['ignore','pipe','inherit'],windowsHide:true});
  const loaded=spawn('docker',['exec','-i',engine,'docker','--host','unix:///var/run/docker.sock','load'],{stdio:['pipe','ignore','inherit'],windowsHide:true});
  saved.stdout.pipe(loaded.stdin);
  const exits=await Promise.all([new Promise(r=>saved.on('exit',r)),new Promise(r=>loaded.on('exit',r))]);assert.deepEqual(exits,[0,0]);
  const appVolume=name+'-data';inside(['volume','create',appVolume]);
  const app=inside(['run','-d','--name',name+'-app','--network','none','--label','sodeck.test='+suffix,'--label','com.docker.compose.project=sodeck-test',
    '--label','com.docker.compose.service=api','--env','SODECK_TOKEN=a=b','--mount','type=volume,source='+appVolume+',target=/data',image,
    'sh','-c','printf "sodeck-docker-log 中文\\n"; exec sleep 600']);
  const publicKey=fs.readFileSync(path.join(directory,'user.pub'),'utf8').trim();
  run(['run','-d','--name',ssh,'--label','sodeck.test='+suffix,'-p','127.0.0.1::22','--mount','type=volume,source='+socket+',target=/var/run',
    '-e','PUBKEY='+publicKey,image,'sh','-c',
    'mkdir -p /root/.ssh /run/sshd; printf "%s\\n" "$PUBKEY" > /root/.ssh/authorized_keys; ssh-keygen -A; exec /usr/sbin/sshd -D -e -o PermitRootLogin=prohibit-password -o PasswordAuthentication=no -o StrictModes=no']);created.push(ssh);
  const port=run(['port',ssh,'22/tcp']).split(':').at(-1);
  let sshReady=false;
  for(let i=0;i<40;i++){
    const probe=spawnSync('docker',['exec',ssh,'test','-f','/etc/ssh/ssh_host_ed25519_key'],{windowsHide:true});if(probe.status===0){sshReady=true;break;}
    await new Promise(r=>setTimeout(r,100));
  }
  assert(sshReady);
  const build=spawnSync(process.execPath,['scripts/package-plugin.mjs','--plugin','docker'],{stdio:'inherit',windowsHide:true});assert.equal(build.status,0);
  const code=await new Promise((resolve,reject)=>{
    const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections','openssh_docker_plugin','--','--ignored','--exact','--nocapture'],
      {stdio:'inherit',windowsHide:true,env:{...process.env,SODECK_TEST_SSH_PORT:port,SODECK_TEST_SSH_USER:'root',SODECK_TEST_SSH_KEY:path.join(directory,'user'),SODECK_TEST_DOCKER_PLUGIN:packageRoot(),SODECK_TEST_DOCKER_CONTAINER:app}});
    test.on('error',reject);test.on('exit',resolve);
  });
  assert.equal(code,0);
  inside(['volume','inspect',appVolume]);assert.equal(inside(['container','ls','--all','--quiet']).length,0,'test container should have been explicitly removed');
  fs.mkdirSync('artifacts/docker-integration',{recursive:true});
  fs.writeFileSync('artifacts/docker-integration/result.json',JSON.stringify({engine:'24.0.9',clientTarget:path.basename(packageRoot()).match(/^sodeck-docker_[^_]+_(.+)$/)[1],actualCore:true,actualPlugin:true,ssh:true,containerRemoved:true,volumePreserved:true},null,2)+'\n');
  console.log('Actual core/native Docker plugin/OpenSSH/isolated Engine: list, details, logs, stats, pause/unpause/stop/start/restart/remove, retained logs on disconnect and volume preservation passed.');
}finally{
  for(const resource of created.reverse()){assert(resource.startsWith(name+'-'));spawnSync('docker',['rm','-f','-v',resource],{windowsHide:true,stdio:'ignore'});}
  if(volumeCreated)spawnSync('docker',['volume','rm',socket],{windowsHide:true,stdio:'ignore'});
  if(imageBuilt)spawnSync('docker',['image','rm',image],{windowsHide:true,stdio:'ignore'});
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-engine-'));fs.rmSync(directory,{recursive:true,force:true});
}
