use crate::{atomic,error,model::*,Result};
use futures_util::StreamExt;
use sha2::{Digest,Sha256};
use std::{path::{Path,PathBuf},time::Duration};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use url::Url;

pub fn permitted(url:&Url)->bool {
    url.scheme()=="https"&&url.username().is_empty()&&url.password().is_none()&&url.port_or_known_default()==Some(443)
        &&matches!(url.host_str(),Some("api.github.com"|"github.com"|"release-assets.githubusercontent.com"|"objects.githubusercontent.com"))
}
pub fn client()->Result<reqwest::Client>{
    let _=rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).user_agent("SoDeck-v2-Updater")
        .redirect(reqwest::redirect::Policy::custom(|attempt|if attempt.previous().len()>=5||!permitted(attempt.url()){attempt.error("update redirect refused")}else{attempt.follow()}))
        .build().map_err(|_|error("UPDATE_SOURCE_INVALID","无法创建更新网络客户端"))
}
pub fn cache_path(root:&Path,asset:&Asset)->Result<PathBuf>{
    if !crate::catalog::sha(&asset.archive_sha256){return Err(error("UPDATE_SOURCE_INVALID","缓存摘要无效"));}
    Ok(atomic::directory(&root.join("cache"))?.join(&asset.archive_sha256))
}
pub fn verify_cached(root:&Path,asset:&Asset)->Result<PathBuf>{
    if asset.size == 0 || asset.size > MAX_INSTALLER { return Err(error("CAPACITY", "更新文件大小超出上限")); }
    use std::io::Read;
    let path=cache_path(root,asset)?;
    let meta=std::fs::symlink_metadata(&path).map_err(|_|error("STORAGE_ERROR","下载缓存不可用"))?;
    if atomic::is_link(&meta)||!meta.is_file()||meta.len()!=asset.size{return Err(error("UPDATE_HASH_MISMATCH","更新文件长度或类型不符"));}
    let mut file=std::fs::File::open(&path).map_err(|_|error("STORAGE_ERROR","无法读取下载缓存"))?.take(asset.size+1);
    let mut hash=Sha256::new();let mut total=0;let mut buffer=[0u8;64*1024];
    loop {let n=file.read(&mut buffer).map_err(|_|error("STORAGE_ERROR","无法核对更新文件"))?;if n==0{break;}total+=n as u64;hash.update(&buffer[..n]);}
    if total!=asset.size||format!("{:x}",hash.finalize())!=asset.archive_sha256{return Err(error("UPDATE_HASH_MISMATCH","更新文件校验不一致，请重新下载"));}Ok(path)
}
pub async fn fetch<F>(client:&reqwest::Client,root:&Path,asset:&Asset,cancel:CancellationToken,mut progress:F)->Result<PathBuf>
where F:FnMut(u64,u64)+Send {
    let url=asset.url();if !permitted(&url){return Err(error("UPDATE_SOURCE_INVALID","更新地址不受支持"));}
    fetch_url(client,root,asset,url,cancel,&mut progress).await
}
// Only fetch constructs production URLs. Tests exercise the real HTTP/stream
// handling against a private loopback listener without widening that boundary.
async fn fetch_url<F>(client:&reqwest::Client,root:&Path,asset:&Asset,url:Url,cancel:CancellationToken,mut progress:F)->Result<PathBuf>
where F:FnMut(u64,u64)+Send {
    if cancel.is_cancelled(){return Err(error("CANCELLED","下载已取消"));}
    if asset.size==0||asset.size>MAX_INSTALLER{return Err(error("CAPACITY","更新文件大小超出上限"));}
    let (cache_root,cache_asset)=(root.to_path_buf(),asset.clone());
    if let Ok(Ok(path))=tokio::task::spawn_blocking(move||verify_cached(&cache_root,&cache_asset)).await{if cancel.is_cancelled(){return Err(error("CANCELLED","下载已取消"));}return Ok(path);}
    let path=cache_path(root,asset)?;
    let part=path.with_extension(format!("{}.part",uuid::Uuid::new_v4()));
    let transfer=async {
        let response=tokio::select!{_=cancel.cancelled()=>return Err(error("CANCELLED","下载已取消")),r=tokio::time::timeout(Duration::from_secs(30),client.get(url).header("Accept","application/octet-stream").send())=>r.map_err(|_|error("UPDATE_TIMEOUT","下载服务器长时间没有响应"))?.map_err(|_|error("UPDATE_NETWORK","无法连接更新服务器"))?};
        if !response.status().is_success(){return Err(error(if matches!(response.status().as_u16(),403|429){"RATE_LIMITED"}else{"UPDATE_NETWORK"},"更新服务器暂时无法提供文件"));}
        if response.content_length().is_some_and(|n|n!=asset.size){return Err(error("UPDATE_HASH_MISMATCH","更新文件长度与发布记录不符"));}
        let mut file=tokio::fs::OpenOptions::new().write(true).create_new(true).open(&part).await.map_err(|_|error("STORAGE_ERROR","无法暂存下载文件"))?;
        let mut stream=response.bytes_stream();let mut hash=Sha256::new();let mut received=0;
        loop {
            let chunk=tokio::select!{_=cancel.cancelled()=>return Err(error("CANCELLED","下载已取消")),r=tokio::time::timeout(Duration::from_secs(30),stream.next())=>r.map_err(|_|error("UPDATE_TIMEOUT","下载长时间没有进展"))?};
            let Some(chunk)=chunk else{break};let chunk=chunk.map_err(|_|error("UPDATE_NETWORK","下载连接已中断"))?;
            received+=chunk.len()as u64;if received>asset.size{return Err(error("UPDATE_HASH_MISMATCH","下载文件超过发布大小"));}
            file.write_all(&chunk).await.map_err(|_|error("STORAGE_ERROR","无法写入更新缓存，请检查空间"))?;hash.update(&chunk);progress(received,asset.size);
        }
        if received!=asset.size||format!("{:x}",hash.finalize())!=asset.archive_sha256{return Err(error("UPDATE_HASH_MISMATCH","下载文件不完整或校验失败"));}
        file.sync_all().await.map_err(|_|error("STORAGE_ERROR","无法同步更新缓存"))?;drop(file);
        if cancel.is_cancelled(){return Err(error("CANCELLED","下载已取消"));}
        atomic::replace(&part,&path)?;Ok(path)
    };
    let result=tokio::time::timeout(Duration::from_secs(15*60),transfer).await.map_err(|_|error("UPDATE_TIMEOUT","下载超出时间限制")).and_then(|r|r);
    if result.is_err(){let _=tokio::fs::remove_file(&part).await;}result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read,Write};
    fn fixture(response:Vec<u8>)->(Url,std::thread::JoinHandle<()>) {
        let _=rustls::crypto::ring::default_provider().install_default();
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}/asset",listener.local_addr().unwrap()).parse().unwrap();
        let thread=std::thread::spawn(move||{let(mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();let mut request=[0;4096];let _=stream.read(&mut request);let _=stream.write_all(&response);});
        (url,thread)
    }
    fn asset(bytes:&[u8])->Asset {Asset{repository:REPOSITORY.into(),release_tag:"sodeck-v2/preview/000001".into(),asset_id:1,name:"fixture.pkg".into(),size:bytes.len()as u64,archive_sha256:crate::hash(bytes)}}
    fn no_parts(root:&Path){assert!(std::fs::read_dir(root.join("cache")).unwrap().all(|e|!e.unwrap().file_name().to_string_lossy().ends_with(".part")));}
    #[tokio::test]
    async fn actual_http_hash_length_and_cache_reuse(){
        let root=tempfile::tempdir().unwrap();let bytes=b"verified fixture";let expected=asset(bytes);
        let response=[format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).as_bytes(),bytes].concat();
        let(url,thread)=fixture(response);let client=reqwest::Client::builder().no_proxy().build().unwrap();
        let path=fetch_url(&client,root.path(),&expected,url.clone(),CancellationToken::new(),|_,_|{}).await.unwrap();thread.join().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(),bytes);
        // The listener has exited. A second call must use the verified cache.
        assert_eq!(fetch_url(&client,root.path(),&expected,url.clone(),CancellationToken::new(),|_,_|panic!("cache hit cannot transfer")).await.unwrap(),path);
        let cancel=CancellationToken::new();cancel.cancel();assert_eq!(fetch_url(&client,root.path(),&expected,url,cancel,|_,_|{}).await.unwrap_err().code,"CANCELLED");no_parts(root.path());
    }
    #[tokio::test]
    async fn damaged_chunked_body_and_cancel_never_publish_cache(){
        for cancelled in [false,true] {
            let root=tempfile::tempdir().unwrap();let expected=asset(b"good");
            let(url,thread)=fixture(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nevil\r\n0\r\n\r\n".to_vec());
            let client=reqwest::Client::builder().no_proxy().build().unwrap();let cancel=CancellationToken::new();let trigger=cancel.clone();
            let error=fetch_url(&client,root.path(),&expected,url,cancel,move|_,_|{if cancelled{trigger.cancel();}}).await.unwrap_err();
            assert!(if cancelled{matches!(error.code.as_str(),"CANCELLED"|"UPDATE_HASH_MISMATCH")}else{error.code=="UPDATE_HASH_MISMATCH"});
            thread.join().unwrap();assert!(!cache_path(root.path(),&expected).unwrap().exists());no_parts(root.path());
        }
    }
    #[tokio::test]
    async fn declared_wrong_length_and_rate_limit_are_reported(){
        for(response,code)in [(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nxx".to_vec(),"UPDATE_HASH_MISMATCH"),(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),"RATE_LIMITED")] {
            let root=tempfile::tempdir().unwrap();let(url,thread)=fixture(response);let client=reqwest::Client::builder().no_proxy().build().unwrap();
            assert_eq!(fetch_url(&client,root.path(),&asset(b"good"),url,CancellationToken::new(),|_,_|{}).await.unwrap_err().code,code);thread.join().unwrap();no_parts(root.path());
        }
    }
    #[test]fn narrow_https_hosts(){for u in ["http://github.com/a","https://github.com.evil.test/a","https://raw.githubusercontent.com/a","https://user@github.com/a","https://github.com:444/a"]{assert!(!super::permitted(&u.parse().unwrap()),"{u}");}assert!(super::permitted(&"https://release-assets.githubusercontent.com/a?token=test".parse().unwrap()));}
}
