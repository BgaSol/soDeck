//! Process admission and one-shot launch intent; never initializes a second AppCore.
use crate::{app_data::Lease, desktop_shell::DesktopShell};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

mod instance;

#[derive(Clone, Debug)]
pub struct Launch {
    pub login: bool,
    pub restart: bool,
    pub isolated: bool,
    pub data: Option<PathBuf>,
}

impl Launch {
    pub fn parse(args: &[OsString]) -> Result<Self, String> {
        let login = args.iter().any(|a| a == "--login-start");
        let restart = args.iter().any(|a| a == "--restart-from");
        let isolated = args
            .iter()
            .any(|a| a == "--smoke-test" || a == "--interactive-test");
        let data = args
            .iter()
            .position(|a| a == "--test-data-dir")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        if args.iter().any(|a| a == "--interactive-test") && data.is_none() {
            return Err("交互验收必须指定隔离的 --test-data-dir".into());
        }
        if login && restart {
            return Err("登录启动与进程替换参数不能同时使用".into());
        }
        if restart {
            let value = args
                .iter()
                .position(|a| a == "--restart-from")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.to_str())
                .and_then(|v| v.parse::<u32>().ok());
            if value.is_none() {
                return Err("进程替换来源无效".into());
            }
        }
        Ok(Self {
            login,
            restart,
            isolated,
            data,
        })
    }

    pub fn restart_environment(args: &[OsString]) -> tauri::Env {
        let mut env = tauri::Env::default();
        let mut retained = Vec::new();
        let mut skip = false;
        for arg in args {
            if skip {
                skip = false;
                continue;
            }
            if arg == "--login-start" {
                continue;
            }
            if arg == "--restart-from" {
                skip = true;
                continue;
            }
            retained.push(arg.clone());
        }
        retained.push("--restart-from".into());
        retained.push(std::process::id().to_string().into());
        env.args_os = retained;
        env
    }
}

pub struct Prepared(pub Mutex<Option<Lease>>);

pub struct Activation {
    pending: AtomicBool,
    _server: instance::Server,
}

impl Activation {
    pub fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

pub fn prepare(handle: &tauri::AppHandle, launch: &Launch) -> Result<bool, String> {
    let root = if launch.isolated {
        launch.data.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("sodeck-smoke-{}", std::process::id()))
        })
    } else {
        handle.path().app_data_dir().map_err(|e| e.to_string())?
    };
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let endpoint = instance::Endpoint::new(&root)?;
    let deadline = Instant::now() + Duration::from_secs(if launch.restart { 15 } else { 4 });
    let lease = loop {
        match Lease::acquire(&root) {
            Ok(lease) => break lease,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if launch.login {
                    return Ok(false);
                }
                if !launch.restart {
                    match endpoint.activate() {
                        Ok(()) => return Ok(false),
                        Err(instance::RequestError::Closing) => {
                            return Err("SoDeck 正在退出或重启，请稍后重新打开。".into())
                        }
                        Err(instance::RequestError::Unavailable) => {}
                    }
                }
                if Instant::now() >= deadline {
                    return Err(
                        "应用数据正在被另一个 SoDeck 实例使用，暂时无法打开窗口，请稍后重试。"
                            .into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(error) => return Err(format!("无法取得应用数据锁：{error}")),
        }
    };
    let native = handle.clone();
    let server = endpoint.listen(move || {
        if let Some(shell) = native.try_state::<DesktopShell>() {
            if !shell.accepts_activation() {
                return false;
            }
        }
        if let Some(activation) = native.try_state::<Activation>() {
            activation.pending.store(true, Ordering::Release);
        }
        let target = native.clone();
        native
            .run_on_main_thread(move || {
                if let Some(shell) = target.try_state::<DesktopShell>() {
                    if shell.accepts_activation() {
                        if let Some(activation) = target.try_state::<Activation>() {
                            activation.take_pending();
                        }
                        if let Err(error) = shell.present(&target, "main", true) {
                            shell.report(&target, error);
                        }
                    }
                }
            })
            .is_ok()
    })?;
    handle.manage(Activation {
        pending: AtomicBool::new(false),
        _server: server,
    });
    handle.manage(Prepared(Mutex::new(Some(lease))));
    Ok(true)
}

pub fn verify_native(handle: &tauri::AppHandle, core: &crate::AppCore) -> Result<(), String> {
    let env = handle.env();
    if env.args_os.iter().any(|arg| arg == "--login-start") {
        return Err("登录来源泄漏到下一次重启参数".into());
    }
    let marker = env
        .args_os
        .iter()
        .position(|arg| arg == "--restart-from")
        .and_then(|i| env.args_os.get(i + 1));
    if marker != Some(&std::process::id().to_string().into()) {
        return Err("重启参数未保留当前进程交接来源".into());
    }
    let login = std::env::args_os().any(|arg| arg == "--login-start");
    let window = handle
        .get_webview_window("main")
        .ok_or("启动主窗口不存在")?;
    if login {
        if cfg!(target_os = "linux") {
            if !window.is_minimized().map_err(|e| e.to_string())? {
                return Err("登录启动未最小化".into());
            }
        } else if window.is_visible().map_err(|e| e.to_string())? {
            return Err("登录启动意外显示主窗口".into());
        }
    }
    let root = core.snapshot().data_directory;
    let mut command = std::process::Command::new(
        tauri::process::current_binary(&handle.env()).map_err(|e| e.to_string())?,
    );
    command
        .arg("--interactive-test")
        .arg("--test-data-dir")
        .arg(&root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("第二实例未能激活原窗口".into());
            }
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("第二实例未在期限内退出".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while !window.is_visible().map_err(|e| e.to_string())?
        || window.is_minimized().map_err(|e| e.to_string())?
    {
        if Instant::now() >= deadline {
            return Err("第二次打开没有显示原主窗口".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if Lease::acquire(&root).is_ok() {
        return Err("激活交接丢失原数据独占".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_intent_is_consumed_but_isolated_paths_survive_restart() {
        let args: Vec<OsString> = [
            "sodeck",
            "--login-start",
            "--interactive-test",
            "--test-data-dir",
            "路径 with spaces",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        assert!(Launch::parse(&args).unwrap().login);
        let next = Launch::restart_environment(&args);
        let parsed = Launch::parse(&next.args_os).unwrap();
        assert!(!parsed.login);
        assert!(parsed.restart);
        assert_eq!(parsed.data.unwrap(), PathBuf::from("路径 with spaces"));
        assert_eq!(
            Launch::restart_environment(&next.args_os).args_os,
            next.args_os
        );
    }
}
