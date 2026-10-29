//! Database replacement is a startup operation, before Core or any worker exists.
//! The desktop owns the data-directory lock and the visible notice. Core::open
//! never silently resets a mismatched database. Recovery always rolls forward
//! from verified copies, with all original files retained beside the backup.
use crate::{model::{id, now_ms}, CoreError, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};
mod dataset;

pub const SCHEMA_VERSION: u32 = 7;
const JOURNAL: &str = "core-reset.json";

#[derive(Debug)]
pub struct ResetNotice {
    pub previous_version: u32,
    pub current_version: u32,
    pub backup_path: PathBuf,
    pub recovering: bool,
    pub rebuild_core: bool,
    pub rebuild_observability: bool,
    pub observation_versions: Vec<(String,Option<u32>,u32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all="camelCase")]
enum Phase { BackingUp, BackupVerified, Ready, Retiring, Publishing, Committed }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Fingerprint { name: String, sha256: String }

fn recovery_error() -> CoreError {
    CoreError::new("DATABASE_RECOVERY_REQUIRED", "数据库重建未完成或恢复文件无效；旧数据已保留，请检查备份与启动日志")
}

fn read_only(path: &Path) -> Result<Connection> {
    // Do not hand an invalid main-file header to SQLite alongside an old WAL.
    // Even a failed open must leave the original evidence untouched.
    let mut header=[0u8;16];
    fs::File::open(path)?.read_exact(&mut header).map_err(|_|CoreError::new("DATABASE_CORRUPT","数据库文件头不完整，原文件已保留"))?;
    if &header!=b"SQLite format 3\0" {return Err(CoreError::new("DATABASE_CORRUPT","数据库文件格式损坏，原文件已保留"));}
    let db=Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,true)?;
    Ok(db)
}

pub fn database_version(root: &Path) -> Result<Option<u32>> {
    let path = root.join("core.sqlite");
    if !path.try_exists()? { return Ok(None); }
    let db = read_only(&path)?;
    Ok(Some(db.pragma_query_value(None, "user_version", |r| r.get(0))?))
}

pub(crate) fn ensure_current(root: &Path) -> Result<()> {
    if root.parent().is_some_and(|p| p.join(JOURNAL).exists()) { return Err(recovery_error()); }
    match database_version(root)? {
        None => {
            // A detached WAL is not a fresh database. Do not replace it with an
            // empty main file and let SQLite replay an unrelated generation.
            if root.join("core.sqlite-wal").exists() || root.join("core.sqlite-shm").exists() { return Err(recovery_error()); }
            fs::create_dir_all(root)?;
            initialize(&root.join("core.sqlite"))
        },
        Some(SCHEMA_VERSION) => Ok(()),
        Some(version) => Err(CoreError::new("UNSUPPORTED_DATA_VERSION", "数据库结构版本不一致，需由应用启动流程提示并备份重建")
            .details(serde_json::json!({"actualVersion":version,"expectedVersion":SCHEMA_VERSION}))),
    }
}

fn initialize(path: &Path) -> Result<()> {
    let version=SCHEMA_VERSION;
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
      CREATE TABLE servers(id TEXT PRIMARY KEY, data TEXT NOT NULL);
      CREATE TABLE host_keys(endpoint TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, algorithm TEXT NOT NULL);
      CREATE TABLE settings(namespace TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(namespace,key));
      CREATE TABLE socks_proxies(id TEXT PRIMARY KEY,data TEXT NOT NULL,revision INTEGER NOT NULL);
      CREATE TABLE server_route_steps(
        target_server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
        ordinal INTEGER NOT NULL CHECK(ordinal>=0), step_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('ssh','socks5')),
        ssh_server_id TEXT REFERENCES servers(id) ON DELETE RESTRICT,
        proxy_id TEXT REFERENCES socks_proxies(id) ON DELETE RESTRICT, resolve TEXT,
        PRIMARY KEY(target_server_id,ordinal), UNIQUE(target_server_id,step_id), UNIQUE(target_server_id,ssh_server_id),
        CHECK((kind='ssh' AND ssh_server_id IS NOT NULL AND proxy_id IS NULL AND resolve IS NULL AND ssh_server_id<>target_server_id)
          OR (kind='socks5' AND proxy_id IS NOT NULL AND ssh_server_id IS NULL AND resolve IS NOT NULL AND resolve IN ('proxy','local'))));
      CREATE INDEX route_ssh_dependents ON server_route_steps(ssh_server_id);
      CREATE INDEX route_proxy_dependents ON server_route_steps(proxy_id);
      CREATE TABLE credential_bindings(owner_kind TEXT NOT NULL CHECK(owner_kind IN ('ssh','socks5')),owner_id TEXT NOT NULL,
        context_key TEXT NOT NULL,secret_id TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL CHECK(revision>0),
        PRIMARY KEY(owner_kind,owner_id,context_key));
      CREATE TRIGGER server_credentials_delete AFTER DELETE ON servers BEGIN
        DELETE FROM credential_bindings WHERE owner_kind='ssh' AND owner_id=OLD.id;
      END;
      CREATE TRIGGER proxy_credentials_delete AFTER DELETE ON socks_proxies BEGIN
        DELETE FROM credential_bindings WHERE owner_kind='socks5' AND owner_id=OLD.id;
      END;
      CREATE TABLE server_groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,name_key TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL);
      CREATE TABLE server_group_membership(server_id TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,group_id TEXT REFERENCES server_groups(id) ON DELETE SET NULL,revision INTEGER NOT NULL);
      CREATE TABLE server_catalog_receipts(operation_id TEXT PRIMARY KEY,caller_key TEXT NOT NULL,request_hash TEXT NOT NULL,result TEXT NOT NULL,created_at INTEGER NOT NULL);
      INSERT INTO settings(namespace,key,value) VALUES('@core','serverCatalogRevision','\"0\"');
      COMMIT;")?;
    db.pragma_update(None, "user_version", version)?;
    drop(db);
    sync_file(path)?;
    validate_database(path, version)
}

fn validate_database(path: &Path, version: u32) -> Result<()> {
    let db = read_only(path)?;
    let actual: u32 = db.pragma_query_value(None,"user_version",|r|r.get(0))?;
    let check: String = db.query_row("PRAGMA integrity_check", [], |r|r.get(0))?;
    if actual != version || check != "ok" { return Err(recovery_error()); }
    Ok(())
}

fn fingerprint(path: &Path) -> Result<String> {
    let mut file=fs::File::open(path)?;
    let mut hash=Sha256::new();
    let mut bytes=[0u8;65536];
    loop { let n=file.read(&mut bytes)?; if n==0 {break} hash.update(&bytes[..n]); }
    Ok(format!("{:x}",hash.finalize()))
}

fn sync_file(path: &Path) -> Result<()> {
    // Windows FlushFileBuffers requires a writable handle, even when no bytes
    // are changed by the flush itself.
    fs::OpenOptions::new().write(true).open(path)?.sync_all()?;
    Ok(())
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let pending=path.with_extension("pending");
    let mut file=fs::File::create(&pending)?;
    file.write_all(&serde_json::to_vec(value)?)?; file.sync_all()?; drop(file);
    crate::secrets::replace_file(&pending,path)
}

/// The caller holds the exclusive data lock. Only the current dataset journal
/// is understood; unknown recovery formats leave originals untouched.
pub fn prepare_database(data: &Path, notify: impl FnOnce(&ResetNotice)->Result<()>) -> Result<Option<PathBuf>> {
    dataset::prepare(data,notify,|_|Ok(()))
}

fn transfer_once(source:&Path,target:&Path)->Result<()> {
    match (source.try_exists()?,target.try_exists()?) {
        (true,false)=>{fs::rename(source,target)?;Ok(())},
        (false,true)=>Ok(()),
        _=>Err(recovery_error()),
    }
}

fn valid_hash(value:&str)->bool {value.len()==64&&value.bytes().all(|c|c.is_ascii_hexdigit())}
fn secret_member(name:&str)->bool {
    name.strip_prefix("secrets/").is_some_and(|tail|{let tail=tail.strip_prefix("proxies/").unwrap_or(tail);!tail.contains(['\\',':','/'])&&matches!(Path::new(tail).components().next(),Some(std::path::Component::Normal(_)))})
}
fn secret_files(root:&Path)->Result<Vec<(String,PathBuf)>> {
    let mut files=Vec::new();
    for entry in fs::read_dir(root)? {let entry=entry?;let name=entry.file_name().into_string().map_err(|_|recovery_error())?;
        if entry.file_type()?.is_dir()&&name=="proxies" {
            for child in fs::read_dir(entry.path())?{let child=child?;let key=format!("secrets/proxies/{}",child.file_name().to_str().ok_or_else(recovery_error)?);if !child.file_type()?.is_file()||!secret_member(&key){return Err(recovery_error());}files.push((key,child.path()));}
        }else{let key=format!("secrets/{name}");if !entry.file_type()?.is_file()||!secret_member(&key){return Err(recovery_error());}files.push((key,entry.path()));}
    }
    Ok(files)
}
fn verify_secrets(data:&Path,backup:&Path,active:&Path,retired:&Path,originals:&[String],files:&[Fingerprint])->Result<()> {
    if !originals.iter().any(|name|name=="secrets") {return Ok(());}
    let root=if retired.join("secrets").exists(){retired.join("secrets")}else{active.join("secrets")};
    dataset::checked(data,root.strip_prefix(data).map_err(|_|recovery_error())?)?;
    let expected:std::collections::HashMap<_,_>=files.iter().filter(|f|f.name.starts_with("secrets/")).map(|f|(f.name.as_str(),f.sha256.as_str())).collect();
    let source=secret_files(&root)?;
    for (key,path) in &source {
        dataset::checked(data,path.strip_prefix(data).map_err(|_|recovery_error())?)?;
        dataset::checked(data,backup.join(key).strip_prefix(data).map_err(|_|recovery_error())?)?;
        let hash=expected.get(key.as_str()).ok_or_else(recovery_error)?;
        if fingerprint(path)?!=*hash||fingerprint(&backup.join(key))?!=*hash{return Err(recovery_error());}
    }
    if source.len()!=expected.len(){return Err(recovery_error());}Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn old_data(data:&Path,version:u32) {
        let core=data.join("core");fs::create_dir_all(core.join("secrets")).unwrap();
        let db=Connection::open(core.join("core.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE evidence(value TEXT); INSERT INTO evidence VALUES('retained');").unwrap();
        db.pragma_update(None,"user_version",version).unwrap();
        fs::write(core.join("secrets/test.dpapi"),b"encrypted-test-data").unwrap();
        fs::write(data.join("appearance-preferences.json"),b"keep").unwrap();
    }
    #[test]
    fn fresh_same_and_mismatched_versions() {
        for version in [0,3,4,5,6,8] {
            let dir=tempfile::tempdir().unwrap();old_data(dir.path(),version);
            assert!(ensure_current(&dir.path().join("core")).is_err());
            let backup=prepare_database(dir.path(),|n|{assert_eq!(n.previous_version,version);assert!(!n.recovering);Ok(())}).unwrap().unwrap();
            let db=read_only(&backup.join("core/core.sqlite")).unwrap();
            assert_eq!(db.query_row("SELECT value FROM evidence",[],|r|r.get::<_,String>(0)).unwrap(),"retained");
            assert_eq!(fs::read(backup.join("core/secrets/test.dpapi")).unwrap(),b"encrypted-test-data");
            assert_eq!(fs::read(dir.path().join("appearance-preferences.json")).unwrap(),b"keep");
            let core=crate::Core::open_dataset(dir.path()).unwrap();
            assert_eq!(core.call(&crate::Caller::shell(),"servers.list",serde_json::json!({})).unwrap(),serde_json::json!({"servers":[]}));
            drop(core);
            assert_eq!(fs::read_dir(dir.path().join("core/secrets")).unwrap().count(),0);
            assert!(prepare_database(dir.path(),|_|panic!("same version should not notify")).unwrap().is_none());
        }
        let dir=tempfile::tempdir().unwrap();
        assert!(prepare_database(dir.path(),|_|panic!("fresh should not notify")).unwrap().is_none());
    }
    #[test]
    fn committed_wal_is_in_backup_and_unrelated_files_survive() {
        let dir=tempfile::tempdir().unwrap();old_data(dir.path(),3);
        let path=dir.path().join("core/core.sqlite");
        let db=Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;INSERT INTO evidence VALUES('wal');").unwrap();
        // Stop before publication while this test connection is deliberately
        // open. Production holds the app lock and has no other writers.
        let error=dataset::prepare(dir.path(),|_|Ok(()),|phase|if phase=="backupVerified"{Err(recovery_error())}else{Ok(())});
        assert!(error.is_err());drop(db);
        // The backup is independent of sidecars disappearing on last close.
        let journal:serde_json::Value=serde_json::from_slice(&fs::read(dir.path().join(JOURNAL)).unwrap()).unwrap();
        let directory=dir.path().join("backups/core-schema-reset").join(format!("{}-{}",journal["createdAt"].as_u64().unwrap(),journal["operationId"].as_str().unwrap()));
        let backup=read_only(&directory.join("core/core.sqlite")).unwrap();
        assert_eq!(backup.query_row("SELECT count(*) FROM evidence",[],|r|r.get::<_,u32>(0)).unwrap(),2);
        assert!(path.exists());
    }
    #[test]
    fn interruption_at_every_publication_boundary_recovers_once() {
        for stop in ["notice","backupVerified","ready","retired:core/core.sqlite","retired:core/secrets","published:core/core.sqlite","published:core/secrets","committed"] {
            let dir=tempfile::tempdir().unwrap();old_data(dir.path(),3);
            assert!(dataset::prepare(dir.path(),|_|Ok(()),|phase|if phase==stop{Err(recovery_error())}else{Ok(())}).is_err(),"{stop}");
            assert!(crate::Core::open(dir.path().join("core")).is_err());
            let backup=prepare_database(dir.path(),|n|{assert!(n.recovering);Ok(())}).unwrap().unwrap();
            assert!(backup.join("core/core.sqlite").exists());
            assert_eq!(database_version(&dir.path().join("core")).unwrap(),Some(SCHEMA_VERSION));
            assert!(prepare_database(dir.path(),|_|panic!("repeated reset")).unwrap().is_none());
        }
    }
    #[test]
    fn corruption_or_backup_failure_keeps_original() {
        let dir=tempfile::tempdir().unwrap();old_data(dir.path(),3);
        let before=fs::read(dir.path().join("core/core.sqlite")).unwrap();
        fs::write(dir.path().join("backups"),b"blocked").unwrap();
        assert!(prepare_database(dir.path(),|_|Ok(())).is_err());
        assert_eq!(fs::read(dir.path().join("core/core.sqlite")).unwrap(),before);
        fs::write(dir.path().join(JOURNAL),b"invalid").unwrap();
        assert!(prepare_database(dir.path(),|_|Ok(())).is_err());
        assert_eq!(fs::read(dir.path().join("core/core.sqlite")).unwrap(),before);
        let broken=tempfile::tempdir().unwrap();fs::create_dir(broken.path().join("core")).unwrap();
        fs::write(broken.path().join("core/core.sqlite"),b"not sqlite").unwrap();
        assert!(prepare_database(broken.path(),|_|panic!("not a version mismatch")).is_err());
        assert_eq!(fs::read(broken.path().join("core/core.sqlite")).unwrap(),b"not sqlite");
    }
    #[test]
    fn corrupt_header_with_committed_wal_is_never_repaired_in_place() {
        let dir=tempfile::tempdir().unwrap();fs::create_dir(dir.path().join("core")).unwrap();
        let path=dir.path().join("core/core.sqlite");let db=Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;CREATE TABLE evidence(value TEXT);INSERT INTO evidence VALUES('original WAL');PRAGMA user_version=3").unwrap();
        db.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,true).unwrap();drop(db);
        let wal=dir.path().join("core/core.sqlite-wal");let bytes=fs::read(&wal).unwrap();assert!(!bytes.is_empty());
        fs::write(&path,b"corrupt header").unwrap();
        assert!(prepare_database(dir.path(),|_|panic!("corruption is not a schema mismatch")).is_err());
        assert_eq!(fs::read(&path).unwrap(),b"corrupt header");assert_eq!(fs::read(&wal).unwrap(),bytes);
    }
}
