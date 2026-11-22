//! Core requests. Plugin-owned payloads remain JSON; core-owned arguments do not.
use super::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn output_budget() -> usize {
    256 * 1024
}
fn page_limit() -> usize {
    20
}

macro_rules! params {
    ($name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
        #[serde(rename_all="camelCase",deny_unknown_fields)]
        pub struct $name { $($(#[$attr])* pub $field: $ty),* }
    };
}
params!(ViewContext {
    view_id: String,
    expected_context: Context
});
params!(FileOpen { view_id: String });
params!(Resource { id: String });
params!(Task { task_id: String });
params!(Process { process_id: String });
params!(ProcessRead {
    process_id: String,
    offset: String
});
params!(ExecPoll {
    task_id: String,
    stdout_offset: String,
    stderr_offset: String
});
params!(ExecStart { view_id:String, expected_context:Context, operation_id:String, sequence:String, command:String, timeout_ms:u64, #[serde(default="output_budget")] max_output_bytes:usize, #[serde(default,skip_serializing_if="Option::is_none")] program_output:Option<String> });
params!(EnvironmentStart { view_id:String, expected_context:Context, operation_id:String, sequence:String, timeout_ms:u64, #[serde(default)] keys:Vec<String> });
params!(TerminalOpen { view_id:String, cols:u32, rows:u32, #[serde(default,skip_serializing_if="Option::is_none")] command:Option<String> });
params!(Page { page_id: String });
params!(BatchBegin {
    id: String,
    batch_id: String,
    context: Context
});
params!(Batch {
    id: String,
    batch_id: String
});
params!(InputClaim {
    id: String,
    input_id: String,
    #[serde(default)]
    renew: bool
});
params!(InputList {
    id: String,
    input_id: String,
    offset: usize
});
params!(InputRelease {
    id: String,
    input_id: String
});
params!(ListenerIntent {
    view_id: String,
    expected_context: Context,
    operation_id: String,
    sequence: String
});
params!(ListenerCreate {
    view_id: String,
    expected_context: Context,
    operation_id: String,
    sequence: String,
    bind: Bind
});
params!(Bind {
    host: String,
    port: u16
});
params!(Listener {
    listener_id: String
});
params!(ListenerProbe {
    listener_id: String,
    operation_id: String
});
params!(NetPoll { view_id: String });
params!(TcpOpen {
    listener_id: String,
    client_stream_id: String,
    operation_id: String,
    host: String,
    port: u16
});
params!(Stream { stream_id: String });
params!(StreamRead {
    stream_id: String,
    max_bytes: usize
});
params!(StreamWrite { stream_id:String, bytes:Vec<u8> });
params!(StreamSplice {
    listener_id: String,
    left_id: String,
    right_id: String,
    operation_id: String
});
params!(ConnectionOpen { request_id:String, server_id:String, expected_profile_token:String, #[serde(default,skip_serializing_if="Option::is_none")] view_type:Option<String> });
params!(ConnectionGet { request_id:String, #[serde(default,skip_serializing_if="Option::is_none")] view_type:Option<String> });
params!(ConnectionCancel { request_id: String });
params!(ServerList { #[serde(default,skip_serializing_if="Option::is_none")] server_id:Option<String>, #[serde(default)] detail:bool, #[serde(default="page_limit")] limit:usize, #[serde(default,skip_serializing_if="Option::is_none")] cursor:Option<String>, #[serde(default,skip_serializing_if="Option::is_none")] view_type:Option<String> });
params!(ServerDelete {
    catalog_revision: String,
    server_id: String
});
params!(Log { level:String, message:String, #[serde(default,skip_serializing_if="Option::is_none")] fields:Option<Value> });
params!(Message { title:String, body:String, level:String, #[serde(default)] notify:bool, #[serde(default,skip_serializing_if="Option::is_none")] dedupe_key:Option<String> });

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProcessInput {
    #[serde(rename_all = "camelCase")]
    Write {
        process_id: String,
        input_sequence: String,
        data_base64: String,
    },
    #[serde(rename_all = "camelCase")]
    Eof {
        process_id: String,
        input_sequence: String,
    },
    #[serde(rename_all = "camelCase")]
    Resize {
        process_id: String,
        input_sequence: String,
        cols: u32,
        rows: u32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "side", rename_all = "camelCase", deny_unknown_fields)]
pub enum Endpoint {
    Remote {
        path: String,
    },
    #[serde(rename_all = "camelCase")]
    Local {
        input_id: String,
        root_id: String,
        relative: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root_name: Option<String>,
    },
    LocalPath {
        path: String,
    },
    PluginData {
        relative: Vec<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Guard {
    Absent,
    Match { stamp: String },
    Capture,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FileOperation {
    Inspect {
        endpoint: Endpoint,
    },
    List {
        endpoint: Endpoint,
    },
    Next {
        cursor: String,
    },
    CloseCursor {
        cursor: String,
    },
    #[serde(rename_all = "camelCase")]
    CopyFile {
        source: Endpoint,
        destination: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_stamp: Option<String>,
        destination_guard: Guard,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata_policy: Option<String>,
    },
    Mkdir {
        endpoint: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guard: Option<Guard>,
    },
    CreateEmpty {
        endpoint: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guard: Option<Guard>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveFile {
        endpoint: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_stamp: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guard: Option<Guard>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveDirectory {
        endpoint: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_stamp: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guard: Option<Guard>,
    },
    #[serde(rename_all = "camelCase")]
    RenameNoReplace {
        endpoint: Endpoint,
        destination: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_stamp: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guard: Option<Guard>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum FileRequest {
    #[serde(rename_all = "camelCase")]
    Start {
        id: String,
        request_id: String,
        sequence: String,
        expected_context: Context,
        batch_id: String,
        operation: FileOperation,
    },
    #[serde(rename_all = "camelCase")]
    Lookup {
        id: String,
        request_id: String,
        batch_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Poll { id: String, operation_id: String },
    #[serde(rename_all = "camelCase")]
    Cancel { id: String, operation_id: String },
    #[serde(rename_all = "camelCase")]
    Release { id: String, operation_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftRef {
    pub document_id: String,
    pub revision: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum EditorRequest {
    #[serde(rename = "draft.describe")]
    Describe { id: String },
    #[serde(rename = "save.start", rename_all = "camelCase")]
    Save {
        id: String,
        request_id: String,
        sequence: String,
        expected_context: Context,
        expected_draft_ref: DraftRef,
    },
    #[serde(rename = "save.status", rename_all = "camelCase")]
    SaveStatus {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    #[serde(rename = "save.cancel", rename_all = "camelCase")]
    SaveCancel {
        id: String,
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_context: Option<Context>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_draft_ref: Option<DraftRef>,
    },
    #[serde(rename = "save.release", rename_all = "camelCase")]
    SaveRelease { id: String, request_id: String },
    #[serde(rename = "export.status", rename_all = "camelCase")]
    ExportStatus { id: String, request_id: String },
    #[serde(rename = "export.cancel", rename_all = "camelCase")]
    ExportCancel {
        id: String,
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_draft_ref: Option<DraftRef>,
    },
    #[serde(rename = "export.release", rename_all = "camelCase")]
    ExportRelease { id: String, request_id: String },
}

params!(CommandQuery {
    expected_context: Context,
    source_view_id: String,
    request_id: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    command_id: Option<String>
});
params!(CommandResolve {
    expected_context: Context,
    source_view_id: String,
    name: String
});
params!(Command {
    expected_context: Context,
    source_view_id: String,
    request_id: String,
    name: String,
    payload: Value
});
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenView {
    pub expected_context: Context,
    pub request_id: String,
    pub view_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_view_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
params!(UpdateView {
    view_id: String,
    metadata: ViewMetadata
});
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<bool>,
}

// The data fields in these records are plugin-defined state, rather than core arguments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateRequest {
    pub view_id: String,
    pub expected_context: Context,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    #[serde(
        default,
        deserialize_with = "present_json",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_subscription_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_revision: Option<String>,
}
fn present_json<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

macro_rules! requests {
    ($($variant:ident($params:ty) => $method:literal, $reply:ty),* $(,)?) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(tag="method",content="params")]
        pub enum Request { $(#[serde(rename=$method)] $variant($params)),* }
        #[derive(Deserialize)]
        enum Method { $(#[serde(rename=$method)] $variant),* }
        impl Request {
            pub fn decode(method: &str, params: Value) -> Result<Self, super::ApiError> {
                let _: Method = serde_json::from_value(Value::String(method.into()))
                    .map_err(|_|super::ApiError::method_not_found())?;
                serde_json::from_value(serde_json::json!({"method":method,"params":params}))
                    .map_err(|_|super::ApiError::invalid("核心请求参数无效"))
            }
        }
        pub mod call {
            use super::*;
            $(
                #[derive(Clone, Debug, Serialize, Deserialize)]
                #[serde(transparent)]
                pub struct $variant(pub $params);
                impl Call for $variant {
                    type Reply = $reply;
                    fn into_request(self) -> Request { Request::$variant(self.0) }
                    fn decode_reply(value: Value) -> Result<Self::Reply, super::super::ApiError> { crate::replies::decode(value) }
                }
            )*
        }
    };
}

pub trait Call {
    type Reply;
    fn into_request(self) -> Request;
    fn decode_reply(value: Value) -> Result<Self::Reply, super::ApiError>;
}
requests! {
    Target(crate::requests::ViewContext) => "session.target.describe", crate::replies::TargetFacts,
    Program(crate::program::Prepare) => "session.program.prepare", crate::replies::Program,
    ExecStart(crate::requests::ExecStart) => "session.exec.start", crate::replies::ExecReceipt,
    ExecPoll(crate::requests::ExecPoll) => "session.exec.poll", crate::replies::ExecPoll,
    ExecCancel(crate::requests::Task) => "session.exec.cancel", serde_json::Value,
    ExecRelease(crate::requests::Task) => "session.exec.release", serde_json::Value,
    Environment(crate::requests::EnvironmentStart) => "session.environment.start", crate::replies::ExecReceipt,
    ProcessStart(crate::session::ProcessStart) => "session.process.start", crate::replies::ProcessReceipt,
    ProcessLookup(crate::session::ProcessStart) => "session.process.lookup", crate::replies::ProcessLookup,
    ProcessRead(crate::requests::ProcessRead) => "session.process.read", crate::replies::ProcessRead,
    ProcessStatus(crate::requests::Process) => "session.process.status", crate::replies::ProcessStatus,
    ProcessInput(crate::requests::ProcessInput) => "session.process.input", crate::replies::ProcessStatus,
    ProcessCancel(crate::requests::Process) => "session.process.cancel", crate::replies::ProcessStatus,
    ProcessRelease(crate::requests::Process) => "session.process.release", crate::replies::ProcessRelease,
    TerminalOpen(crate::requests::TerminalOpen) => "terminal.open", crate::replies::Resource,
    TerminalClose(crate::requests::Resource) => "terminal.close", serde_json::Value,
    FileOpen(crate::requests::FileOpen) => "files.open", crate::replies::Resource,
    FileClose(crate::requests::Resource) => "files.close", serde_json::Value,
    BatchBegin(crate::requests::BatchBegin) => "files.batch.begin", serde_json::Value,
    BatchCancel(crate::requests::Batch) => "files.batch.cancel", serde_json::Value,
    BatchEnd(crate::requests::Batch) => "files.batch.end", serde_json::Value,
    InputClaim(crate::requests::InputClaim) => "files.input.claim", serde_json::Value,
    InputList(crate::requests::InputList) => "files.input.list", serde_json::Value,
    InputRelease(crate::requests::InputRelease) => "files.input.release", serde_json::Value,
    FileOperation(crate::requests::FileRequest) => "files.operations", serde_json::Value,
    FileEditor(crate::requests::EditorRequest) => "files.editor", serde_json::Value,
    ListenerCreate(crate::requests::ListenerCreate) => "net.listener.create", serde_json::Value,
    ListenerLookup(crate::requests::ListenerIntent) => "net.listener.lookup", serde_json::Value,
    ListenerCancel(crate::requests::ListenerIntent) => "net.listener.cancel", serde_json::Value,
    ListenerRelease(crate::requests::ListenerIntent) => "net.listener.release", serde_json::Value,
    ListenerSnapshot(crate::requests::Listener) => "net.listener.snapshot", serde_json::Value,
    ListenerProbe(crate::requests::ListenerProbe) => "net.listener.probe", serde_json::Value,
    NetPoll(crate::requests::NetPoll) => "net.poll", serde_json::Value,
    TcpOpen(crate::requests::TcpOpen) => "net.tcp.open", serde_json::Value,
    StreamClose(crate::requests::Resource) => "net.stream.close", serde_json::Value,
    StreamStatus(crate::requests::Stream) => "net.stream.status", serde_json::Value,
    StreamRead(crate::requests::StreamRead) => "net.stream.read", serde_json::Value,
    StreamWrite(crate::requests::StreamWrite) => "net.stream.write", serde_json::Value,
    StreamFinish(crate::requests::Stream) => "net.stream.finish", serde_json::Value,
    StreamSplice(crate::requests::StreamSplice) => "net.stream.splice", serde_json::Value,
    Settings(crate::requests::Page) => "settings.get", serde_json::Value,
    OpenView(crate::requests::OpenView) => "ui.openView", crate::replies::OpenView,
    UpdateView(crate::requests::UpdateView) => "ui.updateView", serde_json::Value,
    CommandResolve(crate::requests::CommandResolve) => "commands.resolve", serde_json::Value,
    CommandSubmit(crate::requests::Command) => "commands.submit", serde_json::Value,
    CommandGet(crate::requests::CommandQuery) => "commands.get", serde_json::Value,
    StateGet(crate::requests::StateRequest) => "state.get", serde_json::Value,
    StatePut(crate::requests::StateRequest) => "state.put", serde_json::Value,
    StateRemove(crate::requests::StateRequest) => "state.remove", serde_json::Value,
    StateList(crate::requests::StateRequest) => "state.list", serde_json::Value,
    StateSubscribe(crate::requests::StateRequest) => "state.subscribe", serde_json::Value,
    StateUnsubscribe(crate::requests::StateRequest) => "state.unsubscribe", serde_json::Value,
    ConnectionOpen(crate::requests::ConnectionOpen) => "connections.open", serde_json::Value,
    ConnectionGet(crate::requests::ConnectionGet) => "connections.get", serde_json::Value,
    ConnectionCancel(crate::requests::ConnectionCancel) => "connections.cancel", serde_json::Value,
    ServerCreate(crate::servers::Create) => "servers.create", serde_json::Value,
    ServerUpdate(crate::servers::Update) => "servers.update", serde_json::Value,
    ServerList(crate::requests::ServerList) => "servers.list", serde_json::Value,
    ServerDelete(crate::requests::ServerDelete) => "servers.delete", serde_json::Value,
    Log(crate::requests::Log) => "logging.write", serde_json::Value,
    Message(crate::requests::Message) => "messages.publish", serde_json::Value,
}
