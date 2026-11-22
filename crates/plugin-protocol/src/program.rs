use super::Context;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Environment {
    Posix,
    Windows {
        launcher: String,
        #[serde(rename = "exitMode")]
        exit_mode: ExitMode,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExitMode {
    Native,
    Powershell,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Check {
    pub argv: Vec<String>,
    pub expected_stdout: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Program {
    pub file: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub remove_env: Vec<String>,
    #[serde(default)]
    pub checks: Vec<Check>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    NonInteractive,
    Terminal,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prepare {
    pub view_id: String,
    pub expected_context: Context,
    pub environment: Environment,
    pub mode: Mode,
    pub program: Program,
}
