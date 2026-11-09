//! Persist container intent, never sessions, documents or runtime identities.
use crate::{
    component_ui::{Registered, WorkspaceUi},
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
            schema_version: 1,
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
                host.dock(view, side, tool.visible)?;
                if !tool.visible {
                    if side == "left" {
                        host.left.active_view_id = None;
                    } else {
                        host.right.active_view_id = None;
                    }
                }
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
        let tools = w
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
                let side = if host.left.view_ids.contains(&v.id) {
                    Some("left".into())
                } else if host.right.view_ids.contains(&v.id) {
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
                    },
                )
            })
            .collect();
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
            let doc: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if doc.schema_version != 1 || doc.servers.len() > 1024 {
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
            return Ok(());
        }
        let mut next = self.document.clone();
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
                entry.hosts.insert(h.layout_slot, captured);
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
