use crate::model::{id, now_ms, Caller, CoreError, Result, Server};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};

pub struct Store {
    db: Mutex<Connection>,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let db = Connection::open(root.join("core.sqlite"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
          PRAGMA max_page_count=57344; PRAGMA journal_size_limit=4194304; PRAGMA wal_autocheckpoint=256;
          CREATE TABLE IF NOT EXISTS servers(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS host_keys(endpoint TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, algorithm TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS settings(namespace TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(namespace,key));
          CREATE TABLE IF NOT EXISTS calls(id TEXT PRIMARY KEY, started INTEGER NOT NULL, ended INTEGER, caller TEXT NOT NULL, method TEXT NOT NULL, server_id TEXT, summary TEXT NOT NULL, status TEXT NOT NULL, error_code TEXT);
          CREATE INDEX IF NOT EXISTS calls_started ON calls(started);
          UPDATE calls SET status='interrupted',error_code='APP_INTERRUPTED' WHERE ended IS NULL;")?;
        Ok(Self { db: Mutex::new(db) })
    }
    pub fn servers(&self) -> Result<Vec<Server>> {
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare("SELECT data FROM servers ORDER BY rowid")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    pub fn server(&self, id: &str) -> Result<Server> {
        let raw: Option<String> = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT data FROM servers WHERE id=?", [id], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(CoreError::from))
            .transpose()?
            .ok_or_else(|| CoreError::new("NOT_FOUND", "服务器不存在"))
    }
    pub fn save_server(&self, server: &Server, expected: Option<u64>) -> Result<()> {
        server.validate()?;
        let mut db = self.db.lock().unwrap();
        let transaction = db.transaction()?;
        let previous: Option<String> = transaction
            .query_row("SELECT data FROM servers WHERE id=?", [&server.id], |row| {
                row.get(0)
            })
            .optional()?;
        match (previous, expected) {
            (None, None) => {}
            (Some(raw), Some(revision))
                if serde_json::from_str::<Server>(&raw)?.revision == revision => {}
            _ => {
                return Err(CoreError::new(
                    "REVISION_CONFLICT",
                    "服务器配置已变化，请重新加载",
                ))
            }
        }
        transaction.execute("INSERT INTO servers(id,data) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![server.id, serde_json::to_string(server)?])?;
        transaction.commit()?;
        Ok(())
    }
    pub fn delete_server(&self, server_id: &str) -> Result<()> {
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM servers WHERE id=?", [server_id])?;
        Ok(())
    }
    pub fn host_key(&self, endpoint: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT fingerprint FROM host_keys WHERE endpoint=?",
                [endpoint],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn trust(&self, endpoint: &str, fingerprint: &str, algorithm: &str) -> Result<()> {
        self.db.lock().unwrap().execute("INSERT INTO host_keys(endpoint,fingerprint,algorithm) VALUES(?,?,?) ON CONFLICT(endpoint) DO UPDATE SET fingerprint=excluded.fingerprint, algorithm=excluded.algorithm", params![endpoint, fingerprint, algorithm])?;
        Ok(())
    }
    pub fn get_setting(&self, namespace: &str, key: &str) -> Result<Value> {
        let value: Option<String> = self
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM settings WHERE namespace=? AND key=?",
                params![namespace, key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null))
    }
    pub fn set_setting(&self, namespace: &str, key: &str, value: &Value) -> Result<()> {
        let serialized = serde_json::to_string(value)?;
        if namespace.len() > 200 || key.len() > 200 || serialized.len() > 1024 * 1024 {
            return Err(CoreError::new("LIMIT_EXCEEDED", "设置超过容量限制"));
        }
        self.db.lock().unwrap().execute("INSERT INTO settings(namespace,key,value) VALUES(?,?,?) ON CONFLICT(namespace,key) DO UPDATE SET value=excluded.value", params![namespace,key,serialized])?;
        Ok(())
    }
    pub fn call_start(&self, caller: &Caller, method: &str, params: &Value) -> Result<String> {
        let db = self.db.lock().unwrap();
        let record_id = id();
        // Summary uses an explicit allowlist: never serialize request bodies or errors.
        let summary = json!({"fieldCount": params.as_object().map_or(0, |v| v.len()), "byteLength":params.get("bytes").and_then(Value::as_str).map(str::len), "inputSeq":params.get("inputSeq").and_then(Value::as_u64), "generation":params.get("generation").and_then(Value::as_u64)});
        db.execute("INSERT INTO calls(id,started,caller,method,server_id,summary,status) VALUES(?,?,?,?,?,?,'running')", params![record_id,now_ms() as i64,serde_json::to_string(caller)?,method,params.get("serverId").and_then(Value::as_str),summary.to_string()])?;
        Ok(record_id)
    }
    pub fn call_finish(&self, record_id: &str, result: &Result<Value>) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE calls SET ended=?,status=?,error_code=? WHERE id=?",
            params![
                now_ms() as i64,
                if result.is_ok() {
                    "succeeded"
                } else {
                    "failed"
                },
                result.as_ref().err().map(|e| e.code.as_str()),
                record_id
            ],
        )?;
        Ok(())
    }
    pub fn calls(&self, filters: &Value) -> Result<Value> {
        let limit = filters
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 100);
        let before = filters
            .get("before")
            .and_then(Value::as_u64)
            .unwrap_or(i64::MAX as u64)
            .min(i64::MAX as u64);
        let cursor = match filters.get("cursor").filter(|value| !value.is_null()) {
            Some(value) => Some((
                value.get("started").and_then(Value::as_i64).filter(|v| *v >= 0)
                    .ok_or_else(|| CoreError::new("INVALID_ARGUMENT", "记录游标无效"))?,
                value.get("id").and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 100)
                    .ok_or_else(|| CoreError::new("INVALID_ARGUMENT", "记录游标无效"))?,
            )),
            None => None,
        };
        let db = self.db.lock().unwrap();
        let mut statement = db.prepare("SELECT id,started,ended,caller,method,server_id,summary,status,error_code FROM calls WHERE started<? AND (? IS NULL OR started<? OR (started=? AND id<?)) AND (? IS NULL OR method=?) AND (? IS NULL OR server_id=?) AND (? IS NULL OR status=?) AND (? IS NULL OR json_extract(caller,'$.pluginId')=?) ORDER BY started DESC,id DESC LIMIT ?")?;
        let method = filters.get("method").and_then(Value::as_str);
        let server = filters.get("serverId").and_then(Value::as_str);
        let status = filters.get("status").and_then(Value::as_str);
        let plugin = filters.get("pluginId").and_then(Value::as_str);
        let cursor_time = cursor.map(|(started, _)| started);
        let cursor_id = cursor.map(|(_, id)| id);
        let rows = statement.query_map(params![before as i64,cursor_time,cursor_time,cursor_time,cursor_id,method,method,server,server,status,status,plugin,plugin,limit as i64 + 1], |r| Ok(json!({"id":r.get::<_,String>(0)?,"started":r.get::<_,i64>(1)?,"ended":r.get::<_,Option<i64>>(2)?,"caller":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(Value::Null),"method":r.get::<_,String>(4)?,"serverId":r.get::<_,Option<String>>(5)?,"summary":serde_json::from_str::<Value>(&r.get::<_,String>(6)?).unwrap_or(Value::Null),"status":r.get::<_,String>(7)?,"errorCode":r.get::<_,Option<String>>(8)?})))?;
        let mut records: Vec<Value> = rows.collect::<std::result::Result<_, _>>()?;
        let has_more = records.len() > limit as usize;
        records.truncate(limit as usize);
        let next_cursor = if has_more { records.last().map(|record| json!({"started":record["started"],"id":record["id"]})) } else { None };
        let oldest: Option<i64> =
            db.query_row("SELECT MIN(started) FROM calls", [], |r| r.get(0))?;
        Ok(json!({"records": records,"nextCursor":next_cursor,"retainedFrom":oldest,"retentionDays":7,"maxBytes":268435456}))
    }
    pub fn maintain(&self) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM calls WHERE ended IS NOT NULL AND started<?",
            [now_ms().saturating_sub(7 * 24 * 60 * 60 * 1000) as i64],
        )?;
        let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let free: i64 = db.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
        let size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        if pages.saturating_sub(free) * size > 208 * 1024 * 1024 {
            db.execute("DELETE FROM calls WHERE id IN(SELECT id FROM calls WHERE ended IS NOT NULL ORDER BY started LIMIT 10000)", [])?;
        }
        db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
        Ok(())
    }
    #[cfg(test)]
    pub fn fail_writes(&self, fail: bool) {
        self.db
            .lock()
            .unwrap()
            .execute_batch(if fail {
                "PRAGMA query_only=ON"
            } else {
                "PRAGMA query_only=OFF"
            })
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn calls_cursor_preserves_equal_timestamps_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let caller = json!({"pluginId":"sodeck.terminal"}).to_string();
        for index in 0..205 {
            store.db.lock().unwrap().execute("INSERT INTO calls(id,started,ended,caller,method,server_id,summary,status) VALUES(?,1000,1001,?,'pty.open','test-server','{}','succeeded')",params![format!("record-{index:04}"),caller]).unwrap();
        }
        let mut filters = json!({"limit":100,"method":"pty.open","pluginId":"sodeck.terminal","serverId":"test-server","status":"succeeded"});
        let mut ids = HashSet::new();
        let mut pages = Vec::new();
        loop {
            let page = store.calls(&filters).unwrap();
            let records = page["records"].as_array().unwrap();
            pages.push(records.len());
            for record in records { assert!(ids.insert(record["id"].as_str().unwrap().to_owned())); }
            if page["nextCursor"].is_null() { break; }
            filters["cursor"] = page["nextCursor"].clone();
            if pages.len() == 1 {
                store.db.lock().unwrap().execute("INSERT INTO calls(id,started,caller,method,summary,status) VALUES('new-record',2000,?,'pty.open','{}','running')",[&caller]).unwrap();
            }
        }
        assert_eq!(pages, vec![100,100,5]);
        assert_eq!(ids.len(), 205);
        assert!(store.calls(&json!({"cursor":{"started":1000}})).is_err());
        assert!(store.calls(&json!({"before":1000})).unwrap()["records"].as_array().unwrap().is_empty());
        assert!(store.calls(&json!({"serverId":"different"})).unwrap()["records"].as_array().unwrap().is_empty());
    }
}
