use std::{fs, io::Write, path::Path};
use crate::{Result, Error};
pub(crate) fn write(path:&Path, bytes:&[u8])->Result<()> {
    let parent=path.parent().ok_or_else(||Error::Invalid("Missing storage directory".into()))?;
    fs::create_dir_all(parent)?;
    if let Ok(m)=fs::symlink_metadata(path) { if !m.is_file() || crate::package::is_link(&m) { return Err(Error::Invalid("Storage target is not a regular file".into())); } }
    let tmp=parent.join(format!(".plugin-{}.pending",uuid::Uuid::new_v4()));
    let result=(||{
        let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        checkpoint("before-write"); file.write_all(bytes)?; checkpoint("after-write"); file.sync_all()?; checkpoint("after-sync"); drop(file);
        checkpoint("before-replace"); replace(&tmp,path)?; checkpoint("after-replace");
        #[cfg(not(windows))] fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _=fs::remove_file(&tmp); }
    result
}
fn replace(source:&Path,target:&Path)->Result<()> {
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_REPLACE_EXISTING,MOVEFILE_WRITE_THROUGH};
        let s=source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let t=target.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        unsafe { MoveFileExW(PCWSTR(s.as_ptr()),PCWSTR(t.as_ptr()),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH) }
            .map_err(|_|Error::Fault("STORAGE_ERROR","无法原子保存插件配置".into()))?;
    }
    #[cfg(not(windows))] fs::rename(source,target)?;
    Ok(())
}
#[cfg(not(test))]
fn checkpoint(_: &str) {}
#[cfg(test)]
fn checkpoint(stage:&str) {
    if std::env::var("SODECK_PLUGIN_ATOMIC_STAGE").ok().as_deref()==Some(stage) {
        fs::write(std::env::var("SODECK_PLUGIN_ATOMIC_MARKER").unwrap(),stage).unwrap();
        loop {std::thread::sleep(std::time::Duration::from_millis(10));}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn crash_child(){
        if let Ok(path)=std::env::var("SODECK_PLUGIN_ATOMIC_TARGET"){write(Path::new(&path),br#"{"preference":"new","receipt":"new"}"#).unwrap();}
    }
    #[test] fn killed_writer_preserves_whole_old_or_new_record(){
        use std::{process::{Command,Stdio},time::{Instant,Duration}};
        for stage in ["before-write","after-write","after-sync","before-replace","after-replace"] {
            let dir=tempfile::tempdir().unwrap();let target=dir.path().join("enablement.json");let marker=dir.path().join("marker");
            let old=br#"{"preference":"old","receipt":"old"}"#;fs::write(&target,old).unwrap();
            let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","atomic_file::tests::crash_child","--nocapture"])
                .env("SODECK_PLUGIN_ATOMIC_TARGET",&target).env("SODECK_PLUGIN_ATOMIC_STAGE",stage).env("SODECK_PLUGIN_ATOMIC_MARKER",&marker).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let start=Instant::now();while !marker.exists()&&start.elapsed()<Duration::from_secs(10){std::thread::sleep(Duration::from_millis(10));}
            let reached=marker.exists();child.kill().unwrap();child.wait().unwrap();assert!(reached,"checkpoint {stage}");
            let actual:serde_json::Value=serde_json::from_slice(&fs::read(target).unwrap()).unwrap();let expected=if stage=="after-replace"{"new"}else{"old"};assert_eq!(actual["preference"],expected);assert_eq!(actual["receipt"],expected);
        }
    }
}
