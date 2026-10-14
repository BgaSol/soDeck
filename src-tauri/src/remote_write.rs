use crate::model::{AppError, AppResult};
use ssh2::{RenameFlags, Sftp};
use std::{
    io::{Read, Write},
    path::Path,
};

fn packet_string(packet: &mut Vec<u8>, value: &[u8]) {
    packet.extend_from_slice(&(value.len() as u32).to_be_bytes());
    packet.extend_from_slice(value);
}

fn send_packet(channel: &mut ssh2::Channel, payload: &[u8]) -> AppResult<()> {
    channel.write_all(&(payload.len() as u32).to_be_bytes())?;
    channel.write_all(payload)?;
    channel.flush()?;
    Ok(())
}

fn receive_packet(channel: &mut ssh2::Channel) -> AppResult<Vec<u8>> {
    let mut length = [0u8; 4];
    channel.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 256 * 1024 {
        return Err(AppError::new("sftp", "SFTP 返回的数据包大小无效"));
    }
    let mut packet = vec![0; length];
    channel.read_exact(&mut packet)?;
    Ok(packet)
}

fn read_packet_string<'a>(data: &'a [u8], cursor: &mut usize) -> AppResult<&'a [u8]> {
    let length_bytes = data
        .get(*cursor..*cursor + 4)
        .ok_or_else(|| AppError::new("sftp", "SFTP 扩展响应不完整"))?;
    let length = u32::from_be_bytes(length_bytes.try_into().unwrap()) as usize;
    *cursor += 4;
    let value = data
        .get(*cursor..*cursor + length)
        .ok_or_else(|| AppError::new("sftp", "SFTP 扩展响应不完整"))?;
    *cursor += length;
    Ok(value)
}

pub fn atomic_remote_replace(
    session: &ssh2::Session,
    sftp: &Sftp,
    source: &str,
    target: &str,
) -> AppResult<()> {
    // ssh2::Sftp::rename sends v3 RENAME, which cannot replace a file on OpenSSH.
    // Negotiate the standard OpenSSH extension on a separate SFTP channel.
    let mut channel = session.channel_session()?;
    channel.subsystem("sftp")?;
    send_packet(&mut channel, &[1, 0, 0, 0, 3])?;
    let greeting = receive_packet(&mut channel)?;
    if greeting.len() < 5 || greeting[0] != 2 {
        return Err(AppError::new("sftp", "服务器未返回 SFTP 协议版本"));
    }
    let mut cursor = 5;
    let mut posix_rename = false;
    while cursor < greeting.len() {
        let name = read_packet_string(&greeting, &mut cursor)?;
        let version = read_packet_string(&greeting, &mut cursor)?;
        if name == b"posix-rename@openssh.com" && version == b"1" {
            posix_rename = true;
        }
    }
    let result = if posix_rename {
        let mut request = vec![200, 0, 0, 0, 1];
        packet_string(&mut request, b"posix-rename@openssh.com");
        packet_string(&mut request, source.as_bytes());
        packet_string(&mut request, target.as_bytes());
        send_packet(&mut channel, &request).map_err(uncertain_commit)?;
        let response = receive_packet(&mut channel).map_err(uncertain_commit)?;
        if response.len() < 9 || response[0] != 101 || response[1..5] != [0, 0, 0, 1] {
            return Err(AppError::new(
                "unknown",
                "SFTP 原子保存响应无效，需核对目标文件",
            ));
        }
        let status = u32::from_be_bytes(response[5..9].try_into().unwrap());
        if status == 0 {
            Ok(())
        } else {
            Err(AppError::new(
                "save",
                format!("服务器拒绝原子替换文件（SFTP 状态 {status}），请检查目录和文件权限"),
            ))
        }
    } else {
        sftp.rename(
            Path::new(source),
            Path::new(target),
            Some(RenameFlags::OVERWRITE | RenameFlags::ATOMIC | RenameFlags::NATIVE),
        )
        .map_err(|error| {
            if !definitive_sftp_error(&error) {
                return uncertain_commit(error.into());
            }
            AppError::new(
                "save",
                format!("服务器不支持安全替换远端文件，原文件未主动删除：{error}"),
            )
        })
    };
    let _ = channel.send_eof();
    let _ = channel.close();
    result
}

fn uncertain_commit(error: AppError) -> AppError {
    AppError::new(
        "unknown",
        format!("未确认目标文件提交结果，请核对后再操作：{}", error.message),
    )
}
fn definitive_sftp_error(error: &ssh2::Error) -> bool {
    matches!(error.code(), ssh2::ErrorCode::SFTP(code) if code != 6 && code != 7)
}

pub fn metadata(sftp: &Sftp, path: &str) -> AppResult<Option<ssh2::FileStat>> {
    match sftp.lstat(Path::new(path)) {
        Ok(stat) => Ok(Some(stat)),
        Err(e) if e.code() == ssh2::ErrorCode::SFTP(2) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn safe_ancestors(sftp: &Sftp, path: &str) -> AppResult<()> {
    crate::remote_path::absolute(path)?;
    let mut parent = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        parent.push('/');
        parent.push_str(part);
        if let Some(stat) = metadata(sftp, &parent)? {
            if stat.file_type().is_symlink() {
                return Err(AppError::new(
                    "path",
                    format!("写入路径变为符号链接：{parent}"),
                ));
            }
        }
    }
    Ok(())
}

pub struct Stage {
    pub directory: String,
    pub path: String,
}
impl Stage {
    pub fn create(sftp: &Sftp, parent: &str) -> AppResult<Self> {
        safe_ancestors(sftp, parent)?;
        let directory =
            crate::remote_path::join(parent, &format!(".ssh-workspace-{}", uuid::Uuid::new_v4()))?;
        sftp.mkdir(Path::new(&directory), 0o700)?;
        Ok(Self {
            path: format!("{directory}/content"),
            directory,
        })
    }
    pub fn cleanup(&self, sftp: &Sftp) -> AppResult<()> {
        if metadata(sftp, &self.path)?.is_some() {
            sftp.unlink(Path::new(&self.path))?;
        }
        sftp.rmdir(Path::new(&self.directory))?;
        Ok(())
    }
}

pub fn directory_destination(
    sftp: &Sftp,
    requested: &str,
    policy: crate::transfers::download::ConflictPolicy,
    mut before_commit: impl FnMut(&str) -> AppResult<()>,
) -> AppResult<Option<String>> {
    use crate::transfers::download::ConflictPolicy;
    let parent = crate::remote_path::parent(requested);
    safe_ancestors(sftp, parent)?;
    for suffix in 0..100_000 {
        let path = if suffix == 0 {
            requested.to_owned()
        } else {
            crate::remote_path::join(
                parent,
                &crate::remote_path::renamed(crate::remote_path::name(requested)?, true, suffix),
            )?
        };
        if let Some(stat) = metadata(sftp, &path)? {
            match policy {
                ConflictPolicy::Skip => return Ok(None),
                ConflictPolicy::Rename => continue,
                ConflictPolicy::Overwrite if stat.is_dir() => return Ok(Some(path)),
                ConflictPolicy::Overwrite => {
                    return Err(AppError::new(
                        "conflict",
                        format!("同名项目不是普通目录：{path}"),
                    ))
                }
            }
        }
        before_commit(&path)?;
        match sftp.mkdir(Path::new(&path), 0o755) {
            Ok(()) => return Ok(Some(path)),
            Err(e) if !definitive_sftp_error(&e) => return Err(uncertain_commit(e.into())),
            Err(e) => {
                let existing = metadata(sftp, &path)?;
                match policy {
                    ConflictPolicy::Skip if existing.is_some() => return Ok(None),
                    ConflictPolicy::Rename if existing.is_some() => continue,
                    ConflictPolicy::Overwrite if existing.is_some_and(|s| s.is_dir()) => {
                        return Ok(Some(path))
                    }
                    _ => return Err(e.into()),
                }
            }
        }
    }
    Err(AppError::new("conflict", "无法分配目标目录名称"))
}

pub fn publish(
    session: &ssh2::Session,
    sftp: &Sftp,
    staged: &str,
    requested: &str,
    policy: crate::transfers::download::ConflictPolicy,
    mut before_commit: impl FnMut(&str) -> AppResult<()>,
) -> AppResult<Option<String>> {
    use crate::transfers::download::ConflictPolicy;
    safe_ancestors(sftp, crate::remote_path::parent(requested))?;
    for suffix in 0..100_000 {
        let path = if suffix == 0 {
            requested.to_owned()
        } else {
            crate::remote_path::join(
                crate::remote_path::parent(requested),
                &crate::remote_path::renamed(crate::remote_path::name(requested)?, false, suffix),
            )?
        };
        if let Some(stat) = metadata(sftp, &path)? {
            match policy {
                ConflictPolicy::Skip => return Ok(None),
                ConflictPolicy::Rename => continue,
                ConflictPolicy::Overwrite if stat.is_file() => {
                    before_commit(&path)?;
                    atomic_remote_replace(session, sftp, staged, &path)?;
                    return Ok(Some(path));
                }
                ConflictPolicy::Overwrite => {
                    return Err(AppError::new(
                        "conflict",
                        format!("同名项目不是普通文件：{path}"),
                    ))
                }
            }
        }
        // Explicit empty flags: ssh2's None defaults to OVERWRITE.
        before_commit(&path)?;
        match sftp.rename(
            Path::new(staged),
            Path::new(&path),
            Some(RenameFlags::empty()),
        ) {
            Ok(()) => return Ok(Some(path)),
            Err(e) if !definitive_sftp_error(&e) => return Err(uncertain_commit(e.into())),
            Err(_) if metadata(sftp, &path)?.is_some() => {
                if policy == ConflictPolicy::Skip {
                    return Ok(None);
                }
                if policy == ConflictPolicy::Overwrite {
                    return Err(AppError::new(
                        "conflict",
                        "提交时目标被其他操作创建，请重试",
                    ));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(AppError::new("conflict", "无法分配目标文件名称"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sftp_extension_parser_rejects_truncated_strings() {
        let mut bytes = Vec::new();
        packet_string(&mut bytes, b"posix-rename@openssh.com");
        packet_string(&mut bytes, b"1");
        let mut cursor = 0;
        assert_eq!(
            read_packet_string(&bytes, &mut cursor).unwrap(),
            b"posix-rename@openssh.com"
        );
        assert_eq!(read_packet_string(&bytes, &mut cursor).unwrap(), b"1");
        assert_eq!(cursor, bytes.len());
        assert!(read_packet_string(&bytes, &mut cursor).is_err());
        assert!(read_packet_string(&[0, 0, 1, 0, b'a'], &mut 0).is_err());
    }
}
