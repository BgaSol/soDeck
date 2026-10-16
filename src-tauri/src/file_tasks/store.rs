use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

/// Display history is separate from transfer receipts, which remain authoritative
/// for remote commits and resource cleanup. Tombstones never remove receipts.
pub struct Store(Connection);

#[derive(Clone, Serialize, Deserialize)]
pub struct DeleteStep {
    pub root: usize,
    pub path: String,
    pub outcome: String,
    pub message: Option<String>,
}

impl Store {
    pub fn open(path: &Path) -> AppResult<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS records(
               id TEXT PRIMARY KEY, kind TEXT NOT NULL, payload TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS dismissed(id TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS delete_pending(
               task TEXT PRIMARY KEY, root INTEGER NOT NULL, path TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS delete_steps(
               sequence INTEGER PRIMARY KEY AUTOINCREMENT, task TEXT NOT NULL, payload TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS delete_steps_task ON delete_steps(task);
             CREATE TABLE IF NOT EXISTS delete_plans(
               task TEXT NOT NULL, root INTEGER NOT NULL, position INTEGER NOT NULL,
               payload TEXT NOT NULL, PRIMARY KEY(task, root, position));",
        )?;
        Ok(Self(db))
    }

    pub fn save(&self, kind: &str, id: &str, value: &impl Serialize) -> AppResult<()> {
        self.0.execute(
            "INSERT INTO records VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET
             kind=excluded.kind,payload=excluded.payload WHERE payload!=excluded.payload",
            params![id, kind, json(value)?],
        )?;
        Ok(())
    }

    pub fn load<T: DeserializeOwned>(&self, kind: &str) -> AppResult<Vec<T>> {
        let mut query = self
            .0
            .prepare("SELECT payload FROM records WHERE kind=?1 ORDER BY rowid")?;
        let rows = query.query_map([kind], |row| row.get::<_, String>(0))?;
        rows.map(|row| parse(&row?)).collect()
    }

    pub fn hidden(&self) -> AppResult<HashSet<String>> {
        let mut query = self.0.prepare("SELECT id FROM dismissed")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| row.map_err(AppError::from)).collect()
    }

    pub fn dismiss(&mut self, ids: &[String]) -> AppResult<()> {
        let tx = self.0.transaction()?;
        for id in ids {
            tx.execute("INSERT OR IGNORE INTO dismissed VALUES(?1)", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn restore_visibility(&self, id: &str) -> AppResult<()> {
        self.0.execute("DELETE FROM dismissed WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn delete_plan(&mut self, id: &str, root: usize, plan: &[impl Serialize]) -> AppResult<()> {
        let tx = self.0.transaction()?;
        tx.execute(
            "DELETE FROM delete_plans WHERE task=?1 AND root=?2",
            params![id, root as i64],
        )?;
        for (position, item) in plan.iter().enumerate() {
            tx.execute(
                "INSERT INTO delete_plans VALUES(?1,?2,?3,?4)",
                params![id, root as i64, position as i64, json(item)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Must commit before sending a destructive request. A crash after this
    /// point means "possibly sent", never permission to replay the request.
    pub fn begin_delete(&self, id: &str, root: usize, path: &str) -> AppResult<()> {
        self.0.execute(
            "INSERT INTO delete_pending VALUES(?1,?2,?3)",
            params![id, root as i64, path],
        )?;
        Ok(())
    }

    pub fn finish_delete(&mut self, id: &str, step: &DeleteStep) -> AppResult<()> {
        let tx = self.0.transaction()?;
        tx.execute(
            "INSERT INTO delete_steps(task,payload) VALUES(?1,?2)",
            params![id, json(step)?],
        )?;
        tx.execute("DELETE FROM delete_pending WHERE task=?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_steps(&self, id: &str) -> AppResult<Vec<DeleteStep>> {
        let mut query = self
            .0
            .prepare("SELECT payload FROM delete_steps WHERE task=?1 ORDER BY sequence")?;
        let rows = query.query_map([id], |row| row.get::<_, String>(0))?;
        rows.map(|row| parse(&row?)).collect()
    }

    pub fn pending_delete(&self, id: &str) -> AppResult<Option<(usize, String)>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row(
                "SELECT root,path FROM delete_pending WHERE task=?1",
                [id],
                |row| Ok((row.get::<_, i64>(0)? as usize, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn checkpoint_delete(
        &mut self,
        id: &str,
        view: &impl Serialize,
        reconciled: bool,
    ) -> AppResult<()> {
        let tx = self.0.transaction()?;
        tx.execute("INSERT INTO records VALUES(?1,'delete',?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![id, json(view)?])?;
        tx.execute("DELETE FROM delete_steps WHERE task=?1", [id])?;
        if reconciled {
            tx.execute("DELETE FROM delete_pending WHERE task=?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn json(value: &impl Serialize) -> AppResult<String> {
    serde_json::to_string(value).map_err(|e| AppError::new("taskStore", e))
}
fn parse<T: DeserializeOwned>(value: &str) -> AppResult<T> {
    serde_json::from_str(value).map_err(|e| AppError::new("taskStore", e))
}

#[cfg(test)]
impl Store {
    pub(crate) fn fail_insert(&self, table: &str) {
        assert!(matches!(table, "delete_pending" | "delete_steps"));
        self.0.execute_batch(&format!("CREATE TEMP TRIGGER fail_insert BEFORE INSERT ON {table} BEGIN SELECT RAISE(FAIL, 'injected disk failure'); END;")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn acknowledged_step_and_pending_marker_commit_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.sqlite");
        {
            let mut store = Store::open(&path).unwrap();
            store.begin_delete("task", 0, "/data/a").unwrap();
            store
                .finish_delete(
                    "task",
                    &DeleteStep {
                        root: 0,
                        path: "/data/a".into(),
                        outcome: "deleted".into(),
                        message: None,
                    },
                )
                .unwrap();
            assert!(store.pending_delete("task").unwrap().is_none());
            store.begin_delete("task", 0, "/data/b").unwrap();
            store.fail_insert("delete_steps");
            assert!(store
                .finish_delete(
                    "task",
                    &DeleteStep {
                        root: 0,
                        path: "/data/b".into(),
                        outcome: "deleted".into(),
                        message: None
                    }
                )
                .is_err());
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.delete_steps("task").unwrap().len(), 1);
        assert_eq!(
            store.pending_delete("task").unwrap(),
            Some((0, "/data/b".into()))
        );
    }

    #[test]
    fn importing_history_does_not_resurrect_dismissed_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.sqlite");
        {
            let mut store = Store::open(&path).unwrap();
            store.save("transfer", "a", &json!({"id":"a"})).unwrap();
            store.dismiss(&["a".into()]).unwrap();
        }
        let store = Store::open(&path).unwrap();
        store
            .save("transfer", "a", &json!({"id":"a", "status":"completed"}))
            .unwrap();
        assert!(store.hidden().unwrap().contains("a"));
        store.restore_visibility("a").unwrap();
        assert!(!store.hidden().unwrap().contains("a"));
    }
}
