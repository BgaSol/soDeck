//! Persist container intent, never sessions, documents or runtime identities.
use crate::{
    component_ui::{Preference, Registered, WorkspaceUi},
    storage,
    workbench_layout::{Host, Node},
    workspaces::{Context, Layout},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub group: Option<String>,
    pub side: Option<String>,
    pub visible: bool,
    #[serde(default)]
    pub order: Option<u32>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedHost {
    pub slot: u32,
    pub center: Node,
    pub layout: Layout,
    pub tools: BTreeMap<String, Tool>,
    pub bounds: Option<Bounds>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Server {
    primary: u32,
    hosts: BTreeMap<u32, SavedHost>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Document {
    schema_version: u32,
    servers: BTreeMap<String, Server>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 2,
            servers: BTreeMap::new(),
        }
    }
}
#[derive(Clone)]
pub struct Memory {
    root: PathBuf,
    document: Document,
    pub error: Option<String>,
    blocked: bool,
}

impl SavedHost {
    pub fn seed_order(&self, host: &mut Host, preferences: &BTreeMap<String, Preference>) {
        for tools in [&mut host.left, &mut host.right] {
            if tools
                .entries
                .iter()
                .all(|e| self.tools[&e.definition_key].order.is_none())
            {
                tools.entries.sort_by_key(|e| {
                    (
                        preferences
                            .get(&e.definition_key)
                            .map(|p| p.order)
                            .unwrap_or(u32::MAX),
                        e.definition_key.clone(),
                    )
                });
            }
        }
    }
    pub fn seed(&self, id: String) -> Result<Host, String> {
        let mut host = Host::new(id);
        host.layout_slot = self.slot;
        host.bounds = self.bounds.clone();
        host.layout = self.layout.clone();
        fn remap(node: &Node, prefix: &str) -> Node {
            match node {
                Node::Group { group_id } => Node::Group {
                    group_id: format!("{prefix}-{group_id}"),
                },
                Node::Split {
                    id,
                    axis,
                    ratio,
                    first,
                    second,
                } => Node::Split {
                    id: format!("{prefix}-{id}"),
                    axis: axis.clone(),
                    ratio: *ratio,
                    first: Box::new(remap(first, prefix)),
                    second: Box::new(remap(second, prefix)),
                },
            }
        }
        host.center = remap(&self.center, &host.id);
        host.groups.clear();
        for group in host.ordered_groups() {
            host.groups.insert(
                group.clone(),
                crate::workbench_layout::Group {
                    id: group,
                    view_ids: vec![],
                    active_view_id: None,
                    explicit_empty: true,
                },
            );
        }
        host.focused_group_id = host.ordered_groups()[0].clone();
        let mut tools: Vec<_> = self
            .tools
            .iter()
            .filter(|(_, t)| t.side.is_some())
            .collect();
        tools.sort_by_key(|(key, t)| (t.order.unwrap_or(u32::MAX), *key));
        for (key, tool) in tools {
            let side = if tool.side.as_deref() == Some("left") {
                &mut host.left
            } else {
                &mut host.right
            };
            side.entries.push(crate::workbench_layout::ToolSlot {
                definition_key: key.clone(),
                view_id: None,
            });
        }
        host.validate()?;
        Ok(host)
    }
    pub fn destination(
        &self,
        host: &Host,
        key: &str,
    ) -> Option<crate::workbench_move::Destination> {
        let tool = self.tools.get(key)?;
        Some(crate::workbench_move::Destination {
            group_id: tool
                .group
                .as_ref()
                .map(|group| format!("{}-{group}", host.id)),
            side: tool.side.clone(),
            ..Default::default()
        })
    }
    pub fn apply_tools(&self, host: &mut Host, views: &[(String, String)]) -> Result<(), String> {
        for (view, key) in views {
            let Some(tool) = self.tools.get(key) else {
                continue;
            };
            if let Some(group) = &tool.group {
                host.place(
                    view,
                    &format!("{}-{group}", host.id),
                    usize::MAX,
                    tool.visible,
                )?;
            } else if let Some(side) = &tool.side {
                host.dock(view, key, side, tool.visible)?;
            }
        }
        Ok(())
    }
    fn capture(host: &Host, w: &WorkspaceUi, definitions: &BTreeMap<String, Registered>) -> Self {
        let groups: BTreeMap<_, _> = host
            .ordered_groups()
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, format!("g{i}")))
            .collect();
        fn normalize(node: &Node, groups: &BTreeMap<String, String>, next: &mut usize) -> Node {
            match node {
                Node::Group { group_id } => Node::Group {
                    group_id: groups[group_id].clone(),
                },
                Node::Split {
                    axis,
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    let id = format!("s{}", *next);
                    *next += 1;
                    Node::Split {
                        id,
                        axis: axis.clone(),
                        ratio: *ratio,
                        first: Box::new(normalize(first, groups, next)),
                        second: Box::new(normalize(second, groups, next)),
                    }
                }
            }
        }
        let mut tools: BTreeMap<_, _> = w
            .views
            .iter()
            .filter(|v| {
                definitions
                    .get(&v.definition_key)
                    .is_some_and(|d| d.definition.container == "sidebar")
                    && host.contains(&v.id)
            })
            .map(|v| {
                let group = host.group_of(&v.id);
                let side = if host.left.view_ids().contains(&v.id) {
                    Some("left".into())
                } else if host.right.view_ids().contains(&v.id) {
                    Some("right".into())
                } else {
                    None
                };
                let visible = group
                    .as_ref()
                    .is_some_and(|g| host.groups[g].active_view_id.as_ref() == Some(&v.id))
                    || host.left.active_view_id.as_ref() == Some(&v.id)
                    || host.right.active_view_id.as_ref() == Some(&v.id);
                (
                    v.definition_key.clone(),
                    Tool {
                        group: group.map(|g| groups[&g].clone()),
                        side,
                        visible,
                        order: None,
                    },
                )
            })
            .collect();
        for (side, slots) in [("left", &host.left), ("right", &host.right)] {
            for (order, entry) in slots.entries.iter().enumerate() {
                tools.insert(
                    entry.definition_key.clone(),
                    Tool {
                        group: None,
                        side: Some(side.into()),
                        visible: entry.view_id.is_some() && entry.view_id == slots.active_view_id,
                        order: Some(order as u32),
                    },
                );
            }
        }
        let mut layout = host.layout.clone();
        layout.left_open = host.left.active_view_id.is_some();
        layout.right_open = host.right.active_view_id.is_some();
        Self {
            slot: host.layout_slot,
            center: normalize(&host.center, &groups, &mut 0),
            layout,
            tools,
            bounds: host.bounds.clone(),
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.slot > 1024
            || !(160..=480).contains(&self.layout.left_width)
            || !(160..=480).contains(&self.layout.right_width)
            || self.tools.len() > 128
        {
            return Err("保存的工作台布局无效".into());
        }
        let host = self.seed("validation".into())?;
        let mut groups = vec![];
        self.center.groups(&mut groups);
        if self.tools.iter().any(|(key, t)| {
            key.len() > 256
                || key.is_empty()
                || t.order.is_some_and(|o| o >= 128 || t.side.is_none())
                || !(t.group.is_some() ^ t.side.is_some())
                || t.group.as_ref().is_some_and(|g| !groups.contains(g))
                || t.side.as_ref().is_some_and(|s| s != "left" && s != "right")
        }) || self.bounds.as_ref().is_some_and(|b| {
            !b.width.is_finite()
                || !b.height.is_finite()
                || !(720.0..=16384.0).contains(&b.width)
                || !(480.0..=16384.0).contains(&b.height)
        }) {
            return Err("保存的工具位置或窗口尺寸无效".into());
        }
        for side in ["left", "right"] {
            let orders: Vec<_> = self
                .tools
                .values()
                .filter(|t| t.side.as_deref() == Some(side))
                .filter_map(|t| t.order)
                .collect();
            if orders
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != orders.len()
            {
                return Err("保存的工具顺序重复".into());
            }
        }
        host.validate()
    }
}
impl Memory {
    pub fn open(root: &Path) -> Self {
        let loaded = (|| -> Result<Document, String> {
            let bytes = match fs::read(root.join("workbench-layout.json")) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Document::default())
                }
                Err(e) => return Err(e.to_string()),
            };
            if bytes.len() > 1024 * 1024 {
                return Err("布局文件超过 1 MiB".into());
            }
            let mut doc: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if ![1, 2].contains(&doc.schema_version) || doc.servers.len() > 1024 {
                return Err("布局版本或服务器数量无效".into());
            }
            for server in doc.servers.values() {
                if server.hosts.is_empty()
                    || server.hosts.len() > 32
                    || !server.hosts.contains_key(&server.primary)
                {
                    return Err("布局槽无效".into());
                }
                for (slot, host) in &server.hosts {
                    if slot != &host.slot {
                        return Err("布局槽不一致".into());
                    }
                    host.validate()?;
                }
            }
            for server in doc.servers.values_mut() {
                let mut seen = std::collections::HashSet::new();
                let mut slots: Vec<_> = server.hosts.keys().copied().collect();
                slots.sort_by_key(|slot| (*slot != server.primary, *slot));
                for slot in slots {
                    server
                        .hosts
                        .get_mut(&slot)
                        .unwrap()
                        .tools
                        .retain(|key, _| seen.insert(key.clone()));
                }
            }
            Ok(doc)
        })();
        let (document, error) = match loaded {
            Ok(d) => (d, None),
            Err(e) => (
                Document::default(),
                Some(format!("工作台布局无法恢复，原文件已保留：{e}")),
            ),
        };
        Self {
            root: root.into(),
            blocked: error.is_some(),
            document,
            error,
        }
    }
    pub fn primary(&self, server: &str) -> Option<SavedHost> {
        let s = self.document.servers.get(server)?;
        s.hosts.get(&s.primary).cloned()
    }
    pub fn tool_slot(&self, server: &str, key: &str) -> Option<u32> {
        self.document
            .servers
            .get(server)?
            .hosts
            .values()
            .find(|h| h.tools.contains_key(key))
            .map(|h| h.slot)
    }
    pub fn hosts(&self, server: &str) -> Vec<SavedHost> {
        self.document
            .servers
            .get(server)
            .map(|s| s.hosts.values().cloned().collect())
            .unwrap_or_default()
    }
    pub fn capture(
        &mut self,
        contexts: &[Context],
        workspaces: &std::collections::HashMap<String, WorkspaceUi>,
        definitions: &BTreeMap<String, Registered>,
        owners: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.blocked {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "布局原文件异常，未覆盖".into()));
        }
        let mut next = self.document.clone();
        next.schema_version = 2;
        for c in contexts {
            let Some(w) = workspaces.get(&c.workspace_id) else {
                continue;
            };
            let entry = next.servers.entry(c.server_id.clone()).or_default();
            for h in w.hosts.values() {
                let mut captured = SavedHost::capture(h, w, definitions);
                if let Some(previous) = entry.hosts.get(&h.layout_slot) {
                    let mut groups = vec![];
                    captured.center.groups(&mut groups);
                    for (key, tool) in &previous.tools {
                        if !definitions.get(key).is_some_and(|d| d.available) {
                            let mut tool = tool.clone();
                            if tool.group.as_ref().is_some_and(|g| !groups.contains(g)) {
                                tool.group = None;
                                tool.side = Some("left".into());
                                tool.visible = false;
                            }
                            captured.tools.entry(key.clone()).or_insert(tool);
                        }
                    }
                }
                for side in ["left", "right"] {
                    let mut keys: Vec<_> = captured
                        .tools
                        .iter()
                        .filter(|(_, t)| t.side.as_deref() == Some(side))
                        .map(|(key, t)| (t.order.unwrap_or(u32::MAX), key.clone()))
                        .collect();
                    keys.sort();
                    for (order, (_, key)) in keys.into_iter().enumerate() {
                        captured.tools.get_mut(&key).unwrap().order = Some(order as u32);
                    }
                }
                entry.hosts.insert(h.layout_slot, captured);
            }
            let locations: BTreeMap<_, _> = w
                .hosts
                .values()
                .flat_map(|h| {
                    SavedHost::capture(h, w, definitions)
                        .tools
                        .into_keys()
                        .map(move |key| (key, h.layout_slot))
                })
                .collect();
            for h in entry.hosts.values_mut() {
                h.tools
                    .retain(|key, _| locations.get(key).is_none_or(|slot| *slot == h.slot));
            }
            if let Some(host) = owners
                .get(&c.workspace_id)
                .and_then(|window| w.hosts.get(window))
            {
                entry.primary = host.layout_slot;
            }
        }
        if next == self.document {
            return Ok(());
        }
        let result = (|| {
            if next.servers.len() > 1024
                || next.servers.values().any(|s| s.hosts.len() > 32)
                || serde_json::to_vec_pretty(&next)
                    .map_err(|e| e.to_string())?
                    .len()
                    > 1024 * 1024
            {
                return Err("布局记录超出保存额度".into());
            }
            for server in next.servers.values() {
                for host in server.hosts.values() {
                    host.validate()?;
                }
            }
            if self.root.join("workbench-layout.json").exists() {
                fs::copy(
                    self.root.join("workbench-layout.json"),
                    self.root.join("workbench-layout.json.bak"),
                )
                .map_err(|e| e.to_string())?;
            }
            storage::write_json(&self.root.join("workbench-layout.json"), &next)
        })();
        match result {
            Ok(()) => {
                self.document = next;
                self.error = None;
                Ok(())
            }
            Err(e) => {
                self.error = Some(format!("工作台布局未保存：{e}；当前布局继续可用"));
                Err(self.error.clone().unwrap())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_slots_migrate_with_preference_order_and_primary_wins_duplicate_history() {
        let root = std::env::temp_dir().join(format!(
            "sodeck-tool-memory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let saved = SavedHost {
            slot: 1,
            center: Node::Group {
                group_id: "g0".into(),
            },
            layout: Layout::default(),
            bounds: None,
            tools: BTreeMap::from([
                (
                    "a".into(),
                    Tool {
                        group: None,
                        side: Some("right".into()),
                        visible: false,
                        order: None,
                    },
                ),
                (
                    "b".into(),
                    Tool {
                        group: None,
                        side: Some("right".into()),
                        visible: false,
                        order: None,
                    },
                ),
            ]),
        };
        let mut duplicate = saved.clone();
        duplicate.slot = 0;
        let document = Document {
            schema_version: 1,
            servers: BTreeMap::from([(
                "server".into(),
                Server {
                    primary: 1,
                    hosts: BTreeMap::from([(0, duplicate), (1, saved.clone())]),
                },
            )]),
        };
        storage::write_json(&root.join("workbench-layout.json"), &document).unwrap();
        let original = fs::read(root.join("workbench-layout.json")).unwrap();
        let mut memory = Memory::open(&root);
        assert!(memory.error.is_none());
        assert!(memory
            .hosts("server")
            .iter()
            .find(|h| h.slot == 0)
            .unwrap()
            .tools
            .is_empty());
        let mut host = saved.seed("runtime-host".into()).unwrap();
        let prefs = BTreeMap::from([
            (
                "a".into(),
                Preference {
                    side: "left".into(),
                    pinned: true,
                    startup: "none".into(),
                    order: 20,
                },
            ),
            (
                "b".into(),
                Preference {
                    side: "left".into(),
                    pinned: true,
                    startup: "none".into(),
                    order: 10,
                },
            ),
        ]);
        saved.seed_order(&mut host, &prefs);
        assert_eq!(
            host.right
                .entries
                .iter()
                .map(|e| e.definition_key.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert!(host.view_ids().is_empty());
        let context = Context {
            workspace_id: "workspace".into(),
            server_id: "server".into(),
            connection_id: "connection".into(),
            generation: 1,
        };
        let w = WorkspaceUi {
            hosts: BTreeMap::from([("main".into(), host)]),
            ..Default::default()
        };
        memory
            .capture(
                &[context],
                &std::collections::HashMap::from([("workspace".into(), w)]),
                &BTreeMap::new(),
                &BTreeMap::from([("workspace".into(), "main".into())]),
            )
            .unwrap();
        assert_eq!(memory.document.schema_version, 2);
        assert_eq!(memory.primary("server").unwrap().tools["b"].order, Some(0));
        assert_eq!(
            fs::read(root.join("workbench-layout.json.bak")).unwrap(),
            original
        );
        let text = fs::read_to_string(root.join("workbench-layout.json")).unwrap();
        assert!(!text.contains("runtime-host") && !text.contains("connection"));
        let mut invalid = memory.document.clone();
        invalid
            .servers
            .get_mut("server")
            .unwrap()
            .hosts
            .get_mut(&1)
            .unwrap()
            .tools
            .get_mut("a")
            .unwrap()
            .order = Some(0);
        storage::write_json(&root.join("workbench-layout.json"), &invalid).unwrap();
        assert!(Memory::open(&root).error.is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
