use crate::{
    commands,
    resources::{Kind, ResourceRef},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const INDEX_LIMIT: usize = 2 * 1024 * 1024;
pub const RELATION_LIMIT: usize = 256 * 1024;
pub struct Index {
    pub rows: Vec<Value>,
    pub staged: Vec<Value>,
    pub generation: u64,
    pub revision: u64,
    pub phase: String,
    pub complete: bool,
    pub partial: bool,
    pub error: Option<String>,
}
impl Default for Index {
    fn default() -> Self {
        Self {
            rows: vec![],
            staged: vec![],
            generation: 0,
            revision: 0,
            phase: "unloaded".into(),
            complete: false,
            partial: false,
            error: None,
        }
    }
}
pub struct Relation {
    pub generation: u64,
    pub phase: String,
    pub items: Vec<Value>,
    pub observation: Value,
    pub complete: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub enum Work {
    Index {
        kind: Kind,
        generation: u64,
        ids: Option<Vec<String>>,
        offset: usize,
    },
    Relation {
        resource: ResourceRef,
        generation: u64,
        ids: Option<Vec<String>>,
        offset: usize,
    },
}
pub struct BrowserState {
    pub indexes: BTreeMap<Kind, Index>,
    pub relations: BTreeMap<String, Relation>,
    pub last_page: Value,
    pub visible: bool,
    serial: u64,
}
impl Default for BrowserState {
    fn default() -> Self {
        Self {
            indexes: Kind::ALL
                .into_iter()
                .map(|k| (k, Index::default()))
                .collect(),
            relations: BTreeMap::new(),
            last_page: json!({"nodes":[],"total":0}),
            visible: true,
            serial: 0,
        }
    }
}
impl BrowserState {
    pub fn bytes(&self) -> usize {
        self.indexes
            .values()
            .map(|i| bytes(&json!([i.rows, i.staged])))
            .sum::<usize>()
            + self
                .relations
                .values()
                .map(|r| bytes(&json!([r.items, r.observation])))
                .sum::<usize>()
            + bytes(&self.last_page)
    }
    pub fn invalidate(&mut self) {
        self.serial += 1;
        for i in self.indexes.values_mut() {
            i.generation += 1;
            i.complete = false;
            i.staged.clear();
            if i.phase != "unloaded" {
                i.phase = "stale".into();
            }
        }
        self.relations.clear();
    }
    pub fn load(&mut self, kind: Kind) -> Option<Work> {
        let i = self.indexes.get_mut(&kind).unwrap();
        if i.phase == "loading" {
            return None;
        }
        i.generation += 1;
        i.phase = "loading".into();
        i.staged.clear();
        i.partial = false;
        i.complete = false;
        i.error = None;
        Some(Work::Index {
            kind,
            generation: i.generation,
            ids: None,
            offset: 0,
        })
    }
    pub fn load_relation(&mut self, resource: ResourceRef) -> Result<Option<Work>, String> {
        let key = resource.key();
        if self
            .relations
            .get(&key)
            .is_some_and(|r| r.phase == "loading")
        {
            return Ok(None);
        }
        if self.relations.len() >= 32 && !self.relations.contains_key(&key) {
            return Err("关系缓存已达 32 个对象，请刷新后再展开".into());
        }
        self.serial += 1;
        self.relations.insert(
            key,
            Relation {
                generation: self.serial,
                phase: "loading".into(),
                items: vec![],
                observation: Value::Null,
                complete: false,
                error: None,
            },
        );
        Ok(Some(Work::Relation {
            resource,
            generation: self.serial,
            ids: None,
            offset: 0,
        }))
    }
    pub fn owns(&self, work: &Work) -> bool {
        match work {
            Work::Index {
                kind, generation, ..
            } => {
                self.indexes[kind].generation == *generation
                    && self.indexes[kind].phase == "loading"
            }
            Work::Relation {
                resource,
                generation,
                ..
            } => self
                .relations
                .get(&resource.key())
                .is_some_and(|r| r.generation == *generation && r.phase == "loading"),
        }
    }
    pub fn fail(&mut self, work: &Work, error: String) {
        match work {
            Work::Index { kind, .. } => {
                let i = self.indexes.get_mut(kind).unwrap();
                i.phase = "failed".into();
                i.error = Some(commands::text(&error, 1024));
                i.complete = false;
                i.staged.clear();
            }
            Work::Relation { resource, .. } => {
                if let Some(r) = self.relations.get_mut(&resource.key()) {
                    r.phase = "failed".into();
                    r.error = Some(commands::text(&error, 1024));
                    r.complete = false;
                    r.items.clear();
                    r.observation = Value::Null;
                }
            }
        }
    }
    pub fn row(&self, resource: &ResourceRef) -> Option<&Value> {
        self.indexes[&resource.kind]
            .rows
            .iter()
            .find(|r| r["id"] == resource.id)
    }
    pub fn metadata(&self) -> Value {
        json!(self.indexes.iter().map(|(k,i)|(k.name(),json!({"phase":i.phase,"revision":i.revision.to_string(),"count":i.rows.len(),"readCount":i.staged.len(),"complete":i.complete,"error":i.error}))).collect::<BTreeMap<_,_>>())
    }
    pub fn page(&mut self, engine: &Value, state: &Value, budget: usize) -> Value {
        let mut offset = state["offset"].as_u64().unwrap_or(0) as usize;
        let signature = json!([
            state["query"],
            state["status"],
            state["health"],
            state["group"]
        ]);
        if self.last_page["signature"] == signature {
            if let (Some(key), Some(previous)) = (
                state["anchor"]["key"].as_str(),
                state["anchor"]["index"].as_u64(),
            ) {
                let mut position = 0;
                let mut found = None;
                self.visit_nodes(engine, state, |n| {
                    if n["key"] == key {
                        found = Some(position);
                    }
                    position += 1;
                });
                if let Some(index) = found {
                    offset = offset
                        .saturating_add(index)
                        .saturating_sub(previous as usize);
                }
            }
        }
        let mut selected = vec![];
        let mut size = 0;
        let mut total = 0;
        let mut filled = false;
        self.visit_nodes(engine, state, |n| {
            let position = total;
            total += 1;
            if position < offset || filled {
                return;
            }
            let len = bytes(&n) + 1;
            if size + len > budget || selected.len() >= 100 {
                filled = true;
                return;
            }
            size += len;
            selected.push(n);
        });
        if offset >= total && total > 0 {
            let mut state = state.clone();
            state["offset"] = json!(total - 1);
            state["anchor"] = Value::Null;
            return self.page(engine, &state, budget);
        }
        let page = json!({"nodes":selected,"offset":offset,"total":total,"nextOffset":if offset+selected.len()<total{Some(offset+selected.len())}else{None},"revealed":state["revealTarget"],"signature":signature});
        self.last_page = page.clone();
        page
    }
    pub fn locate(&self, engine: &Value, resource: &ResourceRef) -> Value {
        let mut path: Vec<String> = vec![];
        let mut result = None;
        self.visit_nodes(engine,&json!({"query":resource.id,"status":"all","health":"all","group":true,"expandAll":true}),|n| {
            if result.is_some() {return;}
            let depth=n["depth"].as_u64().unwrap_or(0) as usize;path.truncate(depth);
            if n["resource"]==json!(resource) && n["type"]=="resource" {result=Some(json!({"found":true,"key":n["key"],"expandedKeys":path,"resource":resource}));}
            path.push(n["key"].as_str().unwrap_or("").into());
        });
        result.unwrap_or_else(|| json!({"found":false,"branchKind":resource.kind.name()}))
    }
    fn visit_nodes(&self, engine: &Value, state: &Value, mut emit: impl FnMut(Value)) {
        let query = state["query"].as_str().unwrap_or("").to_lowercase();
        let expanded: BTreeSet<&str> = state["expandedKeys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let expanding =
            |key: &str| state["expandAll"] == true || !query.is_empty() || expanded.contains(key);
        let mut root = node(
            "engine",
            "",
            0,
            "engine",
            engine["name"].as_str().unwrap_or("Docker Engine"),
            engine["version"].as_str().unwrap_or("待读取"),
            true,
            None,
        );
        root["expanded"] = json!(expanding("engine"));
        emit(root);
        emit(node(
            "docker-root",
            "engine",
            1,
            "root",
            "Docker Root",
            engine["dockerRootDir"].as_str().unwrap_or("未读取"),
            false,
            None,
        ));
        if !expanding("engine") {
            return;
        }
        for kind in Kind::ALL {
            let index = &self.indexes[&kind];
            let category = format!("category:{}", kind.name());
            let meta = match index.phase.as_str() {
                "unloaded" => "待读取".into(),
                "loading" => format!("读取中 · 已读 {}", index.staged.len()),
                "failed" => "读取失败 · 可重试".into(),
                _ => {
                    if index.complete {
                        format!("{} 个", index.rows.len())
                    } else {
                        format!("已读 {} · 不完整", index.rows.len())
                    }
                }
            };
            let mut n = node(
                &category,
                "engine",
                1,
                "category",
                kind.title(),
                &meta,
                true,
                None,
            );
            n["branchKind"] = json!(kind.name());
            n["expanded"] = json!(expanding(&category));
            emit(n);
            if !expanding(&category) {
                continue;
            }
            let mut rows = index
                .rows
                .iter()
                .filter(|r| {
                    let target = json!({"kind":kind,"id":r["id"]});
                    let reveal = state["revealTarget"] == target;
                    let matches = query.is_empty()
                        || [
                            "name", "id", "image", "search", "project", "service", "tags",
                            "digests",
                        ]
                        .iter()
                        .any(|k| r[*k].to_string().to_lowercase().contains(&query));
                    if kind != Kind::Container {
                        return reveal || matches;
                    }
                    let status = state["status"].as_str().unwrap_or("all");
                    let health = state["health"].as_str().unwrap_or("all");
                    let abnormal = r["status"] == "restarting"
                        || r["status"] == "dead"
                        || r["health"] == "unhealthy"
                        || (r["status"] == "exited"
                            && r["exitCode"].as_i64().is_some_and(|n| n != 0));
                    reveal
                        || matches
                            && (status == "all"
                                || r["status"] == status
                                || status == "abnormal" && abnormal)
                            && (health == "all"
                                || r["health"] == health
                                || health == "none" && r["health"].is_null())
                })
                .collect::<Vec<_>>();
            rows.sort_by(|a, b| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
                    .then(a["id"].as_str().cmp(&b["id"].as_str()))
            });
            let mut groups: BTreeMap<String, Vec<(&Value, String)>> = BTreeMap::new();
            for row in rows {
                if kind == Kind::Image {
                    let tags = row["tags"].as_array().filter(|a| !a.is_empty());
                    if let Some(tags) = tags {
                        for tag in tags.iter().filter_map(Value::as_str) {
                            let split = tag.rfind(':').filter(|i| !tag[*i..].contains('/'));
                            let (repo, label) =
                                split.map_or((tag, "latest"), |i| (&tag[..i], &tag[i + 1..]));
                            groups
                                .entry(repo.into())
                                .or_default()
                                .push((row, label.into()));
                        }
                    } else {
                        groups
                            .entry("无标签镜像".into())
                            .or_default()
                            .push((row, short(row["id"].as_str().unwrap_or("")).into()));
                    }
                } else if kind == Kind::Container && state["group"].as_bool().unwrap_or(true) {
                    let project = row["project"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("独立容器");
                    groups
                        .entry(project.into())
                        .or_default()
                        .push((row, row["name"].as_str().unwrap_or("").into()));
                } else {
                    groups
                        .entry(String::new())
                        .or_default()
                        .push((row, row["name"].as_str().unwrap_or("").into()));
                }
            }
            for (group, rows) in groups {
                let parent = if group.is_empty() {
                    category.clone()
                } else {
                    format!("group:{}:{}", kind.name(), encode(&group))
                };
                let depth = if group.is_empty() { 2 } else { 3 };
                if !group.is_empty() {
                    let running = rows
                        .iter()
                        .filter(|(r, _)| r["status"] == "running")
                        .count();
                    let meta = if kind == Kind::Container {
                        format!("{} 个 · {} 运行", rows.len(), running)
                    } else {
                        format!("{} 个标签", rows.len())
                    };
                    let mut n = node(&parent, &category, 2, "group", &group, &meta, true, None);
                    n["branchKind"] = json!(kind.name());
                    n["expanded"] = json!(expanding(&parent));
                    emit(n);
                    if !expanding(&parent) {
                        continue;
                    }
                }
                for (row, label) in rows {
                    let resource = ResourceRef {
                        kind,
                        id: row["id"].as_str().unwrap_or("").into(),
                    };
                    let key = if kind == Kind::Image {
                        format!("tag:{}:{}:{}", resource.id, encode(&group), encode(&label))
                    } else {
                        resource.key()
                    };
                    let relation = self.relations.get(&resource.key());
                    let mut meta = if kind == Kind::Container {
                        status(row)
                    } else if kind == Kind::Image {
                        short(&resource.id).into()
                    } else {
                        row["driver"].as_str().unwrap_or("").into()
                    };
                    if kind != Kind::Container {
                        if let Some(r) = relation.filter(|r| r.complete) {
                            meta.push_str(&format!(" · {} 个容器引用", r.items.len()));
                        }
                    }
                    let mut n = node(
                        &key,
                        &parent,
                        depth,
                        "resource",
                        &label,
                        &meta,
                        true,
                        Some(&resource),
                    );
                    n["expanded"] = json!(expanding(&key));
                    n["status"] = row["status"].clone();
                    n["health"] = row["health"].clone();
                    n["revision"] = json!(index.revision.to_string());
                    n["system"] = json!(
                        kind == Kind::Network
                            && ["bridge", "host", "none"].contains(&label.as_str())
                    );
                    n["canDelete"] = json!(
                        index.complete
                            && relation.is_some_and(|r| r.complete && r.items.is_empty())
                            && n["system"] != true
                    );
                    emit(n);
                    if !expanding(&key) {
                        continue;
                    }
                    match relation {
                        Some(r) if r.phase == "ready" => {
                            let mut parents = BTreeSet::new();
                            for (index, item) in r.items.iter().enumerate() {
                                let (ref_parent, ref_depth) = if kind == Kind::Container {
                                    let group = item["group"].as_str().unwrap_or("mount");
                                    let p = format!("{key}/relation/{group}");
                                    if parents.insert(p.clone()) {
                                        emit(node(
                                            &p,
                                            &key,
                                            depth + 1,
                                            "relation-group",
                                            match group {
                                                "image" => "镜像",
                                                "network" => "网络",
                                                _ => "挂载",
                                            },
                                            "",
                                            true,
                                            None,
                                        ));
                                    }
                                    if !expanding(&p) {
                                        continue;
                                    }
                                    (p, depth + 2)
                                } else {
                                    (key.clone(), depth + 1)
                                };
                                let target = ResourceRef::read(&item["resource"]).ok();
                                emit(node(
                                    &format!("{key}/ref/{index}"),
                                    &ref_parent,
                                    ref_depth,
                                    if target.is_some() {
                                        "reference"
                                    } else {
                                        "mount"
                                    },
                                    item["label"].as_str().unwrap_or("引用"),
                                    if target.is_some() {
                                        "定位"
                                    } else {
                                        item["mountType"].as_str().unwrap_or("挂载")
                                    },
                                    false,
                                    target.as_ref(),
                                ));
                            }
                            if r.items.is_empty() {
                                emit(node(
                                    &format!("{key}/empty"),
                                    &key,
                                    depth + 1,
                                    "hint",
                                    "没有容器引用",
                                    "",
                                    false,
                                    None,
                                ));
                            }
                        }
                        r => {
                            let mut n = node(
                                &format!("{key}/load"),
                                &key,
                                depth + 1,
                                "load",
                                if r.is_some_and(|r| r.phase == "loading") {
                                    "正在读取关系"
                                } else if r.is_some_and(|r| r.phase == "failed") {
                                    "读取失败 · 重试"
                                } else {
                                    "读取资源关系"
                                },
                                r.and_then(|r| r.error.as_deref()).unwrap_or(""),
                                false,
                                Some(&resource),
                            );
                            n["loading"] = json!(r.is_some_and(|r| r.phase == "loading"));
                            emit(n);
                        }
                    }
                }
            }
            if index.rows.is_empty() || !index.complete {
                let mut n = node(
                    &format!("{category}/hint"),
                    &category,
                    2,
                    "load",
                    if index.phase == "loading" {
                        "正在读取"
                    } else if index.complete {
                        "暂无资源"
                    } else {
                        "读取此分类 / 重试"
                    },
                    index.error.as_deref().unwrap_or(""),
                    false,
                    None,
                );
                n["branchKind"] = json!(kind.name());
                n["loading"] = json!(index.phase == "loading");
                emit(n);
            }
        }
    }
}
pub fn bytes(value: &Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}
pub fn navigation(value: &Value, old: &Value) -> Result<Value, String> {
    if bytes(value) > 8192 {
        return Err("树导航状态超过 8 KiB".into());
    }
    let mut v = value.clone();
    if v.as_object().is_none() {
        return Err("导航状态无效".into());
    }
    v["schemaVersion"] = json!(2);
    v["query"] = json!(commands::text(v["query"].as_str().unwrap_or(""), 1024));
    if ![
        "all",
        "running",
        "paused",
        "exited",
        "abnormal",
        "created",
        "dead",
        "restarting",
    ]
    .contains(&v["status"].as_str().unwrap_or("all"))
    {
        return Err("状态筛选无效".into());
    }
    v["status"] = json!(v["status"].as_str().unwrap_or("all"));
    v["health"] = json!(v["health"].as_str().unwrap_or("all"));
    v["group"] = json!(v["group"].as_bool().unwrap_or(true));
    let expanded = v["expandedKeys"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!("engine"), json!("category:container")]);
    if expanded.len() > 64
        || expanded
            .iter()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 1024))
    {
        return Err("展开节点超过上限或标识无效".into());
    }
    v["expandedKeys"] = json!(expanded);
    if v["selected"].is_null() {
        v["selected"] = old["selected"].clone();
    }
    if v["selected"]
        .as_str()
        .is_some_and(|s| commands::id(s).is_ok())
    {
        v["selected"] = json!(format!("container:{}", v["selected"].as_str().unwrap()));
    }
    if !v["revealTarget"].is_null() {
        ResourceRef::read(&v["revealTarget"])?;
    }
    if bytes(&v) > 8192 {
        return Err("树导航状态超过 8 KiB".into());
    }
    Ok(v)
}
fn node(
    key: &str,
    parent: &str,
    depth: usize,
    kind: &str,
    label: &str,
    meta: &str,
    expandable: bool,
    resource: Option<&ResourceRef>,
) -> Value {
    json!({"key":key,"parent":parent,"depth":depth,"type":kind,"label":commands::text(label,512),"meta":commands::text(meta,256),"expandable":expandable,"resource":resource})
}
fn short(s: &str) -> &str {
    s.get(..s.len().min(if s.starts_with("sha256:") { 19 } else { 12 }))
        .unwrap_or(s)
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"._-".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn status(r: &Value) -> String {
    let state = match r["status"].as_str().unwrap_or("") {
        "running" => "运行中",
        "paused" => "已暂停",
        "exited" => "已退出",
        "created" => "未启动",
        "restarting" => "重启中",
        "dead" => "异常",
        s => s,
    };
    let health = match r["health"].as_str().unwrap_or("") {
        "healthy" => " · 健康",
        "unhealthy" => " · 不健康",
        "starting" => " · 健康检查中",
        _ => "",
    };
    format!(
        "{state}{health}{}",
        if r["status"] == "exited" {
            r["exitCode"]
                .as_i64()
                .filter(|n| *n != 0)
                .map(|n| format!(" · {n}"))
                .unwrap_or_default()
        } else {
            String::new()
        }
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_groups_filters_and_identity() {
        let mut b = BrowserState::default();
        let i = b.indexes.get_mut(&Kind::Container).unwrap();
        i.complete = true;
        i.phase = "ready".into();
        i.rows = vec![
            json!({"id":"a".repeat(64),"name":"shop-api","project":"shop","service":"api","status":"running","imageId":format!("sha256:{}","b".repeat(64))}),
        ];
        let engine = json!({"name":"prod","dockerRootDir":"/custom/docker"});
        let page = b.page(&engine, &json!({"query":"shop","group":true}), 16000);
        assert!(page["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["label"] == "shop-api"));
        assert!(page["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["type"] == "root"));
        let target = ResourceRef {
            kind: Kind::Container,
            id: "a".repeat(64),
        };
        assert_eq!(b.locate(&engine, &target)["found"], true);
        let work = b.load(Kind::Container).unwrap();
        b.invalidate();
        assert!(!b.owns(&work));
    }
    #[test]
    fn refresh_keeps_anchor_when_rows_are_inserted() {
        let mut b = BrowserState::default();
        let index = b.indexes.get_mut(&Kind::Container).unwrap();
        index.complete = true;
        index.phase = "ready".into();
        index.rows = (0..150).map(|n| json!({"id":format!("{n:064x}"),"name":format!("container-{n:03}"),"status":"exited"})).collect();
        let engine = json!({"name":"prod","dockerRootDir":"/data/docker"});
        let mut nav =
            json!({"offset":70,"expandedKeys":["engine","category:container"],"group":false});
        let first = b.page(&engine, &nav, 16000);
        let key = first["nodes"][0]["key"].clone();
        nav["anchor"] = json!({"key":key,"index":first["offset"]});
        b.indexes.get_mut(&Kind::Container).unwrap().rows.insert(
            0,
            json!({"id":"f".repeat(64),"name":"added","status":"exited"}),
        );
        let refreshed = b.page(&engine, &nav, 16000);
        assert_eq!(refreshed["offset"], 71);
        assert_eq!(refreshed["nodes"][0]["key"], key);
    }
}
