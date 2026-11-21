use super::{
    change::{CatalogError, RouteInput, WriteAuthentication},
    *,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Create {
    catalog_revision: String,
    server: NewServer,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    catalog_revision: String,
    server_id: String,
    patch: ServerPatch,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Delete {
    catalog_revision: String,
    server_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewServer {
    #[serde(default)]
    name: String,
    host: String,
    #[serde(default = "ssh_port")]
    port: u16,
    username: String,
    #[serde(default)]
    tag_ids: Vec<String>,
    #[serde(default)]
    route: Vec<RouteInput>,
    #[serde(default)]
    authentication: Option<WriteAuthentication>,
}
fn ssh_port() -> u16 {
    22
}
fn revision(s: &str) -> Result<u64, CatalogError> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(CatalogError::new(
            "invalidParams",
            "catalogRevision 必须为十进制 u64 字符串",
        ));
    }
    s.parse()
        .map_err(|_| CatalogError::new("invalidParams", "catalogRevision 超出范围"))
}
fn parse<T: serde::de::DeserializeOwned>(p: &Value) -> Result<T, CatalogError> {
    // Parsing errors may contain secret input values; never return them to the caller.
    serde_json::from_value(p.clone()).map_err(|_| {
        CatalogError::new(
            "invalidParams",
            "服务器写入参数无效，请检查字段、类型和必需值",
        )
    })
}
fn no_null(p: &Value) -> bool {
    match p {
        Value::Null => false,
        Value::Object(o) => o.values().all(no_null),
        Value::Array(a) => a.iter().all(no_null),
        _ => true,
    }
}
fn valid_strings(p: &Value, field: &str) -> bool {
    match p {
        Value::Object(o) => o.iter().all(|(key, value)| valid_strings(value, key)),
        Value::Array(a) => a.iter().all(|value| valid_strings(value, field)),
        Value::String(s) => {
            let max = match field {
                "privateKeyPath" => 4096,
                "value" => 16384,
                _ => 256,
            };
            !s.contains('\0') && s.len() <= max && (field == "value" || !s.trim().is_empty())
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_parameters_reject_ambiguous_or_secret_bearing_errors() {
        assert_eq!(revision("18446744073709551615").unwrap(), u64::MAX);
        for value in ["", "+1", "1e1", "18446744073709551616"] {
            assert_eq!(revision(value).unwrap_err().code, "invalidParams");
        }
        for server in [
            json!({"host":"test.invalid","username":"ops","copyFromId":"secret-marker"}),
            json!({"host":"test.invalid","username":"ops","authentication":{"type":"password","secret":{"action":"secret-marker","value":"secret-marker"}}}),
        ] {
            let error = parse::<Create>(&json!({"catalogRevision":"1","server":server}))
                .err()
                .unwrap();
            assert!(!error.message.contains("secret-marker"));
        }
        for patch in [
            json!({"port":null}),
            json!({"serverId":"other"}),
            json!({"route":[{"kind":"ssh","serverId":"gateway","id":"step"}]}),
        ] {
            assert!(parse::<Update>(
                &json!({"catalogRevision":"1","serverId":"server","patch":patch})
            )
            .is_err());
        }
        assert!(!no_null(&json!({"authentication":{"secret":null}})));
    }
}
impl AppCore {
    pub(crate) fn servers_write(
        &self,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, CatalogError> {
        self.plugin_instance(plugin, instance)
            .ok_or_else(|| CatalogError::new("staleInstance", "插件实例不可用"))?;
        if !p.is_object() || !no_null(p) || !valid_strings(p, "") {
            return Err(CatalogError::new(
                "invalidParams",
                "服务器写入字段为空、超限、含 NUL 或 null",
            ));
        }
        let (rev, change, requested_id) = match method {
            "servers.create" => {
                let Create {
                    catalog_revision,
                    server: s,
                } = parse(p)?;
                let authentication = s
                    .authentication
                    .map(|a| a.input(SecretChange::Keep { remember: true }))
                    .unwrap_or(AuthInput {
                        r#type: "password".into(),
                        private_key_path: None,
                        username: None,
                        secret: SecretChange::Keep { remember: true },
                    });
                (
                    revision(&catalog_revision)?,
                    CatalogChange::SaveServer {
                        draft: ServerInput {
                            id: None,
                            copy_from_id: None,
                            name: s.name,
                            host: s.host,
                            port: s.port,
                            username: s.username,
                            tag_ids: s.tag_ids,
                            new_tag_names: vec![],
                            route: s.route.into_iter().map(RouteInput::step).collect(),
                            authentication,
                        },
                    },
                    None,
                )
            }
            "servers.update" => {
                let Update {
                    catalog_revision,
                    server_id,
                    patch,
                } = parse(p)?;
                if !crate::connections::rpc::valid_id(&server_id) {
                    return Err(CatalogError::new("invalidParams", "serverId 无效"));
                }
                (
                    revision(&catalog_revision)?,
                    CatalogChange::PatchServer {
                        id: server_id.clone(),
                        patch,
                    },
                    Some(server_id),
                )
            }
            "servers.delete" => {
                let Delete {
                    catalog_revision,
                    server_id,
                } = parse(p)?;
                if !crate::connections::rpc::valid_id(&server_id) {
                    return Err(CatalogError::new("invalidParams", "serverId 无效"));
                }
                (
                    revision(&catalog_revision)?,
                    CatalogChange::DeleteServer {
                        id: server_id.clone(),
                    },
                    Some(server_id),
                )
            }
            _ => return Err(CatalogError::new("invalidParams", "未知服务器写入方法")),
        };
        let changed = self.apply_catalog_change(rev, change, Some((plugin, instance)))?;
        Ok(
            json!({"serverId":changed.selected_id.or(requested_id),"catalogRevision":changed.revision.to_string()}),
        )
    }
}
