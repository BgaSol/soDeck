//! Temporary, bounded recovery bytes. The terminal plugin owns display interpretation.
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
const SEGMENT: u64 = 1024 * 1024;
const LIMIT: usize = 128 * 1024 * 1024;
const GLOBAL: usize = 1024 * 1024 * 1024;
static USED: AtomicUsize = AtomicUsize::new(0);
pub(super) fn reserve(bytes: usize) -> Result<(), String> {
    USED.try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
        n.checked_add(bytes).filter(|v| *v <= GLOBAL)
    })
    .map(|_| ())
    .map_err(|_| "终端恢复资源总额度已用完".into())
}
pub(super) fn release(bytes: usize) {
    USED.fetch_sub(bytes, Ordering::AcqRel);
}

pub(super) struct Journal {
    root: PathBuf,
    segments: BTreeMap<u64, usize>,
    size: usize,
}
impl Journal {
    pub(super) fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| format!("无法创建终端恢复目录：{e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            root,
            segments: BTreeMap::new(),
            size: 0,
        })
    }
    pub(super) fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<(), String> {
        for (start, data) in split(offset, bytes) {
            let segment = start / SEGMENT;
            let previous = *self.segments.get(&segment).unwrap_or(&0);
            let next = previous.max((start % SEGMENT) as usize + data.len());
            let extra = next - previous;
            if self.size + extra > LIMIT
                || USED
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                        n.checked_add(extra).filter(|v| *v <= GLOBAL)
                    })
                    .is_err()
            {
                return Err("终端恢复存储额度已用完".into());
            }
            let result = (|| {
                let mut options = OpenOptions::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(self.root.join(segment.to_string()))?;
                file.seek(SeekFrom::Start(start % SEGMENT))?;
                file.write_all(data)
            })();
            if let Err(e) = result {
                let actual = fs::metadata(self.root.join(segment.to_string()))
                    .map(|m| m.len() as usize)
                    .unwrap_or(previous)
                    .min(next);
                let retained = actual.saturating_sub(previous);
                self.size += retained;
                if actual > 0 {
                    self.segments.insert(segment, actual);
                }
                USED.fetch_sub(extra - retained, Ordering::AcqRel);
                return Err(format!("无法写入终端恢复数据：{e}"));
            }
            self.segments.insert(segment, next);
            self.size += extra;
        }
        Ok(())
    }
    pub(super) fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; length];
        let mut position = 0;
        while position < length {
            let start = offset + position as u64;
            let amount = (length - position).min((SEGMENT - start % SEGMENT) as usize);
            let mut file = OpenOptions::new()
                .read(true)
                .open(self.root.join((start / SEGMENT).to_string()))
                .map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(start % SEGMENT))
                .map_err(|e| e.to_string())?;
            file.read_exact(&mut bytes[position..position + amount])
                .map_err(|e| e.to_string())?;
            position += amount;
        }
        Ok(bytes)
    }
    pub(super) fn prune(&mut self, offset: u64) {
        let old: Vec<_> = self
            .segments
            .keys()
            .copied()
            .take_while(|n| (n + 1) * SEGMENT <= offset)
            .collect();
        for key in old {
            if fs::remove_file(self.root.join(key.to_string())).is_ok() {
                if let Some(size) = self.segments.remove(&key) {
                    self.size -= size;
                    USED.fetch_sub(size, Ordering::AcqRel);
                }
            }
        }
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        USED.fetch_sub(self.size, Ordering::AcqRel);
    }
}
fn split(mut offset: u64, mut bytes: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    std::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }
        let length = bytes.len().min((SEGMENT - offset % SEGMENT) as usize);
        let item = (offset, &bytes[..length]);
        offset += length as u64;
        bytes = &bytes[length..];
        Some(item)
    })
}
