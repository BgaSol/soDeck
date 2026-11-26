use super::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

fn ordinary(path: &Path, dir: bool) -> bool {
    let Ok(m) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !m.file_type().is_symlink() && if dir { m.is_dir() } else { m.is_file() }
}

pub(super) struct Prepared {
    temporary: Option<tempfile::TempDir>,
    destination: Option<PathBuf>,
    published: bool,
    accepted: bool,
}
impl Prepared {
    pub fn new(root: &Path, contents: &[&str]) -> Result<Self, String> {
        if contents.is_empty() {
            return Ok(Self {
                temporary: None,
                destination: None,
                published: false,
                accepted: false,
            });
        }
        let root = root.join("server-keys");
        crate::platform::secure_directory(&root).map_err(|e| e.to_string())?;
        let temporary = tempfile::Builder::new()
            .prefix("pending-")
            .tempdir_in(&root)
            .map_err(|e| e.to_string())?;
        crate::platform::secure_directory(temporary.path()).map_err(|e| e.to_string())?;
        let name = temporary
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace("pending-", "import-");
        let destination = root.join(name);
        for (i, content) in contents.iter().enumerate() {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(temporary.path().join(format!("key-{i}")))
                .map_err(|e| e.to_string())?;
            file.write_all(content.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            temporary: Some(temporary),
            destination: Some(destination),
            published: false,
            accepted: false,
        })
    }
    pub fn path(&self, i: usize) -> String {
        self.destination
            .as_ref()
            .unwrap()
            .join(format!("key-{i}"))
            .to_string_lossy()
            .into()
    }
    pub fn publish(&mut self) -> Result<(), String> {
        if let (Some(temp), Some(dest)) = (&self.temporary, &self.destination) {
            if dest.try_exists().map_err(|e| e.to_string())? {
                return Err("私钥目录已存在".into());
            }
            fs::rename(temp.path(), dest).map_err(|e| e.to_string())?;
            self.published = true;
        }
        Ok(())
    }
    pub fn accept(&mut self) {
        self.accepted = true;
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if self.published && !self.accepted {
            if let Some(dest) = &self.destination {
                if remove_owned(dest).is_err() {
                    eprintln!("本次导入私钥清理未完成，下次启动将重试");
                }
            }
        }
    }
}

fn batch_name(name: &str) -> bool {
    ["import-", "pending-"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|s| s.len() >= 6 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
    })
}
fn remove_owned(path: &Path) -> std::io::Result<()> {
    if !ordinary(path, true) {
        return Err(std::io::Error::other("受管密钥目录类型异常"));
    }
    let entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    if entries.iter().any(|e| {
        !ordinary(&e.path(), false)
            || !e
                .file_name()
                .to_string_lossy()
                .strip_prefix("key-")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    }) {
        return Err(std::io::Error::other("受管目录含未知文件"));
    }
    for entry in entries {
        fs::remove_file(entry.path())?;
    }
    fs::remove_dir(path)
}

// Run before connections start. Invalid recovery files mean we cannot prove a key is unused.
pub(super) fn cleanup(root: &Path, current: &Catalog) {
    let keys = root.join("server-keys");
    if !ordinary(&keys, true) {
        return;
    }
    let Ok(keys) = fs::canonicalize(&keys) else {
        return;
    };
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut catalogs = vec![current.clone()];
    for entry in entries {
        let Ok(entry) = entry else {
            return;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "servers.json.bak"
            || name == "servers.v1.json.bak"
            || name.starts_with("servers.corrupt-")
        {
            if !ordinary(&entry.path(), false) {
                return;
            }
            match read(&entry.path()) {
                Ok(c) => catalogs.push(c),
                Err(_) => return,
            }
        }
    }
    let mut used = HashSet::new();
    for c in &catalogs {
        for s in &c.servers {
            if let Authentication::PrivateKey {
                private_key_path: Some(p),
                ..
            } = &s.authentication
            {
                let resolved = fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
                let p = resolved.as_path();
                if let Some(batch) = p.parent() {
                    if batch.parent() == Some(keys.as_path()) {
                        used.insert(batch.to_path_buf());
                    }
                }
            }
        }
    }
    let Ok(entries) = fs::read_dir(&keys) else {
        return;
    };
    for entry in entries.flatten() {
        if batch_name(&entry.file_name().to_string_lossy()) && !used.contains(&entry.path()) {
            let _ = remove_owned(&entry.path());
        }
    }
}
