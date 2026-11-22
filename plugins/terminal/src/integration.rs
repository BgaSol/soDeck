use serde_json::{json, Value};
use sodeck_plugin_client::protocol::requests as api;
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
        1,
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
    let output = probe(
        client,
        view,
        context,
        &format!("windows-{token}"),
        2,
        script,
    )?;
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
        // The default shell is already known to be PowerShell; use its call operator and exact path.
        format!(
            "& '{}' -NoLogo -NoExit -EncodedCommand {}",
            shell.replace('\'', "''"),
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
    sequence: u64,
    command: &str,
) -> Result<String, String> {
    let task = client
        .request(api::call::ExecStart(api::ExecStart {
            view_id: view.into(),
            expected_context: serde_json::from_value(context.clone())
                .map_err(|_| "终端上下文无效")?,
            operation_id: format!("shell-{id_hint}"),
            sequence: sequence.to_string(),
            command: command.into(),
            timeout_ms: 2500,
            max_output_bytes: 2048,
            program_output: None,
        }))
        .map_err(|e| e.message)?;
    let id = task.task_id;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_millis(2800);
        loop {
            let result = client
                .request(api::call::ExecPoll(api::ExecPoll {
                    task_id: id.to_string(),
                    stdout_offset: "0".to_string(),
                    stderr_offset: "0".to_string(),
                }))
                .map_err(|e| e.message)?;
            if result.output_complete {
                if result.error.as_ref().is_some_and(|e| e.code == "timedOut") {
                    return Err("shell-probe-timeout".into());
                }
                if result.exit_code != Some(0) {
                    return Err("shell-probe-failed".into());
                }
                let bytes = unbase64(&result.stdout.data_base64)?;
                let output = std::str::from_utf8(&bytes).map_err(|_| "shell-probe-failed")?;
                return Ok(output.to_owned());
            }
            if Instant::now() >= deadline {
                return Err("shell-probe-timeout".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    })();
    let _ = client
        .request(api::call::ExecRelease(api::Task {
            task_id: id.to_string(),
        }))
        .map_err(|e| e.message);
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
