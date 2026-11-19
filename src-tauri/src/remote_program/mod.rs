//! Platform encoding for owned remote programs; execution stays in session services.
mod posix;
mod windows;
use crate::{session_exec::Error, workspaces::Context, AppCore};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const ENVIRONMENT_OUTPUT_BUDGET: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Environment {
    Posix,
    Windows {
        launcher: String,
        #[serde(rename = "exitMode")]
        exit_mode: ExitMode,
    },
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExitMode {
    Native,
    Powershell,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Check {
    pub argv: Vec<String>,
    pub expected_stdout: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Program {
    pub file: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub remove_env: Vec<String>,
    #[serde(default)]
    pub checks: Vec<Check>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    NonInteractive,
    Terminal,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    view_id: String,
    expected_context: Context,
    environment: Environment,
    mode: Mode,
    program: Program,
}
pub(crate) fn error(code: &'static str, message: impl Into<String>) -> Error {
    Error {
        code,
        message: message.into(),
    }
}
fn variable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
}
pub(crate) fn encode(
    environment: &Environment,
    mode: Mode,
    program: &Program,
) -> Result<String, Error> {
    if program.file.is_empty()
        || program.file.len() > 4096
        || program.file.chars().any(char::is_control)
        || program.argv.len() > 128
        || program.remove_env.len() > 8
        || program.checks.len() > 2
        || program.remove_env.iter().any(|s| !variable(s))
    {
        return Err(error("invalidParams", "程序描述超出范围"));
    }
    let args = program
        .argv
        .iter()
        .chain(program.checks.iter().flat_map(|c| c.argv.iter()));
    let mut size = program.file.len();
    for arg in args {
        if arg.len() > 4096 || arg.contains('\0') {
            return Err(error("invalidParams", "程序参数无效"));
        }
        size += arg.len();
    }
    if size > 16 * 1024
        || program.checks.iter().any(|c| {
            c.argv.len() > 128
                || c.expected_stdout.len() > 1024
                || c.expected_stdout.chars().any(char::is_control)
        })
    {
        return Err(error("invalidParams", "程序检查超出范围"));
    }
    let command = match environment {
        Environment::Posix => posix::encode(mode, program),
        Environment::Windows {
            launcher,
            exit_mode,
        } => {
            if launcher.is_empty()
                || launcher.len() > 4096
                || launcher.chars().any(char::is_control)
            {
                return Err(error("invalidParams", "Windows 启动器路径无效"));
            }
            if !launcher.rsplit(['\\', '/']).next().is_some_and(|s| {
                s.eq_ignore_ascii_case("powershell.exe") || s.eq_ignore_ascii_case("pwsh.exe")
            }) {
                return Err(error(
                    "invalidParams",
                    "Windows 启动器必须为已探测的 PowerShell",
                ));
            }
            windows::encode(launcher, *exit_mode, mode, program)
        }
    };
    if command.len() > 16 * 1024
        || matches!(environment, Environment::Windows { .. })
            && command.encode_utf16().count() > 7900
    {
        return Err(error("commandTooLong", "程序启动命令超过远端预算"));
    }
    Ok(command)
}
impl AppCore {
    pub(crate) fn prepare_program(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        let r: Prepare =
            serde_json::from_value(p.clone()).map_err(|e| error("invalidParams", e.to_string()))?;
        let owner = self
            .resource_owner(plugin, instance, &r.view_id)
            .map_err(|e| error("staleContext", e))?;
        if owner.context != r.expected_context {
            return Err(error("staleContext", "连接代次已改变"));
        }
        self.connection_session(&owner.context)
            .map_err(|e| error("staleContext", e))?;
        let command = encode(&r.environment, r.mode, &r.program)?;
        Ok(
            json!({"length":command.len(),"command":command,"mode":if r.mode==Mode::Terminal{"terminal"}else{"nonInteractive"},
            "programOutput":if r.mode==Mode::NonInteractive{json!("startup-v1")}else{Value::Null}}),
        )
    }
}
pub(crate) fn probe(keys: &[String], windows: bool) -> String {
    if windows {
        windows::probe(keys)
    } else {
        posix::probe(keys)
    }
}
pub(crate) fn validate_keys(keys: &[String]) -> Result<(), Error> {
    if keys.len() > 8 || keys.iter().any(|s| !variable(s)) {
        Err(error("invalidParams", "环境变量名无效"))
    } else {
        Ok(())
    }
}
pub(crate) fn parse_environment(
    bytes: &[u8],
    keys: &[String],
    exit_code: u32,
) -> Result<Value, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let text = std::str::from_utf8(bytes).map_err(|_| "invalidEnvironment")?;
    let mut lines = text.lines();
    if lines.next() != Some("SODECK_ENV_1") {
        return Err("invalidEnvironment".into());
    }
    let environment = match lines.next() {
        Some("posix") if exit_code == 0 => json!({"kind":"posix"}),
        Some("windows") => {
            let launcher = String::from_utf8(
                STANDARD
                    .decode(lines.next().ok_or("invalidEnvironment")?)
                    .map_err(|_| "invalidEnvironment")?,
            )
            .map_err(|_| "invalidEnvironment")?;
            if launcher.is_empty()
                || launcher.len() > 4096
                || launcher.chars().any(char::is_control)
            {
                return Err("invalidEnvironment".into());
            }
            let exit_mode = match exit_code {
                37 => "native",
                1 => "powershell",
                _ => return Err("unsupportedEnvironment".into()),
            };
            json!({"kind":"windows","launcher":launcher,"exitMode":exit_mode})
        }
        _ => return Err("invalidEnvironment".into()),
    };
    let mut values = serde_json::Map::new();
    for key in keys {
        let bytes = STANDARD
            .decode(lines.next().ok_or("invalidEnvironment")?)
            .map_err(|_| "invalidEnvironment")?;
        let value = String::from_utf8(bytes).map_err(|_| "invalidEnvironment")?;
        values.insert(key.clone(), json!(value));
    }
    if lines.next().is_some() {
        return Err("invalidEnvironment".into());
    }
    let result = json!({"environment":environment,"values":values});
    if serde_json::to_vec(&result).unwrap().len() > ENVIRONMENT_OUTPUT_BUDGET {
        return Err("outputLimit:环境探测结果输出超限".into());
    }
    Ok(result)
}
pub(crate) fn startup(started: bool, index: usize) -> Vec<u8> {
    let payload = if started {
        json!({"v":1,"started":true})
    } else {
        json!({"v":1,"started":false,"checkFailure":{"index":index,"message":"启动前身份检查失败"}})
    };
    let body = serde_json::to_vec(&payload).unwrap();
    let mut bytes = format!("SDP1 {}\n", body.len()).into_bytes();
    bytes.extend(body);
    bytes
}

/// Consumes exactly one negotiated startup frame, preserving subsequent native bytes.
pub(crate) struct Decoder {
    enabled: bool,
    done: bool,
    buffer: Vec<u8>,
    pending: Vec<u8>,
    budget: usize,
    pub started: Option<bool>,
    pub failure: Option<Value>,
}
impl Decoder {
    pub fn new(enabled: bool, budget: usize) -> Self {
        Self {
            enabled,
            done: !enabled,
            buffer: vec![],
            pending: vec![],
            budget: budget.min(64 * 1024),
            started: None,
            failure: None,
        }
    }
    pub fn ready(&self) -> bool {
        !self.enabled || self.started == Some(true)
    }
    pub fn metadata(&self) -> Value {
        if !self.enabled {
            json!({})
        } else {
            json!({"programStarted":self.started,"checkFailure":self.failure.as_ref().filter(|v|v.get("index").is_some()),"programError":self.failure.as_ref().filter(|v|v.get("index").is_none())})
        }
    }
    pub fn push(&mut self, stderr: bool, bytes: &[u8]) -> Result<Vec<(bool, Vec<u8>)>, String> {
        if self.done {
            if self.started == Some(false) && !bytes.is_empty() {
                return Err("unexpectedProgramOutput".into());
            }
            return Ok(vec![(stderr, bytes.to_vec())]);
        }
        if !stderr {
            if self.pending.len() + bytes.len() > self.budget {
                return Err("outputLimit:启动结果等待缓冲超限".into());
            }
            self.pending.extend(bytes);
            return Ok(vec![]);
        }
        self.buffer.extend(bytes);
        let Some(end) = self.buffer.iter().position(|b| *b == b'\n') else {
            if self.buffer.len() > 18 {
                return Err("invalidProgramStartup".into());
            }
            return Ok(vec![]);
        };
        let header =
            std::str::from_utf8(&self.buffer[..end]).map_err(|_| "invalidProgramStartup")?;
        let length = header
            .strip_prefix("SDP1 ")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0 && *n <= 1024)
            .ok_or("invalidProgramStartup")?;
        if self.buffer.len() < end + 1 + length {
            return Ok(vec![]);
        }
        let body: Value = serde_json::from_slice(&self.buffer[end + 1..end + 1 + length])
            .map_err(|_| "invalidProgramStartup")?;
        if body["v"] != 1 || !body["started"].is_boolean() {
            return Err("invalidProgramStartup".into());
        }
        self.started = body["started"].as_bool();
        if self.started == Some(false) {
            if body.get("checkFailure").is_some() {
                let index = body["checkFailure"]["index"]
                    .as_u64()
                    .filter(|n| (1..=2).contains(n))
                    .ok_or("invalidProgramStartup")?;
                let message = body["checkFailure"]["message"]
                    .as_str()
                    .filter(|s| s.len() <= 768)
                    .ok_or("invalidProgramStartup")?;
                self.failure = Some(json!({"index":index,"message":message}));
            } else {
                let message = body["error"]["message"]
                    .as_str()
                    .filter(|s| s.len() <= 768)
                    .ok_or("invalidProgramStartup")?;
                self.failure = Some(json!({"message":message}));
            }
        }
        self.done = true;
        let rest = self.buffer.split_off(end + 1 + length);
        self.buffer.clear();
        let pending = std::mem::take(&mut self.pending);
        if self.started == Some(false) && (!rest.is_empty() || !pending.is_empty()) {
            return Err("unexpectedProgramOutput".into());
        }
        Ok(vec![(false, pending), (true, rest)])
    }
    pub fn finish(&self) -> Result<(), String> {
        if !self.done {
            Err("missingProgramStartup".into())
        } else if self.started == Some(false) {
            if self
                .failure
                .as_ref()
                .is_some_and(|v| v.get("index").is_some())
            {
                Err("programCheckFailed:启动前身份检查失败".into())
            } else {
                Err(format!(
                    "programStartFailed:{}",
                    self.failure
                        .as_ref()
                        .and_then(|v| v["message"].as_str())
                        .unwrap_or("程序未启动")
                ))
            }
        } else {
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_json_escaping_obeys_the_output_budget() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let keys = vec!["DOCKER_HOST".into()];
        let budget = ENVIRONMENT_OUTPUT_BUDGET;
        let overhead = serde_json::to_vec(&json!({
            "environment":{"kind":"posix"},"values":{"DOCKER_HOST":""}
        }))
        .unwrap()
        .len();
        let mut value = "\u{1}".repeat((budget - overhead) / 6);
        value.push_str(&"x".repeat((budget - overhead) % 6));
        let probe = |value: &str| format!("SODECK_ENV_1\nposix\n{}\n", STANDARD.encode(value));
        let bytes = probe(&value);
        assert!(bytes.len() < budget);
        let result = parse_environment(bytes.as_bytes(), &keys, 0).unwrap();
        assert_eq!(serde_json::to_vec(&result).unwrap().len(), budget);
        value.push('x');
        let bytes = probe(&value);
        assert!(bytes.len() < budget);
        assert!(matches!(parse_environment(bytes.as_bytes(), &keys, 0),
            Err(error) if error.starts_with("outputLimit:")));
    }
    #[test]
    fn negotiated_frame_does_not_consume_program_text() {
        let frame = startup(true, 0);
        let mut d = Decoder::new(true, 4096);
        assert!(d.push(false, b"native\0\xff").unwrap().is_empty());
        let mut out = vec![];
        for byte in &frame {
            out.extend(d.push(true, &[*byte]).unwrap());
        }
        assert_eq!(out, vec![(false, b"native\0\xff".to_vec()), (true, vec![])]);
        assert_eq!(d.push(true, &frame).unwrap(), vec![(true, frame)]);
        assert!(d.finish().is_ok());
        let mut raw = Decoder::new(false, 1);
        assert_eq!(
            raw.push(true, b"SDP1 9\nanything").unwrap()[0].1,
            b"SDP1 9\nanything"
        );
    }
    #[test]
    fn failure_missing_frame_and_budget_are_distinct() {
        let mut d = Decoder::new(true, 1);
        assert!(d.push(false, b"xx").is_err());
        assert!(d.finish().is_err());
        let mut d = Decoder::new(true, 4096);
        d.push(true, &startup(false, 2)).unwrap();
        assert_eq!(d.started, Some(false));
        assert!(d.finish().unwrap_err().starts_with("programCheckFailed"));
    }
    #[test]
    fn environment_and_program_boundaries_are_explicit() {
        assert!(validate_keys(&vec!["PATH;exec".into()]).is_err());
        assert!(parse_environment(b"banner\nSODECK_ENV_1\nposix\n", &[], 0).is_err());
        assert!(parse_environment(b"SODECK_ENV_1\nposix\n\n", &["DOCKER_HOST".into()], 0).is_ok());
        let mut p = Program {
            file: "docker".into(),
            argv: vec!["a\0b".into()],
            remove_env: vec![],
            checks: vec![],
        };
        assert!(encode(&Environment::Posix, Mode::NonInteractive, &p).is_err());
        p.argv = vec!["a\nb".into()];
        assert!(encode(&Environment::Posix, Mode::NonInteractive, &p).is_ok());
        p.checks = vec![Check {
            argv: vec![],
            expected_stdout: "id\n".into(),
        }];
        assert!(encode(&Environment::Posix, Mode::Terminal, &p).is_err());
    }
}
