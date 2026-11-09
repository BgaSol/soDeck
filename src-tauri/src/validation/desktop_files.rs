use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
struct Probe {
    paths: Vec<PathBuf>,
    change_marker: PathBuf,
}

pub(super) async fn verify(handle: &tauri::AppHandle, file: &Path) -> Result<(), String> {
    let probe: Probe = serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let info = crate::desktop_files::clipboard(handle.clone(), false).await?;
    let files = crate::desktop_files::clipboard(handle.clone(), true).await?;
    if !info.has_files || files.paths != probe.paths || info.revision != files.revision {
        return Err("原生文件剪贴板与独立来源不一致".into());
    }
    std::fs::write(&probe.change_marker, "change").map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        let next = crate::desktop_files::clipboard(handle.clone(), false).await?;
        if next.revision != files.revision && !next.has_files {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("文件来源切换为文本后 revision 未正确变化".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let text = crate::desktop_files::clipboard(handle.clone(), true).await?;
    if text.has_files || !text.paths.is_empty() {
        return Err("普通文本被当作系统文件来源".into());
    }
    Ok(())
}
