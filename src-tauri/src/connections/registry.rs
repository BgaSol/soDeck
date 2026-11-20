use super::{Error, View};
use crate::workspaces::Context;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct RequestKey {
    pub plugin: String,
    pub instance: String,
    pub request: String,
}

#[derive(Clone, PartialEq)]
pub(super) struct OpenInput {
    pub server: String,
    pub profile: String,
}

pub(super) struct OpenRequest {
    pub input: Option<OpenInput>,
    pub binding: Binding,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(super) enum Binding {
    #[serde(rename_all = "camelCase")]
    Attached {
        attempt_id: String,
    },
    Accepted {
        context: Context,
    },
    Failed {
        error: Error,
    },
    Cancelled,
}

#[derive(Default)]
pub(crate) struct Registry {
    pub(crate) current: HashMap<String, Arc<super::Attempt>>,
    pub(super) opens: HashMap<RequestKey, OpenRequest>,
}

impl Registry {
    pub(super) fn freeze(&mut self, view: &View) {
        for request in self.opens.values_mut() {
            if !matches!(&request.binding, Binding::Attached { attempt_id } if attempt_id == &view.attempt_id)
            {
                continue;
            }
            request.binding = match (&view.state[..], &view.context) {
                ("connected", Some(context)) => Binding::Accepted {
                    context: context.clone(),
                },
                ("cancelled", _) => Binding::Cancelled,
                ("failed" | "disconnected", _) => Binding::Failed {
                    error: view
                        .error
                        .clone()
                        .unwrap_or_else(|| Error::new("connectionLost", "连接已经结束")),
                },
                _ => continue,
            };
        }
    }

    pub(super) fn has_external(&self, attempt: &str) -> bool {
        self.opens.values().any(|request| matches!(&request.binding, Binding::Attached { attempt_id } if attempt_id == attempt))
    }

    pub(super) fn check_capacity(&self, key: &RequestKey) -> Result<(), Error> {
        if self
            .opens
            .keys()
            .filter(|other| other.plugin == key.plugin && other.instance == key.instance)
            .count()
            >= 4096
        {
            Err(Error::new("resourceBusy", "连接请求记录已满"))
        } else {
            Ok(())
        }
    }
}
