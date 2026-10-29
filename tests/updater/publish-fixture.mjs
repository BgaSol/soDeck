// Executes the real publisher with an in-process, fail-closed GitHub simulation.
// No request reaches the network and no public config/key is changed.
import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile,cp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
const base=resolve('.qa/t029/release-local'),out=join(base,'publish-simulation-'+Date.now());await mkdir(out,{recursive:true});
const plan=JSON.parse(await readFile(join(base,'B/plan.json'),'utf8'));
plan.config={...plan.config,qa:false,identifier:'com.sodeck.v2.preview'};
assert.equal(plan.previous,null);assert.ok(plan.components.every(c=>c.action==='build'));
await writeFile(join(out,'plan.json'),JSON.stringify(plan));
for(const name of ['built.json','catalog-keys.json'])await cp(join(base,'B',name),join(out,name));
await cp(join(base,'B/assets'),join(out,'assets'),{recursive:true});
process.argv=[process.execPath,'publisher-test','--plan',join(out,'plan.json')];
Object.assign(process.env,{GITHUB_EVENT_NAME:'push',GITHUB_REF:'refs/tags/'+plan.releaseTag,GITHUB_REPOSITORY:'BgaSol/soDeck',GITHUB_TOKEN:'simulation-only',SODECK_V2_RELEASE_KEY_FILE:join(base,'catalog-private.pem')});
let draft,failOnce=true,posts=0,publications=0,reusedReads=0;const bytesById=new Map(),events=[];
const json=value=>new Response(JSON.stringify(value),{headers:{'Content-Type':'application/json'}});
globalThis.fetch=async(input,options={})=>{
  const url=new URL(input),method=options.method??'GET';events.push({method,path:url.pathname});
  if(url.origin==='https://api.github.com'&&url.pathname==='/repos/BgaSol/soDeck/releases'&&method==='GET')return json(draft&&(!draft.draft||options.headers.Authorization)?[draft]:[]);
  if(url.origin==='https://api.github.com'&&url.pathname==='/repos/BgaSol/soDeck/releases'&&method==='POST'){
    assert.equal(draft,undefined);const body=JSON.parse(options.body);assert.equal(body.draft,true);assert.equal(body.make_latest,'false');draft={...body,id:7,assets:[],html_url:'https://github.com/BgaSol/soDeck/releases/tag/'+plan.releaseTag};return json(draft);
  }
  if(url.origin==='https://uploads.github.com'&&url.pathname==='/repos/BgaSol/soDeck/releases/7/assets'&&method==='POST'){
    assert.equal(draft.draft,true);if(failOnce&&draft.assets.length===1){failOnce=false;throw Error('simulated upload interruption');}
    const name=url.searchParams.get('name');assert.ok(!draft.assets.some(a=>a.name===name));const bytes=Buffer.from(options.body),asset={id:100+posts++,name,size:bytes.length,browser_download_url:`https://github.com/BgaSol/soDeck/releases/download/${encodeURIComponent(plan.releaseTag)}/${encodeURIComponent(name)}`};draft.assets.push(asset);bytesById.set(asset.id,bytes);return json(asset);
  }
  if(url.origin==='https://api.github.com'&&url.pathname.startsWith('/repos/BgaSol/soDeck/releases/assets/')&&method==='GET'){
    const id=Number(url.pathname.split('/').at(-1));assert.ok(bytesById.has(id));assert.ok(options.headers.Authorization);reusedReads++;return new Response(bytesById.get(id));
  }
  if(url.origin==='https://api.github.com'&&url.pathname==='/repos/BgaSol/soDeck/releases/7'&&method==='PATCH'){
    assert.equal(draft.draft,true);const body=JSON.parse(options.body);assert.equal(body.draft,false);assert.equal(body.make_latest,'false');for(const name of ['catalog-v1.json','catalog-v1.sig','SHA256SUMS'])assert.ok(draft.assets.some(a=>a.name===name));draft={...draft,...body};publications++;return json(draft);
  }
  if(url.origin==='https://github.com'&&method==='GET'){
    const asset=draft.assets.find(a=>a.browser_download_url===url.href);assert.ok(asset,'Unknown simulated asset');return new Response(bytesById.get(asset.id));
  }
  throw Error('Unexpected simulated request; network is disabled: '+method+' '+url.origin+url.pathname);
};
const publisher=pathToFileURL(resolve('scripts/release/publish.mjs')).href;
await assert.rejects(import(publisher+'?first'),/simulated upload interruption/);assert.equal(draft.draft,true);assert.equal(publications,0);
const firstId=draft.assets[0].id;await import(publisher+'?resume');assert.equal(draft.draft,false);assert.equal(publications,1);assert.equal(draft.assets[0].id,firstId);assert.ok(reusedReads>0);
await assert.rejects(import(publisher+'?immutable'),/re-plan|immutable/);assert.equal(publications,1);
const catalogAsset=draft.assets.find(a=>a.name==='catalog-v1.json'),body=bytesById.get(catalogAsset.id),catalog=JSON.parse(body);
for(const asset of [catalog.core.current.platforms['windows-x86_64'].asset,...Object.values(catalog.plugins).map(p=>p.current.asset)])assert.equal(createHash('sha256').update(bytesById.get(asset.assetId)).digest('hex'),asset.archiveSha256);
const result={passed:true,kind:'in-process-github-simulation',network:false,out,publications,assetCount:draft.assets.length,reusedReads,checks:['upload failure retained an unpublished draft','resume reused exact asset bytes and IDs','real Rust verification accepted complete signed catalog','one final publication with make_latest=false','published batch cannot be overwritten'],events};
await writeFile(join(out,'result.json'),JSON.stringify(result,null,2));await writeFile('.qa/t029/publish-simulation.json',JSON.stringify(result,null,2));console.log(JSON.stringify({passed:true,out,publications}));
