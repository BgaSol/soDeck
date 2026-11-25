//! MCP input shapes. Resource state, byte budgets and operation identity stay in handlers.
#![allow(dead_code)]
use schemars::{generate::SchemaSettings, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

macro_rules! text {
    ($name:ident, $min:literal, $max:literal) => {
        #[derive(Deserialize, JsonSchema)]
        #[serde(transparent)]
        struct $name(#[schemars(length(min = $min, max = $max))] String);
    };
}
text!(Id, 1, 256);
text!(RequestId, 1, 128);
text!(Path, 1, 4096);
text!(Cursor, 1, 2048);
text!(CommandText, 1, 16384);

#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
struct Decimal(#[schemars(length(min = 1, max = 20), regex(pattern = "^[0-9]+$"))] String);

fn non_null<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    T::json_schema(generator)
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum MatchMode {
    #[default]
    Exact,
    Contains,
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ExecMode {
    #[default]
    Batch,
    Pipe,
    Pty,
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum OutputMode {
    Full,
    #[default]
    Text,
    None,
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum Status {
    #[default]
    All,
    Active,
    Done,
}
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum Kind {
    #[default]
    All,
    Exec,
    Download,
    Upload,
}
fn limit() -> u32 {
    20
}
fn wait() -> u32 {
    1000
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Servers {
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    query: Option<Id>,
    #[serde(rename = "match", default)]
    mode: MatchMode,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    server_id: Option<Id>,
    #[serde(default)]
    detail: bool,
    #[serde(default = "limit")]
    #[schemars(range(min = 1, max = 50))]
    limit: u32,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Cursor>")]
    cursor: Option<Cursor>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Targets {
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    query: Option<Id>,
    #[serde(rename = "match", default)]
    mode: MatchMode,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    server_id: Option<Id>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    workspace_id: Option<Id>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    target_id: Option<Id>,
    #[serde(default)]
    detail: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Tasks {
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    target_id: Option<Id>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Id>")]
    server_id: Option<Id>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<RequestId>")]
    request_id: Option<RequestId>,
    #[serde(default)]
    status: Status,
    #[serde(default)]
    kind: Kind,
    #[serde(default = "limit")]
    #[schemars(range(min = 1, max = 50))]
    limit: u32,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Cursor>")]
    cursor: Option<Cursor>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Open {
    server_id: Id,
    expected_profile_token: Id,
    request_id: RequestId,
    #[serde(default = "wait")]
    #[schemars(range(min = 0, max = 20000))]
    wait_ms: u32,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Connection {
    connection_request_id: Id,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectionGet {
    connection_request_id: Id,
    #[serde(default)]
    detail: bool,
    #[serde(default)]
    #[schemars(range(min = 0, max = 20000))]
    wait_ms: u32,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Decimal>")]
    since_revision: Option<Decimal>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Exec {
    target_id: Id,
    request_id: RequestId,
    command: CommandText,
    #[serde(default)]
    mode: ExecMode,
    #[schemars(range(min = 1, max = 3600))]
    timeout_seconds: Option<u32>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Cols>")]
    cols: Option<Cols>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Rows>")]
    rows: Option<Rows>,
    #[serde(default = "wait")]
    #[schemars(range(min = 0, max = 20000))]
    wait_ms: u32,
}
#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
struct Cols(#[schemars(range(min = 2, max = 1000))] u32);
#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
struct Rows(#[schemars(range(min = 1, max = 500))] u32);
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Task {
    task_id: Id,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskGet {
    task_id: Id,
    #[serde(default)]
    detail: bool,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Decimal>")]
    cursor: Option<Decimal>,
    #[serde(default)]
    output_mode: OutputMode,
    #[serde(default)]
    #[schemars(range(min = 0, max = 20000))]
    wait_ms: u32,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Decimal>")]
    since_revision: Option<Decimal>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    task_id: Id,
    sequence: Decimal,
    action: InputAction,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<String>", length(max = 16384))]
    text: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<String>", length(max = 24576))]
    data_base64: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Cols>")]
    cols: Option<Cols>,
    #[serde(default)]
    #[schemars(schema_with = "non_null::<Rows>")]
    rows: Option<Rows>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum InputAction {
    Write,
    Eof,
    Resize,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Download {
    target_id: Id,
    request_id: RequestId,
    remote_path: Path,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Upload {
    target_id: Id,
    request_id: RequestId,
    local_path: Path,
    remote_path: Path,
    #[serde(default)]
    overwrite: bool,
}

fn generated<T: JsonSchema>() -> Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    settings.meta_schema = None;
    let mut schema =
        serde_json::to_value(settings.into_generator().into_root_schema_for::<T>()).unwrap();
    schema.as_object_mut().unwrap().remove("title");
    for property in schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|p| p.values_mut())
    {
        if property["default"].is_null() {
            property.as_object_mut().unwrap().remove("default");
        }
    }
    schema
}
pub fn input(name: &str) -> Value {
    match name {
        "sodeck_servers_list" => generated::<Servers>(),
        "sodeck_targets_list" => generated::<Targets>(),
        "sodeck_tasks_list" => generated::<Tasks>(),
        "sodeck_connection_open" => generated::<Open>(),
        "sodeck_connection_get" => generated::<ConnectionGet>(),
        "sodeck_connection_cancel" => generated::<Connection>(),
        "sodeck_exec_start" => generated::<Exec>(),
        "sodeck_task_get" => generated::<TaskGet>(),
        "sodeck_task_input" => generated::<Input>(),
        "sodeck_task_cancel" => generated::<Task>(),
        "sodeck_file_download" => generated::<Download>(),
        "sodeck_file_upload" => generated::<Upload>(),
        name => {
            // The core's patch distinguishes omitted fields from explicit null.
            let id = generated::<Id>();
            let revision = generated::<Decimal>();
            let (properties, required) = match name {
                "sodeck_server_create" => {
                    let mut node = patch();
                    node["required"] = json!(["host", "username"]);
                    (
                        json!({"catalogRevision":revision,"server":node}),
                        json!(["catalogRevision", "server"]),
                    )
                }
                "sodeck_server_update" => (
                    json!({"catalogRevision":revision,"serverId":id,"patch":patch()}),
                    json!(["catalogRevision", "serverId", "patch"]),
                ),
                "sodeck_server_delete" => (
                    json!({"catalogRevision":revision,"serverId":id}),
                    json!(["catalogRevision", "serverId"]),
                ),
                _ => panic!("missing MCP input schema: {name}"),
            };
            json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
        }
    }
}
fn patch() -> Value {
    let id = generated::<Id>();
    let route = json!({"type":"array","items":{"oneOf":[
        {"type":"object","properties":{"kind":{"const":"ssh"},"serverId":id},"required":["kind","serverId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"socks5"},"proxyId":id},"required":["kind","proxyId"],"additionalProperties":false}
    ]}});
    let secret = json!({"oneOf":[
        {"type":"object","properties":{"action":{"const":"keep"},"remember":{"type":"boolean"}},"required":["action","remember"],"additionalProperties":false},
        {"type":"object","properties":{"action":{"const":"replace"},"value":{"type":"string"},"remember":{"type":"boolean"}},"required":["action","value","remember"],"additionalProperties":false},
        {"type":"object","properties":{"action":{"const":"clear"}},"required":["action"],"additionalProperties":false}
    ]});
    json!({"type":"object","minProperties":1,"properties":{
        "name":{"type":"string","maxLength":256},"host":id,"port":{"type":"integer","minimum":1,"maximum":65535},"username":id,
        "tagIds":{"type":"array","items":id},"route":route,
        "authentication":{"type":"object","properties":{"type":{"type":"string","enum":["password","privateKey"]},"privateKeyPath":generated::<Path>(),"secret":secret},"required":["type"],"additionalProperties":false}
    },"additionalProperties":false})
}

pub fn validate(name: &str, params: &Value) -> Result<(), Value> {
    static VALIDATORS: std::sync::OnceLock<
        std::collections::HashMap<String, jsonschema::Validator>,
    > = std::sync::OnceLock::new();
    let validators = VALIDATORS.get_or_init(|| {
        crate::mcp::tools()
            .into_iter()
            .map(|tool| {
                let schema = Value::Object(tool.input_schema.as_ref().clone());
                let validator = jsonschema::validator_for(&schema).expect("built-in MCP schema");
                (tool.name.to_string(), validator)
            })
            .collect()
    });
    let validator = validators.get(name).expect("registered MCP tool");
    validator.validate(params).map_err(|error| {
        let field = error.instance_path().to_string();
        let code = if field == "/cursor" {
            "invalidCursor"
        } else if name == "sodeck_task_input" && field == "/sequence" {
            "invalidSequence"
        } else {
            "invalidParams"
        };
        crate::tasks::failure(code, "工具参数类型、范围或字段无效")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid(name: &str, value: Value) {
        assert!(validate(name, &value).is_ok(), "{name}: invalid shape");
    }
    fn invalid(name: &str, value: Value) {
        assert!(validate(name, &value).is_err(), "{name}: {value}");
    }
    #[test]
    fn every_tool_has_a_compiled_schema_and_required_fields() {
        let examples = [
            ("sodeck_servers_list", json!({})),
            ("sodeck_targets_list", json!({})),
            ("sodeck_tasks_list", json!({})),
            (
                "sodeck_server_create",
                json!({"catalogRevision":"1","server":{"host":"host","username":"user"}}),
            ),
            (
                "sodeck_server_update",
                json!({"catalogRevision":"1","serverId":"id","patch":{"host":"other"}}),
            ),
            (
                "sodeck_server_delete",
                json!({"catalogRevision":"1","serverId":"id"}),
            ),
            (
                "sodeck_connection_open",
                json!({"serverId":"id","expectedProfileToken":"token","requestId":"request"}),
            ),
            ("sodeck_connection_get", json!({"connectionRequestId":"id"})),
            (
                "sodeck_connection_cancel",
                json!({"connectionRequestId":"id"}),
            ),
            (
                "sodeck_exec_start",
                json!({"targetId":"id","requestId":"request","command":"true"}),
            ),
            ("sodeck_task_get", json!({"taskId":"id"})),
            (
                "sodeck_task_input",
                json!({"taskId":"id","sequence":"1","action":"eof"}),
            ),
            ("sodeck_task_cancel", json!({"taskId":"id"})),
            (
                "sodeck_file_download",
                json!({"targetId":"id","requestId":"r","remotePath":"/tmp/file"}),
            ),
            (
                "sodeck_file_upload",
                json!({"targetId":"id","requestId":"r","localPath":"/tmp/local","remotePath":"/tmp/file"}),
            ),
        ];
        assert_eq!(examples.len(), crate::mcp::tools().len());
        for (name, value) in examples {
            valid(name, value.clone());
            let mut unknown = value.clone();
            unknown["unexpected"] = json!(true);
            invalid(name, unknown);
            let schema = input(name);
            for key in schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let mut missing = value.clone();
                missing.as_object_mut().unwrap().remove(key);
                invalid(name, missing);
                let mut null = value.clone();
                null[key] = Value::Null;
                invalid(name, null);
            }
        }
    }
    #[test]
    fn optional_fields_are_omitted_not_nullable_and_ranges_are_effective() {
        for name in [
            "sodeck_servers_list",
            "sodeck_targets_list",
            "sodeck_tasks_list",
        ] {
            invalid(name, json!({"query":null})); // unknown on tasks is also rejected
        }
        invalid("sodeck_servers_list", json!({"limit":51}));
        invalid("sodeck_servers_list", json!({"query":""}));
        invalid("sodeck_servers_list", json!({"detail":"true"}));
        invalid(
            "sodeck_connection_get",
            json!({"connectionRequestId":"id","sinceRevision":""}),
        );
        invalid(
            "sodeck_exec_start",
            json!({"targetId":"id","requestId":"r","command":"true","cols":null}),
        );
        valid(
            "sodeck_exec_start",
            json!({"targetId":"id","requestId":"r","command":"true","mode":"pipe","timeoutSeconds":null}),
        );
        invalid(
            "sodeck_exec_start",
            json!({"targetId":"id","requestId":"r","command":"true","timeoutSeconds":0}),
        );
        invalid(
            "sodeck_task_input",
            json!({"taskId":"id","sequence":"1","action":"write","text":"x".repeat(16385)}),
        );
    }
    #[test]
    fn server_patch_matches_core_serde_for_nested_fields_and_explicit_null() {
        use sodeck_plugin_client::protocol::servers::ServerPatch;
        for patch in [
            json!({"name":""}),
            json!({"port":22}),
            json!({"route":[{"kind":"ssh","serverId":"id"}]}),
            json!({"authentication":{"type":"password","secret":{"action":"replace","value":"","remember":true}}}),
        ] {
            valid(
                "sodeck_server_update",
                json!({"catalogRevision":"1","serverId":"id","patch":patch}),
            );
            assert!(serde_json::from_value::<ServerPatch>(patch).is_ok());
        }
        for patch in [
            json!({"host":null}),
            json!({"port":null}),
            json!({"tagIds":null}),
            json!({"route":null}),
            json!({"authentication":null}),
            json!({"authentication":{"type":"password","secret":{"action":"clear","value":"secret"}}}),
            json!({"route":[{"kind":"ssh","serverId":"id","unknown":true}]}),
        ] {
            invalid(
                "sodeck_server_update",
                json!({"catalogRevision":"1","serverId":"id","patch":patch}),
            );
            if patch.as_object().unwrap().values().any(Value::is_null) {
                assert!(serde_json::from_value::<ServerPatch>(patch).is_err());
            }
        }
    }
    #[test]
    fn byte_limits_and_execution_semantics_remain_in_handlers() {
        let value = json!({"targetId":"id","requestId":"r","command":"中".repeat(6000)});
        valid("sodeck_exec_start", value.clone());
        assert!(crate::tasks::parse_exec_start(&value).is_err());
        let value = json!({"targetId":"id","requestId":"r","command":"true","cols":80});
        valid("sodeck_exec_start", value.clone());
        assert!(crate::tasks::parse_exec_start(&value).is_err());
    }
}
