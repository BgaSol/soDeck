use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    LoginShell,
    Command(String),
}
#[derive(Clone, Debug, PartialEq)]
pub enum DirectoryIntegration {
    Auto,
    Manual,
    Off,
}
impl DirectoryIntegration {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Off => "off",
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "manual" => Ok(Self::Manual),
            "off" => Ok(Self::Off),
            _ => Err("未知目录集成策略".into()),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct CreateParams {
    pub title: Option<String>,
    pub launch: Launch,
    pub directory: DirectoryIntegration,
}
impl CreateParams {
    pub fn parse(
        input: &Value,
        default_mode: impl FnOnce() -> String,
        can_command: bool,
    ) -> Result<Self, String> {
        if serde_json::to_vec(input).map_err(|e| e.to_string())?.len() > 16 * 1024 {
            return Err("终端创建参数超过 16 KiB".into());
        }
        if !input.is_null() && !input.is_object() {
            return Err("终端创建参数必须为对象".into());
        }
        if input.get("version").is_some_and(|v| v.as_u64() != Some(1)) {
            return Err("不支持的终端创建参数版本".into());
        }
        let title = input
            .get("title")
            .map(|v| {
                let s = v.as_str().ok_or("终端标题必须为字符串")?;
                if s.trim().is_empty() || s.chars().count() > 128 || s.chars().any(char::is_control)
                {
                    return Err("终端标题必须为 1 至 128 个字符且不含控制字符".into());
                }
                Ok::<_, String>(s.to_string())
            })
            .transpose()?;
        let launch = match input.get("launch") {
            None => Launch::LoginShell,
            Some(v) => match v["kind"].as_str() {
                Some("loginShell") => Launch::LoginShell,
                Some("command") => {
                    let command = v["command"].as_str().ok_or("缺少终端启动命令")?;
                    if command.trim().is_empty() || command.contains('\0') {
                        return Err("启动命令不能为空或包含 NUL".into());
                    }
                    if !can_command {
                        return Err("核心缺少 terminal.launch.v1，请先升级 SoDeck".into());
                    }
                    Launch::Command(command.into())
                }
                _ => return Err("未知终端启动方式".into()),
            },
        };
        let directory = match input.get("directoryIntegration") {
            Some(v) => DirectoryIntegration::parse(v.as_str().ok_or("目录集成策略必须为字符串")?)?,
            None if matches!(launch, Launch::Command(_)) => DirectoryIntegration::Off,
            None => DirectoryIntegration::parse(&default_mode())?,
        };
        if matches!(launch, Launch::Command(_)) && directory != DirectoryIntegration::Off {
            return Err("指定命令终端目前只支持关闭目录集成".into());
        }
        Ok(Self {
            title,
            launch,
            directory,
        })
    }
    pub fn value(&self) -> Value {
        let launch = match &self.launch {
            Launch::LoginShell => json!({"kind":"loginShell"}),
            Launch::Command(command) => json!({"kind":"command","command":command}),
        };
        let mut value =
            json!({"version":1,"launch":launch,"directoryIntegration":self.directory.name()});
        if let Some(title) = &self.title {
            value["title"] = json!(title);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_resolved_once_and_round_trip() {
        for input in [Value::Null, json!({}), json!({"version":1,"future":true})] {
            let resolved = CreateParams::parse(&input, || "manual".into(), false).unwrap();
            assert_eq!(resolved.launch, Launch::LoginShell);
            assert_eq!(
                CreateParams::parse(&resolved.value(), || panic!("must preserve mode"), true)
                    .unwrap(),
                resolved
            );
        }
    }
    #[test]
    fn command_never_reads_host_settings_and_requires_only_launch() {
        let command =
            json!({"title":"容器","launch":{"kind":"command","command":"docker exec -it api sh"}});
        let resolved = CreateParams::parse(&command, || panic!("no host settings"), true).unwrap();
        assert_eq!(resolved.directory, DirectoryIntegration::Off);
        assert_eq!(
            CreateParams::parse(&resolved.value(), || panic!(), true).unwrap(),
            resolved
        );
        assert!(CreateParams::parse(&command, || panic!(), false).is_err());
        for mode in ["auto", "manual"] {
            let mut input = command.clone();
            input["directoryIntegration"] = json!(mode);
            assert!(CreateParams::parse(&input, || panic!(), true).is_err());
        }
    }
    #[test]
    fn rejects_invalid_known_fields_and_bounds() {
        for input in [
            json!([]),
            json!({"version":2}),
            json!({"version":"1"}),
            json!({"title":"\n"}),
            json!({"title":4}),
            json!({"title":"x".repeat(129)}),
            json!({"launch":null}),
            json!({"launch":{"kind":"other"}}),
            json!({"launch":{"kind":"command","command":" "}}),
            json!({"launch":{"kind":"command","command":"a\u{0}b"}}),
            json!({"directoryIntegration":false}),
            json!({"future":"x".repeat(16384)}),
        ] {
            assert!(
                CreateParams::parse(&input, || "auto".into(), true).is_err(),
                "{input}"
            );
        }
    }
}
