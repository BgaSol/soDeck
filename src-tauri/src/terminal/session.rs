use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Launch {
    LoginShell {},
    Command { command: String },
}
impl Launch {
    pub(super) fn validate(&self) -> Result<(), String> {
        if let Self::Command { command } = self {
            if command.is_empty() || command.len() > 32 * 1024 || command.contains('\0') {
                return Err("终端启动命令无效或超过 32 KiB".into());
            }
        }
        Ok(())
    }
}

pub(super) struct Runtime {
    revision: u64,
    state: &'static str,
    start_request: &'static str,
    outcome: Option<Value>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            revision: 1,
            state: "opening",
            start_request: "notSent",
            outcome: None,
        }
    }
}
impl Runtime {
    pub(super) fn update(
        &mut self,
        state: &'static str,
        request: Option<&'static str>,
        outcome: Option<Value>,
    ) {
        if self.state == "closed" {
            return;
        }
        let state = if self.state == "closing" && state != "closed" {
            "closing"
        } else {
            state
        };
        let request = request.unwrap_or(self.start_request);
        if (self.state, self.start_request, &self.outcome) == (state, request, &outcome) {
            return;
        }
        self.state = state;
        self.start_request = request;
        self.outcome = outcome;
        self.revision += 1;
    }
    pub(super) fn wire(&self) -> Value {
        json!({"state":self.state,"startRequest":self.start_request,"revision":self.revision.to_string(),"outcome":self.outcome})
    }
    pub(super) fn request(&self) -> &'static str {
        self.start_request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_launch_never_falls_back_or_accepts_ambiguous_parameters() {
        assert!(serde_json::from_value::<Launch>(json!({"kind":"command"})).is_err());
        assert!(
            serde_json::from_value::<Launch>(json!({"kind":"loginShell","command":"bad"})).is_err()
        );
        for command in ["", "nul\0command"] {
            assert!(Launch::Command {
                command: command.into()
            }
            .validate()
            .is_err());
        }
        assert!(Launch::Command {
            command: "exit 42".into()
        }
        .validate()
        .is_ok());
    }
    #[test]
    fn late_start_confirmation_cannot_reopen_a_closing_or_closed_session() {
        let mut state = Runtime::default();
        state.update("closing", None, None);
        state.update("running", Some("accepted"), None);
        assert_eq!(state.wire()["state"], "closing");
        assert_eq!(state.wire()["startRequest"], "accepted");
        state.update("closed", None, Some(json!({"remoteProcess":"unknown"})));
        let end = state.wire();
        state.update("running", Some("sent"), None);
        assert_eq!(state.wire(), end);
    }
}
