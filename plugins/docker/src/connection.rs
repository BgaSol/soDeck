//! User configuration and immutable remote Docker identities.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Unix(String),
    Pipe(String),
}
impl Endpoint {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() > 1024 || value.chars().any(char::is_control) {
            return Err("Docker 端点无效".into());
        }
        if let Some(path) = value.strip_prefix("unix://") {
            if path.starts_with('/') && path.len() > 1 {
                return Ok(Self::Unix(path.into()));
            }
        }
        let normalized = value.replace('\\', "/");
        if let Some(pipe) = normalized
            .strip_prefix("npipe:////./pipe/")
            .or_else(|| normalized.strip_prefix("npipe://./pipe/"))
        {
            if !pipe.is_empty() && !pipe.contains(['/', '?', '#']) && pipe != "." && pipe != ".." {
                return Ok(Self::Pipe(pipe.into()));
            }
        }
        Err("仅支持远端绝对 Unix socket 或本机 npipe 命名管道".into())
    }
    pub fn uri(&self) -> String {
        match self {
            Self::Unix(path) => format!("unix://{path}"),
            Self::Pipe(name) => format!("npipe:////./pipe/{name}"),
        }
    }
    pub fn compatible(&self, environment: &Value) -> bool {
        matches!(
            (self, environment["kind"].as_str()),
            (Self::Unix(_), Some("posix")) | (Self::Pipe(_), Some("windows"))
        )
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Auto,
    Manual,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_path: Option<String>,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        match self.mode {
            Mode::Manual => {
                Endpoint::parse(self.endpoint.as_deref().ok_or("请填写远端 Docker 端点")?)?;
            }
            Mode::Auto => {
                if self.endpoint.is_some() {
                    return Err("自动方式不能保存固定端点".into());
                }
            }
        }
        if let Some(path) = &self.cli_path {
            if path.is_empty()
                || path.len() > 4096
                || path.chars().any(char::is_control)
                || !absolute(path)
            {
                return Err("Docker CLI 必须是远端绝对可执行路径".into());
            }
        }
        Ok(())
    }
}
pub fn absolute(path: &str) -> bool {
    path.starts_with('/')
        || (path.len() > 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'
            && matches!(path.as_bytes()[2], b'/' | b'\\'))
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct File {
    schema_version: u8,
    servers: BTreeMap<String, Config>,
}
pub fn load(directory: &Path) -> Result<BTreeMap<String, Config>, String> {
    let path = directory.join("endpoints.json");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("Docker 连接配置过大".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Docker 连接配置损坏，原文件已保留")?;
    let configs = if value.get("schemaVersion").is_some() {
        let file: File =
            serde_json::from_value(value).map_err(|_| "Docker 连接配置格式无效，原文件已保留")?;
        if file.schema_version != 2 {
            return Err("Docker 连接配置版本不支持，原文件已保留".into());
        }
        file.servers
    } else {
        let old: BTreeMap<String, String> =
            serde_json::from_value(value).map_err(|_| "Docker 连接配置损坏，原文件已保留")?;
        old.into_iter()
            .map(|(server, endpoint)| {
                (
                    server,
                    Config {
                        mode: Mode::Manual,
                        endpoint: Some(endpoint),
                        cli_path: None,
                    },
                )
            })
            .collect()
    };
    for config in configs.values() {
        config.validate()?
    }
    Ok(configs)
}
pub fn save(directory: &Path, servers: &BTreeMap<String, Config>) -> Result<(), String> {
    for config in servers.values() {
        config.validate()?;
    }
    let bytes = serde_json::to_vec(&File {
        schema_version: 2,
        servers: servers.clone(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("Docker 连接配置已达上限".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join("endpoints.json");
    let temp = directory.join("endpoints.tmp");
    if path.exists() && !directory.join("endpoints.previous.json").exists() {
        std::fs::copy(&path, directory.join("endpoints.previous.json"))
            .map_err(|e| e.to_string())?;
    }
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result.map_err(|e| format!("Docker 连接配置保存失败，原文件已保留：{e}"))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineOs {
    Linux,
    Windows,
    Unknown,
}
impl EngineOs {
    pub fn parse(value: &str) -> Self {
        match value {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            _ => Self::Unknown,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Unknown => "unknown",
        }
    }
    pub fn shell(&self) -> Option<&str> {
        match self {
            Self::Linux => Some("/bin/sh"),
            Self::Windows => Some("cmd.exe"),
            Self::Unknown => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Target {
    pub context: Value,
    pub revision: u64,
    pub environment: Value,
    pub cli: String,
    pub endpoint: Endpoint,
    pub engine_id: String,
    pub os: EngineOs,
    pub source: String,
}
pub type Snapshot = Arc<Target>;
impl Target {
    pub fn info(&self) -> Value {
        json!({"endpoint":self.endpoint.uri(),"engineId":self.engine_id,"os":self.os.name(),"cliPath":self.cli,"source":self.source,"configurationRevision":self.revision})
    }
    pub fn same(&self, other: &Self) -> bool {
        self.context == other.context
            && self.revision == other.revision
            && self.endpoint == other.endpoint
            && self.engine_id == other.engine_id
            && self.cli == other.cli
            && self.environment == other.environment
            && self.os == other.os
    }
}
pub fn discover<F>(
    config: &Config,
    environment: &Value,
    values: &Value,
    mut read: F,
) -> Result<(Endpoint, String, Value, Value), String>
where
    F: FnMut(Option<&Endpoint>, &[&str]) -> Result<Vec<u8>, String>,
{
    config.validate()?;
    let (endpoint, source) = match config.mode {
        Mode::Manual => (
            Endpoint::parse(config.endpoint.as_deref().unwrap())?,
            "手动指定".into(),
        ),
        Mode::Auto => {
            let context = values["DOCKER_CONTEXT"].as_str().unwrap_or("");
            let host = values["DOCKER_HOST"].as_str().unwrap_or("");
            if context.is_empty() && !host.is_empty() {
                (Endpoint::parse(host)?, "DOCKER_HOST".into())
            } else {
                let current = if context.is_empty() {
                    String::from_utf8(read(None, &["context", "show"])?)
                        .map_err(|_| "Docker Context 输出不是 UTF-8")?
                        .trim()
                        .to_string()
                } else {
                    context.into()
                };
                if current.is_empty()
                    || current.len() > 256
                    || current.chars().any(char::is_control)
                {
                    return Err("Docker Context 无效".into());
                }
                let bytes = read(
                    None,
                    &[
                        "context",
                        "inspect",
                        "--format",
                        "{{json .Endpoints.docker}}",
                        &current,
                    ],
                )?;
                let endpoint: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "Docker Context 输出不兼容")?;
                (
                    Endpoint::parse(endpoint["Host"].as_str().ok_or("Docker Context 缺少端点")?)?,
                    format!("Context {current}"),
                )
            }
        }
    };
    if !endpoint.compatible(environment) {
        return Err("Docker 端点与当前远端执行环境不兼容".into());
    }
    let version: Value = serde_json::from_slice(&read(
        Some(&endpoint),
        &["version", "--format", "{{json .}}"],
    )?)
    .map_err(|_| "Docker version 输出不兼容")?;
    let info: Value =
        serde_json::from_slice(&read(Some(&endpoint), &["info", "--format", "{{json .}}"])?)
            .map_err(|_| "Docker info 输出不兼容")?;
    info["ID"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .ok_or("Engine ID 不可用")?;
    Ok((endpoint, source, version, info))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_are_remote_and_local_to_that_host() {
        assert_eq!(
            Endpoint::parse("npipe:////./pipe/dockerDesktopLinuxEngine")
                .unwrap()
                .uri(),
            "npipe:////./pipe/dockerDesktopLinuxEngine"
        );
        for path in [
            "tcp://127.0.0.1:2375",
            "npipe:////server/pipe/docker",
            "unix://relative",
            "unix:///bad\n",
        ] {
            assert!(Endpoint::parse(path).is_err())
        }
    }
    #[test]
    fn discovery_does_not_fall_back_from_overrides() {
        let cfg = Config::default();
        let env = json!({"kind":"windows"});
        let mut calls = vec![];
        let result = discover(
            &cfg,
            &env,
            &json!({"DOCKER_HOST":"tcp://host:2375"}),
            |_, args| {
                calls.push(args.join(" "));
                Ok(vec![])
            },
        );
        assert!(result.is_err());
        assert!(calls.is_empty());
    }
    #[test]
    fn context_override_wins_and_engine_reads_use_the_fixed_pipe() {
        let endpoint = Endpoint::Pipe("desktop-test".into());
        let mut calls = vec![];
        let (actual, source, _, _) = discover(
            &Config::default(),
            &json!({"kind":"windows"}),
            &json!({"DOCKER_CONTEXT":"custom","DOCKER_HOST":"tcp://ignored:2375"}),
            |host, argv| {
                calls.push(argv.join(" "));
                if argv[0] == "context" {
                    assert!(host.is_none());
                    assert_eq!(argv.last(), Some(&"custom"));
                    Ok(serde_json::to_vec(&json!({"Host":endpoint.uri()})).unwrap())
                } else {
                    assert_eq!(host, Some(&endpoint));
                    Ok(serde_json::to_vec(&json!({"ID":"engine"})).unwrap())
                }
            },
        )
        .unwrap();
        assert_eq!(actual, endpoint);
        assert_eq!(source, "Context custom");
        assert_eq!(calls.len(), 3);
    }
    #[test]
    fn legacy_manual_configuration_survives_migration_and_failed_save() {
        let dir = std::env::temp_dir().join(format!(
            "sodeck-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let original = br#"{"server":"unix:///run/user/1000/docker.sock"}"#;
        std::fs::write(dir.join("endpoints.json"), original).unwrap();
        let mut configs = load(&dir).unwrap();
        assert_eq!(configs["server"].mode, Mode::Manual);
        assert_eq!(std::fs::read(dir.join("endpoints.json")).unwrap(), original);
        save(&dir, &configs).unwrap();
        assert_eq!(load(&dir).unwrap(), configs);
        assert_eq!(
            std::fs::read(dir.join("endpoints.previous.json")).unwrap(),
            original
        );
        let valid = std::fs::read(dir.join("endpoints.json")).unwrap();
        configs.insert(
            "large".into(),
            Config {
                mode: Mode::Manual,
                endpoint: Some("x".repeat(65536)),
                cli_path: None,
            },
        );
        assert!(save(&dir, &configs).is_err());
        assert_eq!(std::fs::read(dir.join("endpoints.json")).unwrap(), valid);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
