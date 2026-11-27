// All Docker mutations target a separate daemon. Never mount the host Docker socket.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {spawn,spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {packageRoot} from './docker-process.mjs';
import {testPluginDirectory} from './test-plugin-directory.mjs';
const run=(args,options={})=>{
  const r=spawnSync('docker',args,{encoding:'utf8',windowsHide:true,timeout:120000,...options});
  if(r.status!==0)throw new Error('Docker fixture command failed: '+args[0]+' '+(r.stderr||r.error||''));
  return r.stdout.trim();
};
const suffix=randomUUID().slice(0,8),name='sodeck-docker-test-'+suffix,engine=name+'-engine',ssh=name+'-ssh',socket=name+'-socket',image=name+':local';
const native=process.argv.length>2,executable=process.argv[3];
if(native)assert(process.argv[2]==='--native'&&process.argv.length===4&&executable&&fs.existsSync(executable),'--native requires an actual core executable');
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-engine-'));
const created=[];let volumeCreated=false,imageBuilt=false;
try{
  const key=spawnSync('ssh-keygen',['-q','-t','ed25519','-N','','-f',path.join(directory,'user')],{windowsHide:true,encoding:'utf8'});assert.equal(key.status,0,key.stderr);
  fs.writeFileSync(path.join(directory,'Dockerfile'),'FROM docker:24.0.9-cli\nARG SODECK_ALPINE_MIRROR=https://dl-cdn.alpinelinux.org/alpine\nRUN printf "%s/v3.20/main\\n%s/v3.20/community\\n" "$SODECK_ALPINE_MIRROR" "$SODECK_ALPINE_MIRROR" > /etc/apk/repositories && apk del openssh-client && apk add --no-cache openssh-server busybox-extras bash && passwd -d root && sed -i "/^root:/s|:[^:]*$|:/bin/bash|" /etc/passwd\n');
  const mirror=process.env.SODECK_TEST_ALPINE_MIRROR,buildArgs=['build','-t',image];
  if(mirror){const url=new URL(mirror);assert(url.protocol==='https:'&&!url.username&&!url.password&&!url.search&&!url.hash);buildArgs.push('--build-arg','SODECK_ALPINE_MIRROR='+mirror.replace(/\/$/,''));}
  run([...buildArgs,directory]);imageBuilt=true;
  run(['volume','create',socket]);volumeCreated=true;
  run(['run','-d','--name',engine,'--label','sodeck.test='+suffix,'-p','127.0.0.1::22','--privileged','--init','--entrypoint','dockerd','--mount','type=volume,source='+socket+',target=/var/run',
    'docker:24.0.9-dind','--host=unix:///var/run/docker.sock','--data-root=/var/lib/sodeck-test-docker','--storage-driver=vfs','--iptables=false','--bridge=none']);created.push(engine);
  const inside=args=>run(['exec',engine,'docker','--host','unix:///var/run/docker.sock',...args]);
  let ready=false;
  for(let i=0;i<60;i++){
    const result=spawnSync('docker',['exec',engine,'docker','--host','unix:///var/run/docker.sock','info'],{encoding:'utf8',windowsHide:true,timeout:5000});
    if(result.status===0){ready=true;break;}await new Promise(r=>setTimeout(r,500));
  }
  assert(ready,'isolated Docker daemon unavailable\n'+run(['logs',engine]));
  const saved=spawn('docker',['image','save',image],{stdio:['ignore','pipe','inherit'],windowsHide:true});
  const loaded=spawn('docker',['exec','-i',engine,'docker','--host','unix:///var/run/docker.sock','load'],{stdio:['pipe','ignore','inherit'],windowsHide:true});
  saved.stdout.pipe(loaded.stdin);
  const exits=await Promise.all([new Promise(r=>saved.on('exit',r)),new Promise(r=>loaded.on('exit',r))]);assert.deepEqual(exits,[0,0]);
  const appVolume=name+'-data';inside(['volume','create',appVolume]);
  const freeVolume=name+'-unused-data';inside(['volume','create',freeVolume]);
  const freeNetwork=inside(['network','create',name+'-unused-network']);
  const portNetwork=inside(['network','create',name+'-port-network']);
  const alias=name+':alias';inside(['tag',image,alias]);
  let registryName,pullReference;
  if(!native){registryName=name+'-registry';inside(['run','-d','--name',registryName,'--network','host','registry:2.8.3']);
    pullReference='127.0.0.1:5000/sodeck-fixture:stable';inside(['tag',image,pullReference]);
    const deadline=Date.now()+60000;let pushed=false;
    while(Date.now()<deadline){const result=spawnSync('docker',['exec',engine,'docker','--host','unix:///var/run/docker.sock','push',pullReference],{encoding:'utf8',windowsHide:true,timeout:10000});if(result.status===0){pushed=true;break;}await new Promise(r=>setTimeout(r,200));}
    assert(pushed,'isolated registry did not accept the fixed test image');inside(['image','rm',pullReference]);
  }
  const app=inside(['run','-d','--name',name+'-app','--network',portNetwork,'-p','127.0.0.1::8080','--label','sodeck.test='+suffix,'--label','com.docker.compose.project=sodeck-test',
    '--label','com.docker.compose.service=api','--label','com.docker.compose.project.working_dir=/srv/sodeck-compose','--env','SODECK_TOKEN=a=b','--mount','type=volume,source='+appVolume+',target=/data',
    ...(native?['--log-opt','max-size=10m','--log-opt','max-file=3']:[]),image,
    'sh','-c','mkdir -p /tmp/http; printf sodeck-port-tunnel > /tmp/http/index.html; httpd -p 8080 -h /tmp/http; '+(native?'awk \'BEGIN {for (i=0;i<1000000;i++) printf "INFO historical %07d 中文\\n",i}\'; printf "sodeck-docker-log 中文\\n"; i=0; while true; do awk -v batch="$i" \'BEGIN {for (j=0;j<1000;j++) printf "INFO live %07d 中文 %0100d\\n",batch*1000+j,j}\'; i=$((i+1)); sleep 1; done':'awk \'BEGIN {for (i=0;i<1000000;i++) printf "INFO historical %07d 中文\\n",i}\'; printf "sodeck-docker-log 中文\\n"; exec sleep 600')]);
  const historyStarted=Date.now();
  while(!inside(['logs','--tail',native?'2000':'1',app]).includes(native?'INFO live':'sodeck-docker-log 中文')) {
    assert(Date.now()-historyStarted<60000,'million-line history did not finish generating');
    await new Promise(resolve=>setTimeout(resolve,200));
  }
  const historyMs=Date.now()-historyStarted;
  let benchmark;
  if(native){
    benchmark=inside(['run','-d','--name',name+'-benchmark','--network','none','--log-opt','max-size=64m','--log-opt','max-file=1',image,'sh','-c','awk \'BEGIN {for (i=0;i<1000000;i++) printf "INFO %07d\\n",i}\'']);
    const started=Date.now();while(inside(['inspect','--format','{{.State.Running}}',benchmark])==='true'){assert(Date.now()-started<60000,'benchmark source did not finish');await new Promise(r=>setTimeout(r,200));}
  }
  const publicKey=fs.readFileSync(path.join(directory,'user.pub'),'utf8').trim();
  run(['run','-d','--name',ssh,'--label','sodeck.test='+suffix,'--network','container:'+engine,'--mount','type=volume,source='+socket+',target=/var/run',
    '-e','PUBKEY='+publicKey,image,'sh','-c',
    'mkdir -p /root/.ssh /run/sshd /srv/sodeck-compose /srv/sodeck-terminal /srv/sodeck-manual; printf "%s\\n" "$PUBKEY" > /root/.ssh/authorized_keys; ssh-keygen -A; exec /usr/sbin/sshd -D -e -o PermitRootLogin=prohibit-password -o PasswordAuthentication=no -o StrictModes=no -o AllowTcpForwarding=yes -o PermitOpen=any']);created.push(ssh);
  const port=run(['port',engine,'22/tcp']).split(':').at(-1);
  let sshReady=false;
  for(let i=0;i<40;i++){
    const probe=spawnSync('docker',['exec',ssh,'test','-f','/etc/ssh/ssh_host_ed25519_key'],{windowsHide:true});if(probe.status===0){sshReady=true;break;}
    await new Promise(r=>setTimeout(r,100));
  }
  assert(sshReady);
  packageRoot(); // Fixtures are prepared explicitly before source integration tests.
  if(native) {
    const config=path.join(directory,'ssh.json');fs.writeFileSync(config,JSON.stringify({port:Number(port),username:'root',privateKeyPath:path.join(directory,'user'),containerId:app,benchmarkId:benchmark}));
    const code=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-docker-native.mjs',path.resolve(executable)],{stdio:'inherit',windowsHide:true,env:{...process.env,SODECK_TEST_SSH_CONFIG:config,SODECK_TEST_DOCKER_PLUGIN:packageRoot()}});test.on('error',reject);test.on('exit',resolve);});
    assert.equal(code,0);
  } else {
  const followCode=await new Promise((resolve,reject)=>{const test=spawn(process.execPath,['scripts/test-directory-follow-integration.mjs'],{stdio:'inherit',windowsHide:true,env:{...process.env,SODECK_TEST_SSH_PORT:port,SODECK_TEST_SSH_USER:'root',SODECK_TEST_SSH_KEY:path.join(directory,'user')}});test.on('error',reject);test.on('exit',resolve);});assert.equal(followCode,0,'actual directory collaboration failed');
  const code=await new Promise((resolve,reject)=>{
    const test=spawn('cargo',['test','--manifest-path','src-tauri/Cargo.toml','--features','test-fixtures','--locked','--test','connections','openssh_docker_plugin','--','--ignored','--exact','--nocapture'],
      {stdio:'inherit',windowsHide:true,env:{...process.env,SODECK_TEST_SSH_PORT:port,SODECK_TEST_SSH_USER:'root',SODECK_TEST_SSH_KEY:path.join(directory,'user'),SODECK_TEST_DOCKER_TUNNELS:'1',SODECK_TEST_NETWORK_PLUGIN:testPluginDirectory("network-proxy"),SODECK_TEST_DOCKER_PLUGIN:packageRoot(),SODECK_TEST_TERMINAL_PLUGIN:testPluginDirectory("terminal"),SODECK_TEST_DOCKER_CONTAINER:app,SODECK_TEST_DOCKER_FREE_VOLUME:freeVolume,SODECK_TEST_DOCKER_NETWORK:freeNetwork,SODECK_TEST_DOCKER_PULL:pullReference}});
    test.on('error',reject);test.on('exit',resolve);
  });
  assert.equal(code,0);
  inside(['network','rm',portNetwork]);inside(['volume','inspect',appVolume]);inside(['rm','-f','-v',registryName]);assert.equal(inside(['container','ls','--all','--quiet']).length,0,'test containers should have been explicitly removed');
  fs.mkdirSync('artifacts/docker-integration',{recursive:true});
  fs.writeFileSync('artifacts/docker-integration/result.json',JSON.stringify({engine:'24.0.9',clientTarget:JSON.parse(fs.readFileSync(path.join(packageRoot(),'plugin.json'),'utf8')).target,actualCore:true,actualPlugin:true,ssh:true,terminalCommand:true,sourceCloseKeepsTerminal:true,portTunnel:true,sourceCloseKeepsTunnel:true,historyLines:1000001,historyGenerationMs:historyMs,initialTailLines:1000,sessionOwned:true,initialCaptureLimitBytes:16*1024*1024,containerRemoved:true,volumePreserved:true},null,2)+'\n');
  console.log('Actual core/native Docker plugin/OpenSSH/isolated Engine: inline resource observations, container terminal through commands, terminal survives source close, logs, stats, pause/unpause/stop/start/restart/remove, retained logs on disconnect and volume preservation passed.');
  }
}finally{
  for(const resource of created.reverse()){assert(resource.startsWith(name+'-'));spawnSync('docker',['rm','-f','-v',resource],{windowsHide:true,stdio:'ignore'});}
  if(volumeCreated)spawnSync('docker',['volume','rm',socket],{windowsHide:true,stdio:'ignore'});
  if(imageBuilt)spawnSync('docker',['image','rm',image],{windowsHide:true,stdio:'ignore'});
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-engine-'));fs.rmSync(directory,{recursive:true,force:true});
}
