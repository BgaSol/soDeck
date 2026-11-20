//! Thin remote program requests; platform encoding and task ownership stay in core.
use crate::{Client, RpcError};
use serde_json::{json, Value};
#[derive(Clone, Debug)]
pub struct PreparedProgram {
    command: String,
    mode: String,
    output: Option<String>,
}
impl PreparedProgram {
    pub fn prepare(
        client: &Client,
        view: &str,
        context: &Value,
        environment: &Value,
        program: Value,
        terminal: bool,
    ) -> Result<Self, RpcError> {
        let value = client.call_detailed(
            "session.program.prepare",
            json!({"viewId":view,"expectedContext":context,"environment":environment,
            "mode":if terminal{"terminal"}else{"nonInteractive"},"program":program}),
        )?;
        let command = value["command"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.contains('\0'))
            .ok_or_else(|| RpcError::from("核心程序命令无效".to_string()))?
            .into();
        let mode = value["mode"]
            .as_str()
            .filter(|s| {
                *s == if terminal {
                    "terminal"
                } else {
                    "nonInteractive"
                }
            })
            .ok_or_else(|| RpcError::from("核心程序模式不匹配".to_string()))?
            .into();
        let output = value["programOutput"].as_str().map(str::to_string);
        if (!terminal && output.as_deref() != Some("startup-v1")) || (terminal && output.is_some())
        {
            return Err(RpcError::from("核心程序输出协议无效".to_string()));
        }
        Ok(Self {
            command,
            mode,
            output,
        })
    }
    pub fn apply(&self, params: &mut Value) -> Result<(), String> {
        if self.mode != "nonInteractive" {
            return Err("终端程序不能用于非交互命令".into());
        }
        let p = params.as_object_mut().ok_or("执行参数不是对象")?;
        p.insert("command".into(), json!(self.command));
        p.insert("programOutput".into(), json!(self.output));
        Ok(())
    }
    pub fn terminal_command(&self) -> Result<&str, String> {
        if self.mode == "terminal" {
            Ok(&self.command)
        } else {
            Err("非交互程序不能用于终端".into())
        }
    }
}
pub fn environment_start(
    client: &Client,
    view: &str,
    context: &Value,
    operation: &str,
    sequence: u64,
    timeout: u64,
    keys: &[&str],
) -> Result<Value, RpcError> {
    client.call_detailed("session.environment.start",json!({"viewId":view,"expectedContext":context,"operationId":operation,"sequence":sequence.to_string(),"timeoutMs":timeout,"keys":keys}))
}
