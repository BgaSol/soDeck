use serde_json::{json, Value};
use sodeck_plugin_client::{encoding::unbase64, Client};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn prepare(client: &Client, view: &str, context: &Value) -> Result<(String, Value), String> {
    let token = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let task = client.call("session.exec.start", json!({"viewId":view,"expectedContext":context,"operationId":format!("shell-{token}"),"sequence":"1","command":"printf 'SODECK-SHELL:%s\\n' \"$SHELL\"; uname -s","timeoutMs":2500,"maxOutputBytes":1024}))?;
    let id = &task["taskId"];
    let result = (|| {
        let deadline = Instant::now() + Duration::from_millis(2800);
        loop {
            let result = client.call(
                "session.exec.poll",
                json!({"taskId":id,"stdoutOffset":"0","stderrOffset":"0"}),
            )?;
            if result["outputComplete"] == true {
                if result["error"]["code"] == "timedOut" {
                    return Err("shell-probe-timeout".into());
                }
                if result["exitCode"] != 0 {
                    return Err("shell-probe-failed".into());
                }
                let bytes = unbase64(result["stdout"]["dataBase64"].as_str().unwrap_or(""))?;
                let output = std::str::from_utf8(&bytes).map_err(|_| "shell-probe-failed")?;
                let shell = output
                    .lines()
                    .find_map(|line| line.strip_prefix("SODECK-SHELL:"))
                    .ok_or("unsupported-shell")?;
                if !output.lines().any(|s| s.trim() == "Linux") || !supported(shell) {
                    return Err("unsupported-shell".into());
                }
                return Ok((
                    launch(shell, &token),
                    json!({"mode":"auto","token":token,"shell":shell}),
                ));
            }
            if Instant::now() >= deadline {
                return Err("shell-probe-timeout".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    })();
    let _ = client.call("session.exec.release", json!({"taskId":id}));
    result
}
fn supported(shell: &str) -> bool {
    shell.starts_with('/')
        && (shell.ends_with("/bash") || shell.ends_with("/zsh"))
        && shell
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
}
fn launch(shell: &str, token: &str) -> String {
    let script = include_str!("../shell/launch.sh")
        .replace("@INTEGRATION@", include_str!("../shell/integration.sh"))
        .replace("@BASH@", include_str!("../shell/bash.sh"))
        .replace("@ZSH@", include_str!("../shell/zsh.sh"))
        .replace("@TOKEN@", token)
        .replace("@SHELL@", shell);
    format!("/bin/sh -c '{}'", script.replace('\'', "'\\''"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_absolute_shells_enter_the_launch_script() {
        for shell in ["/bin/bash", "/usr/bin/zsh"] {
            assert!(supported(shell));
        }
        for shell in ["bash", "/bin/fish", "/bin/x;bash", "/bin/$(x)/zsh"] {
            assert!(!supported(shell));
        }
        let command = launch("/bin/bash", "abc123");
        assert!(!command.contains("@TOKEN@"));
        assert!(command.len() < 32 * 1024);
    }
}
