use crate::model::{id, CoreError, Result, Server};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};

pub struct Store {
    pub(crate) db: Mutex<Connection>,
}
impl Store {
    pub(crate) fn configuration_epoch(&self) -> Result<String> {
        let mut db=self.db.lock().unwrap();
        let tx=db.transaction()?;
        let existing:Option<String>=tx.query_row("SELECT value FROM settings WHERE namespace='@core' AND key='configurationEpoch'",[],|r|r.get(0)).optional()?;
        let epoch=existing.and_then(|v|serde_json::from_str::<String>(&v).ok()).unwrap_or_else(id);
        tx.execute("INSERT OR REPLACE INTO settings(namespace,key,value) VALUES('@core','configurationEpoch',?)",[serde_json::to_string(&epoch)?])?;
        tx.commit()?;Ok(epoch)
    }
    pub fn open(root: &Path) -> Result<Self> {
        crate::store_bootstrap::ensure_current(root)?;
        let db = Connection::open(root.join("core.sqlite"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
          PRAGMA max_page_count=57344; PRAGMA journal_size_limit=4194304; PRAGMA wal_autocheckpoint=256;")?;
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
    pub fn delete_server(&self, server_id: &str) -> Result<()> {
        let mut db=self.db.lock().unwrap();
        let tx=db.transaction()?;
        tx.execute("DELETE FROM servers WHERE id=?", [server_id])?;
        crate::server_catalog_store::bump_revision(&tx)?;
        tx.commit()?;
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
}

pub(crate) fn query_calls(db: &Connection, filters: &Value, retention_days:u32) -> Result<Value> {
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
        Ok(json!({"records": records,"nextCursor":next_cursor,"retainedFrom":oldest,"retentionDays":retention_days,"maxBytes":268435456}))
    }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn calls_cursor_preserves_equal_timestamps_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        crate::observability::Observability::initialize_fixture(dir.path()).unwrap();
        let db = Connection::open(dir.path().join("facts.sqlite")).unwrap();
        let caller = json!({"pluginId":"sodeck.terminal"}).to_string();
        for index in 0..205 {
            db.execute("INSERT INTO calls(id,started,ended,caller,method,server_id,summary,status) VALUES(?,1000,1001,?,'pty.open','test-server','{}','succeeded')",params![format!("record-{index:04}"),caller]).unwrap();
        }
        let mut filters = json!({"limit":100,"method":"pty.open","pluginId":"sodeck.terminal","serverId":"test-server","status":"succeeded"});
        let mut ids = HashSet::new();
        let mut pages = Vec::new();
        loop {
            let page = query_calls(&db,&filters,30).unwrap();
            let records = page["records"].as_array().unwrap();
            pages.push(records.len());
            for record in records { assert!(ids.insert(record["id"].as_str().unwrap().to_owned())); }
            if page["nextCursor"].is_null() { break; }
            filters["cursor"] = page["nextCursor"].clone();
            if pages.len() == 1 {
                db.execute("INSERT INTO calls(id,started,caller,method,summary,status) VALUES('new-record',2000,?,'pty.open','{}','running')",[&caller]).unwrap();
            }
        }
        assert_eq!(pages, vec![100,100,5]);
        assert_eq!(ids.len(), 205);
        assert!(query_calls(&db,&json!({"cursor":{"started":1000}}),30).is_err());
        assert!(query_calls(&db,&json!({"before":1000}),30).unwrap()["records"].as_array().unwrap().is_empty());
        assert!(query_calls(&db,&json!({"serverId":"different"}),30).unwrap()["records"].as_array().unwrap().is_empty());
    }
}
