//! Bounded command output, with a rolling tail and explicit immutable reading ranges.
use super::journal::Journal;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
};

pub(crate) struct Capture {
    journal: Journal,
    blocks: VecDeque<(u64, u64)>,
    pins: BTreeMap<String, (u64, u64)>,
    pub produced: u64,
    pub earliest: u64,
    capacity: u64,
    rolling: bool,
}
impl Capture {
    pub fn new(path: PathBuf, capacity: usize, rolling: bool) -> Result<Self, String> {
        Ok(Self {
            journal: Journal::new(path)?,
            blocks: VecDeque::new(),
            pins: BTreeMap::new(),
            produced: 0,
            earliest: 0,
            capacity: capacity as u64,
            rolling,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64, String> {
        if bytes.len() as u64 > self.capacity {
            return Err("outputLimit: 单个输出块超过容量".into());
        }
        if !self.rolling && self.produced + bytes.len() as u64 > self.capacity {
            return Err("outputLimit: 固定输出达到容量".into());
        }
        let mut new_start = self.earliest;
        if self.rolling {
            let required = self.produced + bytes.len() as u64;
            new_start = self
                .blocks
                .iter()
                .find(|(a, _)| required - a <= self.capacity)
                .map(|(a, _)| *a)
                .unwrap_or(self.produced);
        }
        self.journal.append(self.produced, bytes)?;
        let start = self.produced;
        self.produced += bytes.len() as u64;
        self.blocks.push_back((start, self.produced));
        self.earliest = new_start;
        self.prune();
        Ok(start)
    }
    fn prune(&mut self) {
        let mut ranges: Vec<_> = self.pins.values().copied().collect();
        ranges.push((self.earliest, self.produced));
        self.blocks
            .retain(|(a, b)| ranges.iter().any(|(x, y)| *a < *y && *b > *x));
        self.journal.retain_ranges(&ranges);
    }
    pub fn pin(&mut self, id: String, start: u64, end: u64) -> Result<Value, String> {
        if start >= end || end > self.produced || !self.available(start, end) {
            return Err("读取窗口已退出缓冲".into());
        }
        if !self.rolling {
            return Ok(json!({"pinId":id,"start":start.to_string(),"end":end.to_string()}));
        }
        let mut ranges: Vec<_> = self.pins.values().copied().collect();
        ranges.push((start, end));
        ranges.sort_unstable();
        let mut total = 0;
        let mut last = 0;
        for (a, b) in ranges {
            total += b.saturating_sub(a.max(last));
            last = last.max(b);
        }
        if total > 64 * 1024 * 1024 || self.pins.len() >= 8 {
            return Err("固定阅读窗口达到容量".into());
        }
        self.pins.insert(id.clone(), (start, end));
        Ok(json!({"pinId":id,"start":start.to_string(),"end":end.to_string()}))
    }
    pub fn unpin(&mut self, id: &str) {
        self.pins.remove(id);
        self.prune();
    }
    pub fn available(&self, start: u64, end: u64) -> bool {
        let mut cursor = start;
        for (a, b) in &self.blocks {
            if *b <= cursor {
                continue;
            }
            if *a > cursor {
                return false;
            }
            cursor = *b;
            if cursor >= end {
                return true;
            }
        }
        start == end && end <= self.produced
    }
    pub fn read(&self, start: u64, length: usize, watermark: u64) -> Result<Vec<u8>, String> {
        if watermark > self.produced || start > watermark {
            return Err("输出水位无效".into());
        }
        let mut end = start;
        for (a, b) in &self.blocks {
            if *b <= end {
                continue;
            }
            if *a > end {
                break;
            }
            end = *b;
        }
        let amount = length
            .min(16 * 1024)
            .min(watermark.saturating_sub(start) as usize)
            .min(end.saturating_sub(start) as usize);
        if start < watermark && amount == 0 {
            return Err("bufferGap: 输出已退出保留窗口".into());
        }
        if !self.available(start, start + amount as u64) {
            return Err("bufferGap: 输出已退出保留窗口".into());
        }
        self.journal.read(start, amount)
    }
    pub fn boundary(&self, start: u64) -> bool {
        start == self.produced || self.blocks.iter().any(|(a, _)| *a == start)
    }
    pub fn pinned_bytes(&self) -> u64 {
        self.pins.values().map(|(a, b)| b - a).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_keeps_pins_and_reports_gaps() {
        let path = std::env::temp_dir().join(format!(
            "sodeck-capture-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        let mut c = Capture::new(path, 12, true).unwrap();
        c.append(b"aaaaaa").unwrap();
        c.pin("read".into(), 0, 6).unwrap();
        c.append(b"bbbbbb").unwrap();
        c.append(b"cccccc").unwrap();
        assert_eq!(c.earliest, 6);
        assert_eq!(c.read(0, 6, 18).unwrap(), b"aaaaaa");
        c.unpin("read");
        assert!(c.read(0, 6, 18).is_err());
        assert_eq!(c.read(6, 12, 18).unwrap(), b"bbbbbbcccccc");
    }
    #[test]
    fn fixed_never_reports_unaccepted_tail() {
        let path = std::env::temp_dir().join(format!(
            "sodeck-fixed-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        let mut c = Capture::new(path, 6, false).unwrap();
        c.append(b"abcdef").unwrap();
        assert!(c.append(b"g").is_err());
        assert_eq!(c.produced, 6);
        assert_eq!(c.read(0, 9, 6).unwrap(), b"abcdef");
    }
}
