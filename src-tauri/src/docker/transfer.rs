use super::{inspect, model::*, query};
use crate::{
    model::{AppError, AppResult},
    ssh,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, UNIX_EPOCH},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    ImageSave,
    ImageLoad,
    CopyOut,
    CopyIn,
}
impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ImageSave => "导出镜像",
            Self::ImageLoad => "导入镜像",
            Self::CopyOut => "导出容器文件",
            Self::CopyIn => "复制文件到容器",
        }
    }
    pub fn upload(&self) -> bool {
        matches!(self, Self::ImageLoad | Self::CopyIn)
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub source_name: String,
    pub source_path: String,
    pub target_name: String,
    pub target_path: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub sort_order: u64,
}
#[derive(Clone)]
pub struct Spec {
    pub kind: Kind,
    pub target: String,
    pub container_path: String,
    pub local: PathBuf,
    pub conflict: String,
    pub local_fingerprint: String,
}
pub fn validate(kind: &Kind, target: &str, path: &str, conflict: &str) -> AppResult<()> {
    match kind {
        Kind::ImageSave => super::images::validate_id(target)?,
        Kind::ImageLoad => {
            if target != "image-import" {
                return Err(AppError::new("validation", "无效的镜像导入目标"));
            }
        }
        _ => validate_id(target)?,
    }
    if matches!(kind, Kind::CopyIn | Kind::CopyOut)
        && (!path.starts_with('/')
            || path.len() > 4096
            || path.chars().any(char::is_control)
            || path.split('/').any(|p| p == ".."))
    {
        return Err(AppError::new(
            "validation",
            "容器路径须为不包含 .. 的绝对路径",
        ));
    }
    if !["rename", "skip", "overwrite"].contains(&conflict) {
        return Err(AppError::new("validation", "无效的重名策略"));
    }
    Ok(())
}
fn metadata(path: &Path) -> AppResult<String> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(AppError::new(
                    "path",
                    "本地路径须为普通文件，不能是链接或目录",
                ));
            }
            #[cfg(unix)]
            let identity = {
                use std::os::unix::fs::MetadataExt;
                format!("{}:{}", m.dev(), m.ino())
            };
            #[cfg(not(unix))]
            let identity = String::new();
            Ok(format!(
                "{}:{}:{}",
                identity,
                m.len(),
                m.modified()?
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("missing".into()),
        Err(e) => Err(e.into()),
    }
}
pub fn selection(
    kind: Kind,
    target: String,
    container_path: String,
    local: PathBuf,
    conflict: String,
) -> AppResult<Spec> {
    validate(&kind, &target, &container_path, &conflict)?;
    if !local.is_absolute() {
        return Err(AppError::new("path", "请选择本地绝对路径"));
    }
    let parent = local
        .parent()
        .ok_or_else(|| AppError::new("path", "目标目录无效"))?
        .canonicalize()?;
    let name = local
        .file_name()
        .ok_or_else(|| AppError::new("path", "文件名无效"))?;
    let mut local = parent.join(name);
    let mut fingerprint = metadata(&local)?;
    if kind.upload() {
        if fingerprint == "missing" {
            return Err(AppError::new("notFound", "本地文件已不存在"));
        }
        if kind == Kind::CopyIn {
            let name = local
                .file_name()
                .unwrap()
                .to_str()
                .ok_or_else(|| AppError::new("path", "文件名必须为 UTF-8"))?;
            let _ = tar_header(name, fs::metadata(&local)?.len())?;
        }
    } else if fingerprint != "missing" {
        match conflict.as_str() {
            "skip" => {
                return Err(AppError::new(
                    "conflict",
                    "目标文件已存在，按跳过策略未提交传输",
                ))
            }
            "rename" => {
                let stem = local
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let ext = local
                    .extension()
                    .map(|s| format!(".{}", s.to_string_lossy()))
                    .unwrap_or_default();
                let mut found = false;
                for i in 1..10000 {
                    let candidate = parent.join(format!("{stem} ({i}){ext}"));
                    if metadata(&candidate)? == "missing" {
                        local = candidate;
                        fingerprint = "missing".into();
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(AppError::new("limit", "无法生成不重名的目标文件"));
                }
            }
            _ => {}
        }
    }
    Ok(Spec {
        kind,
        target,
        container_path,
        local,
        conflict,
        local_fingerprint: fingerprint,
    })
}
pub fn prepare(
    session: &ssh2::Session,
    engine: &EngineBinding,
    spec: &Spec,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, String, Vec<String>)> {
    validate(
        &spec.kind,
        &spec.target,
        &spec.container_path,
        &spec.conflict,
    )?;
    if metadata(&spec.local)? != spec.local_fingerprint {
        return Err(AppError::new(
            "conflict",
            "本地文件已变化，请重新选择并预览",
        ));
    }
    let (name, target_fp) = match spec.kind {
        Kind::ImageSave => {
            let d = super::images::detail(session, &engine.endpoint, &spec.target, check)?;
            (
                d.tags.first().cloned().unwrap_or_else(|| d.id.clone()),
                serde_json::json!([d.id, d.tags, d.digests]).to_string(),
            )
        }
        Kind::ImageLoad => ("远端 Docker 引擎".into(), engine.token.clone()),
        _ => {
            let d = inspect::container(session, &engine.endpoint, &spec.target, check)?;
            (
                d.name.clone(),
                serde_json::json!([d.id, d.created, d.mounts]).to_string(),
            )
        }
    };
    let mut messages = vec![format!(
        "{}：{}",
        if spec.kind.upload() {
            "本机来源"
        } else {
            "本机目标"
        },
        spec.local.display()
    )];
    match spec.kind {
        Kind::ImageSave=>messages.push("以 tar 归档导出选定镜像，不删除远端镜像。本机不自动解包。".into()),
        Kind::ImageLoad=>messages.push("将归档流送往远端 Docker 引擎并加载镜像及 Tag。归档内同名 Tag 可能改变现有引用；中止不回滚已经加载的内容。".into()),
        Kind::CopyOut=>messages.push(format!("导出容器 {name}:{} 为 tar 归档，本机不自动解包或执行其中内容。",spec.container_path)),
        Kind::CopyIn=>messages.push(format!("复制本机文件到容器 {name}:{} 目录，保留文件名。同名容器文件可能被覆盖；中止后可能留下部分内容。",spec.container_path)),
    }
    if !spec.kind.upload() && spec.local_fingerprint != "missing" {
        messages.push("确认后替换上述本机目标文件。执行前和提交文件前再次核对目标。".into());
    }
    let fingerprint = serde_json::json!([
        target_fp,
        spec.local.to_string_lossy(),
        spec.local_fingerprint,
        spec.container_path,
        spec.conflict
    ])
    .to_string();
    Ok((name, fingerprint, messages))
}
pub fn presentation(spec: &Spec, remote_name: &str, sort_order: u64) -> Progress {
    let local = spec.local.to_string_lossy().into_owned();
    let remote = format!("{}:{}", spec.target, spec.container_path);
    Progress {
        source_name: if spec.kind.upload() {
            "本机".into()
        } else {
            remote_name.into()
        },
        source_path: if spec.kind.upload() {
            local.clone()
        } else {
            remote.clone()
        },
        target_name: if spec.kind.upload() {
            remote_name.into()
        } else {
            "本机".into()
        },
        target_path: if spec.kind.upload() { remote } else { local },
        bytes_done: 0,
        bytes_total: if spec.kind.upload() {
            fs::metadata(&spec.local).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        },
        sort_order,
    }
}
fn tar_header(name: &str, size: u64) -> AppResult<[u8; 512]> {
    if name.is_empty()
        || name.len() > 100
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || size > 0o77777777777
    {
        return Err(AppError::new(
            "validation",
            "复制文件须使用不超过 100 字节的普通文件名，单文件小于 8 GiB",
        ));
    }
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    fn octal(field: &mut [u8], value: u64) {
        let s = format!("{:0width$o}\0", value, width = field.len() - 1);
        field.copy_from_slice(s.as_bytes());
    }
    octal(&mut h[100..108], 0o644);
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0);
    h[148..156].fill(b' ');
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    let checksum = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(checksum.as_bytes());
    Ok(h)
}
pub fn execute(
    session: &ssh2::Session,
    endpoint: &str,
    spec: &Spec,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
    progress: &impl Fn(u64, u64),
) -> AppResult<ssh::ExecResult> {
    if metadata(&spec.local)? != spec.local_fingerprint {
        return Err(AppError::new("conflict", "本地文件已变化"));
    }
    let prefix = query::docker(endpoint);
    let target = ssh::quote(&spec.target);
    let container = ssh::quote(&format!("{}:{}", spec.target, spec.container_path));
    let command = match spec.kind {
        Kind::ImageSave => format!("{prefix} image save {target}"),
        Kind::ImageLoad => format!("{prefix} image load"),
        Kind::CopyOut => format!("{prefix} container cp {container} -"),
        Kind::CopyIn => format!("{prefix} container cp - {container}"),
    };
    let mut total = 0;
    let mut input: Option<Box<dyn Read>> = if spec.kind.upload() {
        let file = File::open(&spec.local)?;
        total = file.metadata()?.len();
        let reader: Box<dyn Read> = if spec.kind == Kind::CopyIn {
            let name = spec.local.file_name().unwrap().to_str().unwrap();
            let header = tar_header(name, total)?;
            let padding = (512 - total % 512) % 512 + 1024;
            total += 512 + padding;
            Box::new(
                Cursor::new(header)
                    .chain(file)
                    .chain(Cursor::new(vec![0u8; padding as usize])),
            )
        } else {
            Box::new(file)
        };
        Some(reader)
    } else {
        None
    };
    let mut output = if spec.kind.upload() {
        None
    } else {
        Some(tempfile::NamedTempFile::new_in(
            spec.local.parent().unwrap(),
        )?)
    };
    session.set_timeout(0);
    session.set_blocking(false);
    let result = (|| {
        let until = Instant::now() + Duration::from_secs(7200);
        let guard = || {
            check()?;
            if Instant::now() >= until {
                Err(AppError::new("timeout", "Docker 文件传输等待超时"))
            } else {
                Ok(())
            }
        };
        let retry = |f: &mut dyn FnMut() -> Result<(), ssh2::Error>| -> AppResult<()> {
            loop {
                guard()?;
                match f() {
                    Ok(()) => return Ok(()),
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
        let mut channel = loop {
            guard()?;
            match session.channel_session() {
                Ok(c) => break c,
                Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e.into()),
            }
        };
        let marker = format!("__SODECK_BINARY_RESULT_{}__", uuid::Uuid::new_v4().simple());
        let wrapped=format!("{command}; sodeck_status=$?; printf '\\n{marker}%s\\n' \"$sodeck_status\" >&2; exit \"$sodeck_status\"");
        attempted.store(true, Ordering::SeqCst);
        retry(&mut || channel.exec(&wrapped))?;
        let mut send = [0u8; 65536];
        let mut receive = [0u8; 65536];
        let mut start = 0;
        let mut end = 0;
        let mut eof = input.is_none();
        let mut eof_sent = false;
        let mut done = 0u64;
        let mut messages = [Vec::<u8>::new(), Vec::new()];
        let mut last = Instant::now();
        loop {
            guard()?;
            let mut activity = false;
            if !eof && start == end {
                end = input.as_mut().unwrap().read(&mut send)?;
                start = 0;
                eof = end == 0;
            }
            if start < end {
                match channel.write(&send[start..end]) {
                    Ok(n) => {
                        start += n;
                        done += n as u64;
                        activity = n > 0;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
            }
            if eof && start == end && !eof_sent {
                match channel.send_eof() {
                    Ok(()) => eof_sent = true,
                    Err(e) if e.code() == ssh2::ErrorCode::Session(-37) => (),
                    Err(e) => return Err(e.into()),
                }
            }
            for (index, buffer) in messages.iter_mut().enumerate() {
                match channel.stream(index as i32).read(&mut receive) {
                    Ok(n) => {
                        activity |= n > 0;
                        if index == 0 && output.is_some() {
                            output.as_mut().unwrap().write_all(&receive[..n])?;
                            done += n as u64;
                        } else {
                            buffer.extend_from_slice(&receive[..n]);
                            if buffer.len() > 32768 {
                                buffer.drain(..buffer.len() - 32768);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
            }
            if last.elapsed() >= Duration::from_millis(500) {
                progress(done, total);
                last = Instant::now();
            }
            if channel.eof() && !activity {
                break;
            }
            if !activity {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        retry(&mut || channel.wait_close())?;
        let stderr = String::from_utf8_lossy(&messages[1]);
        let code = stderr
            .lines()
            .last()
            .and_then(|line| line.strip_prefix(&marker))
            .and_then(|s| s.parse::<i32>().ok())
            .filter(|n| (0..=255).contains(n))
            .ok_or_else(|| AppError::new("unknown", "未收到二进制传输的完成回执"))?;
        if spec.kind.upload() && code == 0 && done != total {
            return Err(AppError::new(
                "unknown",
                "输入流尚未全部传送，不能确认导入结果",
            ));
        }
        progress(done, total);
        Ok(ssh::ExecResult {
            code,
            stdout: inspect::clean(&String::from_utf8_lossy(&messages[0]), 4000),
            stderr: inspect::clean(
                &stderr
                    .lines()
                    .filter(|s| !s.starts_with(&marker))
                    .collect::<Vec<_>>()
                    .join("\n"),
                4000,
            ),
        })
    })();
    session.set_blocking(true);
    session.set_timeout(12_000);
    let result = result?;
    if result.code == 0 {
        if spec.kind.upload() && metadata(&spec.local)? != spec.local_fingerprint {
            return Err(AppError::new(
                "unknown",
                "本地来源文件在传输中发生变化，请核对远端导入结果；不会自动重试",
            ));
        }
        attempted.store(false, Ordering::SeqCst);
        if let Some(mut output) = output {
            check()?;
            output.flush()?;
            output.as_file().sync_all()?;
            if metadata(&spec.local)? != spec.local_fingerprint {
                return Err(AppError::new(
                    "conflict",
                    "本地目标在传输中已变化，未覆盖；请重新导出",
                ));
            }
            if spec.conflict == "overwrite" && spec.local_fingerprint != "missing" {
                output
                    .persist(&spec.local)
                    .map_err(|e| AppError::new("io", e.error))?;
            } else {
                output
                    .persist_noclobber(&spec.local)
                    .map_err(|e| AppError::new("io", e.error))?;
            }
        }
    }
    Ok(result)
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    kind: &Kind,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    match kind {
        Kind::CopyIn | Kind::CopyOut => {
            let d = inspect::container(session, endpoint, id, check)?;
            Ok(format!(
                "容器当前状态 {}；文件复制是否完整仍需检查目标文件",
                d.state
            ))
        }
        Kind::ImageSave => {
            super::images::detail(session, endpoint, id, check)?;
            Ok("原镜像仍存在；导出结果请检查本机文件".into())
        }
        Kind::ImageLoad => {
            let images = query::images_on(session, endpoint, check)?;
            Ok(format!(
                "当前 {} 个镜像引用；请核对导入前后引用变化，这不能证明先前导入是否完整",
                images.len()
            ))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tar_headers_never_allow_traversal_or_links() {
        for name in ["../secret", "/root", "a/b", "a\\b", ".."] {
            assert!(tar_header(name, 12).is_err());
        }
        let h = tar_header("test.txt", 3).unwrap();
        assert_eq!(h[156], b'0');
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*b)
                }
            })
            .sum();
        assert_eq!(
            u64::from_str_radix(std::str::from_utf8(&h[148..154]).unwrap(), 8).unwrap(),
            sum
        );
    }
}
