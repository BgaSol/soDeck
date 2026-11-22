//! A ticket identifies one intent. Timeouts never generate a replacement request ID.
use crate::protocol::requests as api;
use crate::{Client, RpcError};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Ticket {
    pub context: Value,
    pub source_view_id: String,
    pub request_id: String,
    pub name: String,
    pub payload: Value,
}
impl Ticket {
    pub fn new(
        context: Value,
        view: impl Into<String>,
        request: impl Into<String>,
        name: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            context,
            source_view_id: view.into(),
            request_id: request.into(),
            name: name.into(),
            payload,
        }
    }
    fn params(&self) -> Result<api::Command, RpcError> {
        Ok(api::Command {
            expected_context: serde_json::from_value(self.context.clone())
                .map_err(|_| RpcError::invalid("命令上下文无效"))?,
            source_view_id: self.source_view_id.clone(),
            request_id: self.request_id.clone(),
            name: self.name.clone(),
            payload: self.payload.clone(),
        })
    }
    pub fn submit(&self, client: &Client) -> Result<Value, RpcError> {
        client.request(api::call::CommandSubmit(self.params()?))
    }
    pub fn get(&self, client: &Client) -> Result<Value, RpcError> {
        client.request(api::call::CommandGet(api::CommandQuery {
            expected_context: self.params()?.expected_context,
            source_view_id: self.source_view_id.clone(),
            request_id: self.request_id.clone(),
            command_id: None,
        }))
    }
    pub fn resolve(&self, client: &Client) -> Result<Value, RpcError> {
        client.request(api::call::CommandResolve(api::CommandResolve {
            expected_context: self.params()?.expected_context,
            source_view_id: self.source_view_id.clone(),
            name: self.name.clone(),
        }))
    }
}
