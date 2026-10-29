import {join,resolve} from 'node:path';
import {writeFile,mkdir} from 'node:fs/promises';
import {options,tag,json,save,publicConfig,node,root} from './common.mjs';
import {releases,latest,catalog,assetBytes} from './github.mjs';
import {plan} from './plan.mjs';
const args=options(),channel=tag(args.tag).channel,out=resolve(args.out),config=publicConfig(await json(args.config??join(root,`release/channels/${channel}.json`)));
node(['scripts/cargo.mjs','build','--locked','-p','sodeck-updater','--bin','sodeck-release-verify']);
await save(join(out,'catalog-keys.json'),{[config.keyId]:config.catalogPublicKey});
const prior=latest(await releases(),channel);
let previous;
if(prior){const paths=await catalog(prior,join(out,'previous'),join(out,'catalog-keys.json'),channel);previous=await json(paths.body);args.previous=paths.body;args.signature=paths.sig;}
const result=await plan({...args,out});
if(result.plan.components.some(c=>c.id==='core'&&c.action==='build')){
  await mkdir(join(out,'reused'),{recursive:true});
  for(const c of result.plan.components.filter(c=>c.kind==='plugin'&&c.action==='reuse'))await writeFile(join(out,'reused',c.previous.asset.name),await assetBytes(c.previous.asset));
}
console.log(JSON.stringify({output:out,previousSequence:previous?.sequence??0,components:result.plan.components.map(c=>({id:c.id,action:c.action}))}));
