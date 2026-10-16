use super::{
    model::{Source, WriteHandle},
    planner::Entry,
};
use crate::{
    connection,
    model::{AppError, AppResult},
    ssh,
    ssh_transport::{Cancellation, SshConnection},
};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

pub struct SourceCheck<'a> {
    item: &'a WriteHandle,
    sftp: Option<ssh2::Sftp>,
    connection: Option<SshConnection>,
}
impl<'a> SourceCheck<'a> {
    pub fn new(item: &'a WriteHandle) -> AppResult<Self> {
        let connection = match &item.source {
            Source::Local(_) => None,
            Source::Remote { context, .. } => {
                Some(connection::connect_verified(context, &item.cancel, crate::traffic::TrafficPurpose::Transfer)?)
            }
        };
        let sftp = connection
            .as_ref()
            .map(|c| c.session().sftp())
            .transpose()?;
        Ok(Self {
            item,
            sftp,
            connection,
        })
    }
    pub fn unchanged(&self, entry: &Entry) -> AppResult<()> {
        self.item.check()?;
        let (size, modified, file, directory) = if let Some(s) = &self.sftp {
            crate::remote_write::safe_ancestors(s, &entry.source)?;
            let m = s.lstat(Path::new(&entry.source))?;
            (m.size.unwrap_or(0), m.mtime, m.is_file(), m.is_dir())
        } else {
            let m = std::fs::symlink_metadata(&entry.source)?;
            (
                m.len(),
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|t| t.as_secs()),
                m.is_file() && !super::download::is_link(&m),
                m.is_dir() && !super::download::is_link(&m),
            )
        };
        if (entry.directory && !directory)
            || (!entry.directory && (!file || size != entry.size || modified != entry.modified))
        {
            return Err(AppError::new(
                "changed",
                format!("源文件已变化，请重新选择：{}", entry.source),
            ));
        }
        Ok(())
    }
    pub fn digest(&self, entry: &Entry) -> AppResult<String> {
        if let Some(c) = &self.connection {
            remote_digest_checked(c, &entry.source, &self.item.cancel, || self.item.check())
        } else {
            let mut input = std::fs::File::open(&entry.source)?;
            let mut hash = Sha256::new();
            let mut b = [0u8; 65536];
            loop {
                self.item.check()?;
                let n = input.read(&mut b)?;
                if n == 0 {
                    break;
                }
                hash.update(&b[..n]);
            }
            Ok(format!("{:x}", hash.finalize()))
        }
    }
    pub fn check_hash_tool(&self) -> AppResult<()> {
        if let Some(c) = &self.connection {
            check_hash_tool(c, &self.item.cancel)?;
        }
        Ok(())
    }
}
pub fn check_hash_tool(c: &SshConnection, cancel: &Cancellation) -> AppResult<()> {
    let (code, _, _) =
        super::direct::capabilities::exec(c, cancel, "command -v sha256sum >/dev/null")?;
    if code != 0 {
        return Err(AppError::new(
            "environment",
            "选择 SHA-256 复核需要远端安装 sha256sum",
        ));
    }
    Ok(())
}
pub fn remote_digest(c: &SshConnection, path: &str, cancel: &Cancellation) -> AppResult<String> {
    remote_digest_checked(c, path, cancel, || cancel.check())
}
pub fn remote_digest_checked(
    c: &SshConnection,
    path: &str,
    cancel: &Cancellation,
    check: impl FnMut() -> AppResult<()>,
) -> AppResult<String> {
    let (code, out, err) = super::direct::capabilities::exec_checked(
        c,
        cancel,
        &format!("sha256sum < {}", ssh::quote(path)),
        Duration::from_secs(24 * 3600),
        check,
    )?;
    let digest = out.split_whitespace().next().unwrap_or("");
    if code != 0 || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "verification",
            format!("无法计算 SHA-256：{}", err.trim()),
        ));
    }
    Ok(digest.to_ascii_lowercase())
}
