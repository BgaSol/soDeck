use super::{model::WriteHandle, planner::Entry};
use crate::model::{AppError, AppResult};
use ssh2::{OpenFlags, OpenType};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};

pub(super) fn upload_file(
    item: &WriteHandle,
    sftp: &ssh2::Sftp,
    entry: &Entry,
    stage: &str,
) -> AppResult<()> {
    let path = Path::new(&entry.source);
    let metadata = fs::symlink_metadata(path)?;
    if super::download::is_link(&metadata) || !metadata.is_file() {
        return Err(AppError::new("fileType", "本地文件类型已经变化"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut input = options.open(path)?;
    let before = input.metadata()?;
    let modified = before
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    if !before.is_file() || before.len() != entry.size || modified != entry.modified {
        return Err(AppError::new(
            "changed",
            "扫描后本地文件发生变化，请重新创建任务",
        ));
    }
    let mut output = sftp.open_mode(
        Path::new(stage),
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
        0o644,
        OpenType::File,
    )?;
    let mut buffer = [0u8; 65536];
    let mut written = 0;
    let starting_bytes = item.task.lock().unwrap().bytes_done;
    loop {
        item.check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        written += count as u64;
        if written > entry.size {
            return Err(AppError::new(
                "changed",
                "上传期间本地文件增大，未提交目标文件",
            ));
        }
        output.write_all(&buffer[..count])?;
        let mut task = item.task.lock().unwrap();
        task.current_bytes = written;
        task.bytes_done = starting_bytes + written;
    }
    output.close()?;
    let after = input.metadata()?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err(AppError::new(
            "changed",
            "上传期间本地文件发生变化，未提交目标文件",
        ));
    }
    Ok(())
}
