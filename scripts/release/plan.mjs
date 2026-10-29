import {readFile} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {root,json,save,node,git,options,tag,publicConfig,verify} from './common.mjs';
import {fingerprint} from './inputs.mjs';
export function decide(version,fingerprint,previous,compare){
  if(!previous)return 'build';
  const ordering=compare(previous.version??previous.appVersion,version);
  if(ordering==='lower')throw Error('Component versions cannot decrease');
  if(ordering==='equal'){
    if(version!==(previous.version??previous.appVersion))throw Error('Build metadata alone cannot replace a released version');
    if(fingerprint!==previous.inputFingerprint)throw Error('Component inputs changed without a version increase');
    return 'reuse';
  }
  return 'build';
}
export async function plan(args){
  const releaseTag=args.tag,{channel,sequence}=tag(releaseTag),config=publicConfig(await json(resolve(args.config??join(root,`release/channels/${channel}.json`))));
  if(config.channel!==channel)throw Error('Release/config channel mismatch');
  const output=resolve(args.out??join(root,'.qa/t029/release',channel,String(sequence)));
  node(['scripts/cargo.mjs','build','--locked','-p','sodeck-updater','--bin','sodeck-release-verify','-p','sodeck-plugin-manager','--bin','sodeck-package-inspect']);
  const keys=join(output,'catalog-keys.json');await save(keys,{[config.keyId]:config.catalogPublicKey});
  let previous;
  if(args.previous){if(!args.signature)throw Error('Previous catalog requires its exact signature');verify(['catalog',channel,resolve(args.previous),resolve(args.signature),keys]);previous=await json(resolve(args.previous));if(previous.sequence>=sequence)throw Error('Release sequence must advance');}
  const components=await json(join(root,'release/components.json')),cargo=await readFile(join(root,'apps/desktop/src-tauri/Cargo.toml'),'utf8');
  const coreVersion=/^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  for(const p of ['package.json','apps/desktop/package.json','apps/desktop/src-tauri/tauri.conf.json'])if((await json(join(root,p))).version!==coreVersion)throw Error('Core version mismatch: '+p);
  const sourceCommit=git(['rev-parse','HEAD']);
  if(!config.qa){if(git(['status','--porcelain']))throw Error('Public releases require a clean committed checkout');git(['merge-base','--is-ancestor',sourceCommit,'origin/main']);}
  const entries=[];
  for(const component of components.components){
    const evidence=await fingerprint(component,config),old=component.kind==='core'?previous?.core.current:previous?.plugins[component.id]?.current;
    if(channel==='stable'&&evidence.version.includes('-'))throw Error('Stable channel cannot publish prerelease versions');
    const action=decide(evidence.version,evidence.inputFingerprint,old,(a,b)=>verify(['version',a,b]).ordering);
    const entry={...component,version:evidence.version,inputFingerprint:evidence.inputFingerprint,action,previous:old??null};entries.push(entry);await save(join(output,'inputs',component.id+'.json'),evidence);
  }
  if(entries.every(e=>e.action==='reuse'))throw Error('No component changed; refusing an empty release');
  const result={schemaVersion:1,releaseTag,channel,sequence,sourceCommit,config,components:entries,previous:previous??null};
  await save(join(output,'plan.json'),result);return {output,plan:result};
}
if(import.meta.url===pathToFileURL(process.argv[1]).href)console.log(JSON.stringify(await plan(options())));
