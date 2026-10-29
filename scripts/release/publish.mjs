import {readFile,writeFile} from 'node:fs/promises';
import {resolve,dirname,join} from 'node:path';
import {createPrivateKey,createPublicKey,sign} from 'node:crypto';
import {options,json,save,verify,hash,publicConfig} from './common.mjs';
import {api,request,releases,latest,repository,assetBytes,catalog as getCatalog} from './github.mjs';
const args=options(),planPath=resolve(args.plan),out=dirname(planPath),plan=await json(planPath),built=await json(join(out,'built.json')),config=publicConfig(plan.config);
if(config.qa)throw Error('QA artifacts cannot be publicly published by this command');
if(process.env.GITHUB_EVENT_NAME!=='push'||process.env.GITHUB_REF!==`refs/tags/${plan.releaseTag}`||process.env.GITHUB_REPOSITORY!==repository||!process.env.GITHUB_TOKEN)throw Error('Publication requires this repository dedicated trusted tag workflow');
if(built.sourceCommit!==plan.sourceCommit||built.releaseTag!==plan.releaseTag)throw Error('Build manifest does not belong to this release');
const key=createPrivateKey(await readFile(process.env.SODECK_V2_RELEASE_KEY_FILE)),raw=Buffer.from(createPublicKey(key).export({format:'jwk'}).x,'base64url').toString('base64');if(raw!==config.catalogPublicKey)throw Error('Catalog signing key mismatch');
async function previousUnchanged(){const r=latest(await releases(),plan.channel);if((r?.sequence??0)!==(plan.previous?.sequence??0))throw Error('Another release was published; re-plan against its signed catalog');if(r){const paths=await getCatalog(r,join(out,'recheck'),join(out,'catalog-keys.json'),plan.channel);const c=await json(paths.body);if(JSON.stringify(c)!==JSON.stringify(plan.previous))throw Error('Previous catalog changed');}}
await previousUnchanged();
const existing=(await releases({auth:true})).find(r=>r.tag_name===plan.releaseTag);
if(existing&&!existing.draft)throw Error('Published releases are immutable');
let draft=existing;
if(draft&&draft.target_commitish!==plan.sourceCommit)throw Error('Existing draft belongs to a different commit');
draft??=await api('/releases',{method:'POST',auth:true,body:{tag_name:plan.releaseTag,target_commitish:plan.sourceCommit,name:`SoDeck v2 ${plan.channel} · ${String(plan.sequence).padStart(6,'0')}`,draft:true,prerelease:plan.channel==='preview',make_latest:'false'}});
async function upload(name,bytes){
  let old=draft.assets.find(a=>a.name===name);
  if(old){const actual=await request(`https://api.github.com/repos/${repository}/releases/assets/${old.id}`,{auth:true,limit:256*1024*1024,headers:{Accept:'application/octet-stream'}});if(actual.length===bytes.length&&hash(actual)===hash(bytes))return old;throw Error('Existing draft asset differs; review this draft before replacing it');}
  return JSON.parse((await request(`https://uploads.github.com/repos/${repository}/releases/${draft.id}/assets?name=${encodeURIComponent(name)}`,{method:'POST',body:bytes,auth:true,headers:{'Content-Type':'application/octet-stream'},limit:1024*1024})).toString());
}
const records=new Map();
for(const product of built.products){const bytes=await readFile(join(out,'assets',product.name));if(bytes.length!==product.size||hash(bytes)!==product.archiveSha256)throw Error('Build artifact hash mismatch');const a=await upload(product.name,bytes);if(a.size!==bytes.length||!Number.isSafeInteger(a.id))throw Error('Uploaded asset identity mismatch');records.set(product.componentId,{repository,releaseTag:plan.releaseTag,assetId:a.id,name:a.name,size:a.size,archiveSha256:product.archiveSha256});if(product.signature)await upload(product.name+'.sig',Buffer.from(product.signature+'\n'));}
for(const c of plan.components.filter(c=>c.action==='reuse')){const a=c.kind==='core'?c.previous.platforms['windows-x86_64'].asset:c.previous.asset;await assetBytes(a);}
const versions=(current,old)=>({current,history:old?[old.current,...old.history].filter(c=>(c.version??c.appVersion)!==(current.version??current.appVersion)).slice(0,7):[]});
const catalog={schemaVersion:1,product:'sodeck-v2',repository,channel:plan.channel,sequence:plan.sequence,releaseTag:plan.releaseTag,sourceCommit:plan.sourceCommit,core:null,plugins:{}};
for(const entry of plan.components){
  const product=built.products.find(p=>p.componentId===entry.id);let value=entry.previous;
  if(entry.kind==='core'){
    if(product){const {appVersion,buildId,coreCompatibilityVersion,pluginApiVersion,capabilities,dataFormats,bundledPlugins,inputFingerprint}=product;value={appVersion,buildId,coreCompatibilityVersion,pluginApiVersion,capabilities,dataFormats,bundledPlugins,inputFingerprint,platforms:{'windows-x86_64':{asset:records.get('core'),signature:product.signature,signedVersion:product.signedVersion}}};}
    catalog.core=entry.action==='reuse'?plan.previous.core:versions(value,plan.previous?.core);
  }else{
    if(product)value={version:product.manifest.version,requires:product.manifest.requires,dataSchemaVersion:product.manifest.dataSchemaVersion,inputFingerprint:product.inputFingerprint,asset:records.get(entry.id),packageDigest:product.packageDigest,signingKeyId:config.keyId};
    catalog.plugins[entry.id]=entry.action==='reuse'?plan.previous.plugins[entry.id]:versions(value,plan.previous?.plugins[entry.id]);
  }
}
const body=Buffer.from(JSON.stringify(catalog,null,2)+'\n');await writeFile(join(out,'catalog-v1.json'),body);verify(['catalog',plan.channel,join(out,'catalog-v1.json')]);
const signature={schemaVersion:1,keyId:config.keyId,signature:sign(null,Buffer.concat([Buffer.from('sodeck-v2-catalog-v1\0'),body]),key).toString('base64')};await save(join(out,'catalog-v1.sig'),signature);verify(['catalog',plan.channel,join(out,'catalog-v1.json'),join(out,'catalog-v1.sig'),join(out,'catalog-keys.json')]);
await upload('catalog-v1.json',body);await upload('catalog-v1.sig',await readFile(join(out,'catalog-v1.sig')));
const sums=[...built.products.map(p=>`${p.archiveSha256}  ${p.name}`),`${hash(body)}  catalog-v1.json`,`${hash(await readFile(join(out,'catalog-v1.sig')))}  catalog-v1.sig`].join('\n')+'\n';await upload('SHA256SUMS',Buffer.from(sums));
await previousUnchanged();
const published=await api(`/releases/${draft.id}`,{method:'PATCH',auth:true,body:{draft:false,prerelease:plan.channel==='preview',make_latest:'false'}});
await save(join(out,'published.json'),{url:published.html_url,id:published.id,tag:plan.releaseTag,catalogSha256:hash(body)});console.log(published.html_url);
