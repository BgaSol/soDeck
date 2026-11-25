//! Data ownership is acquired before any core initialization or filesystem cleanup.
use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub struct Lease {
    pub(crate) root: PathBuf,
    _file: File,
}

impl Lease {
    pub fn acquire(root: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let root = data_path(std::fs::canonicalize(root)?);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("app.lock"))?;
        file.try_lock().map_err(io::Error::from)?;
        Ok(Self { root, _file: file })
    }
}

// Plugin APIs use ordinary local paths; verbatim Win32 paths reject slash joins in clients.
fn data_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let value: Vec<u16> = path.as_os_str().encode_wide().collect();
        let prefix: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
        let direct: Vec<u16> = "\\\\?\\".encode_utf16().collect();
        if value.starts_with(&prefix) {
            let mut normalized: Vec<u16> = "\\\\".encode_utf16().collect();
            normalized.extend_from_slice(&value[prefix.len()..]);
            return std::ffi::OsString::from_wide(&normalized).into();
        }
        if value.starts_with(&direct) {
            return std::ffi::OsString::from_wide(&value[direct.len()..]).into();
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_excludes_another_writer_until_dropped() {
        let root = std::env::temp_dir().join(format!("sodeck-lease-{}", std::process::id()));
        let first = Lease::acquire(&root).unwrap();
        assert!(Lease::acquire(&root).is_err());
        drop(first);
        drop(Lease::acquire(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
