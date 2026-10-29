use crate::{error,hash,strict,model::*,Result};
use base64::{engine::general_purpose::STANDARD,Engine};
use serde::{Deserialize,Serialize};
use std::{collections::{BTreeMap,BTreeSet},sync::Arc};

pub const DOMAIN:&[u8]=b"sodeck-v2-catalog-v1\0";
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Catalog {
    pub schema_version:u32,
    pub product:String,
    pub repository:String,
    pub channel:Channel,
    pub sequence:u64,
    pub release_tag:String,
    pub source_commit:String,
    pub core:Versions<CoreRelease>,
    pub plugins:BTreeMap<String,Versions<PluginRelease>>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CatalogSignature {pub schema_version:u32,pub key_id:String,pub signature:String}
#[derive(Clone,Debug)]
pub struct VerifiedCatalog {catalog:Arc<Catalog>,bytes:Arc<[u8]>,signature:Arc<[u8]>,digest:String}
impl VerifiedCatalog {
    pub fn verify(bytes:&[u8],signature:&[u8],keys:&BTreeMap<String,Vec<u8>>,channel:Channel)->Result<Self>{
        if bytes.len()>MAX_CATALOG{return Err(error("CAPACITY","发布目录超出容量"));}
        let sig:CatalogSignature=strict::parse(signature,16*1024)?;
        let key=keys.get(&sig.key_id).filter(|k|k.len()==32).ok_or_else(||error("UPDATE_SIGNATURE_INVALID","发布目录签名来源未知"))?;
        if sig.schema_version!=1{return Err(error("UPDATE_SIGNATURE_INVALID","发布目录签名格式不支持"));}
        let raw=STANDARD.decode(&sig.signature).map_err(|_|error("UPDATE_SIGNATURE_INVALID","发布目录签名无效"))?;
        let message=[DOMAIN,bytes].concat();
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519,key).verify(&message,&raw).map_err(|_|error("UPDATE_SIGNATURE_INVALID","发布目录签名验证失败"))?;
        let catalog:Catalog=strict::parse(bytes,MAX_CATALOG)?;catalog.validate(channel)?;
        Ok(Self{catalog:Arc::new(catalog),bytes:Arc::from(bytes),signature:Arc::from(signature),digest:hash(bytes)})
    }
    pub fn catalog(&self)->&Catalog{&self.catalog}
    pub fn bytes(&self)->&[u8]{&self.bytes}
    pub fn signature(&self)->&[u8]{&self.signature}
    pub fn digest(&self)->&str{&self.digest}
}
pub fn sha(value:&str)->bool{value.len()==64&&value.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c))}
pub fn id(value:&str)->bool{!value.is_empty()&&value.len()<=128&&!value.starts_with('.')&&!value.ends_with('.')&&value.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||b".-".contains(&c))}
fn version(value:&str,channel:Channel)->bool{semver::Version::parse(value).is_ok_and(|v|channel!=Channel::Stable||v.pre.is_empty())}
fn unique(items:&[String],max:usize)->bool{items.len()<=max&&items.iter().all(|s|!s.is_empty()&&s.len()<=128)&&items.iter().collect::<BTreeSet<_>>().len()==items.len()}
fn ordered_versions<'a>(versions:impl Iterator<Item=&'a str>)->bool {
    let mut previous=None;for value in versions {let Ok(mut parsed)=semver::Version::parse(value)else{return false;};parsed.build=semver::BuildMetadata::EMPTY;if previous.as_ref().is_some_and(|old|old<=&parsed){return false;}previous=Some(parsed);}true
}
impl Asset {
    pub fn validate(&self,channel:Channel,max:u64)->Result<()> {
        let prefix=format!("sodeck-v2/{}/",channel.name());let seq=self.release_tag.strip_prefix(&prefix).unwrap_or("");
        if self.repository!=REPOSITORY||seq.len()!=6||!seq.bytes().all(|b|b.is_ascii_digit())||seq=="000000"
            ||self.asset_id==0||self.asset_id>9_007_199_254_740_991||self.size==0||self.size>max||!sha(&self.archive_sha256)
            ||self.name.len()>180||self.name.is_empty()||self.name.starts_with('.')||self.name.bytes().any(|b|b<32||b"/\\:?*\"<>|".contains(&b)) {
            return Err(error("UPDATE_SOURCE_INVALID","发布资产身份、路径或大小无效"));
        } Ok(())
    }
}
impl Catalog {
    pub fn validate(&self,channel:Channel)->Result<()> {
        let bad=||error("UPDATE_SOURCE_INVALID","发布目录身份、组件或版本无效");
        if self.schema_version!=1||self.product!="sodeck-v2"||self.repository!=REPOSITORY||self.channel!=channel||self.sequence==0||self.sequence>999999
            ||self.release_tag!=format!("sodeck-v2/{}/{:06}",channel.name(),self.sequence)||self.source_commit.len()!=40||!self.source_commit.bytes().all(|c|c.is_ascii_hexdigit())
            ||self.plugins.len()>1024||self.core.history.len()>7{return Err(bad());}
        let mut asset_ids=BTreeMap::new();
        let mut accept_asset=|a:&Asset,max:u64|->Result<()> {a.validate(channel,max)?;if a.release_tag.rsplit('/').next().and_then(|s|s.parse::<u64>().ok()).is_none_or(|n|n>self.sequence){return Err(bad());}if let Some(old)=asset_ids.insert(a.asset_id,a.clone()){if old!=*a{return Err(bad());}}Ok(())};
        if !ordered_versions(std::iter::once(self.core.current.app_version.as_str()).chain(self.core.history.iter().map(|c|c.app_version.as_str()))){return Err(bad());}
        let mut versions=BTreeSet::new();
        for c in std::iter::once(&self.core.current).chain(&self.core.history){
            if !version(&c.app_version,channel)||!version(&c.core_compatibility_version,Channel::Preview)||!version(&c.plugin_api_version,Channel::Preview)||!versions.insert(&c.app_version)
                ||!sha(&c.input_fingerprint)||!sha(&c.build_id)||!unique(&c.capabilities,512)||c.platforms.is_empty()||c.platforms.len()>8||c.data_formats.is_empty()||c.data_formats.len()>32||c.data_formats.values().any(|v|*v==0)
                ||c.bundled_plugins.len()>1024||c.bundled_plugins.iter().any(|(k,v)|!id(k)||!version(v,channel)){return Err(bad());}
            for (platform,installer) in &c.platforms {
                if platform.is_empty()||platform.len()>64||!platform.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b"-_".contains(&b))||installer.signed_version!=c.app_version||installer.signature.is_empty()||installer.signature.len()>16*1024||STANDARD.decode(&installer.signature).is_err(){return Err(bad());}
                accept_asset(&installer.asset,MAX_INSTALLER)?;
            }
        }
        for (plugin,items) in &self.plugins {
            if !ordered_versions(std::iter::once(items.current.version.as_str()).chain(items.history.iter().map(|p|p.version.as_str()))){return Err(bad());}
            if !id(plugin)||items.history.len()>7{return Err(bad());}let mut versions=BTreeSet::new();
            for p in std::iter::once(&items.current).chain(&items.history){
                if !version(&p.version,channel)||!versions.insert(&p.version)||!sha(&p.package_digest)||!sha(&p.input_fingerprint)||!id(&p.signing_key_id)||p.data_schema_version==0
                    ||semver::VersionReq::parse(&p.requires.core).is_err()||semver::VersionReq::parse(&p.requires.plugin_api).is_err()||!unique(&p.requires.capabilities,512)||!unique(&p.requires.platforms,8){return Err(bad());}
                accept_asset(&p.asset,MAX_PLUGIN)?;
            }
        }
        Ok(())
    }
}
