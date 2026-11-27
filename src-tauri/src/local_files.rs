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

/// Commit only an operation-owned, closed, same-directory temporary file.
pub(crate) async fn commit_temporary(
    temp: PathBuf,
    target: PathBuf,
    replace: bool,
) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let metadata = std::fs::symlink_metadata(&temp)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || std::fs::canonicalize(temp.parent().ok_or_else(|| invalid("临时文件没有父目录"))?)?
                != std::fs::canonicalize(target.parent().ok_or_else(|| invalid("目标没有父目录"))?)?
        {
            return Err(invalid("提交需要同目录的普通临时文件"));
        }
        let temporary = tempfile::TempPath::try_from_path(temp)?;
        if replace {
            temporary.persist(target)
        } else {
            temporary.persist_noclobber(target)
        }
        .map_err(|e| e.error)
    })
    .await
    .map_err(io::Error::other)?
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
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if !matches!(components.next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::UNC(_, _) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _)))
            || components.any(|part| match part {
                Component::Normal(name) => name.as_encoded_bytes().contains(&b':'),
                Component::RootDir => false,
                _ => true,
            })
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
    async fn commit_is_exclusive_and_replacement_preserves_original_on_failure() {
        let root = tempfile::tempdir().unwrap();
        let temp = root.path().join("中文.part");
        let target = root.path().join("target");
        std::fs::write(&temp, b"first").unwrap();
        commit_temporary(temp.clone(), target.clone(), false)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        std::fs::write(&temp, b"second").unwrap();
        assert_eq!(
            commit_temporary(temp.clone(), target.clone(), false)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        std::fs::write(&temp, b"second").unwrap();
        commit_temporary(temp.clone(), target.clone(), true)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        assert!(!temp.exists());
        std::fs::create_dir(&temp).unwrap();
        assert!(commit_temporary(temp, target.clone(), true).await.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn extended_ordinary_sources_work_while_devices_and_streams_stay_rejected() {
        let root = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(root.path())
            .unwrap()
            .join("nested".repeat(20))
            .join("nested".repeat(20))
            .join("中文");
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join("ordinary.txt");
        std::fs::write(&path, b"original").unwrap();
        let source = ReadSource::open(path.clone()).await.unwrap();
        source.verify().await.unwrap();
        drop(source);
        for rejected in [
            PathBuf::from(r"\\.\NUL"),
            PathBuf::from(r"\\?\GLOBALROOT\Device\Null"),
            path.with_extension("txt:stream"),
        ] {
            assert!(ReadSource::open(rejected).await.is_err());
        }
    }
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
