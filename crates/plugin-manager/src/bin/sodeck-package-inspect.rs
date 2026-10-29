use base64::Engine;
use sodeck_plugin_manager::{CompatibilityContext,InspectedPackage,TrustedKeys};
use std::{collections::BTreeMap,fs,path::PathBuf};

fn run()->Result<(),Box<dyn std::error::Error>> {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args.len()!=3{return Err("Usage: sodeck-package-inspect ARCHIVE KEYS_JSON SCRATCH".into());}
    let keys:BTreeMap<String,String>=serde_json::from_slice(&fs::read(&args[1])?)?;
    let keys:TrustedKeys=keys.into_iter().map(|(id,key)|Ok((id,base64::engine::general_purpose::STANDARD.decode(key)?.into()))).collect::<Result<_,base64::DecodeError>>()?;
    let package=InspectedPackage::inspect_archive(&PathBuf::from(&args[0]),&PathBuf::from(&args[2]),&keys,&CompatibilityContext::current())?;
    println!("{}",serde_json::json!({"manifest":package.manifest(),"signature":package.signature(),"packageDigest":package.digest()}));
    Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
