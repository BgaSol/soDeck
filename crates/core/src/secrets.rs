use crate::model::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
use zeroize::Zeroize;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub passphrase: String,
}
impl Drop for Credential {
    fn drop(&mut self) {
        self.password.zeroize();
        self.private_key.zeroize();
        self.passphrase.zeroize();
    }
}

pub struct Secrets {
    directory: PathBuf,
    transient: Mutex<HashMap<(String,String,String),Credential>>,
}
pub struct PreparedSecret { pub secret_id: Option<String>, credential: Credential }
impl Secrets {
    pub fn open(root:&Path)->Result<Self> {
        let directory=root.join("secrets");
        std::fs::create_dir_all(&directory)?;
        Ok(Self{directory,transient:Mutex::new(HashMap::new())})
    }
    fn path(&self,secret_id:&str)->Result<PathBuf> {
        if uuid::Uuid::parse_str(secret_id).is_err(){return Err(CoreError::new("SECRET_STORE_ERROR","Invalid secret identifier"));}
        Ok(self.directory.join(format!("{secret_id}.dpapi")))
    }
    pub fn prepare(&self,credential:Credential,remember:bool)->Result<PreparedSecret> {
        let secret_id=if remember {
            let secret_id=crate::model::id();
            let path=self.path(&secret_id)?;
            let mut plain=serde_json::to_vec(&credential)?;
            let protected=protect(&plain);plain.zeroize();let protected=protected?;
            let pending=path.with_extension("pending");
            use std::io::Write;
            let write=(||->Result<()>{
                let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&pending)?;
                file.write_all(&protected)?;file.sync_all()?;drop(file);
                replace_file(&pending,&path)
            })();
            if write.is_err(){let _=std::fs::remove_file(&pending);}
            write?;Some(secret_id)
        }else{None};
        Ok(PreparedSecret{secret_id,credential})
    }
    pub fn discard(&self,prepared:&PreparedSecret) {
        if let Some(id)=&prepared.secret_id {if let Ok(path)=self.path(id){let _=std::fs::remove_file(path);}}
    }
    /// The caller rechecks the owner/context under the configuration lock first.
    /// Publishing a blob always precedes publishing the SQL reference.
    pub fn commit(&self,db:&rusqlite::Connection,kind:&str,owner:&str,context:&str,prepared:PreparedSecret)->Result<()> {
        self.commit_with(db,kind,owner,context,prepared,|_|Ok(()))
    }
    /// Commit the identity decision and its credential binding in one transaction.
    pub fn commit_with(&self,db:&rusqlite::Connection,kind:&str,owner:&str,context:&str,prepared:PreparedSecret,change:impl FnOnce(&rusqlite::Transaction<'_>)->Result<()>)->Result<()> {
        use rusqlite::{params,OptionalExtension};
        let previous:Option<String>=db.query_row("SELECT secret_id FROM credential_bindings WHERE owner_kind=? AND owner_id=? AND context_key=?",params![kind,owner,context],|r|r.get(0)).optional()?;
        let result=(||->Result<()>{
            let tx=db.unchecked_transaction()?;
            change(&tx)?;
            if let Some(id)=&prepared.secret_id {
                if !self.path(id)?.is_file(){return Err(CoreError::new("SECRET_STORE_ERROR","Secret blob was not published"));}
                tx.execute("INSERT INTO credential_bindings(owner_kind,owner_id,context_key,secret_id,revision) VALUES(?,?,?,?,1) ON CONFLICT(owner_kind,owner_id,context_key) DO UPDATE SET secret_id=excluded.secret_id,revision=revision+1",params![kind,owner,context,id])?;
            }else{
                tx.execute("DELETE FROM credential_bindings WHERE owner_kind=? AND owner_id=? AND context_key=?",params![kind,owner,context])?;
            }
            tx.commit()?;Ok(())
        })();
        if result.is_err(){self.discard(&prepared);return result;}
        self.transient.lock().unwrap().insert((kind.into(),owner.into(),context.into()),prepared.credential);
        // Only this superseded immutable blob can be removed at runtime. A
        // directory sweep could race another owner's prepared publication.
        if let Some(previous)=previous {if let Ok(path)=self.path(&previous){let _=std::fs::remove_file(path);}}
        Ok(())
    }
    pub fn get(&self,db:&rusqlite::Connection,kind:&str,owner:&str,context:&str)->Result<Option<Credential>> {
        use rusqlite::{params,OptionalExtension};
        if let Some(value)=self.transient.lock().unwrap().get(&(kind.into(),owner.into(),context.into())).cloned(){return Ok(Some(value));}
        let secret_id:Option<String>=db.query_row("SELECT secret_id FROM credential_bindings WHERE owner_kind=? AND owner_id=? AND context_key=?",params![kind,owner,context],|r|r.get(0)).optional()?;
        let Some(secret_id)=secret_id else{return Ok(None)};
        let mut plain=unprotect(&std::fs::read(self.path(&secret_id)?)?)?;
        let result:Result<Credential>=serde_json::from_slice(&plain).map_err(CoreError::from);plain.zeroize();
        let credential=result?;self.transient.lock().unwrap().insert((kind.into(),owner.into(),context.into()),credential.clone());Ok(Some(credential))
    }
    pub fn has_preset(&self,db:&rusqlite::Connection,kind:&str,owner:&str,identity:&serde_json::Value)->Result<bool> {
        let expected=crate::route::digest(identity);
        Ok(self.get(db,kind,owner,"preset")?.is_some_and(|c|c.context.as_deref()==Some(expected.as_str())))
    }
    pub fn has_context(&self,db:&rusqlite::Connection,kind:&str,owner:&str,context:&str)->Result<bool> {
        if self.transient.lock().unwrap().contains_key(&(kind.into(),owner.into(),context.into())){return Ok(true)}
        self.remembered(db,kind,owner,context)
    }
    pub fn remembered(&self,db:&rusqlite::Connection,kind:&str,owner:&str,context:&str)->Result<bool> {
        Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM credential_bindings WHERE owner_kind=? AND owner_id=? AND context_key=?)",rusqlite::params![kind,owner,context],|r|r.get(0))?)
    }
    pub fn remove_owner(&self,db:&rusqlite::Connection,kind:&str,owner:&str)->Result<()> {
        let mut query=db.prepare("SELECT secret_id FROM credential_bindings WHERE owner_kind=? AND owner_id=?")?;
        let previous=query.query_map(rusqlite::params![kind,owner],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        db.execute("DELETE FROM credential_bindings WHERE owner_kind=? AND owner_id=?",rusqlite::params![kind,owner])?;
        if db.is_autocommit(){
            self.forget_owner(kind,owner);
            for id in previous{if let Ok(path)=self.path(&id){let _=std::fs::remove_file(path);}}
        }
        Ok(())
    }
    pub fn forget_owner(&self,kind:&str,owner:&str){self.transient.lock().unwrap().retain(|(k,o,_),_|k!=kind||o!=owner);}
    /// Startup only, with data lock held and a successfully checked database.
    pub fn cleanup(&self,db:&rusqlite::Connection)->Result<()> {
        let integrity:String=db.query_row("PRAGMA integrity_check",[],|r|r.get(0))?;
        if integrity!="ok" {return Err(CoreError::new("SECRET_STORE_ERROR","数据库未通过完整性检查，保留所有秘密文件"));}
        let mut query=db.prepare("SELECT secret_id FROM credential_bindings")?;
        let referenced=query.query_map([],|r|r.get::<_,String>(0))?.collect::<std::result::Result<std::collections::HashSet<_>,_>>()?;
        for id in &referenced {if !self.path(id)?.is_file(){return Err(CoreError::new("SECRET_STORE_ERROR","凭据绑定缺少秘密文件，已停止清理"));}}
        let files=std::fs::read_dir(&self.directory)?.collect::<std::result::Result<Vec<_>,_>>()?;
        for entry in files {
            if !entry.file_type()?.is_file(){continue;}
            let path=entry.path();
            if !matches!(path.extension().and_then(|v|v.to_str()),Some("dpapi"|"pending")){continue;}
            let Some(id)=path.file_stem().and_then(|v|v.to_str())else{continue};
            if uuid::Uuid::parse_str(id).is_ok()&&!referenced.contains(id){std::fs::remove_file(path)?;}
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn replace_file(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(CoreError::new("SECRET_STORE_ERROR", "无法提交已加密的凭据"));
    }
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn replace_file(source: &Path, target: &Path) -> Result<()> {
    std::fs::rename(source, target)?;
    Ok(())
}

#[cfg(windows)]
fn protect(input: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        if CryptProtectData(
            &source,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        ) == 0
        {
            return Err(CoreError::new("SECRET_STORE_ERROR", "Windows 无法加密凭据"));
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut _);
        Ok(result)
    }
}
#[cfg(windows)]
fn unprotect(input: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        if CryptUnprotectData(
            &source,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        ) == 0
        {
            return Err(CoreError::new(
                "SECRET_STORE_ERROR",
                "Windows 无法解密凭据，请重新输入",
            ));
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        LocalFree(output.pbData as *mut _);
        Ok(result)
    }
}
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Result<Vec<u8>> {
    Err(CoreError::new(
        "UNSUPPORTED_PLATFORM",
        "此平台尚未实现持久凭据服务",
    ))
}
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    Err(CoreError::new(
        "UNSUPPORTED_PLATFORM",
        "此平台尚未实现持久凭据服务",
    ))
}

#[cfg(all(test,windows))]
mod tests {
    use super::*;
    fn credential(value:&str)->Credential{let mut c=Credential::default();c.password=value.into();c}
    #[test]
    fn immutable_publication_rollback_context_isolation_and_forget_survive_restart() {
        let root=tempfile::tempdir().unwrap();let core=crate::Core::open(root.path()).unwrap();
        let db=core.store.db.lock().unwrap();let secrets=&core.secrets;
        secrets.commit(&db,"socks5","p","via-a",secrets.prepare(credential("old-a"),true).unwrap()).unwrap();
        secrets.commit(&db,"socks5","p","via-b",secrets.prepare(credential("old-b"),true).unwrap()).unwrap();
        db.execute_batch("CREATE TRIGGER fail_secret BEFORE UPDATE ON credential_bindings BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();
        let prepared=secrets.prepare(credential("failed-a"),true).unwrap();let failed_id=prepared.secret_id.clone().unwrap();
        assert!(secrets.commit(&db,"socks5","p","via-a",prepared).is_err());assert!(!secrets.path(&failed_id).unwrap().exists());
        assert_eq!(secrets.get(&db,"socks5","p","via-a").unwrap().unwrap().password,"old-a");
        db.execute_batch("DROP TRIGGER fail_secret;").unwrap();
        secrets.commit(&db,"socks5","p","via-a",secrets.prepare(credential("memory-a"),false).unwrap()).unwrap();
        assert_eq!(secrets.get(&db,"socks5","p","via-a").unwrap().unwrap().password,"memory-a");
        assert_eq!(std::fs::read_dir(root.path().join("secrets")).unwrap().count(),1);
        let orphan=secrets.prepare(credential("orphan"),true).unwrap().secret_id.unwrap();
        // A missing published binding aborts the sweep before any orphan deletion.
        db.execute("INSERT INTO credential_bindings VALUES('ssh','t','c',?,1)",[crate::model::id()]).unwrap();
        assert!(secrets.cleanup(&db).is_err());assert!(secrets.path(&orphan).unwrap().exists());
        db.execute("DELETE FROM credential_bindings WHERE owner_id='t'",[]).unwrap();
        secrets.cleanup(&db).unwrap();assert!(!secrets.path(&orphan).unwrap().exists());
        drop(db);drop(core);
        let core=crate::Core::open(root.path()).unwrap();let db=core.store.db.lock().unwrap();
        assert!(core.secrets.get(&db,"socks5","p","via-a").unwrap().is_none());
        assert_eq!(core.secrets.get(&db,"socks5","p","via-b").unwrap().unwrap().password,"old-b");
    }
    #[test]
    fn failed_old_blob_cleanup_keeps_committed_value_and_restart_reclaims_orphan() {
        use std::os::windows::fs::OpenOptionsExt;
        let root=tempfile::tempdir().unwrap();let core=crate::Core::open(root.path()).unwrap();let db=core.store.db.lock().unwrap();
        let prepared=core.secrets.prepare(credential("old"),true).unwrap();let old=prepared.secret_id.clone().unwrap();
        core.secrets.commit(&db,"ssh","t","c",prepared).unwrap();
        let locked=std::fs::OpenOptions::new().read(true).share_mode(0).open(core.secrets.path(&old).unwrap()).unwrap();
        core.secrets.commit(&db,"ssh","t","c",core.secrets.prepare(credential("new"),true).unwrap()).unwrap();
        assert_eq!(core.secrets.get(&db,"ssh","t","c").unwrap().unwrap().password,"new");
        assert!(core.secrets.path(&old).unwrap().exists());drop(locked);drop(db);drop(core);
        let core=crate::Core::open(root.path()).unwrap();assert!(!core.secrets.path(&old).unwrap().exists());
        assert_eq!(core.secrets.get(&core.store.db.lock().unwrap(),"ssh","t","c").unwrap().unwrap().password,"new");
    }
}
