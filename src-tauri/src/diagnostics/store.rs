use super::event::{id, now, timestamp};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub const LIMIT: u64 = 100 * 1024 * 1024;
pub const SEGMENT: u64 = 10 * 1024 * 1024;
pub const RETENTION: i64 = 7 * 24 * 60 * 60 * 1000;
pub const RESERVE: u64 = 512 * 1024;
pub const README:&str="SoDeck 本地诊断\n可以直接复制整个 diagnostics 目录，或某次运行的整个子目录。建议使用应用中的打包日志按钮取得稳定快照。\n先阅读 summary.txt，再按 traceId 查找 events-*.jsonl；errors.jsonl 仅为索引。文件为 UTF-8。\n本目录不包含终端正文、文件内容、凭据或服务器配置。日志按 7 天 / 100 MiB 上限自动清理，可能不足 7 天。\n运行中手工复制可能缺少末尾或遇到轮转；忽略 JSONL 不完整末行。未确认正常结束不等于已经证明应用崩溃。\n";

pub fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !linked(&m))
}
pub fn directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !linked(&m))
}
fn linked(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    m.file_type().is_symlink()
}
pub fn checked_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    // System parents such as macOS /var may themselves be OS-managed links.
    // Managed descendants are checked individually before every traversal.
    if !directory(path) {
        return Err(io::Error::other("invalid diagnostics directory"));
    }
    Ok(())
}
pub fn open(path: &Path) -> io::Result<File> {
    if fs::symlink_metadata(path).is_ok() && !regular(path) {
        return Err(io::Error::other("invalid diagnostics file"));
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}
pub fn lock(root: &Path) -> io::Result<File> {
    if !directory(root) || !directory(&root.join("runs")) {
        return Err(io::Error::other("invalid diagnostics root"));
    }
    let file = open(&root.join(".maintenance.lock"))?;
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock)
                if started.elapsed() < Duration::from_secs(2) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => {
                return Err(io::Error::other(format!(
                    "diagnostics maintenance lock: {e}"
                )))
            }
        }
    }
}
pub fn atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok() && !regular(path) {
        return Err(io::Error::other("invalid metadata file"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("missing parent"))?,
    )?;
    temp.write_all(bytes)?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub schema_version: u32,
    pub run_id: String,
    pub directory: String,
    pub process_id: u32,
    pub version: String,
    pub build_id: String,
    pub os: String,
    pub arch: String,
    pub system_version: String,
    #[serde(default)]
    pub webview_version: String,
    pub started_at: i64,
    pub updated_at: i64,
    pub ended_at: Option<i64>,
    pub last_seq: u64,
    pub warnings: u64,
    pub errors: u64,
    pub dropped: u64,
    pub removed_segments: u64,
    pub recent_errors: Vec<serde_json::Value>,
    pub fallback: bool,
    pub incomplete: bool,
}
impl Session {
    pub fn new(fallback: bool) -> Self {
        let run_id = id();
        let started_at = now();
        let date = timestamp(started_at).replace([':', '-', '.'], "");
        Self {
            schema_version: 1,
            directory: format!("{}_{}", date, &run_id[..8]),
            run_id,
            process_id: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
            build_id: option_env!("SODECK_BUILD_ID")
                .unwrap_or("workspace-unknown")
                .into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            system_version: system_version(),
            webview_version: tauri::webview_version().unwrap_or_else(|_| "unavailable".into()),
            started_at,
            updated_at: started_at,
            ended_at: None,
            last_seq: 0,
            warnings: 0,
            errors: 0,
            dropped: 0,
            removed_segments: 0,
            recent_errors: vec![],
            fallback,
            incomplete: false,
        }
    }
    pub fn summary(&self, active: bool) -> String {
        format!("SoDeck 本地诊断\n应用版本: {}\n构建: {}\n系统: {} {} {}\n运行编号: {}\n开始: {}\n摘要截止: {} (seq {})\n状态: {}\n警告: {} / 错误: {} / 丢弃: {} / 已清理分片: {}\n记录不完整: {}\n临时目录降级: {}\n\n最近错误（完整上下文请按 traceId 查看 events）:\n{}\n",self.version,self.build_id,self.os,self.arch,self.system_version,self.run_id,timestamp(self.started_at),timestamp(self.updated_at),self.last_seq,if self.ended_at.is_some(){"正常结束"}else if active{"运行中；手工复制后此状态可能过期"}else{"未确认正常结束"},self.warnings,self.errors,self.dropped,self.removed_segments,self.incomplete||self.dropped>0||self.removed_segments>0,self.fallback,serde_json::to_string_pretty(&self.recent_errors).unwrap_or_default())
    }
}
fn system_version() -> String {
    #[cfg(windows)]
    {
        let key = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "ver"])
            .creation_flags_hidden()
            .output();
        return key
            .ok()
            .map(|v| {
                String::from_utf8_lossy(&v.stdout)
                    .chars()
                    .filter(|c| c.is_ascii_digit() || ". ".contains(*c))
                    .take(64)
                    .collect()
            })
            .unwrap_or_else(|| "unavailable".into());
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .map(|v| {
                String::from_utf8_lossy(&v.stdout)
                    .trim()
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || ".-_".contains(*c))
                    .take(64)
                    .collect()
            })
            .unwrap_or_else(|| "unavailable".into())
    }
}
#[cfg(windows)]
trait Hidden {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}
#[cfg(windows)]
impl Hidden for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x08000000)
    }
}
pub struct Run {
    pub path: PathBuf,
    pub session: Session,
    pub active: bool,
}
pub fn runs(root: &Path) -> io::Result<Vec<Run>> {
    let mut out = vec![];
    for entry in fs::read_dir(root.join("runs"))? {
        let path = entry?.path();
        if !directory(&path) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if name.len() > 80
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            continue;
        }
        let meta = path.join("session.json");
        if !regular(&meta) || fs::metadata(&meta)?.len() > 65536 {
            continue;
        }
        let Ok(session) = serde_json::from_slice::<Session>(&fs::read(meta)?) else {
            continue;
        };
        if session.directory != name || uuid::Uuid::parse_str(&session.run_id).is_err() {
            continue;
        }
        let active = match open(&path.join(".active.lock")) {
            Ok(file) => file.try_lock().is_err(),
            Err(_) => true,
        };
        out.push(Run {
            path,
            session,
            active,
        });
    }
    out.sort_by_key(|r| r.session.started_at);
    Ok(out)
}
pub fn files(path: &Path) -> io::Result<Vec<PathBuf>> {
    if !directory(path) {
        return Err(io::Error::other("invalid diagnostics directory"));
    }
    let mut out = vec![];
    for e in fs::read_dir(path)? {
        let path = e?.path();
        if regular(&path) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
pub fn allowed(name: &str) -> bool {
    matches!(
        name,
        "summary.txt" | "session.json" | "errors.jsonl" | "panic.jsonl"
    ) || (name.starts_with("events-")
        && name.ends_with(".jsonl")
        && name[7..name.len() - 6].bytes().all(|c| c.is_ascii_digit()))
}
pub fn usage(root: &Path) -> io::Result<u64> {
    let mut size = 0;
    for p in files(root)? {
        size += fs::metadata(p)?.len();
    }
    for e in fs::read_dir(root.join("runs"))? {
        let p = e?.path();
        if directory(&p) {
            for file in files(&p)? {
                size += fs::metadata(file)?.len();
            }
        }
    }
    Ok(size)
}
pub fn write_metadata(root: &Path, path: &Path, session: &Session) -> io::Result<()> {
    atomic(
        &path.join("session.json"),
        &serde_json::to_vec_pretty(session)?,
    )?;
    atomic(
        &path.join("summary.txt"),
        session.summary(session.ended_at.is_none()).as_bytes(),
    )?;
    let recent:Vec<_>=runs(root)?.iter().rev().take(100).map(|r|json!({"runId":r.session.run_id,"directory":format!("runs/{}",r.session.directory),"startedAt":r.session.started_at,"active":r.active,"endedAt":r.session.ended_at})).collect();
    atomic(
        &root.join("latest.json"),
        &serde_json::to_vec_pretty(&recent)?,
    )?;
    Ok(())
}
/// Caller holds maintenance lock. Never follows links or deletes unknown files.
pub fn remove_run(run: &Run) -> io::Result<()> {
    if run.active {
        return Err(io::Error::other("active run"));
    }
    for p in files(&run.path)? {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if allowed(name) || name == ".active.lock" {
            fs::remove_file(p)?;
        }
    }
    let _ = fs::remove_dir(&run.path);
    Ok(())
}
pub fn maintain(root: &Path, current: &Path, segment: &Path, needed: u64) -> io::Result<u64> {
    let list = runs(root)?;
    // Reserve one bounded emergency channel per live process, including the
    // process whose metadata has not been created yet.
    let reserve = RESERVE + list.iter().filter(|r| r.active).count() as u64 * 256 * 1024;
    let mut used = usage(root)?;
    let mut removed = 0;
    for run in &list {
        if !run.active
            && (now() - run.session.updated_at > RETENTION || used + needed + reserve > LIMIT)
        {
            remove_run(run)?;
            used = usage(root)?;
        }
    }
    for p in files(current)? {
        if p != segment
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("events-") && allowed(n))
        {
            let meta = fs::metadata(&p)?;
            let expired = meta
                .modified()
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age.as_millis() > RETENTION as u128);
            if !expired && used + needed + reserve <= LIMIT {
                continue;
            }
            fs::remove_file(p)?;
            used = used.saturating_sub(meta.len());
            removed += 1;
        }
    }
    if used + needed + reserve > LIMIT {
        return Err(io::Error::other("diagnostics capacity reached"));
    }
    Ok(removed)
}
