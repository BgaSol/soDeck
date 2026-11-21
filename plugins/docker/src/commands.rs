#[cfg(test)]
use serde_json::json;
use serde_json::Value;

pub const SUMMARY: &str = r#"{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Config.Image}},"imageId":{{json .Image}},"status":{{json .State.Status}},"exitCode":{{json .State.ExitCode}},"health":{{with index .State "Health"}}{{json .Status}}{{else}}null{{end}},"labels":{{json .Config.Labels}},"startedAt":{{json .State.StartedAt}}}"#;

pub fn id(value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("需要完整容器 ID".into())
    }
}
pub fn lines(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Docker 输出不是 UTF-8")?;
    text.lines()
        .filter(|s| !s.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|_| "Docker 输出格式不兼容".into()))
        .collect()
}
pub fn object(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| "Docker 输出格式不兼容".into())
}
pub fn action(action: &str, container: &Value) -> Result<Option<Vec<String>>, String> {
    let status = container["State"]["Status"]
        .as_str()
        .ok_or("缺少容器状态")?;
    let id = container["Id"].as_str().ok_or("缺少容器 ID")?;
    self::id(id)?;
    let args: Vec<&str> = match action {
        "start" if status == "running" => return Ok(None),
        "stop" if matches!(status, "exited" | "created") => return Ok(None),
        "start" => vec!["container", "start", id],
        "stop" => vec!["container", "stop", "--time", "10", id],
        "restart" => vec!["container", "restart", "--time", "10", id],
        "pause" if status == "running" => vec!["container", "pause", id],
        "unpause" if status == "paused" => vec!["container", "unpause", id],
        "remove" if matches!(status, "exited" | "created" | "dead") => vec!["container", "rm", id],
        "pause" | "unpause" | "remove" => {
            return Err("当前状态不允许此操作，请刷新后重试；仅删除已停止的容器".into())
        }
        _ => return Err("未知容器操作".into()),
    };
    Ok(Some(args.into_iter().map(str::to_owned).collect()))
}
pub fn text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.into();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [已截断]", &value[..end])
}
pub fn operation_name(action: &str) -> &str {
    match action {
        "start" => "启动",
        "stop" => "停止",
        "restart" => "重启",
        "pause" => "暂停",
        "unpause" => "恢复",
        "remove" => "删除",
        _ => action,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sodeck_plugin_client::encoding::unbase64;
    #[test]
    fn command_output_rejects_malformed_base64() {
        for input in ["Zg=a", "Zh==", "Zg", "Zg==!", "====", "Zm9v=", "Zg==\n"] {
            assert!(
                unbase64(input).is_err(),
                "accepted malformed output: {input:?}"
            );
        }
    }
    #[test]
    fn boundaries_and_shell() {
        assert!(id(&"a".repeat(64)).is_ok());
        assert!(id("abcdef").is_err());
        assert_eq!(unbase64("5Lit5paH").unwrap(), "中文".as_bytes());
        assert_eq!(text("中文中文", 7), "中文… [已截断]");
    }
    #[test]
    fn state_is_checked_before_mutation() {
        let mut v = json!({"Id":"a".repeat(64),"State":{"Status":"running"}});
        assert!(action("remove", &v).is_err());
        assert!(action("start", &v).unwrap().is_none());
        v["State"]["Status"] = json!("exited");
        assert_eq!(
            action("remove", &v).unwrap().unwrap(),
            vec!["container", "rm", &"a".repeat(64)]
        );
        assert!(action("--force", &v).is_err());
    }
}
