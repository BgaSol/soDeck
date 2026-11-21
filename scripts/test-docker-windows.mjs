// Real Windows OpenSSH + Docker Desktop; mutations use uniquely named test resources.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {spawn,spawnSync} from 'node:child_process';import {randomUUID} from 'node:crypto';
const run=args=>{const r=spawnSync('docker',args,{encoding:'utf8',windowsHide:true,timeout:120000});assert.equal(r.status,0,r.stderr||r.error?.message);return r.stdout.trim();};
assert.equal(process.platform,'win32','Requires native Windows and a running Docker Desktop Linux Engine');
const version=JSON.parse(run(['version','--format','{{json .}}']));assert.equal(version.Client.Os,'windows');assert.equal(version.Server.Os,'linux');
const info=JSON.parse(run(['info','--format','{{json .}}']));
const suffix=randomUUID(),prefix='sodeck-windows-'+suffix,image=prefix+':fixture',alias=prefix+':alias';
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-docker-windows-')),volumes=[],networks=[],containers=[];let built=false;
try{
  fs.writeFileSync(path.join(directory,'Dockerfile'),`FROM busybox:1.37.0\nLABEL sodeck.test="${suffix}"\n`);
  run(['build','-t',image,directory]);built=true;run(['tag',image,alias]);
  for(const name of ['data','unused']){const volume=prefix+'-'+name;run(['volume','create','--label','sodeck.test='+suffix,volume]);volumes.push(volume);}
  const network=run(['network','create','--label','sodeck.test='+suffix,prefix+'-network']);networks.push(network);
  const id=run(['run','-d','--name',prefix+'-app','--network','bridge','-p','127.0.0.1::8080','--label','sodeck.test='+suffix,'--label','com.docker.compose.project=sodeck-test','--env','SODECK_TOKEN=a=b','--mount','type=volume,source='+volumes[0]+',target=/data',image,'sh','-c',`mkdir -p /tmp/http; printf 'sodeck-port-tunnel' > /tmp/http/index.html; httpd -p 8080 -h /tmp/http; awk 'BEGIN {for (i=0;i<1000000;i++) printf "INFO historical %07d 中文\\n",i}'; printf 'sodeck-docker-log 中文\\n'; exec sleep 600`]);containers.push(id);
  const end=Date.now()+60000;while(!run(['logs','--tail','1',id]).includes('sodeck-docker-log 中文')){assert(Date.now()<end,'fixture log generation timed out');await new Promise(r=>setTimeout(r,200));}
  const env={...process.env,SODECK_TEST_DOCKER_TUNNELS:'1',SODECK_TEST_DOCKER_WINDOWS:'1',SODECK_TEST_DOCKER_ROOT:info.DockerRootDir,SODECK_TEST_DOCKER_CONTAINER:id,SODECK_TEST_DOCKER_FREE_VOLUME:volumes[1],SODECK_TEST_DOCKER_NETWORK:network,SODECK_TEST_DOCKER_PULL:'docker.io/library/busybox:1.37.0'};
  const code=await new Promise((resolve,reject)=>{const child=spawn(process.execPath,['scripts/test-openssh.mjs','--docker'],{stdio:'inherit',windowsHide:true,env});child.on('error',reject);child.on('exit',resolve);});assert.equal(code,0);
  console.log(JSON.stringify({test:'Windows SSH Docker Desktop',client:version.Client.Version,server:version.Server.Version,engineOs:version.Server.Os,arch:version.Server.Arch,root:info.DockerRootDir,result:'passed'}));
}finally{
  for(const id of containers)spawnSync('docker',['container','rm','--force',id],{windowsHide:true});
  for(const id of networks)spawnSync('docker',['network','rm',id],{windowsHide:true});
  for(const name of volumes)spawnSync('docker',['volume','rm',name],{windowsHide:true});
  if(built)spawnSync('docker',['image','rm',alias,image],{windowsHide:true});
  assert.equal(path.dirname(directory),os.tmpdir());assert(path.basename(directory).startsWith('sodeck-docker-windows-'));fs.rmSync(directory,{recursive:true,force:true});
}
