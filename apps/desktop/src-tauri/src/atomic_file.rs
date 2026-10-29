use std::path::Path;
pub(crate) fn replace(source: &Path, target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        let s = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let t = target
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        unsafe {
            windows::Win32::Storage::FileSystem::MoveFileExW(
                PCWSTR(s.as_ptr()),
                PCWSTR(t.as_ptr()),
                windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|e| e.to_string())
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, target).map_err(|e| e.to_string())
    }
}

pub(crate) fn write(target: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::{fs, io::Write};
    let parent=target.parent().ok_or("Missing parent directory")?;
    fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    if fs::symlink_metadata(target).is_ok_and(|m|!m.file_type().is_file()){return Err("Target is not a regular file".into());}
    let pending=parent.join(format!(".settings-{}.pending",uuid::Uuid::new_v4()));
    let result=(|| {let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&pending).map_err(|e|e.to_string())?;
        checkpoint("before-write");file.write_all(bytes).map_err(|e|e.to_string())?;checkpoint("after-write");
        file.sync_all().map_err(|e|e.to_string())?;checkpoint("after-sync");drop(file);checkpoint("before-replace");
        replace(&pending,target)?;checkpoint("after-replace");Ok(())})();
    if result.is_err(){let _=fs::remove_file(&pending);}result
}
#[cfg(not(test))]
fn checkpoint(_: &str) {}
#[cfg(test)]
fn checkpoint(stage:&str){if std::env::var("SODECK_ATOMIC_TEST_STAGE").ok().as_deref()==Some(stage){std::fs::write(std::env::var("SODECK_ATOMIC_TEST_MARKER").unwrap(),stage).unwrap();loop{std::thread::sleep(std::time::Duration::from_millis(50));}}}
#[cfg(test)]
mod tests {
    use super::*;
    const OLD:&[u8]=br#"{"value":14,"revision":"0","receipts":[]}"#;
    const NEW:&[u8]=br#"{"value":18,"revision":"1","receipts":["operation"]}"#;
    #[test] #[ignore] fn crash_child(){write(Path::new(&std::env::var("SODECK_ATOMIC_TEST_TARGET").unwrap()),NEW).unwrap();}
    #[test] fn process_termination_keeps_complete_value_and_receipt(){
        for stage in ["before-write","after-write","after-sync","before-replace","after-replace"]{
            let dir=tempfile::tempdir().unwrap();let target=dir.path().join("settings.json");let marker=dir.path().join("ready");std::fs::write(&target,OLD).unwrap();
            let mut command=std::process::Command::new(std::env::current_exe().unwrap());command.args(["--exact","atomic_file::tests::crash_child","--ignored"]).env("SODECK_ATOMIC_TEST_STAGE",stage).env("SODECK_ATOMIC_TEST_TARGET",&target).env("SODECK_ATOMIC_TEST_MARKER",&marker).stdout(std::process::Stdio::null());
            #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
            let mut child=command.spawn().unwrap();let began=std::time::Instant::now();while !marker.exists()&&began.elapsed()<std::time::Duration::from_secs(10){std::thread::sleep(std::time::Duration::from_millis(10));}
            let reached=marker.exists();let _=child.kill();child.wait().unwrap();assert!(reached,"{stage}");assert_eq!(std::fs::read(&target).unwrap(),if stage=="after-replace"{NEW}else{OLD},"{stage}");
        }
    }
}
