use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum SecretChange {
    Keep { remember: bool },
    Replace { value: String, remember: bool },
    Clear,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerPatch {
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub host: Option<String>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub port: Option<u16>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub username: Option<String>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub tag_ids: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub route: Option<Vec<RouteInput>>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub authentication: Option<WriteAuthentication>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RouteInput {
    #[serde(rename_all = "camelCase")]
    Ssh { server_id: String },
    #[serde(rename_all = "camelCase")]
    Socks5 { proxy_id: String },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteAuthentication {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub private_key_path: Option<String>,
    #[serde(
        default,
        deserialize_with = "provided",
        skip_serializing_if = "Option::is_none"
    )]
    pub secret: Option<SecretChange>,
}
fn provided<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
    pub catalog_revision: String,
    pub server: NewServer,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Update {
    pub catalog_revision: String,
    pub server_id: String,
    pub patch: ServerPatch,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delete {
    pub catalog_revision: String,
    pub server_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewServer {
    #[serde(default)]
    pub name: String,
    pub host: String,
    #[serde(default = "ssh_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub route: Vec<RouteInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<WriteAuthentication>,
}
fn ssh_port() -> u16 {
    22
}
