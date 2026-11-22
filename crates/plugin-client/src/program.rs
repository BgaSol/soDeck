//! Thin remote program requests; platform encoding and task ownership stay in core.
use crate::protocol::requests as api;
use crate::{Client, RpcError};
use serde_json::{json, Value};
#[derive(Clone, Debug)]
pub struct PreparedProgram {
    command: String,
    mode: crate::protocol::program::Mode,
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
        use crate::protocol::program::{Mode, Prepare};
        let value = client.request(api::call::Program(Prepare {
            view_id: view.into(),
            expected_context: serde_json::from_value(context.clone())
                .map_err(|_| RpcError::invalid("程序上下文无效"))?,
            environment: serde_json::from_value(environment.clone())
                .map_err(|_| RpcError::invalid("程序环境无效"))?,
            mode: if terminal {
                Mode::Terminal
            } else {
                Mode::NonInteractive
            },
            program: serde_json::from_value(program)
                .map_err(|_| RpcError::invalid("程序参数无效"))?,
        }))?;
        let command = value.command;
        if command.is_empty() || command.contains('\0') {
            return Err(RpcError::local("核心程序命令无效"));
        }
        let mode = value.mode;
        if (mode == crate::protocol::program::Mode::Terminal) != terminal {
            return Err(RpcError::local("核心程序模式不匹配"));
        }
        let output = value.program_output;
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
        if self.mode != crate::protocol::program::Mode::NonInteractive {
            return Err("终端程序不能用于非交互命令".into());
        }
        let p = params.as_object_mut().ok_or("执行参数不是对象")?;
        p.insert("command".into(), json!(self.command));
        p.insert("programOutput".into(), json!(self.output));
        Ok(())
    }
    pub fn terminal_command(&self) -> Result<&str, String> {
        if self.mode == crate::protocol::program::Mode::Terminal {
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
) -> Result<crate::protocol::replies::ExecReceipt, RpcError> {
    client.request(api::call::Environment(api::EnvironmentStart {
        view_id: view.into(),
        expected_context: serde_json::from_value(context.clone())
            .map_err(|_| RpcError::invalid("环境探测上下文无效"))?,
        operation_id: operation.into(),
        sequence: sequence.to_string(),
        timeout_ms: timeout,
        keys: keys.iter().map(|key| (*key).into()).collect(),
    }))
}
