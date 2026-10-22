use super::*;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
pub(super) const MAX_MANIFEST: usize = 1024 * 1024;
pub(super) const MAX_PACKAGE: usize = 512 * 1024 * 1024;
const MANIFEST_URL: &str = "https://github.com/BgaSol/soDeck/releases/latest/download/latest.json";
pub(super) async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}
pub(super) fn redirect_allowed(url: &tauri::Url, hops: usize) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && hops <= 5
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
            )
        )
}
fn client() -> AppResult<reqwest::Client> {
    // We fetch before Updater::check, which normally installs Tauri's ring provider.
    // Installing an existing default is harmless and does not replace it.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("SoDeck/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if redirect_allowed(a.url(), a.previous().len()) {
                a.follow()
            } else {
                a.error("untrusted update redirect")
            }
        }))
        .build()
        .map_err(|_| error("无法初始化更新网络连接。"))
}
pub(super) fn append_limited(buffer: &mut Vec<u8>, chunk: &[u8], limit: usize) -> AppResult<()> {
    if buffer
        .len()
        .checked_add(chunk.len())
        .is_none_or(|n| n > limit)
    {
        return Err(error("更新文件超过大小上限。"));
    }
    buffer
        .try_reserve(chunk.len())
        .map_err(|_| error("内存不足，无法继续下载更新。"))?;
    buffer.extend_from_slice(chunk);
    Ok(())
}
async fn body(
    response: reqwest::Response,
    limit: usize,
    progress: Option<&watch::Sender<Progress>>,
) -> AppResult<Vec<u8>> {
    if !response.status().is_success() {
        return Err(error("更新服务暂不可用，请稍后重试。"));
    }
    let total = response.content_length();
    if total.is_some_and(|n| n > limit as u64) {
        return Err(error("更新文件超过大小上限。"));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| error("更新下载中断，请重试。"))?;
        append_limited(&mut bytes, &chunk, limit)?;
        if let Some(progress) = progress {
            progress.send_replace(Progress {
                downloaded: bytes.len() as u64,
                total,
            });
        }
    }
    if bytes.is_empty() || total.is_some_and(|n| n != bytes.len() as u64) {
        return Err(error("更新文件不完整。"));
    }
    Ok(bytes)
}
pub(super) async fn manifest(cancel: watch::Receiver<bool>) -> AppResult<serde_json::Value> {
    tokio::select! {
        _ = cancelled(cancel) => Err(error("更新检查已取消。")),
        result = async {
            let response = client()?.get(MANIFEST_URL).timeout(Duration::from_secs(30)).send().await.map_err(|_| error("无法连接更新服务，请稍后重试。"))?;
            let bytes = body(response, MAX_MANIFEST, None).await?;
            serde_json::from_slice(&bytes).map_err(|_| error("更新清单格式无效。"))
        } => result,
    }
}
pub(super) fn metadata(raw: &serde_json::Value) -> AppResult<Release> {
    let version = raw
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| error("清单缺少版本。"))?;
    policy::stable_version(version)?;
    if raw
        .pointer("/sodeck/schemaVersion")
        .and_then(|v| v.as_u64())
        != Some(1)
    {
        return Err(error("更新清单 schema 不受支持。"));
    }
    let notes = raw
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if notes.len() > 64 * 1024 {
        return Err(error("版本说明超过大小上限。"));
    }
    Ok(Release {
        id: uuid::Uuid::new_v4().to_string(),
        version: version.into(),
        notes: notes.into(),
        published_at: raw
            .get("pub_date")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    })
}
pub(super) fn candidate(app: &tauri::AppHandle, raw: serde_json::Value) -> AppResult<Candidate> {
    let release = metadata(&raw)?;
    let mut capability = platform::capability();
    let manual_url = capability
        .manual_format
        .as_deref()
        .and_then(|f| platform::manual_asset(&release.version, f))
        .filter(|(key, name)| {
            raw.pointer("/sodeck/manualAssets")
                .and_then(|m| m.get(key))
                .and_then(|n| n.as_str())
                == Some(name)
        })
        .map(|(_, name)| format!("{REPOSITORY}/releases/download/v{}/{name}", release.version));
    if manual_url.is_none() && capability.mode != "unavailable" {
        capability.mode = "unavailable".into();
        capability.reason = Some("新版缺少当前安装方式对应的安装包，请查看发布页。".into());
    }
    let update = if capability.mode == "inApp" {
        let target = capability
            .target
            .as_deref()
            .ok_or_else(|| error("缺少安装目标。"))?;
        let entry = raw.get("platforms").and_then(|p| p.get(target));
        if entry.is_none() {
            capability.mode = "manual".into();
            capability.reason = Some("新版缺少应用内更新包，请下载对应安装包。".into());
            None
        } else {
            let u = app
                .updater_builder()
                .target(target)
                .build()
                .map_err(|_| error("无法初始化更新器。"))?
                .restore_update(raw.clone())
                .map_err(|_| error("应用内更新清单无效。"))?
                .ok_or_else(|| error("更新版本无效。"))?;
            let name = platform::package_name(&release.version, target)
                .ok_or_else(|| error("未知更新目标。"))?;
            if u.download_url.as_str()
                != format!("{REPOSITORY}/releases/download/v{}/{name}", release.version)
            {
                return Err(error("更新包必须来自此版本对应的官方附件。"));
            }
            Some(u)
        }
    } else {
        None
    };
    let fingerprint = update
        .as_ref()
        .map(|u| {
            format!(
                "{:x}",
                Sha256::digest(format!(
                    "{}|{}|{}|{}",
                    u.version, u.target, u.download_url, u.signature
                ))
            )
        })
        .unwrap_or_else(|| {
            format!(
                "{:x}",
                Sha256::digest(format!("{}|{:?}", release.version, manual_url))
            )
        });
    Ok(Candidate {
        release,
        update,
        fingerprint,
        manual_url,
        capability,
    })
}
pub(super) async fn check(
    app: tauri::AppHandle,
    cancel: watch::Receiver<bool>,
) -> AppResult<Option<Candidate>> {
    let raw = manifest(cancel).await?;
    let release = metadata(&raw)?;
    if !policy::stable_newer(&app.package_info().version.to_string(), &release.version) {
        return Ok(None);
    }
    candidate(&app, raw).map(Some)
}
pub(super) async fn download(
    update: Update,
    cancel: watch::Receiver<bool>,
    progress: watch::Sender<Progress>,
    tx: mpsc::Sender<manager::Request>,
    id: String,
) -> AppResult<Arc<Vec<u8>>> {
    let bytes = tokio::select! {
        _ = cancelled(cancel.clone()) => return Err(error("下载已取消。")),
        result = async {
            let response = client()?.get(update.download_url.clone()).timeout(Duration::from_secs(1800)).send().await.map_err(|_| error("更新下载无法连接，请重试。"))?;
            body(response, MAX_PACKAGE, Some(&progress)).await
        } => result?,
    };
    if *cancel.borrow() {
        return Err(error("下载已取消。"));
    }
    let _ = tx.send(manager::Request::Verifying(id)).await;
    // Blocking verification cannot be forcibly cancelled. Keep the job registered until it finishes.
    crate::blocking(move || {
        update
            .verify(&bytes)
            .map_err(|_| error("更新签名或签名版本验证失败。"))?;
        Ok(Arc::new(bytes))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn response(wire: &'static [u8]) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(wire).await.unwrap();
        });
        client()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn chunked_limit_and_truncated_responses_never_succeed() {
        let r = response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n4\r\n5678\r\n0\r\n\r\n").await;
        assert!(body(r, 7, None).await.is_err());
        let r = response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n123").await;
        assert!(body(r, 12, None).await.is_err());
        let r = response(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n").await;
        assert!(body(r, 7, None).await.is_err());
    }
    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_network_read() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let r = client()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let (cancel, rx) = watch::channel(false);
        let work = tokio::spawn(async move {
            tokio::select! { _ = cancelled(rx) => true, _ = body(r, 2048, None) => false }
        });
        cancel.send_replace(true);
        assert!(tokio::time::timeout(Duration::from_millis(250), work)
            .await
            .unwrap()
            .unwrap());
        server.abort();
    }
}
