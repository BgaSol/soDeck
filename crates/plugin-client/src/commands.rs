//! A ticket identifies one intent. Timeouts never generate a replacement request ID.
use crate::{Client, RpcError};
use serde_json::{json, Value};

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
    fn params(&self) -> Value {
        json!({"expectedContext":self.context,"sourceViewId":self.source_view_id,"requestId":self.request_id,"name":self.name,"payload":self.payload})
    }
    pub fn submit(&self, client: &Client) -> Result<Value, RpcError> {
        client.call_detailed("commands.submit", self.params())
    }
    pub fn get(&self, client: &Client) -> Result<Value, RpcError> {
        client.call_detailed("commands.get", self.params())
    }
    pub fn resolve(&self, client: &Client) -> Result<Value, RpcError> {
        client.call_detailed("commands.resolve", self.params())
    }
}
