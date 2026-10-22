use super::model::{Rule, Service, Settings};
use crate::model::{AppError, AppResult};
use rusqlite::{Connection, TransactionBehavior};
use serde::Deserialize;
use std::{path::Path, time::Duration};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacySettingsV1 {
    server_id: String,
    listen_address: String,
    listen_port: Option<u16>,
    target_host: String,
    target_port: u16,
    auto_reconnect: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRuleV1 {
    id: String,
    revision: u32,
    name: String,
    web_scheme: Option<String>,
    settings: LegacySettingsV1,
}
type Row = (i64, String, u32, String, String);
fn rows(db: &Connection) -> AppResult<Vec<Row>> {
    Ok(db
        .prepare(
            "SELECT rowid,id,revision,server_id,configuration FROM forward_rules ORDER BY rowid",
        )?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<Vec<_>, _>>()?)
}
fn schema() -> AppError {
    AppError::new("databaseSchema", "规则数据库内容无效；原文件已保留")
}
fn convert(row: &Row) -> AppResult<Rule> {
    let old: LegacyRuleV1 = serde_json::from_str(&row.4).map_err(|_| schema())?;
    let s = old.settings;
    let rule = Rule {
        id: old.id,
        revision: old.revision,
        name: old.name,
        web_scheme: old.web_scheme,
        settings: Settings {
            server_id: s.server_id,
            listen_address: s.listen_address,
            listen_port: s.listen_port,
            auto_reconnect: s.auto_reconnect,
            service: Service::PortForward {
                target_host: s.target_host,
                target_port: s.target_port,
            },
        },
    };
    validate(row, &rule)?;
    Ok(rule)
}
fn validate(row: &Row, rule: &Rule) -> AppResult<()> {
    if rule.id.is_empty()
        || rule.revision == 0
        || rule.id != row.1
        || rule.revision != row.2
        || rule.settings.server_id != row.3
    {
        return Err(schema());
    }
    rule.clone().validate().map_err(|_| schema())
}
pub fn read(db: &Connection) -> AppResult<Vec<Rule>> {
    rows(db)?
        .iter()
        .map(|row| {
            let rule = serde_json::from_str(&row.4).map_err(|_| schema())?;
            validate(row, &rule)?;
            Ok(rule)
        })
        .collect()
}
pub fn upgrade(db: &mut Connection, path: &Path) -> AppResult<()> {
    // A SQLite snapshot includes committed WAL pages. Never copy the main file.
    let snapshot = rows(db)?;
    snapshot
        .iter()
        .map(convert)
        .collect::<AppResult<Vec<_>>>()?;
    let mut backup = if path == Path::new(":memory:") {
        Connection::open_in_memory()?
    } else {
        let name = format!(
            "port-forwards.schema-v1.{}.{}.sqlite",
            crate::traffic::now(),
            uuid::Uuid::new_v4()
        );
        let destination = path.with_file_name(name);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        Connection::open(destination)?
    };
    rusqlite::backup::Backup::new(db, &mut backup)?.run_to_completion(
        64,
        Duration::from_millis(10),
        None,
    )?;
    let check: String = backup.pragma_query_value(None, "quick_check", |r| r.get(0))?;
    let version: i64 = backup.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let backed = rows(&backup)?;
    if check != "ok" || version != 1 || backed != snapshot {
        return Err(schema());
    }
    backed.iter().map(convert).collect::<AppResult<Vec<_>>>()?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if rows(&tx)? != backed {
        return Err(AppError::new("conflict", "配置在迁移前发生变化，请重试"));
    }
    for row in &backed {
        let rule = convert(row)?;
        tx.execute(
            "UPDATE forward_rules SET configuration=?1 WHERE rowid=?2",
            rusqlite::params![serde_json::to_string(&rule).map_err(|_| schema())?, row.0],
        )?;
    }
    read(&tx)?;
    tx.pragma_update(None, "user_version", 2)?;
    tx.commit()?;
    Ok(())
}
