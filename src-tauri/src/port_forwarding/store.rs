use super::model::Rule;
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
        let tables: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if version != 1 && (version != 0 || tables > 0) {
            return Err(AppError::new(
                "databaseSchema",
                "端口转发数据库版本不受支持；原文件已保留",
            ));
        }
        db.pragma_update(None, "journal_mode", "WAL")?;
        if version == 0 {
            db.execute_batch(
                "BEGIN IMMEDIATE;
              CREATE TABLE forward_rules(id TEXT PRIMARY KEY, revision INTEGER NOT NULL,
                server_id TEXT NOT NULL, configuration TEXT NOT NULL);
              CREATE INDEX forward_rules_server ON forward_rules(server_id);
              PRAGMA user_version=1; COMMIT;",
            )?;
        }
        let result = Self { db };
        result.list()?;
        Ok(result)
    }
    pub fn list(&self) -> AppResult<Vec<Rule>> {
        let mut stmt = self
            .db
            .prepare("SELECT configuration FROM forward_rules ORDER BY rowid")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).map_err(|e| AppError::new("databaseSchema", e)))
            .collect()
    }
    pub fn get(&self, id: &str) -> AppResult<Rule> {
        let value: Option<String> = self
            .db
            .query_row(
                "SELECT configuration FROM forward_rules WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&value.ok_or_else(|| AppError::new("notFound", "规则已删除"))?)
            .map_err(|e| AppError::new("databaseSchema", e))
    }
    pub fn save(&mut self, mut rule: Rule) -> AppResult<Rule> {
        rule.validate()?;
        let old = rule.revision;
        rule.revision = old
            .checked_add(1)
            .ok_or_else(|| AppError::new("limit", "规则版本已超过限制"))?;
        let tx = self.db.transaction()?;
        let count = if rule.id.is_empty() {
            if old != 0 {
                return Err(AppError::new("conflict", "新规则版本错误"));
            }
            rule.id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO forward_rules(id,revision,server_id,configuration) VALUES(?1,?2,?3,?4)",
                params![rule.id, rule.revision, rule.settings.server_id, serde_json::to_string(&rule).unwrap()])?
        } else {
            tx.execute("UPDATE forward_rules SET revision=?2,server_id=?3,configuration=?4 WHERE id=?1 AND revision=?5",
                params![rule.id, rule.revision, rule.settings.server_id, serde_json::to_string(&rule).unwrap(), old])?
        };
        if count != 1 {
            return Err(AppError::new("conflict", "规则已修改或删除，请重新读取"));
        }
        tx.commit()?;
        Ok(rule)
    }
    pub fn delete(&mut self, id: &str, revision: u32) -> AppResult<()> {
        if self.db.execute(
            "DELETE FROM forward_rules WHERE id=?1 AND revision=?2",
            params![id, revision],
        )? != 1
        {
            return Err(AppError::new("conflict", "规则已修改，请重新读取"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_forwarding::model::Settings;
    pub fn rule() -> Rule {
        Rule {
            id: String::new(),
            revision: 0,
            name: "数据库".into(),
            web_scheme: None,
            settings: Settings {
                server_id: "server".into(),
                listen_address: "127.0.0.1".into(),
                listen_port: None,
                target_host: "db.internal".into(),
                target_port: 3306,
                auto_reconnect: true,
            },
        }
    }
    #[test]
    fn configuration_cas_roundtrip_and_validation() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let saved = store.save(rule()).unwrap();
        let mut edited = saved.clone();
        edited.name = "新名称".into();
        let current = store.save(edited).unwrap();
        assert!(store.save(saved.clone()).is_err());
        assert!(store.delete(&saved.id, saved.revision).is_err());
        assert_eq!(store.get(&current.id).unwrap().name, "新名称");
        let mut invalid = current.clone();
        invalid.settings.listen_address = "0.0.0.0".into();
        assert!(store.save(invalid).is_err());
        store.delete(&current.id, current.revision).unwrap();
        assert!(store.list().unwrap().is_empty());
    }
}
