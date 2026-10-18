use super::compose::Association;
use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
pub struct Store {
    db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> AppResult<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let count: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if version != 1 && (version != 0 || count > 0) {
            return Err(AppError::new(
                "databaseSchema",
                "Docker 关联数据库版本不受支持，原文件已保留",
            ));
        }
        db.pragma_update(None, "journal_mode", "WAL")?;
        if version == 0 {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE projects(id TEXT PRIMARY KEY, revision INTEGER NOT NULL, server_id TEXT NOT NULL, configuration TEXT NOT NULL); CREATE INDEX projects_server ON projects(server_id); PRAGMA user_version=1; COMMIT;")?;
        }
        Ok(Self { db })
    }
    pub fn list(&self, server: &str) -> AppResult<Vec<Association>> {
        let mut stmt = self
            .db
            .prepare("SELECT configuration FROM projects WHERE server_id=?1 ORDER BY rowid")?;
        let values = stmt
            .query_map([server], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(|e| AppError::new("databaseSchema", e)))
            .collect()
    }
    pub fn get(&self, id: &str) -> AppResult<Association> {
        let value: Option<String> = self
            .db
            .query_row(
                "SELECT configuration FROM projects WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&value.ok_or_else(|| AppError::new("notFound", "项目关联已删除"))?)
            .map_err(|e| AppError::new("databaseSchema", e))
    }
    pub fn save(&mut self, mut association: Association) -> AppResult<Association> {
        association.validate()?;
        let previous = association.revision;
        association.revision = previous
            .checked_add(1)
            .ok_or_else(|| AppError::new("limit", "关联版本达到上限"))?;
        let tx = self.db.transaction()?;
        let count = if association.id.is_empty() {
            if previous != 0 {
                return Err(AppError::new("conflict", "无效的新关联版本"));
            }
            association.id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO projects(id,revision,server_id,configuration) VALUES(?1,?2,?3,?4)",
                params![
                    association.id,
                    association.revision,
                    association.server_id,
                    serde_json::to_string(&association).unwrap()
                ],
            )?
        } else {
            tx.execute("UPDATE projects SET revision=?1,configuration=?2 WHERE id=?3 AND revision=?4 AND server_id=?5", params![association.revision, serde_json::to_string(&association).unwrap(), association.id, previous, association.server_id])?
        };
        if count != 1 {
            return Err(AppError::new(
                "conflict",
                "关联已被其他窗口修改，请重新读取",
            ));
        }
        tx.commit()?;
        Ok(association)
    }
    pub fn remove(&mut self, id: &str, revision: u32) -> AppResult<()> {
        if self.db.execute(
            "DELETE FROM projects WHERE id=?1 AND revision=?2",
            params![id, revision],
        )? != 1
        {
            return Err(AppError::new("conflict", "关联已变化，请刷新后重试"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn association() -> Association {
        Association {
            id: String::new(),
            revision: 0,
            server_id: "server".into(),
            endpoint: "unix:///var/run/docker.sock".into(),
            daemon_id: Some("engine".into()),
            project: "app".into(),
            directory: "/opt/app".into(),
            files: vec![
                "/opt/app/compose.yaml".into(),
                "/opt/app/production.yaml".into(),
            ],
            env_files: vec!["/opt/app/.env".into()],
            profiles: vec!["web".into()],
        }
    }
    #[test]
    fn associations_are_persistent_versioned_and_never_overwrite_newer_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker.sqlite");
        let mut store = Store::open(&path).unwrap();
        let first = store.save(association()).unwrap();
        let mut second = first.clone();
        second.profiles.push("jobs".into());
        let second = store.save(second).unwrap();
        assert_eq!(second.revision, 2);
        assert!(store.save(first.clone()).is_err());
        assert!(store.remove(&first.id, first.revision).is_err());
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.get(&second.id).unwrap(), second);
        assert!(store.list("different-server").unwrap().is_empty());
        store.remove(&second.id, second.revision).unwrap();
        assert!(store.list("server").unwrap().is_empty());
    }
    #[test]
    fn unknown_database_schema_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE future(data TEXT); INSERT INTO future VALUES('keep'); PRAGMA user_version=99;").unwrap();
        drop(db);
        assert!(Store::open(&path).is_err());
        let db = Connection::open(path).unwrap();
        assert_eq!(
            db.query_row("SELECT data FROM future", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "keep"
        );
    }
}
