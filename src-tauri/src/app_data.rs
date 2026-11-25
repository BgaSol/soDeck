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
        let root = dunce::canonicalize(root)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn extended_data_paths_keep_special_names_and_support_component_joins() {
        let directory = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(directory.path()).unwrap();
        let paths = [
            canonical.join("trailing."),
            canonical.join("CON.txt"),
            canonical
                .join("long".repeat(25))
                .join("long".repeat(25))
                .join("child"),
        ];
        for path in paths {
            let lease = Lease::acquire(&path).unwrap();
            assert_eq!(
                std::fs::canonicalize(&lease.root).unwrap(),
                std::fs::canonicalize(&path).unwrap()
            );
            let file = lease
                .root
                .join("plugin-data")
                .join("sodeck.test")
                .join("settings.json");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            crate::storage::write_json(&file, &serde_json::json!({"ok":true})).unwrap();
            assert!(file.is_file());
        }
        let unc = Path::new(r"\\?\UNC\server\share\data");
        assert_eq!(dunce::simplified(unc), unc);
    }
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
