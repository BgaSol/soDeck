import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';

// Runs the shipped hooks with real PSReadLine loaded and a deterministic host ReadLine.
// The browser/SSH tests cover transport and input separately; this does not simulate a native key press.
const shells=process.platform==='win32'?['powershell.exe','pwsh.exe']:['pwsh'];
const folder=fs.mkdtempSync(path.join(os.tmpdir(),'sodeck-powershell-reading-'));
try{
  for(const shell of shells){
    const probe=spawnSync(shell,['-NoProfile','-Command','$PSVersionTable.PSVersion.ToString()'],{encoding:'utf8',windowsHide:true});
    if(probe.error?.code==='ENOENT'){console.log(`${shell}: not installed (skipped)`);continue;}
    assert.equal(probe.status,0,probe.stderr);
    const script=fs.readFileSync('plugins/terminal/shell/powershell.ps1','utf8').replaceAll('@TOKEN@','ps-test');
    const harness=`Import-Module PSReadLine -ErrorAction Stop
function global:PSConsoleHostReadLine { return 'cat app.log' }
function global:prompt { return 'custom> ' }
${script}
$first = prompt
if ($first -notlike 'custom> *') { throw 'user prompt was replaced' }
$line = PSConsoleHostReadLine
if ($line -ne 'cat app.log') { throw 'submitted input was changed' }
Write-Output 'fixture-command'
$null = prompt
$line = PSConsoleHostReadLine
Write-Error fixture -ErrorAction SilentlyContinue
$null = prompt
`;
    const file=path.join(folder,'hooks.ps1');fs.writeFileSync(file,'\uFEFF'+harness);
    const result=spawnSync(shell,['-NoLogo','-NoProfile','-File',file],{encoding:'utf8',windowsHide:true});assert.equal(result.status,0,result.stderr);
    const events=[...result.stdout.matchAll(/\x1b\]633;SODECK;ps-test;reading;1;([^\x07]*)\x07/g)].map(m=>m[1]);
    assert.deepEqual(events,['prompt;1','start;1','end;1;0','prompt;2','start;2','end;2;?','prompt;3']);
    assert(!result.stdout.includes('cat app.log'),'Hooks must not transmit unobserved command text');
    console.log(`${shell} ${probe.stdout.trim()}: real hooks, custom prompt, input preservation, success/unknown, no command text passed.`);
  }
}finally{fs.rmSync(folder,{recursive:true,force:true});}
