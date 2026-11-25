//! Real OS failures at the final target; independent of the temporary filename.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Block {
    #[cfg(windows)]
    file: Option<fs::File>,
    #[cfg(windows)]
    created: Option<PathBuf>,
    #[cfg(unix)]
    parent: PathBuf,
    #[cfg(unix)]
    permissions: fs::Permissions,
}
pub fn block(path: impl AsRef<Path>) -> Option<Block> {
    let path = path.as_ref();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let created = if path.exists() {
            None
        } else {
            fs::write(path, b"[]").unwrap();
            Some(path.to_path_buf())
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(path)
            .unwrap();
        Some(Block {
            file: Some(file),
            created,
        })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("write-denial scenario skipped: root bypasses directory mode bits");
            return None;
        }
        let parent = path.parent().unwrap().to_path_buf();
        let permissions = fs::metadata(&parent).unwrap().permissions();
        fs::set_permissions(
            &parent,
            fs::Permissions::from_mode(permissions.mode() & !0o222),
        )
        .unwrap();
        Some(Block {
            parent,
            permissions,
        })
    }
}
impl Drop for Block {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            drop(self.file.take());
            if let Some(path) = self.created.take() {
                fs::remove_file(path).unwrap();
            }
        }
        #[cfg(unix)]
        fs::set_permissions(&self.parent, self.permissions.clone()).unwrap();
    }
}
