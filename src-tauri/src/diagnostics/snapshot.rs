use super::{
    event::{now, timestamp},
    store, Diagnostics,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Recent,
    Current,
    All,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    bytes: u64,
    sha256: String,
    #[serde(skip)]
    path: PathBuf,
}
pub struct Snapshot {
    temp: tempfile::TempDir,
    entries: Vec<Entry>,
    pub warnings: Vec<String>,
    cutoff: i64,
    coverage: Vec<serde_json::Value>,
}
pub fn capture(root: &Path, current: &store::Session, scope: Scope) -> io::Result<Snapshot> {
    let staging = root.join(".snapshots");
    store::checked_dir(&staging)?;
    let temp = tempfile::Builder::new()
        .prefix("snapshot-")
        .tempdir_in(staging)?;
    let mut snapshot = Snapshot {
        temp,
        entries: vec![],
        warnings: vec![],
        cutoff: now(),
        coverage: vec![],
    };
    snapshot.add("README.txt".into(), store::README.as_bytes())?;
    let runs = store::runs(root)?;
    let last_abnormal = runs
        .iter()
        .rev()
        .find(|r| !r.active && r.session.ended_at.is_none())
        .map(|r| r.session.run_id.clone());
    for run in runs {
        let include = match scope {
            Scope::Current => run.session.run_id == current.run_id,
            Scope::All => true,
            Scope::Recent => {
                run.session.updated_at >= now() - 24 * 60 * 60 * 1000
                    || Some(&run.session.run_id) == last_abnormal.as_ref()
            }
        };
        if !include {
            continue;
        }
        let prefix = format!("runs/{}/", run.session.directory);
        if run.session.incomplete || run.session.dropped > 0 || run.session.removed_segments > 0 {
            snapshot
                .warnings
                .push(format!("{}: 部分事件已丢弃或过期清理", run.session.run_id));
        }
        if run.active && run.session.run_id != current.run_id {
            snapshot.warnings.push(format!(
                "{}: 其他运行实例的内存缓冲不保证已提交",
                run.session.run_id
            ));
        }
        snapshot.add(
            format!("{prefix}session.json"),
            &serde_json::to_vec_pretty(&run.session)?,
        )?;
        snapshot.add(
            format!("{prefix}summary.txt"),
            run.session.summary(run.active).as_bytes(),
        )?;
        let mut sequences = vec![];
        for path in store::files(&run.path)? {
            let name = path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default();
            if !store::allowed(name) || matches!(name, "session.json" | "summary.txt") {
                continue;
            }
            let length = fs::metadata(&path)?.len();
            if length > store::SEGMENT + 65536 {
                return Err(io::Error::other("诊断分片超过大小限制"));
            }
            let mut bytes = vec![];
            File::open(&path)?.take(length).read_to_end(&mut bytes)?;
            if bytes.last().is_some_and(|v| *v != b'\n') {
                snapshot
                    .warnings
                    .push(format!("{}{}: 已忽略不完整末行", prefix, name));
                if let Some(p) = bytes.iter().rposition(|v| *v == b'\n') {
                    bytes.truncate(p + 1);
                } else {
                    bytes.clear();
                }
            }
            if name.starts_with("events-") || name == "errors.jsonl" {
                #[derive(Deserialize)]
                struct Position {
                    seq: u64,
                }
                let mut bounded = Vec::with_capacity(bytes.len());
                let mut omitted = false;
                for line in bytes.split_inclusive(|b| *b == b'\n') {
                    match serde_json::from_slice::<Position>(line) {
                        Ok(record) if record.seq <= run.session.last_seq => {
                            bounded.extend_from_slice(line);
                            if name.starts_with("events-") {
                                sequences.push(record.seq);
                            }
                        }
                        _ => omitted = true,
                    }
                }
                if omitted {
                    snapshot.warnings.push(format!(
                        "{prefix}{name}: 已忽略无效记录或摘要截止序号之后的记录"
                    ));
                }
                bytes = bounded;
            }
            if name == "panic.jsonl" && !bytes.is_empty() {
                snapshot.warnings.push(format!(
                    "{}: 应急记录独立于正常队列，需结合记录时间判断",
                    run.session.run_id
                ));
            }
            snapshot.add(format!("{prefix}{name}"), &bytes)?;
        }
        sequences.sort_unstable();
        sequences.dedup();
        let mut next = 1;
        let mut missing = vec![];
        for &seq in &sequences {
            if seq > next {
                missing.push([next, seq - 1]);
            }
            next = seq.saturating_add(1);
        }
        if next <= run.session.last_seq {
            missing.push([next, run.session.last_seq]);
        }
        if !missing.is_empty() {
            snapshot.warnings.push(format!(
                "{}: 日志序号存在缺口，见 coverage.missingRanges",
                run.session.run_id
            ));
        }
        snapshot.coverage.push(json!({"runId":run.session.run_id,"startedAt":timestamp(run.session.started_at),"metadataCutoffAt":timestamp(run.session.updated_at),"snapshotLastSeq":run.session.last_seq,"firstAvailableSeq":sequences.first(),"lastAvailableSeq":sequences.last(),"missingRanges":missing,"dropped":run.session.dropped,"removedSegments":run.session.removed_segments}));
    }
    Ok(snapshot)
}
impl Snapshot {
    fn add(&mut self, name: String, bytes: &[u8]) -> io::Result<()> {
        if self.entries.iter().map(|e| e.bytes).sum::<u64>() + bytes.len() as u64 > store::LIMIT {
            return Err(io::Error::other("所选日志超过 100 MiB，请缩小范围"));
        }
        let path = self
            .temp
            .path()
            .join(format!("{:05}.bin", self.entries.len()));
        fs::write(&path, bytes)?;
        self.entries.push(Entry {
            name,
            bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            path,
        });
        Ok(())
    }
    pub fn zip(
        mut self,
        destination: &Path,
        service: &Diagnostics,
        frontend_warnings: Vec<String>,
    ) -> io::Result<usize> {
        self.warnings.extend(frontend_warnings);
        let mut output = tempfile::NamedTempFile::new_in(
            destination
                .parent()
                .ok_or_else(|| io::Error::other("无效保存位置"))?,
        )?;
        let total = self.entries.iter().map(|v| v.bytes).sum();
        let mut completed = 0;
        {
            let mut writer = zip::ZipWriter::new(output.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o600);
            for entry in &self.entries {
                if service.cancelled.load(Ordering::Relaxed) {
                    return Err(io::Error::other("日志打包已取消"));
                }
                writer.start_file(&entry.name, options)?;
                let mut source = File::open(&entry.path)?;
                let mut buffer = [0; 65536];
                loop {
                    let count = source.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    if service.cancelled.load(Ordering::Relaxed) {
                        return Err(io::Error::other("日志打包已取消"));
                    }
                    writer.write_all(&buffer[..count])?;
                    completed += count as u64;
                    service.progress("compressing", completed, total);
                }
            }
            writer.start_file("manifest.json", options)?;
            writer.write_all(&serde_json::to_vec_pretty(&json!({"schemaVersion":1,"createdAt":timestamp(now()),"cutoffAt":timestamp(self.cutoff),"timezone":"UTC","incomplete":!self.warnings.is_empty(),"warnings":self.warnings,"coverage":self.coverage,"files":self.entries}))?)?;
            writer.finish()?;
        }
        output.as_file().sync_all()?;
        if service.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("日志打包已取消"));
        }
        output.persist(destination).map_err(|e| e.error)?;
        Ok(self.warnings.len())
    }
}
pub fn cleanup(root: &Path) -> io::Result<()> {
    let staging = root.join(".snapshots");
    if !store::directory(&staging) {
        return Ok(());
    }
    for e in fs::read_dir(&staging)? {
        let p = e?.path();
        if !store::directory(&p)
            || !p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("snapshot-"))
        {
            continue;
        }
        for file in store::files(&p)? {
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.ends_with(".bin") && name[..name.len() - 4].bytes().all(|c| c.is_ascii_digit())
            {
                fs::remove_file(file)?;
            }
        }
        let _ = fs::remove_dir(p);
    }
    Ok(())
}
