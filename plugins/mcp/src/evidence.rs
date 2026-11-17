use serde_json::{json, Value};
use sodeck_plugin_client::encoding::unbase64;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

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
            if chunk["v"] != 1
                || !["stdout", "stderr"].contains(&chunk["source"].as_str().unwrap_or(""))
                || chunk["sequence"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .is_none()
                || !chunk["receivedAt"].is_u64()
            {
                return Err("核心输出块无效".into());
            }
            unbase64(chunk["dataBase64"].as_str().ok_or("核心输出编码缺失")?)?;
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
pub fn page(path: PathBuf, committed: u64, offset: u64) -> Result<Value, String> {
    if offset > committed {
        return Err("cursor 超出已保存范围".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset - 1))
            .map_err(|e| e.to_string())?;
        let mut previous = [0];
        file.read_exact(&mut previous).map_err(|e| e.to_string())?;
        if previous[0] != b'\n' {
            return Err("cursor 不在完整行边界".into());
        }
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file.take(committed - offset));
    let mut cursor = offset;
    let mut chunks = Vec::new();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    loop {
        let mut line = Vec::new();
        let size = reader
            .by_ref()
            .take(16 * 1024 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        if size > 16 * 1024 || !line.ends_with(b"\n") {
            return Err("证据行不完整或超限".into());
        }
        if cursor - offset + size as u64 > 16 * 1024 && !chunks.is_empty() {
            break;
        }
        let chunk: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
        let bytes = unbase64(chunk["dataBase64"].as_str().ok_or("证据编码缺失")?)?;
        if chunk["source"] == "stderr" {
            stderr.extend(bytes);
        } else {
            stdout.extend(bytes);
        }
        chunks.push(chunk);
        cursor += size as u64;
    }
    Ok(
        json!({"chunks":chunks,"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"nextCursor":cursor.to_string(),"savedBytes":committed.to_string(),"hasMore":cursor<committed,"previewEncoding":"utf-8-lossy-per-page"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_repeat_preserve_bytes_and_exclude_uncommitted_tail() {
        let root = std::env::temp_dir().join(format!("mcp-evidence-{}", rand::random::<u64>()));
        fs::create_dir(&root).unwrap();
        let mut output = Output::new(&root).unwrap();
        let raw = [0, 255, 0xe4, 0xb8];
        output.append(&[json!({"v":1,"sequence":"1","source":"stdout","receivedAt":1,"dataBase64":sodeck_plugin_client::encoding::base64(&raw)})]).unwrap();
        let committed = output.committed;
        output.file.write_all(b"partial").unwrap();
        let a = page(root.join("output.jsonl"), committed, 0).unwrap();
        assert_eq!(a, page(root.join("output.jsonl"), committed, 0).unwrap());
        assert_eq!(
            unbase64(a["chunks"][0]["dataBase64"].as_str().unwrap()).unwrap(),
            raw
        );
        assert!(page(root.join("output.jsonl"), committed, 1).is_err());
        metadata(&root, &json!({"done":false})).unwrap();
        metadata(&root, &json!({"done":true})).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(root.join("metadata.json")).unwrap())
                .unwrap()["done"],
            true
        );
        output.file = File::open(root.join("output.jsonl")).unwrap();
        assert!(output
            .append(&[
                json!({"v":1,"sequence":"2","source":"stdout","receivedAt":2,"dataBase64":"YQ=="})
            ])
            .is_err());
        assert_eq!(
            output.committed, committed,
            "failed OS writes do not advance the saved boundary"
        );
        drop(output);
        fs::remove_dir_all(root).unwrap();
    }
}
