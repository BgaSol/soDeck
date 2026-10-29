import {readFile,writeFile,mkdir,cp,readdir,copyFile} from 'node:fs/promises';
import {resolve,join,dirname,basename,delimiter} from 'node:path';
import {homedir} from 'node:os';
import {createPrivateKey,createPublicKey} from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {root,json,save,node,run,hash,options,publicConfig} from './common.mjs';
import {fingerprint} from './inputs.mjs';

export async function build(args){
  const planPath=resolve(args.plan),plan=await json(planPath),out=dirname(planPath),config=publicConfig(plan.config);
  async function assertInputs(){for(const entry of plan.components){if((await fingerprint(entry,config)).inputFingerprint!==entry.inputFingerprint)throw Error('Release inputs changed after planning: '+entry.id);}}
  await assertInputs();
  const pluginKey=process.env.SODECK_V2_RELEASE_KEY_FILE;
  if(!pluginKey)throw Error('Set SODECK_V2_RELEASE_KEY_FILE to the protected v2 signing key file');
  const privateKey=createPrivateKey(await readFile(pluginKey)),publicKey=Buffer.from(createPublicKey(privateKey).export({format:'jwk'}).x,'base64url').toString('base64');
  if(publicKey!==config.catalogPublicKey)throw Error('Signing key does not match this channel');
  const artifacts=join(out,'assets');await mkdir(artifacts,{recursive:true});
  const keysPath=join(out,'catalog-keys.json');await save(keysPath,{[config.keyId]:publicKey});
  const products=[];
  for(const entry of plan.components.filter(c=>c.kind==='plugin'&&c.action==='build')){
    const name=`${entry.id}-${entry.version}.sodeck-plugin`,path=join(artifacts,name);
    node(['scripts/pack-plugin.mjs','--plugin',basename(entry.path),'--signing','key','--key',pluginKey,'--key-id',config.keyId,'--out',path]);
    const inspection=JSON.parse(run(join(root,'target/debug/sodeck-package-inspect'+(process.platform==='win32'?'.exe':'')),[path,keysPath,join(out,'inspection')],{capture:true}));
    if(inspection.signature.status!=='verified'||inspection.signature.keyId!==config.keyId||inspection.manifest.pluginId!==entry.id||inspection.manifest.version!==entry.version)throw Error('Built plugin identity/signature mismatch');
    const bytes=await readFile(path);products.push({componentId:entry.id,name,size:bytes.length,archiveSha256:hash(bytes),manifest:inspection.manifest,packageDigest:inspection.packageDigest,inputFingerprint:entry.inputFingerprint});
  }
  const core=plan.components.find(c=>c.id==='core');
  if(core.action==='build'){
    if(process.platform!=='win32')throw Error('The initial installer target is Windows x64');
    if(!process.env.SODECK_V2_UPDATER_PRIVATE_KEY)throw Error('Missing v2 updater signing key');
    const resources=join(out,'bundle/resources');await mkdir(resources,{recursive:true});
    const bundled=[];
    for(const entry of plan.components.filter(c=>c.kind==='plugin')){
      let source,name,version;
      if(entry.action==='build'){const product=products.find(p=>p.componentId===entry.id);source=join(artifacts,product.name);name=product.name;version=entry.version;}
      else {
        const asset=entry.previous.asset;name=asset.name;version=entry.previous.version;source=join(out,'reused',name);
        // Reused assets must be supplied by the verified release discovery step.
        const bytes=await readFile(source);if(bytes.length!==asset.size||hash(bytes)!==asset.archiveSha256)throw Error('Reused plugin archive differs from signed catalog');
      }
      const inspection=JSON.parse(run(join(root,'target/debug/sodeck-package-inspect.exe'),[source,keysPath,join(out,'inspection')],{capture:true}));
      if(inspection.signature.status!=='verified')throw Error('Factory packages require official signatures');
      await copyFile(source,join(resources,name));bundled.push({pluginId:entry.id,path:name,version});
    }
    await save(join(resources,'public-keys.json'),{[config.keyId]:publicKey});
    await save(join(resources,'bundled-plugins.json'),{schemaVersion:1,publicKeys:'public-keys.json',plugins:bundled.map(({pluginId,path})=>({pluginId,path}))});
    const buildId=hash(JSON.stringify({appVersion:core.version,sourceCommit:plan.sourceCommit,inputFingerprint:core.inputFingerprint,channel:plan.channel}));
    const identity=join(out,'build-identity.json');await save(identity,{schemaVersion:1,appVersion:core.version,buildId,channel:plan.channel,identifier:config.identifier,productName:config.productName,catalogKeys:{[config.keyId]:publicKey},updaterKey:config.updaterPublicKey,qa:config.qa});
    for(const dir of ['themes','assets/brand'])await cp(join(root,dir),join(out,'bundle',dir),{recursive:true});
    for(const [dir,names] of [['ssh2',['LICENSE-MIT','LICENSE-APACHE','SODECK-PATCH.md']],['libssh2-sys',['LICENSE-MIT','LICENSE-APACHE','libssh2/COPYING']],['tauri-runtime-wry',['LICENSE-MIT','LICENSE-APACHE-2.0']]])for(const name of names){const dest=join(out,'bundle/licenses',dir,name);await mkdir(dirname(dest),{recursive:true});await copyFile(join(root,'third-party',dir,name),dest);}
    node(['scripts/cargo.mjs','build','--release','--locked','-p','sodeck-plugin-host','--bin','sodeck-plugin-host']);
    await copyFile(join(root,'target/release/sodeck-plugin-host.exe'),join(out,'bundle/sodeck-plugin-host.exe'));
    const resourceMap={[resources.replaceAll('\\','/')+'/*']:'resources/',[join(out,'bundle/themes').replaceAll('\\','/')+'/*']:'themes/',[join(out,'bundle/assets/brand').replaceAll('\\','/')+'/*']:'assets/brand/',[join(out,'bundle/sodeck-plugin-host.exe').replaceAll('\\','/')]:'sodeck-plugin-host.exe'};
    resourceMap[join(out,'bundle/licenses').replaceAll('\\','/')+'/']='licenses/';
    const overlay=join(out,'tauri.release.json');await save(overlay,{productName:config.productName,identifier:config.identifier,build:{beforeBuildCommand:'npm run build --workspace @sodeck/desktop'},bundle:{active:true,targets:['nsis'],createUpdaterArtifacts:false,resources:resourceMap,windows:{nsis:{installMode:'currentUser',displayLanguageSelector:false,languages:['SimpChinese','English']}}}});
    node([join(root,'node_modules/@tauri-apps/cli/tauri.js'),'build','--config',overlay],{cwd:join(root,'apps/desktop'),env:{SODECK_BUILD_IDENTITY:identity,SODECK_DEV_MODE:'0',PATH:[join(homedir(),'.cargo/bin'),process.env.PATH??''].join(delimiter),...(process.env.OPENSSL_SRC_PERL?{}:{OPENSSL_SRC_PERL:join(root,'.local/build-tools/strawberry/perl/bin/perl.exe')})}});
    const candidates=(await readdir(join(root,'target/release/bundle/nsis'))).filter(n=>n.endsWith('.exe'));
    const generated=candidates.find(n=>n.includes(core.version)&&n.startsWith(config.productName));if(!generated)throw Error('Missing expected NSIS artifact');
    const name=`SoDeck-v2-${plan.channel}-${core.version}-windows-x86_64-setup.exe`,path=join(artifacts,name);await copyFile(join(root,'target/release/bundle/nsis',generated),path);
    node([join(root,'node_modules/@tauri-apps/cli/tauri.js'),'signer','sign','--app-version',core.version,path],{env:{TAURI_SIGNING_PRIVATE_KEY:process.env.SODECK_V2_UPDATER_PRIVATE_KEY,TAURI_SIGNING_PRIVATE_KEY_PASSWORD:process.env.SODECK_V2_UPDATER_PASSWORD??''}});
    const bytes=await readFile(path),signature=(await readFile(path+'.sig','utf8')).trim(),compat=await json(join(root,'contracts/runtime-compatibility.json'));
    products.push({componentId:'core',name,size:bytes.length,archiveSha256:hash(bytes),signature,signedVersion:core.version,buildId,appVersion:core.version,coreCompatibilityVersion:compat.coreCompatibilityVersion,pluginApiVersion:compat.pluginApiVersion,capabilities:compat.capabilities,dataFormats:compat.dataFormats,bundledPlugins:Object.fromEntries(bundled.map(p=>[p.pluginId,p.version])),inputFingerprint:core.inputFingerprint});
  }
  await assertInputs();
  await save(join(out,'built.json'),{schemaVersion:1,sourceCommit:plan.sourceCommit,releaseTag:plan.releaseTag,products});return {output:out,products:products.map(p=>p.name)};
}
if(import.meta.url===pathToFileURL(process.argv[1]).href)console.log(JSON.stringify(await build(options())));
