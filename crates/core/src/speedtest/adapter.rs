use base64::{engine::general_purpose::STANDARD, Engine};
use crate::{CoreError, Result};

pub struct Agent {
    pub command: String,
    pub bootstrap: &'static [u8],
    pub method: &'static str,
}

/// Only fixed, compiled templates enter shell command text.
pub fn agent(platform: &str, powershell7: bool) -> Result<Agent> {
    match platform {
        "linux" => {
            let source = include_str!("agents/memory.py");
            Ok(Agent { command: format!("python3 -I -u -c '{}'", source.replace('\'', "'\\''")),
                bootstrap: &[], method: "python-memory-v1" })
        }
        "windows" => {
            let source = include_bytes!("agents/memory.ps1");
            if source.len() > 16 * 1024 { return Err(CoreError::new("CAPACITY", "测速模板超过大小限制")); }
            let loader = include_str!("agents/bootstrap.ps1").replace("__SOURCE_BYTES__", &source.len().to_string());
            let encoded: Vec<u8> = loader.encode_utf16().flat_map(u16::to_le_bytes).collect();
            Ok(Agent { command: format!("{} -NoProfile -NonInteractive -EncodedCommand {}",
                if powershell7 { "pwsh.exe" } else { "powershell.exe" }, STANDARD.encode(encoded)),
                bootstrap: source, method: "powershell-memory-v1" })
        }
        _ => Err(CoreError::new("UNSUPPORTED", "该服务器没有受支持的内存测速方法")),
    }
}
