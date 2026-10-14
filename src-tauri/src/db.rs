use crate::model::{AppError, AppResult, Server};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const SCHEMA: &str = "
  CREATE TABLE IF NOT EXISTS servers (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, host TEXT NOT NULL, port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),
    username TEXT NOT NULL, auth_type TEXT NOT NULL, private_key TEXT NOT NULL, password TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
  CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL, normalized_name TEXT NOT NULL UNIQUE);
  CREATE TABLE IF NOT EXISTS server_tags (server_id TEXT REFERENCES servers(id) ON DELETE CASCADE,
    tag_id INTEGER REFERENCES tags(id), PRIMARY KEY(server_id,tag_id));
  CREATE TABLE IF NOT EXISTS host_keys (host TEXT NOT NULL, port INTEGER NOT NULL, fingerprint TEXT NOT NULL, PRIMARY KEY(host,port));";

pub struct Database {
    pub connection: Connection,
}
impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON;")?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }
    fn missing_columns(&self) -> AppResult<Vec<String>> {
        let mut missing = Vec::new();
        for (table, required) in [
            (
                "servers",
                &[
                    "id",
                    "name",
                    "host",
                    "port",
                    "username",
                    "auth_type",
                    "private_key",
                    "password",
                    "created_at",
                    "updated_at",
                ][..],
            ),
            ("tags", &["id", "name", "normalized_name"][..]),
            ("server_tags", &["server_id", "tag_id"][..]),
            ("host_keys", &["host", "port", "fingerprint"][..]),
        ] {
            let mut statement = self
                .connection
                .prepare("SELECT name FROM pragma_table_info(?1)")?;
            let columns = statement
                .query_map([table], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for column in required {
                if !columns
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(column))
                {
                    missing.push(format!("{table}.{column}"));
                }
            }
        }
        Ok(missing)
    }
    fn validate_schema(&self) -> AppResult<()> {
        let missing = self.missing_columns()?;
        if !missing.is_empty() {
            return Err(AppError::new("databaseSchema", format!(
                "当前数据库结构与程序不匹配，缺少 {}。\n实际数据库：{}\n可备份旧库并重新创建空数据库；不会升级或迁移旧数据。",
                missing.join("、"), self.connection.path().unwrap_or(":memory:")
            )));
        }
        Ok(())
    }
    // Explicit recovery only: never reset a compatible database and never migrate
    // old records. Run inside the app so recovery uses the very same open file.
    pub fn reset_incompatible(&mut self) -> AppResult<PathBuf> {
        if self.missing_columns()?.is_empty() {
            return Err(AppError::new(
                "validation",
                "数据库结构正常，无需重建，已有配置保持不变",
            ));
        }
        let path = self
            .connection
            .path()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| AppError::new("database", "无法确定数据库备份位置"))?;
        let backup =
            Path::new(path).with_file_name(format!("servers.backup-{}.db", uuid::Uuid::new_v4()));
        self.reset_with_backup(&backup)?;
        Ok(backup)
    }
    fn reset_with_backup(&mut self, backup: &Path) -> AppResult<()> {
        // VACUUM INTO snapshots committed data, including WAL, and refuses to
        // overwrite a nonempty file. No tables are touched if backup fails.
        self.connection
            .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        let transaction = self.connection.transaction()?;
        transaction.execute_batch(
            "DROP TABLE server_tags; DROP TABLE servers; DROP TABLE tags; DROP TABLE host_keys;",
        )?;
        transaction.execute_batch(SCHEMA)?;
        transaction.commit()?;
        self.validate_schema()
    }
    pub fn list(&self) -> AppResult<Vec<Server>> {
        self.validate_schema()?;
        let mut statement = self.connection.prepare("SELECT id,name,host,port,username,auth_type,private_key,password FROM servers ORDER BY name COLLATE NOCASE,host,port,id")?;
        let servers = statement
            .query_map([], |row| {
                Ok(Server {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    host: row.get(2)?,
                    port: row.get(3)?,
                    username: row.get(4)?,
                    auth_type: row.get(5)?,
                    private_key: row.get(6)?,
                    password: row.get(7)?,
                    tags: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        servers.into_iter().map(|mut server| {
            let mut tags = self.connection.prepare("SELECT t.name FROM tags t JOIN server_tags st ON t.id=st.tag_id WHERE st.server_id=? ORDER BY t.name")?;
            server.tags = tags.query_map([&server.id], |row| row.get(0))?.collect::<Result<_,_>>()?;
            Ok(server)
        }).collect()
    }
    pub fn get(&self, id: &str) -> AppResult<Server> {
        self.list()?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::new("notFound", "服务器不存在"))
    }
    pub fn save(&mut self, mut server: Server) -> AppResult<Server> {
        self.validate_schema()?;
        server.name = server.name.trim().into();
        server.host = server.host.trim().into();
        server.username = server.username.trim().into();
        if server.name.is_empty()
            || server.host.is_empty()
            || server.port == 0
            || server.host.chars().any(char::is_whitespace)
            || server.host.contains('/')
        {
            return Err(AppError::new(
                "validation",
                "名称和地址不能为空，地址不能包含空格或路径，端口范围为 1–65535",
            ));
        }
        if !["agent", "password", "key"].contains(&server.auth_type.as_str()) {
            return Err(AppError::new("validation", "不支持的认证方式"));
        }
        if server.id.is_empty() {
            server.id = uuid::Uuid::new_v4().to_string();
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("INSERT INTO servers(id,name,host,port,username,auth_type,private_key,password) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
          ON CONFLICT(id) DO UPDATE SET name=excluded.name,host=excluded.host,port=excluded.port,username=excluded.username,auth_type=excluded.auth_type,private_key=excluded.private_key,password=excluded.password,updated_at=CURRENT_TIMESTAMP",
          params![server.id,server.name,server.host,server.port,server.username,server.auth_type,server.private_key,server.password])?;
        transaction.execute("DELETE FROM server_tags WHERE server_id=?", [&server.id])?;
        let mut canonical = Vec::new();
        for tag in &server.tags {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            let normalized = tag.to_lowercase();
            transaction.execute(
                "INSERT OR IGNORE INTO tags(name,normalized_name) VALUES (?,?)",
                params![tag, normalized],
            )?;
            let (tag_id, name): (i64, String) = transaction.query_row(
                "SELECT id,name FROM tags WHERE normalized_name=?",
                [&normalized],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO server_tags(server_id,tag_id) VALUES (?,?)",
                params![server.id, tag_id],
            )?;
            if !canonical.contains(&name) {
                canonical.push(name);
            }
        }
        transaction.commit()?;
        server.tags = canonical;
        Ok(server)
    }
    pub fn save_password(&self, id: &str, password: &str) -> AppResult<Server> {
        let updated = self.connection.execute(
            "UPDATE servers SET password=?,updated_at=CURRENT_TIMESTAMP WHERE id=?",
            params![password, id],
        )?;
        if updated == 0 {
            return Err(AppError::new("notFound", "服务器不存在"));
        }
        self.get(id)
    }
    pub fn delete(&self, id: &str) -> AppResult<()> {
        self.connection
            .execute("DELETE FROM servers WHERE id=?", [id])?;
        Ok(())
    }
    pub fn fingerprint(&self, host: &str, port: u16) -> AppResult<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT fingerprint FROM host_keys WHERE host=? AND port=?",
                params![host, port],
                |row| row.get(0),
            )
            .optional()?)
    }
    pub fn trust(&self, host: &str, port: u16, fingerprint: &str) -> AppResult<()> {
        self.connection.execute("INSERT INTO host_keys(host,port,fingerprint) VALUES (?,?,?) ON CONFLICT(host,port) DO UPDATE SET fingerprint=excluded.fingerprint", params![host,port,fingerprint])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_database(path: &Path) -> Database {
        let db = Database::open(path).unwrap();
        db.connection.execute_batch("ALTER TABLE servers DROP COLUMN password;
          INSERT INTO servers(id,name,host,port,username,auth_type,private_key) VALUES ('old','Legacy','127.0.0.1',22,'ops','agent','');
          INSERT INTO tags(id,name,normalized_name) VALUES (1,'QA','qa');
          INSERT INTO server_tags VALUES ('old',1);
          INSERT INTO host_keys VALUES ('127.0.0.1',22,'SHA256:fixture');").unwrap();
        db
    }
    #[test]
    fn incompatible_schema_reports_actual_file_without_migrating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.db");
        drop(legacy_database(&path));
        let db = Database::open(&path).unwrap();
        let error = db.list().unwrap_err();
        assert_eq!(error.kind, "databaseSchema");
        assert!(error.message.contains("servers.password"));
        assert!(error.message.contains("servers.db"));
        assert_eq!(db.missing_columns().unwrap(), vec!["servers.password"]);
        assert_eq!(
            db.connection
                .query_row("SELECT count(*) FROM servers", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn explicit_rebuild_backs_up_old_data_and_new_schema_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.db");
        let mut db = legacy_database(&path);
        let backup_path = db.reset_incompatible().unwrap();
        assert!(backup_path.exists());
        let backup =
            Connection::open_with_flags(&backup_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(
            backup
                .query_row("SELECT name FROM servers", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "Legacy"
        );
        assert_eq!(
            backup
                .query_row("SELECT count(*) FROM server_tags", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            backup
                .query_row("SELECT fingerprint FROM host_keys", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "SHA256:fixture"
        );
        assert!(backup.prepare("SELECT password FROM servers").is_err());
        assert!(db.list().unwrap().is_empty());
        assert!(db.fingerprint("127.0.0.1", 22).unwrap().is_none());
        let saved = db.save(server()).unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        assert_eq!(db.get(&saved.id).unwrap().name, saved.name);
        assert!(db.reset_incompatible().is_err());
        assert_eq!(db.list().unwrap().len(), 1);
    }
    #[test]
    fn failed_backup_leaves_legacy_data_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = legacy_database(&dir.path().join("servers.db"));
        let backup = dir.path().join("already-exists.db");
        std::fs::write(&backup, b"keep existing file").unwrap();
        assert!(db.reset_with_backup(&backup).is_err());
        assert_eq!(std::fs::read(&backup).unwrap(), b"keep existing file");
        assert_eq!(
            db.connection
                .query_row("SELECT name FROM servers", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "Legacy"
        );
        assert_eq!(db.missing_columns().unwrap(), vec!["servers.password"]);
    }
    fn server() -> Server {
        Server {
            id: String::new(),
            name: "Web".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "ops".into(),
            tags: vec![" Production ".into(), "production".into(), "Web".into()],
            auth_type: "agent".into(),
            private_key: String::new(),
            password: String::new(),
        }
    }
    #[test]
    fn persistence_tag_deduplication_and_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.db");
        let id = {
            let mut db = Database::open(&path).unwrap();
            let saved = db.save(server()).unwrap();
            assert_eq!(saved.tags.len(), 2);
            saved.id
        };
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get(&id).unwrap().tags.len(), 2);
        db.delete(&id).unwrap();
        assert!(db.list().unwrap().is_empty());
        let count: i64 = db
            .connection
            .query_row("SELECT COUNT(*) FROM server_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn invalid_update_does_not_destroy_saved_data() {
        let mut db = Database::open(Path::new(":memory:")).unwrap();
        let mut saved = db.save(server()).unwrap();
        saved.host = " ".into();
        assert!(db.save(saved.clone()).is_err());
        assert_eq!(db.get(&saved.id).unwrap().host, "127.0.0.1");
    }
    #[test]
    fn password_persists_exactly_and_can_be_updated_or_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.db");
        let password = "  密码'\"$\\\n  ";
        let mut configured = server();
        configured.auth_type = "password".into();
        configured.password = password.into();
        let id = Database::open(&path).unwrap().save(configured).unwrap().id;
        let mut db = Database::open(&path).unwrap();
        let mut saved = db.get(&id).unwrap();
        assert_eq!(saved.password, password);
        // Editing other configuration must preserve the stored password.
        saved.name = "Updated".into();
        db.save(saved).unwrap();
        assert_eq!(db.get(&id).unwrap().password, password);
        let tags = db.get(&id).unwrap().tags;
        db.save_password(&id, "replacement").unwrap();
        assert_eq!(db.get(&id).unwrap().name, "Updated");
        assert_eq!(db.get(&id).unwrap().tags, tags);
        drop(db);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get(&id).unwrap().password, "replacement");
        db.save_password(&id, "").unwrap();
        assert_eq!(db.get(&id).unwrap().password, "");
        assert!(db.save_password("missing", "other").is_err());
        db.delete(&id).unwrap();
        assert!(db.list().unwrap().is_empty());
    }
}
