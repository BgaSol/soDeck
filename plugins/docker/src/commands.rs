use serde_json::{json, Value};

pub const SUMMARY: &str = r#"{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Config.Image}},"status":{{json .State.Status}},"exitCode":{{json .State.ExitCode}},"health":{{with index .State "Health"}}{{json .Status}}{{else}}null{{end}},"labels":{{json .Config.Labels}},"ports":{{json .NetworkSettings.Ports}},"startedAt":{{json .State.StartedAt}}}"#;

pub fn endpoint(value: &str) -> Result<(), String> {
    let path = value
        .strip_prefix("unix://")
        .ok_or("请填写 unix:///绝对/socket/路径")?;
    if !path.starts_with('/')
        || path.len() < 2
        || value.len() > 1024
        || value.chars().any(char::is_control)
    {
        return Err("Docker socket 必须是远端绝对路径".into());
    }
    Ok(())
}
pub fn id(value: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("需要完整容器 ID".into())
    }
}
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn docker(endpoint: &str, args: &[&str]) -> String {
    let mut argv = vec!["sh", "-c", "unset DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH DOCKER_API_VERSION; host=$1; shift; exec docker --host \"$host\" \"$@\"", "sodeck", endpoint];
    argv.extend_from_slice(args);
    argv.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
}
pub fn terminal_launch(
    endpoint_value: &str,
    engine: &str,
    container: &str,
    shell: &str,
) -> Result<String, String> {
    endpoint(endpoint_value)?;
    id(container)?;
    if engine.is_empty() || engine.len() > 256 || engine.chars().any(char::is_control) {
        return Err("Engine ID 无效".into());
    }
    if !shell.starts_with('/') || shell.len() > 256 || shell.chars().any(char::is_control) {
        return Err("Shell 必须为容器内的绝对路径".into());
    }
    let check = docker(endpoint_value, &["info", "--format", "{{.ID}}"]);
    let enter = docker(
        endpoint_value,
        &[
            "container",
            "exec",
            "--interactive",
            "--tty",
            container,
            shell,
        ],
    );
    Ok(format!("actual=$({check}) || exit; [ \"$actual\" = {} ] || {{ printf '%s\\n' 'Docker Engine changed; terminal not started' >&2; exit 1; }}; exec {enter}",quote(engine)))
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
pub fn fields(value: &Value) -> Value {
    let list = |items: &Value| -> Vec<Value> {
        items
            .as_array()
            .map(|a| a.iter().take(100).cloned().collect())
            .unwrap_or_default()
    };
    let mut result = json!({
        "basic": [], "state": [], "environment": [], "ports": [], "mounts": [], "networks": [], "labels": []
    });
    let add = |section: &mut Value, key: &str, value: &Value| {
        let body = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        section
            .as_array_mut()
            .unwrap()
            .push(json!({"key":text(key,1024),"value":text(&body,4096)}));
    };
    for (key, v) in [
        ("镜像", &value["Config"]["Image"]),
        ("镜像 ID", &value["Image"]),
        ("创建时间", &value["Created"]),
        ("入口", &value["Config"]["Entrypoint"]),
        ("启动命令", &value["Config"]["Cmd"]),
        ("实际命令", &value["Path"]),
        ("参数", &value["Args"]),
        ("用户", &value["Config"]["User"]),
        ("容器工作目录", &value["Config"]["WorkingDir"]),
        ("重启策略", &value["HostConfig"]["RestartPolicy"]),
    ] {
        add(&mut result["basic"], key, v);
    }
    for key in [
        "Status",
        "ExitCode",
        "OOMKilled",
        "Error",
        "StartedAt",
        "FinishedAt",
        "Running",
        "Paused",
        "Restarting",
    ] {
        add(&mut result["state"], key, &value["State"][key]);
    }
    add(&mut result["state"], "重启次数", &value["RestartCount"]);
    add(
        &mut result["state"],
        "健康状态",
        &value["State"]["Health"]["Status"],
    );
    for entry in value["State"]["Health"]["Log"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .take(4)
    {
        add(
            &mut result["state"],
            entry["End"].as_str().unwrap_or("健康检查"),
            &entry["Output"],
        );
    }
    for entry in list(&value["Config"]["Env"]) {
        if let Some(s) = entry.as_str() {
            let (key, val) = s.split_once('=').unwrap_or((s, ""));
            result["environment"]
                .as_array_mut()
                .unwrap()
                .push(json!({"key":text(key,1024),"value":text(val,4096),"secret":true}));
        }
    }
    for (section, map) in [
        ("ports", &value["NetworkSettings"]["Ports"]),
        ("networks", &value["NetworkSettings"]["Networks"]),
        ("labels", &value["Config"]["Labels"]),
    ] {
        if let Some(map) = map.as_object() {
            for (key, v) in map.iter().take(100) {
                add(&mut result[section], key, v);
            }
        }
    }
    for mount in list(&value["Mounts"]) {
        add(
            &mut result["mounts"],
            mount["Destination"].as_str().unwrap_or("挂载"),
            &mount,
        );
    }
    for (section, source) in [
        ("environment", &value["Config"]["Env"]),
        ("ports", &value["NetworkSettings"]["Ports"]),
        ("mounts", &value["Mounts"]),
        ("networks", &value["NetworkSettings"]["Networks"]),
        ("labels", &value["Config"]["Labels"]),
    ] {
        let count = source
            .as_array()
            .map(Vec::len)
            .or_else(|| source.as_object().map(|m| m.len()))
            .unwrap_or(0);
        if count > 100 {
            add(
                &mut result[section],
                "已截断",
                &json!(format!("仅保留前 100 项，共 {count} 项")),
            );
        }
    }
    result
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
        assert!(endpoint("unix:///run/user/1000/docker.sock").is_ok());
        for e in ["tcp://host:2375", "unix://relative", "unix:///a\nb"] {
            assert!(endpoint(e).is_err());
        }
        assert_eq!(quote("a'b;$(x)"), "'a'\\''b;$(x)'");
        assert!(id(&"a".repeat(64)).is_ok());
        assert!(id("abcdef").is_err());
        assert_eq!(unbase64("5Lit5paH").unwrap(), "中文".as_bytes());
        assert_eq!(text("中文中文", 7), "中文… [已截断]");
    }
    #[test]
    fn terminal_launch_fixes_identity_and_quotes_the_shell_as_one_argument() {
        let container = "a".repeat(64);
        let launch = terminal_launch(
            "unix:///run/docker.sock",
            "engine-1",
            &container,
            "/a'b;$(touch nope)",
        )
        .unwrap();
        assert!(launch.contains(&quote("/a'b;$(touch nope)")));
        assert!(launch.contains(&quote(&container)));
        assert!(launch.contains("actual=$("));
        assert!(launch.contains("[ \"$actual\" = 'engine-1' ]"));
        assert_eq!(launch.matches("DOCKER_CONTEXT").count(), 2);
        for shell in ["sh", "/bin/sh\n", "", &format!("/{}", "a".repeat(256))] {
            assert!(
                terminal_launch("unix:///run/docker.sock", "engine-1", &container, shell).is_err()
            );
        }
        assert!(terminal_launch("tcp://host", "engine-1", &container, "/bin/sh").is_err());
        assert!(terminal_launch("unix:///run/docker.sock", "", &container, "/bin/sh").is_err());
        assert!(terminal_launch("unix:///run/docker.sock", "engine-1", "abc", "/bin/sh").is_err());
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
    #[test]
    fn inspect_fields_preserve_values_without_unbounded_output() {
        let v = json!({"Config":{"Env":["TOKEN=a=b","EMPTY="],"Labels":{"comma":"a,b\"中"}},"State":{"Health":null},"Mounts":[{"Type":"bind","Source":"/host","Destination":"/container"}]});
        let f = fields(&v);
        assert_eq!(f["environment"][0]["value"], "a=b");
        assert_eq!(f["environment"][0]["secret"], true);
        assert_eq!(f["mounts"][0]["key"], "/container");
        assert_eq!(f["labels"][0]["value"], "a,b\"中");
        let limited = fields(&json!({"Config":{"Env":vec!["TOKEN=value";101]}}));
        assert_eq!(limited["environment"].as_array().unwrap().len(), 101);
        assert_eq!(limited["environment"][100]["key"], "已截断");
        assert!(lines(b"{invalid}\n").is_err());
    }
}
