//! Database replacement is a startup operation, before Core or any worker exists.
//! The desktop owns the data-directory lock and the visible notice. Core::open
//! never silently resets a mismatched database. Recovery always rolls forward
//! from verified copies, with all original files retained beside the backup.
use crate::{model::{id, now_ms}, CoreError, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}};

pub const SCHEMA_VERSION: u32 = 4;
const JOURNAL: &str = "core-reset.json";
const ACTIVE_FILES: &[&str] = &["core.sqlite", "core.sqlite-wal", "core.sqlite-shm", "secrets"];

#[derive(Debug)]
pub struct ResetNotice {
    pub previous_version: u32,
    pub current_version: u32,
    pub backup_path: PathBuf,
    pub recovering: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all="camelCase")]
enum Phase { BackingUp, BackupVerified, Ready, Retiring, Publishing, Committed }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Fingerprint { name: String, sha256: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Journal {
    operation_id: String,
    created_at: u64,
    previous_version: u32,
    target_version: u32,
    phase: Phase,
    originals: Vec<String>,
    backup_files: Vec<Fingerprint>,
    new_database_hash: Option<String>,
}

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
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
      CREATE TABLE servers(id TEXT PRIMARY KEY, data TEXT NOT NULL);
      CREATE TABLE host_keys(endpoint TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, algorithm TEXT NOT NULL);
      CREATE TABLE settings(namespace TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(namespace,key));
      CREATE TABLE calls(id TEXT PRIMARY KEY, started INTEGER NOT NULL, ended INTEGER, caller TEXT NOT NULL, method TEXT NOT NULL, server_id TEXT, summary TEXT NOT NULL, status TEXT NOT NULL, error_code TEXT);
      CREATE INDEX calls_started ON calls(started);
      CREATE TABLE server_routes(server_id TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,data TEXT NOT NULL,jump_server_id TEXT REFERENCES servers(id) ON DELETE RESTRICT);
      CREATE INDEX server_routes_jump ON server_routes(jump_server_id);
      CREATE TABLE server_groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,name_key TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL);
      CREATE TABLE server_group_membership(server_id TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,group_id TEXT REFERENCES server_groups(id) ON DELETE SET NULL,revision INTEGER NOT NULL);
      CREATE TABLE server_catalog_receipts(operation_id TEXT PRIMARY KEY,caller_key TEXT NOT NULL,request_hash TEXT NOT NULL,result TEXT NOT NULL,created_at INTEGER NOT NULL);
      INSERT INTO settings(namespace,key,value) VALUES('@core','serverCatalogRevision','\"0\"');
      PRAGMA user_version=4; COMMIT;")?;
    drop(db);
    sync_file(path)?;
    validate_database(path, SCHEMA_VERSION)
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

impl Journal {
    fn directory(&self, data: &Path) -> PathBuf {
        data.join("backups/core-schema-reset").join(format!("{}-{}",self.created_at,self.operation_id))
    }
    fn save(&self,data:&Path)->Result<()> { atomic_json(&data.join(JOURNAL),self) }
    fn advance(&mut self,data:&Path,phase:Phase)->Result<()> {self.phase=phase;self.save(data)}
    fn validate(&self)->Result<()> {
        if uuid::Uuid::parse_str(&self.operation_id).is_err() || self.target_version!=SCHEMA_VERSION
            || !self.originals.iter().any(|n|n=="core.sqlite")
            || self.originals.iter().any(|n|!ACTIVE_FILES.contains(&n.as_str()))
            || self.originals.iter().collect::<std::collections::HashSet<_>>().len()!=self.originals.len()
            || self.backup_files.iter().any(|f| f.name!="core.sqlite" && !(f.name.starts_with("secrets/") && Path::new(&f.name[8..]).components().count()==1 && !f.name[8..].contains(['\\',':']))) {
            return Err(recovery_error());
        }
        Ok(())
    }
}

/// The caller must hold the application's exclusive data-directory lock.
/// notify runs once before a new reset, and again when resuming an interrupted
/// one. Returning an error leaves all active files untouched.
pub fn prepare_database(data: &Path, notify: impl FnOnce(&ResetNotice)->Result<()>) -> Result<Option<PathBuf>> {
    prepare_with_hook(data,notify,|_|Ok(()))
}

fn prepare_with_hook(data: &Path, notify: impl FnOnce(&ResetNotice)->Result<()>, mut hook: impl FnMut(&str)->Result<()>) -> Result<Option<PathBuf>> {
    let core=data.join("core");
    let recovering=data.join(JOURNAL).try_exists()?;
    let mut journal=if recovering {
        let journal:Journal=serde_json::from_slice(&fs::read(data.join(JOURNAL))?).map_err(|_|recovery_error())?;
        journal.validate()?; journal
    } else {
        let Some(version)=database_version(&core)? else {ensure_current(&core)?;return Ok(None)};
        if version==SCHEMA_VERSION {return Ok(None)}
        Journal { operation_id:id(),created_at:now_ms(),previous_version:version,target_version:SCHEMA_VERSION,phase:Phase::BackingUp,
            originals:ACTIVE_FILES.iter().filter(|n|core.join(n).exists()).map(|n|n.to_string()).collect(),backup_files:vec![],new_database_hash:None }
    };
    let directory=journal.directory(data);
    notify(&ResetNotice {previous_version:journal.previous_version,current_version:SCHEMA_VERSION,backup_path:directory.clone(),recovering})?;
    fs::create_dir_all(&directory)?;
    journal.save(data)?;
    hook("notice")?;
    if journal.phase==Phase::BackingUp {
        let path=directory.join("core.sqlite");
        // Only this operation's incomplete copy may be replaced on retry.
        if path.exists() {fs::remove_file(&path)?;}
        let db=read_only(&core.join("core.sqlite"))?;
        let actual:u32=db.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if actual!=journal.previous_version {return Err(recovery_error());}
        db.backup("main",&path,None)?;
        drop(db);
        sync_file(&path)?;
        validate_database(&path,journal.previous_version)?;
        journal.backup_files=vec![Fingerprint{name:"core.sqlite".into(),sha256:fingerprint(&path)?}];
        if core.join("secrets").exists() {
            fs::create_dir_all(directory.join("secrets"))?;
            for entry in fs::read_dir(core.join("secrets"))? {
                let entry=entry?;
                if !entry.file_type()?.is_file() {return Err(recovery_error());}
                let name=entry.file_name().into_string().map_err(|_|recovery_error())?;
                let target=directory.join("secrets").join(&name);
                fs::copy(entry.path(),&target)?; sync_file(&target)?;
                let hash=fingerprint(&entry.path())?;
                if hash!=fingerprint(&target)? {return Err(recovery_error());}
                journal.backup_files.push(Fingerprint{name:format!("secrets/{name}"),sha256:hash});
            }
        }
        // SQLite may have removed or created sidecars as the last connection
        // closed; capture the actual set only once backup connections are gone.
        journal.originals=ACTIVE_FILES.iter().filter(|n|core.join(n).exists()).map(|n|n.to_string()).collect();
        atomic_json(&directory.join("backup.json"),&journal)?;
        journal.advance(data,Phase::BackupVerified)?; hook("backupVerified")?;
    }
    for file in &journal.backup_files {
        if fingerprint(&directory.join(&file.name))?!=file.sha256 {return Err(recovery_error());}
    }
    if journal.backup_files.is_empty() {return Err(recovery_error());}
    let staged=directory.join("new");
    if journal.phase==Phase::BackupVerified {
        fs::create_dir_all(staged.join("secrets"))?;
        let path=staged.join("core.sqlite");
        if path.exists() {fs::remove_file(&path)?;}
        initialize(&path)?;
        journal.new_database_hash=Some(fingerprint(&path)?);
        journal.advance(data,Phase::Ready)?; hook("ready")?;
    }
    if journal.phase==Phase::Ready { journal.advance(data,Phase::Retiring)?; }
    let retired=directory.join("originals");
    if journal.phase==Phase::Retiring {
        fs::create_dir_all(&retired)?;
        for name in &journal.originals {
            transfer_once(&core.join(name),&retired.join(name))?;
            hook(&format!("retired:{name}"))?;
        }
        journal.advance(data,Phase::Publishing)?;
    }
    if journal.phase==Phase::Publishing {
        let hash=journal.new_database_hash.as_ref().ok_or_else(recovery_error)?;
        let path=if staged.join("core.sqlite").exists(){staged.join("core.sqlite")}else{core.join("core.sqlite")};
        if &fingerprint(&path)?!=hash {return Err(recovery_error());}
        for name in ["core.sqlite","secrets"] {
            transfer_once(&staged.join(name),&core.join(name))?;
            hook(&format!("published:{name}"))?;
        }
        validate_database(&core.join("core.sqlite"),SCHEMA_VERSION)?;
        if fs::read_dir(core.join("secrets"))?.next().is_some() {return Err(recovery_error());}
        journal.advance(data,Phase::Committed)?; hook("committed")?;
    }
    if journal.phase!=Phase::Committed {return Err(recovery_error());}
    validate_database(&core.join("core.sqlite"),SCHEMA_VERSION)?;
    atomic_json(&directory.join("completed.json"),&journal)?;
    fs::remove_file(data.join(JOURNAL))?;
    Ok(Some(directory))
}

fn transfer_once(source:&Path,target:&Path)->Result<()> {
    match (source.try_exists()?,target.try_exists()?) {
        (true,false)=>{fs::rename(source,target)?;Ok(())},
        (false,true)=>Ok(()),
        _=>Err(recovery_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn old_data(data:&Path,version:u32) {
        let core=data.join("core");fs::create_dir_all(core.join("secrets")).unwrap();
        let db=Connection::open(core.join("core.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE evidence(value TEXT); INSERT INTO evidence VALUES('retained');").unwrap();
        db.pragma_update(None,"user_version",version).unwrap();
        fs::write(core.join("secrets/test.dpapi"),b"encrypted-test-data").unwrap();
        fs::write(data.join("appearance-preferences.json"),b"keep").unwrap();
    }
    #[test]
    fn fresh_same_and_mismatched_versions() {
        for version in [0,3,5] {
            let dir=tempfile::tempdir().unwrap();old_data(dir.path(),version);
            assert!(ensure_current(&dir.path().join("core")).is_err());
            let backup=prepare_database(dir.path(),|n|{assert_eq!(n.previous_version,version);assert!(!n.recovering);Ok(())}).unwrap().unwrap();
            let db=read_only(&backup.join("core.sqlite")).unwrap();
            assert_eq!(db.query_row("SELECT value FROM evidence",[],|r|r.get::<_,String>(0)).unwrap(),"retained");
            assert_eq!(fs::read(backup.join("secrets/test.dpapi")).unwrap(),b"encrypted-test-data");
            assert_eq!(fs::read(dir.path().join("appearance-preferences.json")).unwrap(),b"keep");
            let core=crate::Core::open(dir.path().join("core")).unwrap();
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
        let error=prepare_with_hook(dir.path(),|_|Ok(()),|phase|if phase=="backupVerified"{Err(recovery_error())}else{Ok(())});
        assert!(error.is_err());drop(db);
        // The backup is independent of sidecars disappearing on last close.
        let journal:Journal=serde_json::from_slice(&fs::read(dir.path().join(JOURNAL)).unwrap()).unwrap();
        let backup=read_only(&journal.directory(dir.path()).join("core.sqlite")).unwrap();
        assert_eq!(backup.query_row("SELECT count(*) FROM evidence",[],|r|r.get::<_,u32>(0)).unwrap(),2);
        assert!(path.exists());
    }
    #[test]
    fn interruption_at_every_publication_boundary_recovers_once() {
        for stop in ["notice","backupVerified","ready","retired:core.sqlite","retired:secrets","published:core.sqlite","published:secrets","committed"] {
            let dir=tempfile::tempdir().unwrap();old_data(dir.path(),3);
            assert!(prepare_with_hook(dir.path(),|_|Ok(()),|phase|if phase==stop{Err(recovery_error())}else{Ok(())}).is_err(),"{stop}");
            assert!(crate::Core::open(dir.path().join("core")).is_err());
            let backup=prepare_database(dir.path(),|n|{assert!(n.recovering);Ok(())}).unwrap().unwrap();
            assert!(backup.join("core.sqlite").exists());
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
