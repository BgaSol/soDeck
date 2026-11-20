import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import net from 'node:net';import {spawn,spawnSync} from 'node:child_process';import assert from 'node:assert/strict';
assert.equal(process.platform,'win32','Run this Windows OpenSSH/PowerShell integration test on Windows');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-reading-ssh-'));
let daemon;
try{
  for(const name of ['host','user'])assert.equal(spawnSync('ssh-keygen',['-q','-t','ed25519','-N','','-f',path.join(dir,name)],{windowsHide:true}).status,0);
  const server=net.createServer();await new Promise(r=>server.listen(0,'127.0.0.1',r));const port=server.address().port;await new Promise(r=>server.close(r));
  const p=name=>path.join(dir,name).replaceAll('\\','/');
  fs.writeFileSync(path.join(dir,'sshd_config'),`Port ${port}\nListenAddress 127.0.0.1\nHostKey "${p('host')}"\nPidFile "${p('pid')}"\nAuthorizedKeysFile "${p('user.pub')}"\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nLogLevel ERROR\n`);
  const env={...process.env};delete env.PSModulePath;
  daemon=spawn(path.join(process.env.WINDIR,'System32/OpenSSH/sshd.exe'),['-D','-e','-f',path.join(dir,'sshd_config')],{env,windowsHide:true,stdio:['ignore','ignore','pipe']});let diagnostic='';daemon.stderr.on('data',b=>diagnostic+=b);
  for(let i=0;i<50;i++){await new Promise(r=>setTimeout(r,100));if(await new Promise(r=>{const s=net.connect({host:'127.0.0.1',port});s.on('connect',()=>{s.destroy();r(true);});s.on('error',()=>r(false));}))break;if(daemon.exitCode!==null)throw new Error(diagnostic);}
  const results=[];
  for(const shell of ['powershell.exe',spawnSync('where.exe',['pwsh.exe'],{encoding:'utf8',windowsHide:true}).stdout.trim().split(/\r?\n/)[0]]){
    const script=fs.readFileSync('plugins/terminal/shell/powershell.ps1','utf8').replaceAll('@TOKEN@','pty-reading-test');
    const command=`"${shell}" -NoLogo -NoExit -EncodedCommand ${Buffer.from(script,'utf16le').toString('base64')}`;
    const child=spawn('ssh',['-tt','-o','BatchMode=yes','-o','StrictHostKeyChecking=no','-o','UserKnownHostsFile=NUL','-o','IdentitiesOnly=yes','-i',path.join(dir,'user'),'-p',String(port),os.userInfo().username+'@127.0.0.1',command],{env,windowsHide:true,stdio:['pipe','pipe','pipe']});
    let output='',errors='';child.stdout.on('data',b=>output+=b.toString('utf8'));child.stderr.on('data',b=>errors+=b.toString());
    const wait=async predicate=>{const end=Date.now()+12000;while(!predicate()){if(child.exitCode!==null||Date.now()>end)throw new Error(shell+' PTY timeout '+JSON.stringify({output:output.slice(-4000),errors}));await new Promise(r=>setTimeout(r,50));}};
    try{
      await wait(()=>output.includes(';reading;1;input;1\x07'));
      child.stdin.write('Write-Output sodeck-pty-reading-ok\r');
      await wait(()=>output.includes(';reading;1;end;1;0\x07')&&output.includes('sodeck-pty-reading-ok'));
      child.stdin.write('Write-Error fixture -ErrorAction SilentlyContinue\r');
      await wait(()=>output.includes(';reading;1;end;2;?\x07'));
      child.stdin.write('exit\r');await new Promise(r=>child.once('exit',r));
      results.push({shell,passed:true,scope:'real Windows OpenSSH PTY, PSReadLine Enter, output, success and unknown'});
    }finally{if(child.exitCode===null)child.kill();}
  }
  fs.mkdirSync('artifacts',{recursive:true});fs.writeFileSync('artifacts/powershell-pty-results.json',JSON.stringify(results,null,2)+'\n');console.log(results);
}finally{if(daemon&&daemon.exitCode===null){daemon.kill();await new Promise(r=>daemon.once('exit',r));}assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert(path.basename(dir).startsWith('sodeck-reading-ssh-'));fs.rmSync(dir,{recursive:true,force:true});}
