// Test-only JSON-RPC peer for the actual native Docker plugin.
import {spawn,spawnSync} from 'node:child_process';
import path from 'node:path';
import {cargoPackageVersion} from './cargo-package.mjs';
import readline from 'node:readline';
export function pluginPeer(executable, core) {
  const child=spawn(executable,[],{stdio:['pipe','pipe','pipe'],windowsHide:true});
  let seq=0,stderr='';const pending=new Map();
  child.stderr.on('data',b=>stderr+=b);
  const send=frame=>child.stdin.write(JSON.stringify({jsonrpc:'2.0',...frame})+'\n');
  const fail=error=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(error);}pending.clear();};
  child.on('error',fail);child.on('exit',code=>fail(new Error('Plugin exited '+code+': '+stderr)));
  readline.createInterface({input:child.stdout}).on('line',async line=>{
    const f=JSON.parse(line);
    if(f.method){
      try{send({id:f.id,result:await core(f.method,f.params)});}
      catch(error){send({id:f.id,error:{code:error.code??-32000,message:error.message,data:error.data}});}
    }else{const p=pending.get(f.id);if(!p)return;pending.delete(f.id);clearTimeout(p.timer);f.error?p.reject(new Error(f.error.message)):p.resolve(f.result);}
  });
  return {
    child,
    request(method,params={}){return new Promise((resolve,reject)=>{const id=String(++seq),timer=setTimeout(()=>{pending.delete(id);reject(new Error('RPC timeout: '+method));},10000);pending.set(id,{resolve,reject,timer});send({id,method,params});});},
    async close(){if(child.exitCode!==null)return;const exited=new Promise(resolve=>child.once('exit',resolve));await this.request('shutdown');await exited;}
  };
}
export const context={workspaceId:'docker-test',serverId:'server',connectionId:'connection',generation:1};
export const capabilities=['session.exec.v1','ui.workbench.v2','ui.openView.v1','settings.v1'];
export function packageRoot(){
  const compiler=spawnSync('rustc',['-vV'],{encoding:'utf8',windowsHide:true});
  if(compiler.status!==0)throw new Error('Docker tests require the repository Rust toolchain on PATH: '+(compiler.error?.message||compiler.stderr));
  const target=compiler.stdout.match(/^host: (.+)$/m)?.[1];
  if(!target)throw new Error('Native Rust host unavailable');
  return path.resolve(process.env.SODECK_TEST_DOCKER_PLUGIN||'artifacts/plugins/sodeck-docker_'+cargoPackageVersion('plugins/docker/Cargo.toml')+'_'+target);
}
export async function until(read, predicate, timeout=10000){
  const end=Date.now()+timeout;
  while(Date.now()<end){const value=await read();if(predicate(value))return value;await new Promise(resolve=>setTimeout(resolve,25));}
  throw new Error('Timed out waiting for plugin state');
}
