import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {resolve,dirname,join} from 'node:path';
import {createHash} from 'node:crypto';
export const root=resolve(import.meta.dirname,'../..');
export const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
export const json=async path=>JSON.parse(await readFile(path,'utf8'));
export async function save(path,value){await mkdir(dirname(path),{recursive:true});await writeFile(path,JSON.stringify(value,null,2)+'\n');}
export function run(program,args,{cwd=root,env={},capture=false}={}){const r=spawnSync(program,args,{cwd,env:{...process.env,...env},windowsHide:true,encoding:'utf8',maxBuffer:64*1024*1024,stdio:capture?'pipe':'inherit'});if(r.status!==0)throw Error(`${program} failed (${r.status}): ${capture?r.stderr:''}`);return r.stdout;}
export const node=(args,options)=>run(process.execPath,args,options);
export const git=args=>run('git',args,{capture:true}).trim();
export const options=()=>{const result={};for(let i=2;i<process.argv.length;i+=2){if(!process.argv[i].startsWith('--')||!process.argv[i+1])throw Error('Options require values');result[process.argv[i].slice(2)]=process.argv[i+1];}return result;};
export function tag(value){const match=/^sodeck-v2\/(stable|preview)\/([0-9]{6})$/.exec(value??'');if(!match||Number(match[2])<1)throw Error('A dedicated SoDeck v2 release tag is required');return {channel:match[1],sequence:Number(match[2])};}
export function verifyBinary(){return join(root,'target','debug',process.platform==='win32'?'sodeck-release-verify.exe':'sodeck-release-verify');}
export const verify=args=>JSON.parse(run(verifyBinary(),args,{capture:true}));
export function publicConfig(config){if(config.schemaVersion!==1||!['stable','preview'].includes(config.channel)||typeof config.keyId!=='string'||typeof config.catalogPublicKey!=='string'||Buffer.from(config.catalogPublicKey,'base64').length!==32||!config.updaterPublicKey)throw Error('Configure the v2 public keys before preparing a signed release');if(config.qa?!config.identifier.startsWith('com.sodeck.v2.qa.'):config.identifier!==(config.channel==='stable'?'com.sodeck.v2':'com.sodeck.v2.preview'))throw Error('Invalid isolated channel identity');return config;}
