use crate::{atomic,catalog::VerifiedCatalog,download,error,hash,model::*,now_ms,strict,Result};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{collections::BTreeMap,path::{Path,PathBuf},time::Duration};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone,Debug,Deserialize)]
pub struct ReleaseAsset {pub id:u64,pub name:String,pub size:u64,pub browser_download_url:String}
#[derive(Clone,Debug,Deserialize)]
pub struct Release {pub tag_name:String,pub draft:bool,pub prerelease:bool,pub assets:Vec<ReleaseAsset>}
pub struct Session {
    client:reqwest::Client,
    root:PathBuf,
    pub retry_after_ms:u64,
}
impl Session {
    pub fn new(root:&Path)->Result<Self>{Ok(Self{client:download::client()?,root:atomic::directory(&root.join("metadata"))?,retry_after_ms:0})}
    fn make_room(&self,additional:u64,keep:&Path)->Result<()> {
        let mut files=Vec::new();let mut size=0;
        for entry in std::fs::read_dir(&self.root).map_err(|_|error("STORAGE_ERROR","无法检查发布元数据缓存"))? {
            let entry=entry.map_err(|_|error("STORAGE_ERROR","无法读取发布元数据缓存"))?;let m=std::fs::symlink_metadata(entry.path()).map_err(|_|error("STORAGE_ERROR","无法读取发布元数据缓存"))?;
            if atomic::is_link(&m)||!m.is_file(){return Err(error("STORAGE_ERROR","发布元数据缓存类型异常"));}
            size+=m.len();if entry.path()!=keep{files.push((m.modified().ok(),entry.path(),m.len()));}
        }
        files.sort_by_key(|v|v.0);let mut count=files.len()+usize::from(keep.exists());
        for (_,path,len) in files {if size.saturating_add(additional)<=96*1024*1024&&count.saturating_add(2)<=128{break;}
            std::fs::remove_file(path).map_err(|_|error("STORAGE_ERROR","无法清理发布元数据缓存"))?;size=size.saturating_sub(len);count-=1;
        }
        if size.saturating_add(additional)>96*1024*1024{return Err(error("CAPACITY","发布元数据缓存超出容量"));}Ok(())
    }
    async fn metadata(&mut self,url:Url,limit:usize,cancel:&CancellationToken)->Result<Vec<u8>>{
        if !download::permitted(&url){return Err(error("UPDATE_SOURCE_INVALID","发布地址越界"));}
        let key=hash(url.as_str().as_bytes());let body=self.root.join(format!("{key}.body"));let tag=self.root.join(format!("{key}.etag"));
        let cached=atomic::read(&body,limit as u64).ok();let etag=atomic::read(&tag,1024).ok().and_then(|v|String::from_utf8(v).ok()).filter(|v|v.bytes().all(|c|(32..127).contains(&c)));
        let mut request=self.client.get(url).header("Accept","application/vnd.github+json").header("X-GitHub-Api-Version","2022-11-28");
        if cached.is_some(){if let Some(etag)=etag{request=request.header("If-None-Match",etag);}}
        let operation=async {
            let response=request.send().await.map_err(|_|error("UPDATE_NETWORK","无法连接GitHub，请检查网络"))?;
            if response.status().as_u16()==304{return cached.ok_or_else(||error("UPDATE_SOURCE_INVALID","GitHub缓存已失效，请重试检查"));}
            if matches!(response.status().as_u16(),403|429){
                self.retry_after_ms=response.headers().get("retry-after").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()).map(|s|now_ms().saturating_add(s.saturating_mul(1000)))
                    .or_else(||response.headers().get("x-ratelimit-reset").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()).map(|s|s.saturating_mul(1000))).unwrap_or_else(||now_ms()+60000).max(now_ms()+30000);
                return Err(error("RATE_LIMITED","GitHub暂时限制检查频率，请稍后再试"));
            }
            if !response.status().is_success(){return Err(error("UPDATE_NETWORK","GitHub发布暂不可用"));}
            if response.content_length().is_some_and(|n|n>limit as u64){return Err(error("CAPACITY","GitHub响应超过容量"));}
            let etag=response.headers().get("etag").and_then(|v|v.to_str().ok()).map(str::to_owned).filter(|v|v.len()<=1024);
            let mut stream=response.bytes_stream();let mut bytes=Vec::new();while let Some(chunk)=stream.next().await {let chunk=chunk.map_err(|_|error("UPDATE_NETWORK","GitHub响应中断"))?;if bytes.len()+chunk.len()>limit{return Err(error("CAPACITY","GitHub响应超过容量"));}bytes.extend_from_slice(&chunk);}
            // Body is committed first; an old ETag must never be paired with new bytes.
            self.make_room(bytes.len()as u64+1024,&body)?;
            let _=std::fs::remove_file(&tag);atomic::write(&body,&bytes)?;if let Some(etag)=etag{atomic::write(&tag,etag.as_bytes())?;}Ok(bytes)
        };
        tokio::select!{_=cancel.cancelled()=>Err(error("CANCELLED","检查已取消")),r=tokio::time::timeout(Duration::from_secs(15),operation)=>r.map_err(|_|error("UPDATE_TIMEOUT","检查GitHub超时")).and_then(|r|r)}
    }
    pub async fn discover(&mut self,channel:Channel,keys:&BTreeMap<String,Vec<u8>>,cancel:CancellationToken)->Result<VerifiedCatalog>{
        let operation=async {
            let mut latest:Option<(u64,Release)>=None;let mut exhausted=false;
            for page in 1..=10 {
                let url=format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100&page={page}").parse().unwrap();
                let bytes=self.metadata(url,8*1024*1024,&cancel).await?;let releases:Vec<Release>=strict::parse(&bytes,8*1024*1024)?;
                if releases.len()>100{return Err(error("UPDATE_SOURCE_INVALID","GitHub发布分页无效"));}let count=releases.len();
                for release in releases {if let Some(sequence)=sequence(&release,channel){if latest.as_ref().is_none_or(|(old,_)|sequence>*old){latest=Some((sequence,release));}}}
                if count<100{exhausted=true;break;}
            }
            if !exhausted{return Err(error("CAPACITY","发布记录超出检查预算，尚不能确认最新版"));}
            let (_,release)=latest.ok_or_else(||error("UPDATE_NOT_PUBLISHED","此渠道尚无可用的SoDeck v2发布"))?;
            let catalog_asset=unique_asset(&release,"catalog-v1.json",MAX_CATALOG as u64)?;
            let signature_asset=unique_asset(&release,"catalog-v1.sig",16*1024)?;
            let bytes=self.metadata(asset_url(&release,catalog_asset)?,MAX_CATALOG,&cancel).await?;
            let sig=self.metadata(asset_url(&release,signature_asset)?,16*1024,&cancel).await?;
            if bytes.len()as u64!=catalog_asset.size||sig.len()as u64!=signature_asset.size{return Err(error("UPDATE_SOURCE_INVALID","发布目录长度与GitHub资产不符"));}
            let verified=VerifiedCatalog::verify(&bytes,&sig,keys,channel)?;
            if verified.catalog().release_tag!=release.tag_name{return Err(error("UPDATE_SOURCE_INVALID","发布目录与标签不一致"));}
            let catalog=verified.catalog();
            for asset in catalog.core.current.platforms.values().map(|p|&p.asset).chain(catalog.plugins.values().map(|p|&p.current.asset)) {
                if asset.release_tag==release.tag_name&&!release.assets.iter().any(|a|a.id==asset.asset_id&&a.name==asset.name&&a.size==asset.size){return Err(error("UPDATE_SOURCE_INVALID","最新发布的组件资产尚不完整"));}
            }
            Ok(verified)
        };
        tokio::time::timeout(Duration::from_secs(60),operation).await.map_err(|_|error("UPDATE_TIMEOUT","检查更新超出时间预算")).and_then(|r|r)
    }
}
fn sequence(release:&Release,channel:Channel)->Option<u64>{
    if release.draft||release.prerelease!=(channel==Channel::Preview){return None;}
    let prefix=format!("sodeck-v2/{}/",channel.name());let tail=release.tag_name.strip_prefix(&prefix)?;
    if tail.len()!=6||!tail.bytes().all(|v|v.is_ascii_digit()){return None;}tail.parse::<u64>().ok().filter(|v|*v>0)
}
fn unique_asset<'a>(release:&'a Release,name:&str,max:u64)->Result<&'a ReleaseAsset>{let mut a=release.assets.iter().filter(|a|a.name==name);let first=a.next().filter(|a|a.id>0&&a.size>0&&a.size<=max).ok_or_else(||error("UPDATE_SOURCE_INVALID","最新发布缺少有效目录或签名"))?;if a.next().is_some(){return Err(error("UPDATE_SOURCE_INVALID","最新发布的目录或签名重复"));}Ok(first)}
fn asset_url(release:&Release,asset:&ReleaseAsset)->Result<Url>{
    let url:Url=asset.browser_download_url.parse().map_err(|_|error("UPDATE_SOURCE_INVALID","发布资产地址无效"))?;
    let expected=Asset{repository:REPOSITORY.into(),release_tag:release.tag_name.clone(),asset_id:asset.id,name:asset.name.clone(),size:asset.size,archive_sha256:"0".repeat(64)}.url();
    // GitHub may leave slashes in release tags unescaped. Compare the decoded path.
    let same_path=url.path_segments().is_some_and(|_|url.path().replace("%2F","/")==expected.path().replace("%2F","/"));
    if !download::permitted(&url)||url.host_str()!=Some("github.com")||!same_path||url.query().is_some()||url.fragment().is_some(){return Err(error("UPDATE_SOURCE_INVALID","发布资产不属于固定仓库标签"));}Ok(url)
}

#[cfg(test)]
mod tests {
    fn release(tag:&str,preview:bool)->super::Release{super::Release{tag_name:tag.into(),draft:false,prerelease:preview,assets:vec![]}}
    #[test]fn separates_v1_and_channels(){use crate::Channel::*;assert_eq!(super::sequence(&release("v1.0.2",false),Stable),None);assert_eq!(super::sequence(&release("sodeck-v2/stable/000002",false),Stable),Some(2));assert_eq!(super::sequence(&release("sodeck-v2/preview/000099",true),Stable),None);assert_eq!(super::sequence(&release("sodeck-v2/stable/000000",false),Stable),None);}
    #[test]fn metadata_room_reserves_both_body_and_etag_slots(){
        let temp=tempfile::tempdir().unwrap();let session=super::Session::new(temp.path()).unwrap();
        for n in 0..128{std::fs::write(session.root.join(format!("old-{n}")),b"x").unwrap();}
        let body=session.root.join("new.body");session.make_room(20,&body).unwrap();
        std::fs::write(body,b"body").unwrap();std::fs::write(session.root.join("new.etag"),b"tag").unwrap();
        assert!(std::fs::read_dir(&session.root).unwrap().count()<=128);
    }
}
