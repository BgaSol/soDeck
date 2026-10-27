//! GUI applications need a durable, bounded error channel even before setup.
use std::{fs,io::{self,Write},path::{Path,PathBuf},sync::{Mutex,OnceLock},time::{SystemTime,UNIX_EPOCH}};
const LIMIT:u64=2*1024*1024;
static TARGET:OnceLock<Mutex<Option<PathBuf>>>=OnceLock::new();
fn target()->&'static Mutex<Option<PathBuf>>{TARGET.get_or_init(||Mutex::new(None))}
fn fallback()->PathBuf{std::env::temp_dir().join("SoDeck-v2-diagnostics").join(format!("startup-{}.log",std::process::id()))}
fn append(path:&Path,text:&str)->io::Result<()>{
    fs::create_dir_all(path.parent().ok_or_else(||io::Error::other("日志目录无效"))?)?;
    if fs::metadata(path).is_ok_and(|m|m.len()>=LIMIT){
        let numbered=|n|path.with_extension(format!("log.{n}"));
        if numbered(2).exists(){fs::remove_file(numbered(2))?;}
        if numbered(1).exists(){fs::rename(numbered(1),numbered(2))?;}
        fs::rename(path,numbered(1))?;
    }
    let mut file=fs::OpenOptions::new().create(true).append(true).open(path)?;
    let time=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let line=text.chars().filter(|c|*c!='\r').take(8192).collect::<String>();
    writeln!(file,"{time} {} {line}",env!("CARGO_PKG_VERSION"))?;file.flush()
}
pub fn use_data(data:&Path){*target().lock().unwrap()=Some(data.join("logs/startup.log"));record("data-lock acquired");}
pub fn record(message:&str)->PathBuf{
    let mut current=target().lock().unwrap();let path=current.clone().unwrap_or_else(fallback);
    if append(&path,message).is_ok(){*current=Some(path.clone());return path;}
    let path=fallback();let _=append(&path,message);*current=Some(path.clone());path
}
pub fn fatal(error:&dyn std::fmt::Display){
    let path=record(&format!("startup failed: {error}"));
    rfd::MessageDialog::new().set_title("SoDeck · 启动失败").set_description(format!("无法启动 SoDeck。\n\n{error}\n\n诊断日志：{}",path.display())).set_level(rfd::MessageLevel::Error).set_buttons(rfd::MessageButtons::Ok).show();
}
#[cfg(test)]mod tests{use super::*;#[test]fn rotates_bounded_logs(){let t=tempfile::tempdir().unwrap();let p=t.path().join("startup.log");for _ in 0..4{fs::write(&p,vec![b'x';LIMIT as usize]).unwrap();append(&p,"next").unwrap();}assert!(fs::metadata(&p).unwrap().len()<1024);assert!(p.with_extension("log.1").exists());assert!(p.with_extension("log.2").exists());assert!(!p.with_extension("log.3").exists());}}
