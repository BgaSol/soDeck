import {mkdir,writeFile} from 'node:fs/promises';
import {dirname} from 'node:path';
import {hash,tag,verify,save} from './common.mjs';
export const repository='BgaSol/soDeck';
const permitted=url=>url.protocol==='https:'&&!url.username&&!url.password&&(!url.port||url.port==='443')&&['api.github.com','uploads.github.com','github.com','release-assets.githubusercontent.com','objects.githubusercontent.com'].includes(url.hostname);
export async function request(url,{method='GET',body,auth=false,limit=8*1024*1024,headers={}}={}){
  let current=new URL(url);
  for(let redirect=0;redirect<=5;redirect++){
    if(!permitted(current))throw Error('Release URL is outside the fixed HTTPS hosts');
    const response=await fetch(current,{method,body,redirect:'manual',signal:AbortSignal.timeout(60_000),headers:{'User-Agent':'SoDeck-v2-Release','X-GitHub-Api-Version':'2022-11-28',...(auth&&['api.github.com','uploads.github.com'].includes(current.hostname)?{Authorization:`Bearer ${process.env.GITHUB_TOKEN}`} : {}),...headers}});
    if([301,302,303,307,308].includes(response.status)){if(method!=='GET')throw Error('Write redirect refused');current=new URL(response.headers.get('location'),current);continue;}
    if(!response.ok)throw Error(`GitHub ${method} failed (${response.status})`);
    const chunks=[];let size=0;for await(const chunk of response.body){size+=chunk.length;if(size>limit)throw Error('Release response exceeds capacity');chunks.push(chunk);}return Buffer.concat(chunks);
  }throw Error('Too many release redirects');
}
export const api=async(path,options={})=>JSON.parse((await request(`https://api.github.com/repos/${repository}${path}`,{...options,headers:{Accept:'application/vnd.github+json',...(options.body?{'Content-Type':'application/json'}:{}),...options.headers},body:options.body===undefined?undefined:JSON.stringify(options.body)})).toString());
export async function releases({auth=false}={}){const all=[];for(let page=1;page<=10;page++){const rows=await api(`/releases?per_page=100&page=${page}`,{auth});if(!Array.isArray(rows)||rows.length>100)throw Error('Invalid releases page');all.push(...rows);if(rows.length<100)return all;}throw Error('Release discovery budget exhausted');}
export function latest(rows,channel){return rows.filter(r=>!r.draft&&r.prerelease===(channel==='preview')).map(r=>{try{const t=tag(r.tag_name);return t.channel===channel?{...r,sequence:t.sequence}:null;}catch{return null;}}).filter(Boolean).sort((a,b)=>b.sequence-a.sequence)[0];}
export function assetUrl(asset){if(asset.repository!==repository)throw Error('Foreign asset repository');return `https://github.com/${repository}/releases/download/${encodeURIComponent(asset.releaseTag)}/${encodeURIComponent(asset.name)}`;}
export async function assetBytes(asset){const bytes=await request(assetUrl(asset),{limit:asset.size});if(bytes.length!==asset.size||hash(bytes)!==asset.archiveSha256)throw Error('Release asset differs from signed metadata');return bytes;}
export function sameAssetUrl(actual,expected){try{const a=new URL(actual),e=new URL(expected);return permitted(a)&&a.origin===e.origin&&!a.search&&!a.hash&&decodeURIComponent(a.pathname)===decodeURIComponent(e.pathname);}catch{return false;}}
export async function catalog(release,out,keys,channel){
  const download=async(name,max)=>{const matches=release.assets.filter(a=>a.name===name);if(matches.length!==1||matches[0].size>max)throw Error('Newest release is missing a complete catalog');const asset=matches[0];const expected=assetUrl({repository,releaseTag:release.tag_name,name});if(!sameAssetUrl(asset.browser_download_url,expected))throw Error('Invalid catalog URL');const bytes=await request(expected,{limit:max});if(bytes.length!==asset.size)throw Error('Catalog asset size mismatch');const path=out+'/'+name;await mkdir(dirname(path),{recursive:true});await writeFile(path,bytes);return path;};
  const body=await download('catalog-v1.json',1024*1024),sig=await download('catalog-v1.sig',16*1024);const result=verify(['catalog',channel,body,sig,keys]);if(result.sequence!==release.sequence)throw Error('Catalog sequence mismatch');return {body,sig};
}
