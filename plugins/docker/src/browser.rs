use crate::{
    commands,
    object::ObjectInfo,
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
pub struct ObjectObservation {
    pub generation: u64,
    pub phase: String,
    pub items: Vec<Value>,
    pub info: Option<ObjectInfo>,
    pub stats: Value,
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
    Object {
        resource: ResourceRef,
        generation: u64,
        ids: Option<Vec<String>>,
        offset: usize,
    },
}
pub struct BrowserState {
    pub indexes: BTreeMap<Kind, Index>,
    pub objects: BTreeMap<String, ObjectObservation>,
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
            objects: BTreeMap::new(),
            last_page: json!({"nodes":[],"total":0}),
            visible: true,
            serial: 0,
        }
    }
}
impl BrowserState {
    pub fn trim_objects(&mut self, protected: &str) -> Result<(), String> {
        while self
            .objects
            .values()
            .map(|r| bytes(&json!([r.items, r.observation, r.info, r.stats])))
            .sum::<usize>()
            > 2 * 1024 * 1024
        {
            let victim = self
                .objects
                .iter()
                .filter(|(key, r)| key.as_str() != protected && r.phase != "loading")
                .min_by_key(|(_, r)| r.generation)
                .map(|(key, _)| key.clone())
                .ok_or("对象缓存达到 2 MiB 上限，请等待当前读取完成")?;
            self.objects.remove(&victim);
        }
        Ok(())
    }
    pub fn bytes(&self) -> usize {
        self.indexes
            .values()
            .map(|i| bytes(&json!([i.rows, i.staged])))
            .sum::<usize>()
            + self
                .objects
                .values()
                .map(|r| bytes(&json!([r.items, r.observation, r.info, r.stats])))
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
        for object in self.objects.values_mut() {
            object.phase = "stale".into();
            object.complete = false;
        }
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
    pub fn load_object(&mut self, resource: ResourceRef) -> Result<Option<Work>, String> {
        let key = resource.key();
        if self.objects.get(&key).is_some_and(|r| r.phase == "loading") {
            return Ok(None);
        }
        if self.objects.len() >= 32 && !self.objects.contains_key(&key) {
            let victim = self
                .objects
                .iter()
                .filter(|(_, r)| r.phase != "loading")
                .min_by_key(|(_, r)| r.generation)
                .map(|(k, _)| k.clone())
                .ok_or("对象读取已达上限，请等待当前读取完成")?;
            self.objects.remove(&victim);
        }
        self.serial += 1;
        let previous = self.objects.remove(&key);
        self.objects.insert(
            key,
            ObjectObservation {
                generation: self.serial,
                phase: "loading".into(),
                items: vec![],
                info: previous.as_ref().and_then(|o| o.info.clone()),
                stats: previous.map_or(Value::Null, |o| o.stats),
                observation: Value::Null,
                complete: false,
                error: None,
            },
        );
        Ok(Some(Work::Object {
            resource,
            generation: self.serial,
            ids: None,
            offset: 0,
        }))
    }
    pub fn owns(&self, work: &Work) -> bool {
        self.current(work)
            && match work {
                Work::Index { kind, .. } => self.indexes[kind].phase == "loading",
                Work::Object { resource, .. } => self.objects[&resource.key()].phase == "loading",
            }
    }
    pub fn current(&self, work: &Work) -> bool {
        match work {
            Work::Index {
                kind, generation, ..
            } => {
                self.indexes[kind].generation == *generation && self.indexes[kind].phase != "stale"
            }
            Work::Object {
                resource,
                generation,
                ..
            } => self
                .objects
                .get(&resource.key())
                .is_some_and(|r| r.generation == *generation && r.phase != "stale"),
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
            Work::Object { resource, .. } => {
                if let Some(r) = self.objects.get_mut(&resource.key()) {
                    r.phase = "failed".into();
                    r.error = Some(commands::text(&error, 1024));
                    r.complete = false;
                    r.items.clear();
                    r.info = None;
                    r.stats = Value::Null;
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
        let anchor_key = inline_key(state);
        let mut inline_anchor = Value::Null;
        self.visit_nodes(engine, state, |n| {
            let position = total;
            if n["key"] == anchor_key
                && (state["inline"]["kind"] != "resource"
                    || n["resource"] == state["inline"]["resource"])
            {
                inline_anchor = json!({"index":position,"node":n});
            }
            total += 1;
            if position < offset || filled {
                return;
            }
            let len = bytes(&n) + 1;
            if size + len > budget.saturating_sub(2048) || selected.len() >= 100 {
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
        let page = json!({"nodes":selected,"offset":offset,"total":total,"nextOffset":if offset+selected.len()<total{Some(offset+selected.len())}else{None},"revealed":state["revealTarget"],"signature":signature,"inlineAnchor":inline_anchor});
        self.last_page = page.clone();
        page
    }
    pub fn locate(&self, engine: &Value, resource: &ResourceRef) -> Value {
        let mut path: Vec<String> = vec![];
        let mut result = None;
        let mut position = 0;
        self.visit_nodes(engine,&json!({"query":resource.id,"status":"all","health":"all","group":true,"expandAll":true}),|n| {
            if result.is_some() {return;}
            let depth=n["depth"].as_u64().unwrap_or(0) as usize;path.truncate(depth);
            if n["resource"]==json!(resource) && n["type"]=="resource" {result=Some(json!({"found":true,"index":position,"key":n["key"],"expandedKeys":path,"resource":resource}));}
            path.push(n["key"].as_str().unwrap_or("").into());position+=1;
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
        for (key, label) in [("tool:connection", "连接设置"), ("tool:tasks", "任务")] {
            let mut n = node(key, "engine", 1, "tool", label, "", true, None);
            n["expanded"] = json!(inline_key(state) == key);
            emit(n);
        }
        for kind in Kind::ALL {
            let index = &self.indexes[&kind];
            let category = format!("category:{}", kind.name());
            let meta = match index.phase.as_str() {
                "unloaded" => "展开加载".into(),
                "loading" if !index.rows.is_empty() => format!("{} 个 · 更新中", index.rows.len()),
                "loading" if index.staged.is_empty() => "加载中".into(),
                "loading" => format!("加载中 · 已读 {}", index.staged.len()),
                "failed" => "读取失败 · 可重试".into(),
                "stale" => format!("{} 个 · 待刷新", index.rows.len()),
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
            n["loading"] = json!(index.phase == "loading");
            emit(n);
            if kind == Kind::Image && expanding(&category) {
                let mut n = node(
                    "tool:pull",
                    "category:image",
                    2,
                    "tool",
                    "拉取公开镜像",
                    "",
                    true,
                    None,
                );
                n["expanded"] = json!(inline_key(state) == "tool:pull");
                emit(n);
            }
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
            let no_matches = rows.is_empty() && !index.rows.is_empty();
            let mut groups: BTreeMap<(bool, String), Vec<(&Value, String)>> = BTreeMap::new();
            for row in rows {
                if kind == Kind::Image {
                    let tags = row["tags"].as_array().filter(|a| !a.is_empty());
                    if let Some(tags) = tags {
                        for tag in tags.iter().filter_map(Value::as_str) {
                            let split = tag.rfind(':').filter(|i| !tag[*i..].contains('/'));
                            let (repo, label) =
                                split.map_or((tag, "latest"), |i| (&tag[..i], &tag[i + 1..]));
                            groups
                                .entry((false, repo.into()))
                                .or_default()
                                .push((row, label.into()));
                        }
                    } else {
                        groups
                            .entry((false, "无标签镜像".into()))
                            .or_default()
                            .push((row, short(row["id"].as_str().unwrap_or("")).into()));
                    }
                } else if kind == Kind::Container && state["group"].as_bool().unwrap_or(true) {
                    let project = row["project"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("独立容器");
                    groups
                        .entry((
                            row["project"].as_str().is_some_and(|s| !s.is_empty()),
                            project.into(),
                        ))
                        .or_default()
                        .push((row, row["name"].as_str().unwrap_or("").into()));
                } else {
                    groups
                        .entry((false, String::new()))
                        .or_default()
                        .push((row, row["name"].as_str().unwrap_or("").into()));
                }
            }
            for ((is_compose, group), rows) in groups {
                let parent = if group.is_empty() {
                    category.clone()
                } else {
                    format!(
                        "group:{}:{}{}",
                        kind.name(),
                        if kind == Kind::Container && !is_compose {
                            "standalone:"
                        } else {
                            ""
                        },
                        encode(&group)
                    )
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
                    if is_compose {
                        n["composeProject"] = json!(group);
                        n["composeLocation"] = match crate::compose::directory(
                            index,
                            &group,
                            &engine["locationEnvironment"],
                        ) {
                            Ok(value) => value,
                            Err(error) => json!({"error":error}),
                        };
                        n["observationRevision"] = json!(index.revision.to_string());
                    }
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
                    let relation = self.objects.get(&resource.key());
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
                    let loading = relation.is_some_and(|r| r.phase == "loading");
                    if loading {
                        meta.push_str(" · 信息更新中");
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
                    n["expanded"] = json!(
                        state["inline"]["kind"] == "resource" && state["inline"]["nodeKey"] == key
                    );
                    n["status"] = row["status"].clone();
                    n["health"] = row["health"].clone();
                    n["loading"] = json!(loading);
                    n["revision"] = json!(index.revision.to_string());
                    n["system"] = json!(
                        kind == Kind::Network
                            && ["bridge", "host", "none", "nat", "Default Switch"]
                                .contains(&label.as_str())
                    );
                    n["canDelete"] = json!(
                        index.complete
                            && relation.is_some_and(|r| r.complete && r.items.is_empty())
                            && n["system"] != true
                    );
                    emit(n);
                }
            }
            if index.phase != "loading" && (index.rows.is_empty() || no_matches || !index.complete)
            {
                let mut n = node(
                    &format!("{category}/hint"),
                    &category,
                    2,
                    if index.complete { "hint" } else { "load" },
                    if index.complete {
                        if no_matches {
                            "没有匹配的资源"
                        } else {
                            "暂无资源"
                        }
                    } else if index.phase == "failed" {
                        "读取失败，Enter 重试"
                    } else if index.phase == "stale" {
                        "数据待刷新，Enter 重新读取"
                    } else {
                        "Enter 读取此分类"
                    },
                    index.error.as_deref().unwrap_or(""),
                    false,
                    None,
                );
                n["branchKind"] = json!(kind.name());
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
    v["schemaVersion"] = json!(3);
    let mut inline = v["inline"].clone();
    if !inline.is_null() {
        match inline["kind"].as_str() {
            Some("resource") => {
                let resource = ResourceRef::read(&inline["resource"])?;
                if inline["nodeKey"].as_str().is_none_or(|k| k.len() > 1024) {
                    return Err("行内对象标识无效".into());
                }
                if resource.kind == Kind::Container {
                    inline["tab"] = json!(match inline["tab"].as_str() {
                        Some(tab @ ("ports" | "mounts" | "networks")) => tab,
                        _ => "overview",
                    });
                    let mut offsets = serde_json::Map::new();
                    let mut previous = serde_json::Map::new();
                    for field in ["offsets", "previousOffsets"] {
                        if !inline[field].is_null()
                            && inline[field].as_object().is_none_or(|map| {
                                map.keys().any(|name| {
                                    !["ports", "mounts", "networks"].contains(&name.as_str())
                                })
                            })
                        {
                            return Err("详情分页状态无效".into());
                        }
                    }
                    for name in ["ports", "mounts", "networks"] {
                        let offset = if inline["offsets"][name].is_null() {
                            0
                        } else {
                            inline["offsets"][name]
                                .as_u64()
                                .filter(|offset| *offset < 100)
                                .ok_or("详情分页位置无效")?
                        };
                        let history = if inline["previousOffsets"][name].is_null() {
                            vec![]
                        } else {
                            let rows = inline["previousOffsets"][name]
                                .as_array()
                                .ok_or("详情前页状态无效")?;
                            if rows.len() > 100 {
                                return Err("详情前页状态超限".into());
                            }
                            let rows = rows
                                .iter()
                                .map(|value| {
                                    value
                                        .as_u64()
                                        .filter(|value| *value < offset)
                                        .ok_or("详情前页位置无效")
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            if rows.windows(2).any(|pair| pair[0] >= pair[1]) {
                                return Err("详情前页顺序无效".into());
                            }
                            rows
                        };
                        offsets.insert(name.into(), json!(offset));
                        previous.insert(name.into(), json!(history));
                    }
                    inline["offsets"] = json!(offsets);
                    inline["previousOffsets"] = json!(previous);
                } else {
                    for field in ["tab", "offsets", "previousOffsets"] {
                        inline.as_object_mut().unwrap().remove(field);
                    }
                }
            }
            Some("connection" | "pull" | "tasks") => {}
            _ => return Err("行内区域类型无效".into()),
        }
    }
    v["inline"] = inline;
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
    v["expandedKeys"] = json!(expanded
        .into_iter()
        .filter(|key| key.as_str().is_some_and(|k| k == "engine"
            || k.starts_with("category:")
            || k.starts_with("group:")))
        .collect::<Vec<_>>());
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
pub fn inline_key(state: &Value) -> &str {
    match state["inline"]["kind"].as_str() {
        Some("resource") => state["inline"]["nodeKey"].as_str().unwrap_or(""),
        Some("connection") => "tool:connection",
        Some("pull") => "tool:pull",
        Some("tasks") => "tool:tasks",
        _ => "",
    }
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
    fn completed_work_is_current_until_invalidated() {
        let mut b = BrowserState::default();
        let index = b.load(Kind::Container).unwrap();
        let resource = ResourceRef {
            kind: Kind::Container,
            id: "a".repeat(64),
        };
        let object = b.load_object(resource.clone()).unwrap().unwrap();
        assert!(b.owns(&index) && b.owns(&object));
        b.indexes.get_mut(&Kind::Container).unwrap().phase = "ready".into();
        b.objects.get_mut(&resource.key()).unwrap().phase = "ready".into();
        assert!(!b.owns(&index) && !b.owns(&object));
        assert!(b.current(&index) && b.current(&object));
        b.invalidate();
        assert!(!b.current(&index) && !b.current(&object));
        b.load(Kind::Container);
        b.load_object(resource).unwrap();
        assert!(!b.current(&index) && !b.current(&object));
    }
    #[test]
    fn loading_is_parent_state_and_keeps_cached_resources() {
        let mut b = BrowserState::default();
        let resource = ResourceRef {
            kind: Kind::Container,
            id: "a".repeat(64),
        };
        let index = b.indexes.get_mut(&Kind::Container).unwrap();
        index.rows = vec![json!({"id":resource.id,"name":"api","status":"running"})];
        b.load(Kind::Container);
        b.load(Kind::Volume);
        b.load_object(resource.clone()).unwrap();
        let nav = json!({"expandAll":true,"group":false});
        let page = b.page(&json!({"name":"prod"}), &nav, 16000);
        let nodes = page["nodes"].as_array().unwrap();
        assert!(!nodes
            .iter()
            .any(|n| n["type"] == "load" && n["loading"] == true));
        let category = nodes
            .iter()
            .find(|n| n["key"] == "category:container")
            .unwrap();
        assert_eq!(category["meta"], "1 个 · 更新中");
        assert_eq!(category["loading"], true);
        let row = nodes.iter().find(|n| n["key"] == resource.key()).unwrap();
        assert_eq!(row["loading"], true);
        assert!(row["meta"].as_str().unwrap().contains("信息更新中"));
        let volume = nodes
            .iter()
            .find(|n| n["key"] == "category:volume")
            .unwrap();
        assert_eq!(volume["meta"], "加载中");
        b.indexes.get_mut(&Kind::Volume).unwrap().staged = vec![json!({"id":"data"})];
        let page = b.page(&json!({"name":"prod"}), &nav, 16000);
        assert_eq!(
            page["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["key"] == "category:volume")
                .unwrap()["meta"],
            "加载中 · 已读 1"
        );
    }
    #[test]
    fn empty_and_failed_indexes_remain_distinct() {
        let mut b = BrowserState::default();
        let work = b.load(Kind::Volume).unwrap();
        b.fail(&work, "socket unavailable".into());
        let nav = json!({"expandAll":true,"group":false,"query":"missing"});
        let image = b.indexes.get_mut(&Kind::Image).unwrap();
        image.phase = "ready".into();
        image.complete = true;
        let container = b.indexes.get_mut(&Kind::Container).unwrap();
        container.phase = "ready".into();
        container.complete = true;
        container.rows = vec![json!({"id":"a".repeat(64),"name":"api"})];
        let page = b.page(&json!({"name":"prod"}), &nav, 16000);
        let nodes = page["nodes"].as_array().unwrap();
        let failed = nodes
            .iter()
            .find(|n| n["key"] == "category:volume/hint")
            .unwrap();
        assert_eq!(failed["type"], "load");
        assert_eq!(failed["meta"], "socket unavailable");
        assert_eq!(failed["label"], "读取失败，Enter 重试");
        let empty = nodes
            .iter()
            .find(|n| n["key"] == "category:image/hint")
            .unwrap();
        assert_eq!(empty["type"], "hint");
        assert_eq!(empty["label"], "暂无资源");
        let filtered = nodes
            .iter()
            .find(|n| n["key"] == "category:container/hint")
            .unwrap();
        assert_eq!(filtered["type"], "hint");
        assert_eq!(filtered["label"], "没有匹配的资源");
    }
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
    fn one_inline_object_keeps_the_tree_skeleton_flat_and_locates_absolute_rows() {
        let mut b = BrowserState::default();
        let id = "a".repeat(64);
        let other = "b".repeat(64);
        let index = b.indexes.get_mut(&Kind::Container).unwrap();
        index.complete = true;
        index.phase = "ready".into();
        index.rows = vec![
            json!({"id":id,"name":"first"}),
            json!({"id":other,"name":"second"}),
        ];
        let nav = json!({"expandedKeys":["engine","category:container"],"group":false,"inline":{"kind":"resource","nodeKey":format!("container:{other}"),"resource":{"kind":"container","id":other}}});
        let page = b.page(&json!({}), &nav, 16000);
        let nodes = page["nodes"].as_array().unwrap();
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n["type"] == "resource" && n["expanded"] == true)
                .count(),
            1
        );
        assert!(!nodes
            .iter()
            .any(|n| matches!(n["type"].as_str(), Some("relation-group" | "reference"))));
        let found = b.locate(
            &json!({}),
            &ResourceRef {
                kind: Kind::Container,
                id: other,
            },
        );
        assert_eq!(found["key"], format!("container:{}", "b".repeat(64)));
        assert!(found["index"].as_u64().is_some());
        assert_eq!(page["inlineAnchor"]["node"]["key"], found["key"]);
    }
    #[test]
    fn old_navigation_keeps_preferences_but_discards_resource_subgroups() {
        let nav=navigation(&json!({"schemaVersion":2,"selected":"a".repeat(64),"shell":"/bin/bash","expandedKeys":["engine","category:container","container:old/relation/network"],"query":"api"}),&Value::Null).unwrap();
        assert_eq!(nav["schemaVersion"], 3);
        assert_eq!(nav["shell"], "/bin/bash");
        assert_eq!(nav["expandedKeys"].as_array().unwrap().len(), 2);
        assert_eq!(nav["selected"], format!("container:{}", "a".repeat(64)));
    }
    #[test]
    fn container_detail_navigation_restores_tabs_and_actual_page_cursors() {
        let id = "a".repeat(64);
        let mut input = json!({"inline":{"kind":"resource","nodeKey":format!("container:{id}"),"resource":{"kind":"container","id":id},"tab":"mounts","offsets":{"mounts":3},"previousOffsets":{"mounts":[0,1]}}});
        let nav = navigation(&input, &Value::Null).unwrap();
        assert_eq!(nav["inline"]["tab"], "mounts");
        assert_eq!(nav["inline"]["previousOffsets"]["mounts"], json!([0, 1]));
        input["inline"]["tab"] = json!("future-tab");
        assert_eq!(
            navigation(&input, &Value::Null).unwrap()["inline"]["tab"],
            "overview"
        );
        input["inline"]["offsets"]["mounts"] = json!(100);
        assert!(navigation(&input, &Value::Null).is_err());
        input["inline"]["offsets"]["mounts"] = json!(3);
        input["inline"]["previousOffsets"]["mounts"] = json!([1, 0]);
        assert!(navigation(&input, &Value::Null).is_err());
        input["inline"]["resource"] = json!({"kind":"volume","id":"data"});
        let nav = navigation(&input, &Value::Null).unwrap();
        assert!(nav["inline"].get("tab").is_none());
        assert!(nav["inline"].get("previousOffsets").is_none());
    }
    #[test]
    fn idle_observation_cache_is_bounded_and_generation_never_reuses_an_old_request() {
        let mut b = BrowserState::default();
        let first = ResourceRef {
            kind: Kind::Volume,
            id: "vol0".into(),
        };
        let old = b.load_object(first.clone()).unwrap().unwrap();
        b.objects.get_mut(&first.key()).unwrap().phase = "ready".into();
        for i in 1..34 {
            let resource = ResourceRef {
                kind: Kind::Volume,
                id: format!("vol{i}"),
            };
            b.load_object(resource.clone()).unwrap();
            b.objects.get_mut(&resource.key()).unwrap().phase = "ready".into();
        }
        assert_eq!(b.objects.len(), 32);
        assert!(!b.objects.contains_key(&first.key()));
        b.load_object(first).unwrap();
        assert!(!b.owns(&old));
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
