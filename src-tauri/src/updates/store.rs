use super::*;
use std::io::Write;
pub(super) fn load(path: &std::path::Path) -> (Saved, bool) {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Saved>(&bytes) {
            Ok(saved) if saved.schema_version == 1 => (saved, true),
            _ => (Saved::default(), false),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Saved::default(), true),
        Err(_) => (Saved::default(), false),
    }
}
pub(super) fn persist(path: &std::path::Path, saved: &Saved) -> AppResult<()> {
    let action = || -> Result<(), Box<dyn std::error::Error>> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("path")?)?;
        file.write_all(&serde_json::to_vec_pretty(saved)?)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    };
    action().map_err(|_| error("无法保存更新设置，自动任务已暂停。请检查配置目录的写入权限。"))
}
pub(super) async fn save(path: PathBuf, saved: Saved) -> AppResult<()> {
    crate::blocking(move || persist(&path, &saved)).await
}
