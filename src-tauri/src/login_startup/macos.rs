use super::{command_path, Status};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn path() -> Result<PathBuf, String> {
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

fn plist(path: &Path) -> Result<serde_json::Value, String> {
    let mut command = Command::new("/usr/bin/plutil");
    command
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(path);
    serde_json::from_str(&output(command)?).map_err(|e| format!("登录项 plist 格式无效：{e}"))
}

fn content(executable: &Path) -> Result<String, String> {
    let executable = executable.to_str().ok_or("启动路径不是有效 Unicode")?;
    if executable.contains(['\0', '\n', '\r']) {
        return Err("启动路径包含无法登记的字符".into());
    }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.bgasol.sodeck</string><key>ProgramArguments</key><array><string>{}</string><string>--login-start</string></array><key>RunAtLoad</key><true/><key>LimitLoadToSessionType</key><string>Aqua</string><key>AssociatedBundleIdentifiers</key><array><string>com.bgasol.sodeck</string></array></dict></plist>\n",xml(executable)))
}

pub fn read() -> Result<Status, String> {
    let path = path()?;
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(Status::disabled());
    }
    let value = plist(&path)?;
    if value["Label"] != "com.bgasol.sodeck" {
        return Err("登录项身份不匹配，原文件已保留".into());
    }
    if value["RunAtLoad"] != true {
        return Ok(Status {
            enabled: false,
            state: "blocked".into(),
            reason: Some("登录项已被关闭。".into()),
        });
    }
    let mut command = Command::new("/bin/launchctl");
    command
        .arg("print-disabled")
        .arg(format!("gui/{}", unsafe { libc::geteuid() }));
    let disabled = output(command)?
        .lines()
        .any(|line| line.contains("\"com.bgasol.sodeck\"") && line.contains("=> true"));
    if disabled {
        return Ok(Status {
            enabled: false,
            state: "blocked".into(),
            reason: Some(
                "已被 macOS 禁用。请在系统设置 → 通用 → 登录项与扩展中允许 SoDeck。".into(),
            ),
        });
    }
    if value["ProgramArguments"] != serde_json::json!([command_path()?, "--login-start"]) {
        return Ok(Status {
            enabled: true,
            state: "stale".into(),
            reason: Some("登录项指向旧程序位置。关闭后重新开启可登记当前位置。".into()),
        });
    }
    Ok(Status {
        enabled: true,
        state: "enabled".into(),
        reason: None,
    })
}

pub fn set(enabled: bool) -> Result<(), String> {
    let path = path()?;
    if !enabled {
        return fs::remove_file(path)
            .or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .map_err(|e| format!("无法关闭 macOS 登录项：{e}"));
    }
    let content = content(&command_path()?)?;
    fs::create_dir_all(path.parent().ok_or("登录项路径无效")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("plist.tmp");
    fs::write(&temporary, content).map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| format!("无法提交 macOS 登录项：{e}"))
}

pub fn installed(executable: &Path) -> Result<(), String> {
    if executable
        .to_string_lossy()
        .contains(".app/Contents/MacOS/")
        && !executable.starts_with("/Volumes")
        && !executable.starts_with(std::env::temp_dir())
    {
        Ok(())
    } else {
        Err("请先将 SoDeck.app 安装到本机，再开启登录启动。".into())
    }
}

pub(super) fn verify(directory: &Path) -> Result<(), String> {
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
    {
        return Err("登录项参数、会话或守护策略错误".into());
    }
    Ok(())
}
