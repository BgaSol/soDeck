use base64::Engine;
use sodeck_updater::{Catalog,Channel,VerifiedCatalog};
use std::{collections::BTreeMap,fs};
fn run()->Result<(),Box<dyn std::error::Error>> {
    let a=std::env::args().skip(1).collect::<Vec<_>>();
    match a.first().map(String::as_str) {
        Some("version") if a.len()==3 => {
            let mut old=semver::Version::parse(&a[1])?;let mut next=semver::Version::parse(&a[2])?;
            old.build=semver::BuildMetadata::EMPTY;next.build=semver::BuildMetadata::EMPTY;
            println!("{}",serde_json::json!({"ordering":if next>old{"higher"}else if next==old{"equal"}else{"lower"}}));
        },
        Some("catalog") if a.len()==3||a.len()==5 => {
            let channel=match a[1].as_str(){"stable"=>Channel::Stable,"preview"=>Channel::Preview,_=>return Err("Invalid channel".into())};
            let bytes=fs::read(&a[2])?;
            if a.len()==5 {
                let raw:BTreeMap<String,String>=serde_json::from_slice(&fs::read(&a[4])?)?;
                let keys=raw.into_iter().map(|(id,key)|Ok((id,base64::engine::general_purpose::STANDARD.decode(key)?))).collect::<Result<BTreeMap<_,_>,base64::DecodeError>>()?;
                let catalog=VerifiedCatalog::verify(&bytes,&fs::read(&a[3])?,&keys,channel)?;
                println!("{}",serde_json::json!({"valid":true,"digest":catalog.digest(),"sequence":catalog.catalog().sequence}));
            }else{let catalog:Catalog=sodeck_updater::strict::parse(&bytes,sodeck_updater::MAX_CATALOG)?;catalog.validate(channel)?;println!("{}",serde_json::json!({"valid":true,"sequence":catalog.sequence}));}
        },
        _=>return Err("Usage: sodeck-release-verify version OLD NEW | catalog CHANNEL JSON [SIG KEYS]".into()),
    }Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
