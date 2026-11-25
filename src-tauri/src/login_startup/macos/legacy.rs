use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(super) fn path() -> Result<PathBuf, String> {
    Ok(
        PathBuf::from(std::env::var_os("HOME").ok_or("无法确定当前用户的主目录")?)
            .join("Library/LaunchAgents/com.bgasol.sodeck.plist"),
    )
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn output(mut command: Command) -> Result<String, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().ok_or("无法读取系统查询结果")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.take(65537).read_to_string(&mut text).map(|_| text)
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("读取 macOS 登录项状态超时，请重新读取".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = reader
        .join()
        .map_err(|_| "系统查询未完成")?
        .map_err(|e| e.to_string())?;
    if !status.success() || text.len() > 65536 {
        return Err("macOS 未能返回有效的登录项状态，请重新读取".into());
    }
    Ok(text)
}

pub(super) fn plist(path: &Path) -> Result<serde_json::Value, String> {
    let mut command = Command::new("/usr/bin/plutil");
    command
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(path);
    serde_json::from_str(&output(command)?).map_err(|e| format!("登录项 plist 格式无效：{e}"))
}

pub(super) fn content(executable: &Path) -> Result<String, String> {
    let executable = executable.to_str().ok_or("启动路径不是有效 Unicode")?;
    if executable.contains(['\0', '\n', '\r']) {
        return Err("启动路径包含无法登记的字符".into());
    }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.bgasol.sodeck</string><key>ProgramArguments</key><array><string>{}</string><string>--login-start</string></array><key>RunAtLoad</key><true/><key>LimitLoadToSessionType</key><string>Aqua</string><key>AssociatedBundleIdentifiers</key><array><string>com.bgasol.sodeck</string></array></dict></plist>\n",xml(executable)))
}

pub struct Entry {
    pub path: PathBuf,
    pub enabled: bool,
}

pub fn inspect(executable: &Path) -> Result<Option<Entry>, String> {
    inspect_at(&path()?, executable)
}

fn inspect_at(path: &Path, executable: &Path) -> Result<Option<Entry>, String> {
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("旧登录项不是普通文件，已保留".into());
    }
    let value = plist(path)?;
    if value["Label"] != "com.bgasol.sodeck"
        || value["ProgramArguments"] != serde_json::json!([executable, "--login-start"])
        || value["AssociatedBundleIdentifiers"] != serde_json::json!(["com.bgasol.sodeck"])
        || value.get("KeepAlive").is_some()
        || value.get("Program").is_some()
        || value["LimitLoadToSessionType"] != "Aqua"
    {
        return Err(
            "旧登录项不属于当前 SoDeck 安装或已被修改，原文件已保留。请检查系统登录项。".into(),
        );
    }
    Ok(Some(Entry {
        path: path.into(),
        enabled: value["RunAtLoad"] == true && !disabled()?,
    }))
}

fn disabled() -> Result<bool, String> {
    let mut command = Command::new("/bin/launchctl");
    command
        .arg("print-disabled")
        .arg(format!("gui/{}", unsafe { libc::geteuid() }));
    Ok(output(command)?
        .lines()
        .any(|line| line.contains("\"com.bgasol.sodeck\"") && line.contains("=> true")))
}

pub fn remove(entry: &Entry) -> Result<(), String> {
    fs::remove_file(&entry.path).map_err(|e| format!("旧登录项清理未完成：{e}"))
}

pub fn write(executable: &Path) -> Result<(), String> {
    let path = path()?;
    fs::create_dir_all(path.parent().ok_or("登录项路径无效")?).map_err(|e| e.to_string())?;
    let bytes = content(executable)?;
    crate::storage::atomic_write(&path, |file| {
        use std::io::Write;
        file.write_all(bytes.as_bytes())
    })
    .map_err(|e| format!("无法提交 macOS 登录项：{e}"))
}

pub fn verify(directory: &Path) -> Result<(), String> {
    let file = directory.join("login-test.plist");
    let executable = directory.join("中文 space &/SoDeck.app/Contents/MacOS/sodeck");
    fs::write(&file, content(&executable)?).map_err(|e| e.to_string())?;
    let result = plist(&file);
    fs::remove_file(file).map_err(|e| e.to_string())?;
    let value = result?;
    if value["ProgramArguments"] != serde_json::json!([executable, "--login-start"])
        || value["RunAtLoad"] != true
        || value.get("KeepAlive").is_some()
        || value["LimitLoadToSessionType"] != "Aqua"
        || value["AssociatedBundleIdentifiers"] != serde_json::json!(["com.bgasol.sodeck"])
    {
        return Err("登录项参数、会话、归属或守护策略错误".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_registration_is_never_deleted_or_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("sodeck-login-owner-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("foreign.plist");
        let ours = Path::new("/Applications/SoDeck.app/Contents/MacOS/sodeck");
        let text = content(Path::new("/other/SoDeck.app/Contents/MacOS/sodeck")).unwrap();
        fs::write(&file, &text).unwrap();
        assert!(inspect_at(&file, ours).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
        fs::remove_dir_all(directory).unwrap();
    }
}
