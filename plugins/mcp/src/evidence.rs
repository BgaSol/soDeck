use serde_json::Value;
use sodeck_plugin_client::encoding::unbase64;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

fn decode(chunk: &Value) -> Result<Vec<u8>, String> {
    if chunk["v"] != 1
        || !["stdout", "stderr"].contains(&chunk["source"].as_str().unwrap_or(""))
        || chunk["sequence"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .is_none()
        || !chunk["receivedAt"].is_u64()
    {
        return Err("输出信封无效".into());
    }
    unbase64(chunk["dataBase64"].as_str().ok_or("输出编码缺失")?)
}

pub fn metadata(directory: &Path, value: &Value) -> Result<(), String> {
    let temporary = directory.join("metadata.tmp");
    let mut file = File::create(&temporary).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temporary, directory.join("metadata.json")).map_err(|e| e.to_string())
}
pub struct Output {
    file: File,
    pub committed: u64,
}
impl Output {
    pub fn new(directory: &Path) -> Result<Self, String> {
        Ok(Self {
            file: OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join("output.jsonl"))
                .map_err(|e| e.to_string())?,
            committed: 0,
        })
    }
    pub fn append(&mut self, chunks: &[Value]) -> Result<(), String> {
        for chunk in chunks {
            decode(chunk)?;
            let mut bytes = serde_json::to_vec(chunk).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            if bytes.len() > 16 * 1024 {
                return Err("核心输出块超限".into());
            }
            self.file.write_all(&bytes).map_err(|e| e.to_string())?;
            self.committed += bytes.len() as u64;
        }
        Ok(())
    }
    pub fn sync(&self) -> Result<(), String> {
        self.file.sync_all().map_err(|e| e.to_string())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PageFormat {
    Text,
    Full,
}
#[derive(Debug, PartialEq)]
pub enum PageBody {
    Text { stdout: String, stderr: String },
    Full { chunks: Vec<Value> },
}
#[derive(Debug, PartialEq)]
pub struct Page {
    pub body: PageBody,
    pub next_cursor: u64,
    pub saved_bytes: u64,
    pub has_more: bool,
}
impl Page {
    fn empty(format: PageFormat) -> Self {
        Self {
            body: match format {
                PageFormat::Text => PageBody::Text {
                    stdout: String::new(),
                    stderr: String::new(),
                },
                PageFormat::Full => PageBody::Full { chunks: Vec::new() },
            },
            next_cursor: 0,
            saved_bytes: 0,
            has_more: false,
        }
    }
}
#[derive(Debug)]
pub enum ReadError {
    InvalidCursor(String),
    EvidenceReadFailed(String),
}
impl From<std::io::Error> for ReadError {
    fn from(e: std::io::Error) -> Self {
        Self::EvidenceReadFailed(e.to_string())
    }
}
impl From<String> for ReadError {
    fn from(e: String) -> Self {
        Self::EvidenceReadFailed(e)
    }
}
impl From<serde_json::Error> for ReadError {
    fn from(e: serde_json::Error) -> Self {
        Self::EvidenceReadFailed(e.to_string())
    }
}
pub fn page(
    path: PathBuf,
    committed: u64,
    offset: u64,
    format: PageFormat,
) -> Result<Page, ReadError> {
    if offset > committed {
        return Err(ReadError::InvalidCursor("cursor 超出已保存范围".into()));
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if committed == 0 && e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Page::empty(format))
        }
        Err(e) => return Err(e.into()),
    };
    if offset > 0 {
        file.seek(SeekFrom::Start(offset - 1))?;
        let mut previous = [0];
        file.read_exact(&mut previous)?;
        if previous[0] != b'\n' {
            return Err(ReadError::InvalidCursor("cursor 不在完整行边界".into()));
        }
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file.take(committed - offset));
    let mut cursor = offset;
    let mut chunks = Vec::new();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    loop {
        let mut line = Vec::new();
        let size = reader
            .by_ref()
            .take(16 * 1024 + 1)
            .read_until(b'\n', &mut line)?;
        if size == 0 {
            if cursor != committed {
                return Err(ReadError::EvidenceReadFailed(
                    "证据文件短于已保存水位".into(),
                ));
            }
            break;
        }
        if size > 16 * 1024 || !line.ends_with(b"\n") {
            return Err(ReadError::EvidenceReadFailed("证据行不完整或超限".into()));
        }
        if cursor - offset + size as u64 > 16 * 1024 && cursor > offset {
            break;
        }
        let chunk: Value = serde_json::from_slice(&line)?;
        let bytes = decode(&chunk)?;
        match format {
            PageFormat::Text => {
                if chunk["source"] == "stderr" {
                    stderr.extend(bytes);
                } else {
                    stdout.extend(bytes);
                }
            }
            PageFormat::Full => chunks.push(chunk),
        }
        cursor += size as u64;
    }
    Ok(Page {
        body: match format {
            PageFormat::Text => PageBody::Text {
                stdout: String::from_utf8_lossy(&stdout).into(),
                stderr: String::from_utf8_lossy(&stderr).into(),
            },
            PageFormat::Full => PageBody::Full { chunks },
        },
        next_cursor: cursor,
        saved_bytes: committed,
        has_more: cursor < committed,
    })
}
pub fn tail(path: PathBuf, committed: u64, format: PageFormat) -> Result<Page, ReadError> {
    let mut offset = committed.saturating_sub(16 * 1024);
    if offset > 0 {
        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(offset - 1))?;
        let mut reader = BufReader::new(file.take(committed - offset + 1));
        let mut skipped = Vec::new();
        let count = reader.read_until(b'\n', &mut skipped)?;
        offset = offset - 1 + count as u64;
    }
    page(path, committed, offset, format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pages_preserve_bytes_watermark_and_format_and_classify_failures() {
        let root = std::env::temp_dir().join(format!("mcp-evidence-{}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("output.jsonl");
        assert_eq!(
            page(path.clone(), 0, 0, PageFormat::Full).unwrap(),
            Page::empty(PageFormat::Full)
        );
        let mut output = Output::new(&root).unwrap();
        let raw = [0, 255, 0xe4, 0xb8];
        output.append(&[json!({"v":1,"sequence":"1","source":"stdout","receivedAt":1,"dataBase64":sodeck_plugin_client::encoding::base64(&raw)})]).unwrap();
        let committed = output.committed;
        output.file.write_all(b"partial").unwrap();
        let a = page(path.clone(), committed, 0, PageFormat::Full).unwrap();
        assert_eq!(
            a,
            page(path.clone(), committed, 0, PageFormat::Full).unwrap()
        );
        let PageBody::Full { chunks } = &a.body else {
            panic!()
        };
        assert_eq!(
            unbase64(chunks[0]["dataBase64"].as_str().unwrap()).unwrap(),
            raw
        );
        let text = page(path.clone(), committed, 0, PageFormat::Text).unwrap();
        let PageBody::Text { stdout, stderr } = text.body else {
            panic!()
        };
        assert_eq!(stdout, String::from_utf8_lossy(&raw));
        assert!(stderr.is_empty());
        assert_eq!(a.next_cursor, text.next_cursor);
        assert_eq!(a.saved_bytes, text.saved_bytes);
        assert!(matches!(
            page(path.clone(), committed, 1, PageFormat::Text),
            Err(ReadError::InvalidCursor(_))
        ));
        metadata(&root, &json!({"done":false})).unwrap();
        metadata(&root, &json!({"done":true})).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(root.join("metadata.json")).unwrap())
                .unwrap()["done"],
            true
        );
        output.file = File::open(&path).unwrap();
        assert!(output
            .append(&[
                json!({"v":1,"sequence":"2","source":"stdout","receivedAt":2,"dataBase64":"YQ=="})
            ])
            .is_err());
        assert_eq!(output.committed, committed);
        drop(output);
        fs::write(&path, b"").unwrap();
        assert!(matches!(
            page(path.clone(), committed, 0, PageFormat::Full),
            Err(ReadError::EvidenceReadFailed(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tail_uses_recent_complete_envelopes_and_excludes_uncommitted_bytes() {
        let root = std::env::temp_dir().join(format!("mcp-tail-{}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let path = root.join("output.jsonl");
        let mut output = Output::new(&root).unwrap();
        for n in 1..=20 {
            let text = format!("line-{n} {}\n", "x".repeat(2000));
            output.append(&[json!({"v":1,"sequence":n.to_string(),"source":"stdout","receivedAt":n,"dataBase64":sodeck_plugin_client::encoding::base64(text.as_bytes())})]).unwrap();
        }
        output.file.write_all(b"uncommitted").unwrap();
        let page = tail(path, output.committed, PageFormat::Text).unwrap();
        let PageBody::Text { stdout, .. } = page.body else {
            panic!()
        };
        assert!(stdout.contains("line-20 "));
        assert!(!stdout.contains("line-1 "));
        assert!(!stdout.contains("uncommitted"));
        assert_eq!(page.next_cursor, output.committed);
        assert!(!page.has_more);
        drop(output);
        fs::remove_dir_all(root).unwrap();
    }
}
