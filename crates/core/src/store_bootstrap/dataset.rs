//! Version-2 journal: all selected members are verified before any is retired.
use super::*;
use crate::observability::schema::{self,Partition,PARTITIONS};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Member {
    path:String,previous_version:Option<u32>,target_version:u32,originals:Vec<String>,
    backup_files:Vec<Fingerprint>,new_database_hash:Option<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct DatasetJournal {
    journal_version:u32,operation_id:String,created_at:u64,phase:Phase,store_epoch:String,members:Vec<Member>,
}
impl DatasetJournal {
    fn directory(&self,data:&Path)->PathBuf {data.join("backups/core-schema-reset").join(format!("{}-{}",self.created_at,self.operation_id))}
    fn save(&self,data:&Path)->Result<()> {atomic_json(&data.join(JOURNAL),self)}
    fn advance(&mut self,data:&Path,phase:Phase)->Result<()> {self.phase=phase;self.save(data)}
    fn validate(&self)->Result<()> {
        if self.journal_version!=2||uuid::Uuid::parse_str(&self.operation_id).is_err()||uuid::Uuid::parse_str(&self.store_epoch).is_err()||self.members.is_empty()||self.members.len()>5{return Err(recovery_error());}
        let mut seen=std::collections::HashSet::new();
        for member in &self.members {
            let partition=partition(&member.path);
            if !seen.insert(&member.path)||(member.path!="core/core.sqlite"&&partition.is_none())||member.target_version!=if partition.is_some(){schema::VERSION}else{SCHEMA_VERSION}{return Err(recovery_error());}
            let name=Path::new(&member.path).file_name().unwrap().to_str().unwrap();
            let allowed=[name.to_string(),format!("{name}-wal"),format!("{name}-shm"),"secrets".into()];
            if member.originals.iter().any(|s|!allowed.contains(s)||(s=="secrets"&&partition.is_some()))||member.originals.iter().collect::<std::collections::HashSet<_>>().len()!=member.originals.len(){return Err(recovery_error());}
            for file in &member.backup_files {if (file.name!=name&&!(partition.is_none()&&secret_member(&file.name)))||!valid_hash(&file.sha256){return Err(recovery_error());}}
            if member.backup_files.iter().map(|f|&f.name).collect::<std::collections::HashSet<_>>().len()!=member.backup_files.len(){return Err(recovery_error());}
            if member.previous_version.is_some()!=member.originals.iter().any(|n|n==name){return Err(recovery_error());}
            if member.previous_version.is_none()&&(!member.originals.is_empty()||!member.backup_files.is_empty()){return Err(recovery_error());}
            if self.phase!=Phase::BackingUp&&member.previous_version.is_some()&&!member.backup_files.iter().any(|f|f.name==name){return Err(recovery_error());}
            if !matches!(self.phase,Phase::BackingUp|Phase::BackupVerified)&&!member.new_database_hash.as_deref().is_some_and(valid_hash){return Err(recovery_error());}
        }
        let count=self.members.iter().filter(|m|partition(&m.path).is_some()).count();if count!=0&&count!=4{return Err(recovery_error());}
        Ok(())
    }
}
fn partition(path:&str)->Option<Partition> {PARTITIONS.into_iter().find(|p|path==format!("observability/{}",p.file()))}
fn reject_links(path:&Path)->Result<()> {
    if !path.try_exists()? {return Ok(());}
    let metadata=fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink(){return Err(recovery_error());}
    #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Err(recovery_error());}}
    Ok(())
}
pub(super) fn checked(data:&Path,relative:&Path)->Result<PathBuf> {
    if relative.is_absolute(){return Err(recovery_error());}
    reject_links(data)?;let mut path=data.to_owned();
    for part in relative.components(){let std::path::Component::Normal(part)=part else{return Err(recovery_error());};path.push(part);reject_links(&path)?;}
    Ok(path)
}
fn version(data:&Path,path:&str)->Result<Option<u32>> {
    let path=checked(data,Path::new(path))?;
    if !path.exists(){
        for suffix in ["-wal","-shm"]{if PathBuf::from(format!("{}{suffix}",path.display())).exists(){return Err(recovery_error());}}
        return Ok(None);
    }
    let db=read_only(&path)?;let value=db.pragma_query_value(None,"user_version",|r|r.get(0))?;
    let integrity:String=db.query_row("PRAGMA quick_check",[],|r|r.get(0))?;if integrity!="ok"{return Err(recovery_error());}Ok(Some(value))
}
fn member(data:&Path,path:String,previous_version:Option<u32>,target_version:u32)->Result<Member> {
    let source=Path::new(&path);let parent=source.parent().unwrap();let name=source.file_name().unwrap().to_str().unwrap();
    let mut names=vec![name.to_owned(),format!("{name}-wal"),format!("{name}-shm")];if path=="core/core.sqlite"{names.push("secrets".into());}
    let mut originals=vec![];for name in names {if checked(data,&parent.join(&name))?.exists(){originals.push(name);}}
    if previous_version.is_none()&&!originals.is_empty(){return Err(recovery_error());}
    Ok(Member{path,previous_version,target_version,originals,backup_files:vec![],new_database_hash:None})
}

pub(super) fn prepare(data:&Path,notify:impl FnOnce(&ResetNotice)->Result<()>,mut hook:impl FnMut(&str)->Result<()>)->Result<Option<PathBuf>> {
    fs::create_dir_all(data)?;let journal_path=checked(data,Path::new(JOURNAL))?;let recovering=journal_path.exists();
    let mut journal=if recovering {
        let value:DatasetJournal=serde_json::from_slice(&fs::read(&journal_path)?).map_err(|_|recovery_error())?;value.validate()?;value
    }else{
        let core_version=version(data,"core/core.sqlite")?;
        let mut observed=vec![];for p in PARTITIONS {observed.push((p,version(data,&format!("observability/{}",p.file()))?));}
        let present=observed.iter().filter(|(_,v)|v.is_some()).count();if present!=0&&present!=4{return Err(recovery_error());}
        let mut members=vec![];
        if core_version!=Some(SCHEMA_VERSION){members.push(member(data,"core/core.sqlite".into(),core_version,SCHEMA_VERSION)?);}
        if observed.iter().any(|(_,v)|*v!=Some(schema::VERSION)) {for (p,v) in observed {members.push(member(data,format!("observability/{}",p.file()),v,schema::VERSION)?);}}
        if members.is_empty(){return Ok(None);}
        DatasetJournal{journal_version:2,operation_id:id(),created_at:now_ms(),phase:Phase::BackingUp,store_epoch:id(),members}
    };
    let directory=journal.directory(data);let relative=directory.strip_prefix(data).map_err(|_|recovery_error())?;checked(data,relative)?;
    let replaces=journal.members.iter().any(|m|m.previous_version.is_some());
    if replaces||recovering {notify(&ResetNotice{previous_version:journal.members.iter().find(|m|m.path=="core/core.sqlite").and_then(|m|m.previous_version).unwrap_or(SCHEMA_VERSION),current_version:SCHEMA_VERSION,backup_path:directory.clone(),recovering,
        rebuild_core:journal.members.iter().any(|m|m.path=="core/core.sqlite"),rebuild_observability:journal.members.iter().any(|m|partition(&m.path).is_some()),observation_versions:journal.members.iter().filter(|m|partition(&m.path).is_some()).map(|m|(m.path.clone(),m.previous_version,m.target_version)).collect()})?;}
    fs::create_dir_all(&directory)?;journal.save(data)?;hook("notice")?;
    if journal.phase==Phase::BackingUp {
        for member in &mut journal.members {
            let source=checked(data,Path::new(&member.path))?;let backup=checked(data,&relative.join(&member.path))?;
            fs::create_dir_all(backup.parent().unwrap())?;
            member.backup_files.clear();
            if let Some(previous)=member.previous_version {
                if backup.exists(){fs::remove_file(&backup)?;}
                let db=read_only(&source)?;db.backup("main",&backup,None)?;drop(db);sync_file(&backup)?;validate_database(&backup,previous)?;
                member.backup_files.push(Fingerprint{name:source.file_name().unwrap().to_str().unwrap().into(),sha256:fingerprint(&backup)?});
                if member.path=="core/core.sqlite"&&source.parent().unwrap().join("secrets").exists(){
                    let secrets=source.parent().unwrap().join("secrets");reject_links(&secrets)?;
                    fs::create_dir_all(backup.parent().unwrap().join("secrets"))?;
                    for (name,source) in secret_files(&secrets)? {
                        checked(data,source.strip_prefix(data).map_err(|_|recovery_error())?)?;
                        let copy=checked(data,&relative.join("core").join(&name))?;fs::create_dir_all(copy.parent().unwrap())?;fs::copy(&source,&copy)?;sync_file(&copy)?;
                        let hash=fingerprint(&source)?;if fingerprint(&copy)?!=hash{return Err(recovery_error());}
                        member.backup_files.push(Fingerprint{name,sha256:hash});
                    }
                }
            }
            // SQLite can retire empty sidecars on last close; enumerate after backup.
            member.originals=crate::store_bootstrap::dataset::member(data,member.path.clone(),member.previous_version,member.target_version)?.originals;
        }
        atomic_json(&directory.join("backup.json"),&journal)?;journal.advance(data,Phase::BackupVerified)?;hook("backupVerified")?;
    }
    for member in &journal.members {
        if member.previous_version.is_some()&&member.backup_files.is_empty(){return Err(recovery_error());}
        let base=relative.join(Path::new(&member.path).parent().unwrap());
        for file in &member.backup_files {let path=checked(data,&base.join(&file.name))?;if fingerprint(&path)?!=file.sha256{return Err(recovery_error());}}
        if member.path=="core/core.sqlite" {verify_secrets(data,&directory.join("core"),&data.join("core"),&directory.join("originals/core"),&member.originals,&member.backup_files)?;}
    }
    if journal.phase==Phase::BackupVerified {
        for member in &mut journal.members {
            let path=checked(data,&relative.join("new").join(&member.path))?;fs::create_dir_all(path.parent().unwrap())?;
            if path.exists(){fs::remove_file(&path)?;}
            if let Some(partition)=partition(&member.path){schema::initialize(&path,partition,&journal.store_epoch)?;}else{initialize(&path)?;fs::create_dir_all(path.parent().unwrap().join("secrets"))?;}
            member.new_database_hash=Some(fingerprint(&path)?);
        }
        journal.advance(data,Phase::Ready)?;hook("ready")?;
    }
    if journal.phase==Phase::Ready{
        for member in &journal.members {
            let path=checked(data,&relative.join("new").join(&member.path))?;
            if Some(fingerprint(&path)?)!=member.new_database_hash{return Err(recovery_error());}
            validate_database(&path,member.target_version)?;
        }
        journal.advance(data,Phase::Retiring)?;
    }
    if journal.phase==Phase::Retiring {
        for member in &journal.members {
            let parent=Path::new(&member.path).parent().unwrap();
            for name in &member.originals {
                let source=checked(data,&parent.join(name))?;let retired=checked(data,&relative.join("originals").join(parent).join(name))?;
                fs::create_dir_all(retired.parent().unwrap())?;transfer_once(&source,&retired)?;hook(&format!("retired:{}/{}",parent.display(),name))?;
            }
        }
        journal.advance(data,Phase::Publishing)?;
    }
    if journal.phase==Phase::Publishing {
        for member in &journal.members {
            let staged=checked(data,&relative.join("new").join(&member.path))?;let active=checked(data,Path::new(&member.path))?;
            if Some(fingerprint(if staged.exists(){&staged}else{&active})?)!=member.new_database_hash{return Err(recovery_error());}
            fs::create_dir_all(active.parent().unwrap())?;transfer_once(&staged,&active)?;hook(&format!("published:{}",member.path))?;
            if member.path=="core/core.sqlite" {
                let fresh=checked(data,&relative.join("new/core/secrets"))?;let active_secrets=checked(data,Path::new("core/secrets"))?;
                transfer_once(&fresh,&active_secrets)?;
                if fs::read_dir(&active_secrets)?.next().is_some(){return Err(recovery_error());}
                hook("published:core/secrets")?;
            }
            validate_database(&active,member.target_version)?;
        }
        journal.advance(data,Phase::Committed)?;hook("committed")?;
    }
    if journal.phase!=Phase::Committed{return Err(recovery_error());}
    for member in &journal.members{validate_database(&checked(data,Path::new(&member.path))?,member.target_version)?;}
    atomic_json(&directory.join("completed.json"),&journal)?;fs::remove_file(journal_path)?;
    Ok(replaces.then_some(directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn old(data:&Path){super::super::tests::old_data(data,3);fs::create_dir_all(data.join("observability")).unwrap();for p in PARTITIONS{let db=Connection::open(data.join("observability").join(p.file())).unwrap();db.execute_batch("CREATE TABLE retained(value TEXT);INSERT INTO retained VALUES('original');PRAGMA user_version=9;").unwrap();}}
    #[test]
    fn dataset_restarts_every_boundary_without_mixing_members(){
        for boundary in ["notice","backupVerified","ready","retired:core/core.sqlite","retired:observability/facts.sqlite","published:core/core.sqlite","published:core/secrets","published:observability/traffic.sqlite","committed"] {
            let data=tempfile::tempdir().unwrap();old(data.path());
            fs::create_dir_all(data.path().join("core/secrets/proxies")).unwrap();fs::write(data.path().join("core/secrets/proxies/proxy.dpapi"),b"proxy-encrypted").unwrap();
            let failed=prepare(data.path(),|_|Ok(()),|stage|if stage==boundary{Err(recovery_error())}else{Ok(())});assert!(failed.is_err(),"{boundary}");
            let backup=prepare(data.path(),|n|{assert!(n.recovering);Ok(())},|_|Ok(())).unwrap().unwrap();
            assert_eq!(fs::read(backup.join("core/secrets/test.dpapi")).unwrap(),b"encrypted-test-data");
            assert_eq!(fs::read(backup.join("core/secrets/proxies/proxy.dpapi")).unwrap(),b"proxy-encrypted");
            let mut epochs=std::collections::HashSet::new();
            for p in PARTITIONS{
                let active=read_only(&data.path().join("observability").join(p.file())).unwrap();epochs.insert(active.query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get::<_,String>(0)).unwrap());
                let prior=read_only(&backup.join("observability").join(p.file())).unwrap();assert_eq!(prior.query_row("SELECT value FROM retained",[],|r|r.get::<_,String>(0)).unwrap(),"original");
            }
            assert_eq!(epochs.len(),1);assert!(prepare(data.path(),|_|panic!("duplicate notice"),|_|Ok(())).unwrap().is_none());
        }
    }
    #[test]
    fn altered_backup_or_journal_path_never_retires_originals(){
        let data=tempfile::tempdir().unwrap();old(data.path());
        assert!(prepare(data.path(),|_|Ok(()),|s|if s=="backupVerified"{Err(recovery_error())}else{Ok(())}).is_err());
        let mut journal:DatasetJournal=serde_json::from_slice(&fs::read(data.path().join(JOURNAL)).unwrap()).unwrap();
        let file=journal.directory(data.path()).join("core/secrets/test.dpapi");fs::write(file,b"changed").unwrap();
        assert!(prepare(data.path(),|_|Ok(()),|_|Ok(())).is_err());assert!(data.path().join("core/core.sqlite").exists());
        journal.members[0].path="../outside.sqlite".into();journal.save(data.path()).unwrap();
        assert!(prepare(data.path(),|_|Ok(()),|_|Ok(())).is_err());assert!(data.path().join("core/core.sqlite").exists());
    }
    #[test]
    fn schema_four_rebuild_has_no_calls_or_configuration_migration() {
        let data=tempfile::tempdir().unwrap();let root=data.path();
        fs::create_dir(root.join("core")).unwrap();initialize(&root.join("core/core.sqlite")).unwrap();
        let old=Connection::open(root.join("core/core.sqlite")).unwrap();
        old.execute_batch("PRAGMA user_version=4;CREATE TABLE calls(id TEXT,started INTEGER,ended INTEGER,caller TEXT,method TEXT,server_id TEXT,summary TEXT,status TEXT,error_code TEXT);INSERT INTO servers VALUES('old-server','{}');INSERT INTO settings VALUES('old','setting','true');INSERT INTO calls VALUES('old-call',1,2,'{}','old.method',NULL,'{}','succeeded',NULL);").unwrap();drop(old);
        let backup=prepare_database(root,|notice|{assert!(notice.rebuild_core&&notice.rebuild_observability);assert_eq!(notice.previous_version,4);Ok(())}).unwrap().unwrap();
        let core=crate::Core::open_dataset(root).unwrap();
        assert!(core.call(&crate::Caller::shell(),"servers.list",serde_json::json!({})).unwrap()["servers"].as_array().unwrap().is_empty());
        assert!(core.call(&crate::Caller::shell(),"calls.list",serde_json::json!({"method":"old.method"})).unwrap()["records"].as_array().unwrap().is_empty());
        let active=read_only(&root.join("core/core.sqlite")).unwrap();
        assert_eq!(active.query_row("SELECT count(*) FROM sqlite_master WHERE name='calls'",[],|r|r.get::<_,u32>(0)).unwrap(),0);
        let old=read_only(&backup.join("core/core.sqlite")).unwrap();assert_eq!(old.query_row("SELECT count(*) FROM calls",[],|r|r.get::<_,u32>(0)).unwrap(),1);
    }
    #[test]
    fn resetting_one_dataset_preserves_the_other_and_rejects_partial_members() {
        let data=tempfile::tempdir().unwrap();let root=data.path();prepare_database(root,|_|panic!("fresh")).unwrap();
        let obs=root.join("observability/facts.sqlite");let config=root.join("core/core.sqlite");
        let epoch=|path:&Path|read_only(path).unwrap().query_row("SELECT value FROM meta WHERE key='storeEpoch'",[],|r|r.get::<_,String>(0)).unwrap();
        let first=epoch(&obs);
        Connection::open(&config).unwrap().execute_batch("PRAGMA user_version=4").unwrap();
        prepare_database(root,|n|{assert!(n.rebuild_core&&!n.rebuild_observability);Ok(())}).unwrap();assert_eq!(epoch(&obs),first);
        Connection::open(&config).unwrap().execute_batch("INSERT INTO settings VALUES('keep','value','true')").unwrap();
        Connection::open(&obs).unwrap().execute_batch("PRAGMA user_version=99").unwrap();
        prepare_database(root,|n|{assert!(!n.rebuild_core&&n.rebuild_observability);Ok(())}).unwrap();assert_ne!(epoch(&obs),first);
        assert_eq!(read_only(&config).unwrap().query_row("SELECT value FROM settings WHERE namespace='keep'",[],|r|r.get::<_,String>(0)).unwrap(),"true");
        fs::rename(root.join("observability/logs.sqlite"),root.join("logs-retained.sqlite")).unwrap();
        assert!(prepare_database(root,|_|panic!("partial dataset must fail before notice")).is_err());
    }
    #[test]
    fn omitted_secret_backup_entry_is_rejected_before_retirement() {
        let data=tempfile::tempdir().unwrap();old(data.path());
        assert!(prepare(data.path(),|_|Ok(()),|s|if s=="backupVerified"{Err(recovery_error())}else{Ok(())}).is_err());
        let mut journal:DatasetJournal=serde_json::from_slice(&fs::read(data.path().join(JOURNAL)).unwrap()).unwrap();
        journal.members[0].backup_files.retain(|file|!file.name.starts_with("secrets/"));journal.save(data.path()).unwrap();
        assert!(prepare_database(data.path(),|_|Ok(())).is_err());assert!(data.path().join("core/secrets/test.dpapi").exists());
    }
    #[test]
    fn observation_one_to_two_preserves_current_core_credentials_and_committed_wal(){
        let data=tempfile::tempdir().unwrap();let root=data.path();prepare_database(root,|_|panic!("fresh")).unwrap();
        let config=root.join("core/core.sqlite");let db=Connection::open(&config).unwrap();db.execute_batch("INSERT INTO settings VALUES('fixture','keep','true')").unwrap();drop(db);
        fs::create_dir_all(root.join("core/secrets")).unwrap();fs::write(root.join("core/secrets/fixture.dpapi"),b"isolated-encrypted-fixture").unwrap();let core_hash=fingerprint(&config).unwrap();
        let mut held=Vec::new();for p in PARTITIONS{let db=Connection::open(root.join("observability").join(p.file())).unwrap();db.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;PRAGMA user_version=1;CREATE TABLE wal_fixture(value TEXT);INSERT INTO wal_fixture VALUES('committed-in-wal')").unwrap();db.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,true).unwrap();held.push(db);}drop(held);assert!(root.join("observability/facts.sqlite-wal").exists());
        let backup=prepare_database(root,|n|{assert!(!n.rebuild_core&&n.rebuild_observability);assert_eq!(n.previous_version,SCHEMA_VERSION);Ok(())}).unwrap().unwrap();
        assert_eq!(fingerprint(&config).unwrap(),core_hash);assert_eq!(fs::read(root.join("core/secrets/fixture.dpapi")).unwrap(),b"isolated-encrypted-fixture");assert!(!backup.join("core").exists());
        for p in PARTITIONS{let prior=read_only(&backup.join("observability").join(p.file())).unwrap();assert_eq!(prior.query_row("SELECT value FROM wal_fixture",[],|r|r.get::<_,String>(0)).unwrap(),"committed-in-wal");let current=read_only(&root.join("observability").join(p.file())).unwrap();assert_eq!(current.pragma_query_value(None,"user_version",|r|r.get::<_,u32>(0)).unwrap(),2);}
    }

}
