use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Probe {
    directory: PathBuf,
    url: String,
    #[serde(default)]
    expect_failure: bool,
}

pub(super) async fn verify(handle: &tauri::AppHandle, file: &Path) -> Result<(), String> {
    let probe: Probe = serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for invalid in ["javascript:alert(1)", "file:///tmp", "relative", "--help"] {
        if crate::updates::open_web_page(handle.clone(), invalid)
            .await
            .is_ok()
        {
            return Err("非法网页目标未被拒绝".into());
        }
    }
    for invalid in [
        PathBuf::from("relative"),
        probe.directory.join("missing"),
        file.to_path_buf(),
        probe.directory.join("bad\0path"),
    ] {
        if crate::desktop_files::reveal_directory(handle.clone(), invalid)
            .await
            .is_ok()
        {
            return Err("非法目录目标未被拒绝".into());
        }
    }
    let web = crate::updates::open_web_page(handle.clone(), &probe.url).await;
    let directory = crate::desktop_files::reveal_directory(handle.clone(), probe.directory).await;
    if probe.expect_failure {
        if web.is_ok() || directory.is_ok() {
            return Err(format!(
                "系统启动失败被误报成功：web={web:?}, directory={directory:?}"
            ));
        }
    } else {
        web?;
        directory?;
    }
    Ok(())
}
