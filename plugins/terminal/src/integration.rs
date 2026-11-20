use serde_json::{json, Value};
use sodeck_plugin_client::{
    encoding::{base64, unbase64},
    Client,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn prepare(client: &Client, view: &str, context: &Value) -> Result<(String, Value), String> {
    let token = format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let posix = probe(
        client,
        view,
        context,
        &token,
        "printf 'SODECK-SHELL:%s\\n' \"$SHELL\"; uname -s",
    );
    if let Ok(output) = posix {
        if let Some(shell) = output
            .lines()
            .find_map(|line| line.strip_prefix("SODECK-SHELL:"))
        {
            if output
                .lines()
                .any(|s| matches!(s.trim(), "Linux" | "Darwin"))
                && supported(shell)
            {
                return Ok((
                    launch(shell, &token),
                    json!({"mode":"auto","token":token,"shell":shell}),
                ));
            }
            return Err("unsupported-shell".into());
        }
    }
    // Execute in the user's actual default shell. CMD must remain CMD even with PowerShell installed.
    let script = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);[Console]::WriteLine('SODECK-POWERSHELL:' + (Get-Process -Id $PID).Path)";
    let output = probe(client, view, context, &format!("windows-{token}"), script)?;
    let shell = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("SODECK-POWERSHELL:"))
        .ok_or("unsupported-shell")?;
    if !(shell.to_ascii_lowercase().ends_with("\\powershell.exe")
        || shell.to_ascii_lowercase().ends_with("\\pwsh.exe"))
        || shell.chars().any(char::is_control)
    {
        return Err("unsupported-shell".into());
    }
    let script = include_str!("../shell/powershell.ps1").replace("@TOKEN@", &token);
    Ok((
        format!(
            "{} -NoLogo -NoExit -EncodedCommand {}",
            if shell.to_ascii_lowercase().ends_with("\\pwsh.exe") {
                "pwsh.exe"
            } else {
                "powershell.exe"
            },
            encoded(&script)
        ),
        json!({"mode":"reading","token":token,"shell":shell}),
    ))
}
fn encoded(script: &str) -> String {
    base64(
        &script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}
fn probe(
    client: &Client,
    view: &str,
    context: &Value,
    id_hint: &str,
    command: &str,
) -> Result<String, String> {
    let task = client.call("session.exec.start", json!({"viewId":view,"expectedContext":context,"operationId":format!("shell-{id_hint}"),"sequence":"1","command":command,"timeoutMs":2500,"maxOutputBytes":2048}))?;
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
                return Ok(output.to_owned());
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
