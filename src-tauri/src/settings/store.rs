use super::declaration::Page;
use crate::storage::write_json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};

const FILE_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Document {
    pub pages: BTreeMap<String, StoredPage>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredPage {
    pub token: String,
    pub revision: u64,
    pub structure: Value,
    pub values: Map<String, Value>,
    pub history: Vec<History>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct History {
    pub structure: Value,
    pub values: Map<String, Value>,
}
#[derive(Clone)]
pub struct Receipt {
    pub key: String,
    pub request: String,
    pub body: Value,
    pub result: Value,
}
pub struct SettingsStore {
    path: PathBuf,
    pub document: Document,
    pub error: Option<String>,
    corrupt: bool,
    blocked: bool,
    pub receipts: VecDeque<Receipt>,
    pub applications: BTreeMap<String, Value>,
}
fn read(path: &Path) -> Result<Document, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > FILE_BYTES {
        return Err("组件设置文件超过 4 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
impl SettingsStore {
    pub fn open(root: &Path) -> Self {
        let path = root.join("component-settings.json");
        let mut s = Self {
            path,
            document: Document::default(),
            error: None,
            corrupt: false,
            blocked: false,
            receipts: VecDeque::new(),
            applications: BTreeMap::new(),
        };
        match read(&s.path) {
            Ok(doc) => s.document = doc,
            Err(_) if !s.path.exists() => {}
            Err(e) => {
                s.corrupt = true;
                s.error = Some(format!("组件设置无法读取，原文件已保留：{e}"));
                match read(&s.path.with_extension("json.bak")) {
                    Ok(doc) => {
                        s.document = doc;
                        s.error = Some(format!("组件设置已从备份恢复，损坏原文件保留：{e}"));
                    }
                    Err(_) => s.blocked = true,
                }
            }
        }
        s
    }
    pub fn commit(&mut self, doc: Document) -> Result<(), String> {
        if self.blocked {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "组件设置处于只读状态".into()));
        }
        let result: Result<(), String> = (|| {
            if serde_json::to_vec_pretty(&doc)
                .map_err(|e| e.to_string())?
                .len()
                + 1
                > FILE_BYTES
            {
                return Err("组件设置文件超过 4 MiB，原设置已保留".into());
            }
            if self.corrupt {
                let copy = self
                    .path
                    .with_extension(format!("json.corrupt-{}", crate::core::now()));
                fs::copy(&self.path, &copy).map_err(|e| format!("无法保留损坏设置文件：{e}"))?;
            }
            let backup = self.path.with_extension("json.bak");
            // Serialize the previous known-good memory snapshot, never the corrupt file.
            write_json(&backup, &self.document)?;
            write_json(&self.path, &doc)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.document = doc;
                self.error = None;
                self.corrupt = false;
                Ok(())
            }
            Err(e) => {
                self.error = Some(e.clone());
                Err(e)
            }
        }
    }
    pub fn reconcile(&mut self, key: &str, page: &Page, token: &str) -> Result<StoredPage, String> {
        let mut saved = self.document.pages.get(key).cloned().unwrap_or_default();
        if saved.token == token {
            return Ok(saved);
        }
        let structure = page.structure();
        if !saved.token.is_empty() && saved.structure != structure {
            let old = History {
                structure: saved.structure.clone(),
                values: saved.values.clone(),
            };
            saved.history.retain(|h| h.structure != old.structure);
            saved.history.insert(0, old);
            let mut values = Map::new();
            for field in page.fields() {
                let candidate = if saved.structure[&field.key] == structure[&field.key] {
                    saved.values.get(&field.key)
                } else {
                    saved
                        .history
                        .iter()
                        .find(|h| h.structure[&field.key] == structure[&field.key])
                        .and_then(|h| h.values.get(&field.key))
                };
                if let Some(value) = candidate {
                    values.insert(field.key.clone(), value.clone());
                }
            }
            saved.values = values;
            saved.history.truncate(2);
        }
        saved.token = token.into();
        saved.structure = structure;
        saved.revision = saved.revision.checked_add(1).ok_or("设置修订已耗尽")?;
        let mut doc = self.document.clone();
        doc.pages.insert(key.into(), saved.clone());
        self.commit(doc)?;
        Ok(saved)
    }
    pub fn remember(&mut self, receipt: Receipt) {
        self.receipts.push_back(receipt);
        while self.receipts.len() > 128 {
            self.receipts.pop_front();
        }
    }
}
