import {spawn} from 'node:child_process';
const child=spawn(process.execPath,['scripts/test-openssh.mjs','--mcp'],{stdio:'inherit',windowsHide:true});
child.on('error',error=>{throw error;});
process.exitCode=await new Promise(resolve=>child.on('exit',code=>resolve(code??1)));
