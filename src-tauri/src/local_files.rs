//! Read-only ordinary local sources. Native paths and file identities stay in core.
use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub(crate) struct ReadSource {
    pub file: tokio::fs::File,
    path: PathBuf,
    parent: PathBuf,
    version: String,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn open(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid("来源需要绝对路径"));
    }
    #[cfg(windows)]
    {
        let value = path.to_string_lossy().replace('/', "\\");
        if value.starts_with("\\\\?\\")
            || value.starts_with("\\\\.\\")
            || value.contains(':') && value.chars().skip(2).any(|c| c == ':')
        {
            return Err(invalid("不支持设备或数据流路径"));
        }
    }
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(invalid("来源必须是普通文件"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(invalid("来源必须是普通文件"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("来源不能是重解析文件"));
        }
    }
    Ok(file)
}
fn version(file: &File) -> io::Result<String> {
    let m = file.metadata()?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{}", m.dev(), m.ino(), m.ctime(), m.ctime_nsec())
    };
    #[cfg(windows)]
    let identity = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        format!(
            "{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        )
    };
    Ok(format!("{identity}:{}:{:?}", m.len(), m.modified()?))
}
impl ReadSource {
    pub async fn open(path: PathBuf) -> io::Result<Self> {
        tokio::task::spawn_blocking(move || {
            let parent =
                std::fs::canonicalize(path.parent().ok_or_else(|| invalid("来源没有父目录"))?)?;
            let file = open(&path)?;
            let initial = version(&file)?;
            if version(&open(&path)?)? != initial {
                return Err(invalid("打开期间来源已变化"));
            }
            Ok(Self {
                file: tokio::fs::File::from_std(file),
                path,
                parent,
                version: initial,
            })
        })
        .await
        .map_err(io::Error::other)?
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub async fn verify(&self) -> io::Result<()> {
        let file = self.file.try_clone().await?.into_std().await;
        let path = self.path.clone();
        let parent = self.parent.clone();
        let initial = self.version.clone();
        tokio::task::spawn_blocking(move || {
            if std::fs::canonicalize(path.parent().ok_or_else(|| invalid("来源没有父目录"))?)?
                != parent
                || version(&file)? != initial
                || version(&open(&path)?)? != initial
            {
                return Err(invalid("传输期间来源已变化"));
            }
            Ok(())
        })
        .await
        .map_err(io::Error::other)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn keeps_open_file_and_detects_replacement_and_changes() {
        let root = std::env::temp_dir().join(format!("sodeck-source-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("中文 $ source");
        std::fs::write(&path, b"first").unwrap();
        let source = ReadSource::open(path.clone()).await.unwrap();
        source.verify().await.unwrap();
        std::fs::rename(&path, root.join("old")).unwrap();
        std::fs::write(&path, b"other").unwrap();
        assert!(source.verify().await.is_err());
        drop(source);
        assert!(ReadSource::open(root.clone()).await.is_err());
        assert!(ReadSource::open(PathBuf::from("relative")).await.is_err());
        let source = ReadSource::open(path.clone()).await.unwrap();
        std::fs::write(&path, b"changed size").unwrap();
        assert!(source.verify().await.is_err());
        drop(source);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, root.join("link")).unwrap();
            assert!(ReadSource::open(root.join("link")).await.is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
