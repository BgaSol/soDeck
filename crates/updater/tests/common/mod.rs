use base64::{engine::general_purpose::STANDARD,Engine};
use ring::signature::{Ed25519KeyPair,KeyPair};
use sodeck_updater::{catalog::*,model::*};
use std::collections::BTreeMap;
pub fn fixture()->(Catalog,InstalledSnapshot){
    let formats=BTreeMap::from([("core".into(),7),("observabilityFacts".into(),2),("observabilityLogs".into(),2),("observabilityTraffic".into(),2),("observabilityDiagnostics".into(),2),("activation".into(),1),("enablement".into(),1),("updates".into(),1)]);
    let requirements=Requirements{core:">=2.0.0, <3.0.0".into(),plugin_api:">=6.2.0, <7.0.0".into(),capabilities:vec!["remote.pty.v1".into()],platforms:vec!["windows".into()]};
    let asset=|n:u64|Asset{repository:REPOSITORY.into(),release_tag:"sodeck-v2/stable/000001".into(),asset_id:n,name:format!("asset-{n}.pkg"),size:100,archive_sha256:format!("{n:064x}")};
    let core=CoreRelease{app_version:"2.0.1".into(),build_id:"a".repeat(64),core_compatibility_version:"2.0.0".into(),plugin_api_version:"6.3.0".into(),capabilities:vec!["remote.pty.v1".into()],data_formats:formats.clone(),input_fingerprint:"b".repeat(64),platforms:BTreeMap::from([("windows-x86_64".into(),CoreInstaller{asset:asset(1),signature:STANDARD.encode("test-signature"),signed_version:"2.0.1".into()})]),bundled_plugins:BTreeMap::new()};
    let mut plugins=BTreeMap::new();let mut installed=vec![];
    for (i,id) in ["sodeck.terminal","sodeck.files","sodeck.network-proxy","sodeck.monitor"].into_iter().enumerate(){
        plugins.insert(id.into(),Versions{current:PluginRelease{version:"1.1.0".into(),requires:requirements.clone(),data_schema_version:1,input_fingerprint:"c".repeat(64),asset:asset(i as u64+2),package_digest:format!("{:064x}",i+100),signing_key_id:"test".into()},history:vec![]});
        installed.push(InstalledPlugin{plugin_id:id.into(),selected:Some(PackageSelection{version:"1.0.0".into(),digest:format!("{:064x}",i+200),requires:requirements.clone(),data_schema_version:1}),pending:None,enabled:true,origin:Origin::Official,follow_official:true,failure:None});
    }
    let catalog=Catalog{schema_version:1,product:"sodeck-v2".into(),repository:REPOSITORY.into(),channel:Channel::Stable,sequence:1,release_tag:"sodeck-v2/stable/000001".into(),source_commit:"a".repeat(40),core:Versions{current:core,history:vec![]},plugins};
    let snapshot=InstalledSnapshot{runtime:RuntimeIdentity{app_version:"2.0.0".into(),build_id:"e".repeat(64),channel:Channel::Stable,compatibility:Compatibility{core_compatibility_version:"2.0.0".into(),plugin_api_version:"6.2.0".into(),capabilities:vec!["remote.pty.v1".into()],platform:"windows-x86_64".into()},data_formats:formats,distribution:"installed".into()},selection_revision:"selection".into(),enablement_revision:"enablement".into(),source_revision:"sources".into(),plugins:installed};
    (catalog,snapshot)
}
pub fn signed(c:&Catalog)->(Vec<u8>,Vec<u8>,BTreeMap<String,Vec<u8>>){
    let pair=Ed25519KeyPair::from_seed_unchecked(&[7;32]).unwrap();let bytes=serde_json::to_vec(c).unwrap();let signature=pair.sign(&[DOMAIN,bytes.as_slice()].concat());
    let sig=serde_json::to_vec(&CatalogSignature{schema_version:1,key_id:"test".into(),signature:STANDARD.encode(signature.as_ref())}).unwrap();
    (bytes,sig,BTreeMap::from([("test".into(),pair.public_key().as_ref().to_vec())]))
}
pub fn verified(c:&Catalog)->VerifiedCatalog{let (b,s,k)=signed(c);VerifiedCatalog::verify(&b,&s,&k,c.channel).unwrap()}
