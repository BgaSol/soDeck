use super::model::*;
use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub identity: String,
    pub target: String,
    pub servers: Vec<String>,
    pub signature: String,
    pub binding_token: String,
    pub preview_id: String,
    pub invocation: String,
    pub returned: bool,
    pub operation: Operation,
}
pub struct Store(Connection);
impl Store {
    pub fn open(path: &Path) -> AppResult<Self> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS settings(server TEXT PRIMARY KEY,path TEXT NOT NULL); CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY, identity TEXT NOT NULL, request TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(identity,request));")?;
        let store = Self(db);
        let rows: Vec<String> = store
            .0
            .prepare("SELECT data FROM receipts")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for row in rows {
            let mut r: Receipt =
                serde_json::from_str(&row).map_err(|e| AppError::new("database", e))?;
            if r.operation.active() {
                r.operation.phase = "uncertain".into();
                r.operation.message = "应用中断，远端结果待核对；不会自动重试".into();
                r.operation.finished_at = Some(crate::traffic::now());
                store.save(&r)?;
            }
        }
        Ok(store)
    }
    pub fn save(&self, r: &Receipt) -> AppResult<()> {
        let data = serde_json::to_string(r).map_err(|e| AppError::new("database", e))?;
        self.0.execute("INSERT INTO receipts(id,identity,request,data) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![r.operation.id,r.identity,r.operation.request_id,data])?;
        Ok(())
    }
    pub fn list(&self, identity: &str) -> AppResult<Vec<Receipt>> {
        let rows: Vec<String> = self
            .0
            .prepare("SELECT data FROM receipts WHERE identity=?1 ORDER BY rowid DESC LIMIT 100")?
            .query_map([identity], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        rows.iter()
            .map(|v| serde_json::from_str(v).map_err(|e| AppError::new("database", e)))
            .collect()
    }
    pub fn request(&self, identity: &str, request: &str) -> AppResult<Option<Receipt>> {
        let row: Option<String> = self
            .0
            .query_row(
                "SELECT data FROM receipts WHERE identity=?1 AND request=?2",
                params![identity, request],
                |r| r.get(0),
            )
            .optional()?;
        row.map(|s| serde_json::from_str(&s).map_err(|e| AppError::new("database", e)))
            .transpose()
    }
    pub fn get(&self, id: &str) -> AppResult<Receipt> {
        let s: String = self
            .0
            .query_row("SELECT data FROM receipts WHERE id=?1", [id], |r| r.get(0))?;
        serde_json::from_str(&s).map_err(|e| AppError::new("database", e))
    }
    pub fn needs_update_attention(&self) -> AppResult<bool> {
        let rows = self
            .0
            .prepare("SELECT data FROM receipts")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for row in rows {
            let r: Receipt =
                serde_json::from_str(&row).map_err(|e| AppError::new("database", e))?;
            if r.operation.active()
                || (r.operation.phase == "uncertain" && !r.operation.acknowledged)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn path(&self, server: &str) -> AppResult<Option<String>> {
        Ok(self
            .0
            .query_row("SELECT path FROM settings WHERE server=?1", [server], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn set_path(&self, server: &str, path: Option<&str>) -> AppResult<()> {
        if let Some(p) = path {
            self.0.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(server) DO UPDATE SET path=excluded.path",params![server,p])?;
        } else {
            self.0
                .execute("DELETE FROM settings WHERE server=?1", [server])?;
        }
        Ok(())
    }
}
