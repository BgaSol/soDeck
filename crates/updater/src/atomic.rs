use crate::{error,Result};
use std::{fs,io::{Read,Write},path::{Path,PathBuf}};

pub fn is_link(m:&fs::Metadata)->bool {
    #[cfg(windows)] {use std::os::windows::fs::MetadataExt;m.file_type().is_symlink()||m.file_attributes()&0x400!=0}
    #[cfg(not(windows))] {m.file_type().is_symlink()}
}
pub fn directory(path:&Path)->Result<PathBuf>{
    if let Ok(m)=fs::symlink_metadata(path){if is_link(&m)||!m.is_dir(){return Err(error("STORAGE_ERROR","更新目录不是普通目录"));}}
    fs::create_dir_all(path).map_err(|_|error("STORAGE_ERROR","无法创建更新目录"))?;
    fs::canonicalize(path).map_err(|_|error("STORAGE_ERROR","无法定位更新目录"))
}
pub fn read(path:&Path,max:u64)->Result<Vec<u8>> {
    let m=fs::symlink_metadata(path).map_err(|_|error("STORAGE_ERROR","无法读取更新文件"))?;
    if is_link(&m)||!m.is_file()||m.len()>max{return Err(error("STORAGE_ERROR","更新文件类型或大小无效"));}
    let mut file=fs::File::open(path).map_err(|_|error("STORAGE_ERROR","无法打开更新文件"))?;
    let mut bytes=Vec::new();Read::by_ref(&mut file).take(max+1).read_to_end(&mut bytes).map_err(|_|error("STORAGE_ERROR","无法读取更新文件"))?;
    if bytes.len()as u64>max{return Err(error("CAPACITY","更新文件超出容量"));}Ok(bytes)
}
pub fn replace(source:&Path,target:&Path)->Result<()> {
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        use windows::{core::PCWSTR,Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_REPLACE_EXISTING,MOVEFILE_WRITE_THROUGH}};
        let s=source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();let t=target.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        unsafe{MoveFileExW(PCWSTR(s.as_ptr()),PCWSTR(t.as_ptr()),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)}.map_err(|_|error("STORAGE_ERROR","无法原子保存更新状态"))?;
    }
    #[cfg(not(windows))] fs::rename(source,target).map_err(|_|error("STORAGE_ERROR","无法原子保存更新状态"))?;
    Ok(())
}
pub fn write(path:&Path,bytes:&[u8])->Result<()> {
    let parent=directory(path.parent().ok_or_else(||error("STORAGE_ERROR","更新文件没有父目录"))?)?;
    if let Ok(m)=fs::symlink_metadata(path){if is_link(&m)||!m.is_file(){return Err(error("STORAGE_ERROR","更新状态不是普通文件"));}}
    let temporary=parent.join(format!(".update-{}.part",uuid::Uuid::new_v4()));
    let result=(||{let mut f=fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|_|error("STORAGE_ERROR","无法暂存更新状态"))?;
        f.write_all(bytes).and_then(|_|f.sync_all()).map_err(|_|error("STORAGE_ERROR","无法保存更新状态"))?;drop(f);replace(&temporary,path)?;
        #[cfg(not(windows))] fs::File::open(&parent).and_then(|f|f.sync_all()).map_err(|_|error("STORAGE_ERROR","无法同步更新状态"))?;
        Ok(())})();if result.is_err(){let _=fs::remove_file(temporary);}result
}
