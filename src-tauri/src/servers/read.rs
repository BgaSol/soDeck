use super::*;
use crate::connections::{
    rpc::{bounded, valid_id},
    Error,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Params {
    server_id: Option<String>,
    #[serde(default)]
    detail: bool,
    #[serde(default = "limit")]
    limit: usize,
    cursor: Option<String>,
    view_type: Option<String>,
}
fn limit() -> usize {
    20
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    revision: u64,
    offset: usize,
    server: Option<String>,
    limit: usize,
}

impl AppCore {
    pub(crate) fn servers_read(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        self.plugin_instance(plugin, instance)
            .ok_or_else(|| Error::new("staleInstance", "插件实例不可用"))?;
        let params: Params = serde_json::from_value(p.clone())
            .map_err(|e| Error::new("invalidParams", e.to_string()))?;
        if !(1..=50).contains(&params.limit)
            || params.server_id.as_ref().is_some_and(|s| !valid_id(s))
            || params.view_type.as_ref().is_some_and(|s| !valid_id(s))
            || p.as_object().is_none_or(|o| o.values().any(Value::is_null))
        {
            return Err(Error::new("invalidParams", "服务器查询参数无效"));
        }
        if let Some(view_type) = &params.view_type {
            self.validate_background_view(plugin, instance, view_type)
                .map_err(|e| Error::new("invalidParams", e))?;
        }
        let (revision, rows, mut next) = {
            let store = self.servers.lock().unwrap();
            if store.error.is_some() {
                return Err(Error::new(
                    "catalogUnavailable",
                    "服务器目录不可用，请先恢复目录",
                ));
            }
            let revision = store.catalog.revision;
            let offset = match &params.cursor {
                None => 0,
                Some(c) => {
                    if c.len() > 2048 {
                        return Err(Error::new("invalidParams", "游标超限"));
                    }
                    let c: Cursor = serde_json::from_str(c)
                        .map_err(|_| Error::new("invalidCursor", "资料游标无效"))?;
                    if c.revision != revision {
                        return Err(Error::new(
                            "catalogChanged",
                            "资料目录已经变化，请从第一页查询",
                        ));
                    }
                    if c.server != params.server_id
                        || c.limit != params.limit
                        || c.offset > store.catalog.servers.len()
                    {
                        return Err(Error::new("invalidCursor", "游标范围不同"));
                    }
                    c.offset
                }
            };
            let registry = self.connections.registry.lock().unwrap();
            let state = self.workbench_state();
            let mut rows = Vec::new();
            let mut bytes = 512;
            let mut next = None;
            for (position, server) in store.catalog.servers.iter().enumerate().skip(offset) {
                if params.server_id.as_ref().is_some_and(|id| id != &server.id) {
                    continue;
                }
                let profile = store
                    .connection_snapshot(&server.id)
                    .map_err(|e| Error::new("catalogUnavailable", e))?;
                let workspace = state.workspaces.by_server(&server.id);
                let attempt = registry.current.get(&server.id);
                let connection = attempt.map(|a| {
                    let v = a.view();
                    json!({"state":v.state,"endpoint":v.endpoint,"context":v.context,"matchesSavedTarget":a.route_identity==profile.route_identity(),"error":v.error})
                }).or_else(|| workspace.map(|w| {
                    json!({"state":if w.connected {"connected"} else {"disconnected"},"endpoint":w.endpoint,"context":w.context,"matchesSavedTarget":null,"error":null})
                }));
                let mut row = json!({"serverId":server.id,"profileToken":profile.identity,"name":server.name,"protocol":server.protocol,"host":server.host,"port":server.port,"endpoint":format!("{}:{}",if server.host.contains(':'){format!("[{}]",server.host)}else{server.host.clone()},server.port),"username":server.username,"connection":connection,"context":workspace.map(|w|&w.context)});
                if params.detail {
                    row["tagIds"] = json!(server.tag_ids);
                    row["route"]=json!(profile.nodes.iter().map(|n|json!({"name":n.name,"protocol":if n.is_ssh(){"ssh"}else{"socks5"},"endpoint":n.endpoint(),"username":n.username})).collect::<Vec<_>>());
                }
                let size = serde_json::to_vec(&row)
                    .map_err(|e| Error::new("invalidResult", e.to_string()))?
                    .len();
                if size + 512 > 40 * 1024 {
                    return Err(Error::new("resourceLimit", "一项服务器资料超过控制帧预算"));
                }
                if rows.len() == params.limit || bytes + size > 40 * 1024 {
                    next = Some(position);
                    break;
                }
                bytes += size;
                rows.push((position, row));
            }
            (revision, rows, next)
        };
        let mut output = Vec::new();
        let mut bytes = 512;
        for (position, mut row) in rows {
            if !row["context"].is_null() {
                let context: crate::workspaces::Context =
                    serde_json::from_value(row["context"].clone())
                        .map_err(|e| Error::new("invalidResult", e.to_string()))?;
                row["current"] = self
                    .context_target_facts(&context)
                    .unwrap_or_else(|e| json!({"error":{"code":e.code,"message":e.message}}));
                if let Some(view_type) = &params.view_type {
                    row["background"] = self
                        .background_creation(plugin, instance, view_type, &context)
                        .map_err(|e| Error::new("invalidParams", e))?;
                }
            }
            let size = serde_json::to_vec(&row)
                .map_err(|e| Error::new("invalidResult", e.to_string()))?
                .len();
            if size + 512 > 47 * 1024 {
                return Err(Error::new("resourceLimit", "一项服务器观察超过控制帧预算"));
            }
            if bytes + size > 47 * 1024 {
                next = Some(position);
                break;
            }
            bytes += size;
            output.push(row);
        }
        let cursor = next
            .map(|offset| {
                serde_json::to_string(&Cursor {
                    revision,
                    offset,
                    server: params.server_id,
                    limit: params.limit,
                })
            })
            .transpose()
            .map_err(|e| Error::new("invalidResult", e.to_string()))?;
        bounded(
            json!({"catalogRevision":revision.to_string(),"servers":output,"hasMore":cursor.is_some(),"nextCursor":cursor}),
        )
    }
}
