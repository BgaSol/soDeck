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
    transient: Mutex<HashMap<String, Credential>>,
}
impl Secrets {
    pub fn open(root: &Path) -> Result<Self> {
        let directory = root.join("secrets");
        std::fs::create_dir_all(&directory)?;
        Ok(Self {
            directory,
            transient: Mutex::new(HashMap::new()),
        })
    }
    fn path(&self, server_id: &str) -> Result<PathBuf> {
        if uuid::Uuid::parse_str(server_id).is_err() {
            return Err(CoreError::new("INVALID_PARAMS", "服务器 ID 无效"));
        }
        Ok(self.directory.join(format!("{server_id}.dpapi")))
    }
    pub fn set(&self, server_id: &str, credential: Credential, remember: bool) -> Result<()> {
        let path = self.path(server_id)?;
        if remember {
            let mut plain = serde_json::to_vec(&credential)?;
            let protected = protect(&plain);
            plain.zeroize();
            let protected = protected?;
            let temporary = path.with_extension("pending");
            use std::io::Write;
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&protected)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, &path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.transient
            .lock()
            .unwrap()
            .insert(server_id.into(), credential);
        Ok(())
    }
    pub fn get(&self, server_id: &str) -> Result<Option<Credential>> {
        if let Some(value) = self.transient.lock().unwrap().get(server_id).cloned() {
            return Ok(Some(value));
        }
        let path = self.path(server_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let mut plain = unprotect(&std::fs::read(path)?)?;
        let result = serde_json::from_slice(&plain).map_err(CoreError::from);
        plain.zeroize();
        result.map(Some)
    }
    pub fn remove(&self, server_id: &str) -> Result<()> {
        self.transient.lock().unwrap().remove(server_id);
        let path = self.path(server_id)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    pub fn remembered(&self, server_id: &str) -> bool {
        self.path(server_id).map(|p| p.exists()).unwrap_or(false)
    }
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
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
fn replace_file(source: &Path, target: &Path) -> Result<()> {
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
