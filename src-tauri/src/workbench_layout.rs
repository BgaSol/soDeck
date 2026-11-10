//! Pure host layout. A layout never creates or disposes a plugin resource.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Node {
    Group {
        #[serde(rename = "groupId")]
        group_id: String,
    },
    Split {
        id: String,
        axis: String,
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}
impl Node {
    pub fn minimum(&self) -> (f64, f64) {
        match self {
            Self::Group { .. } => (280., 180.),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let (aw, ah) = first.minimum();
                let (bw, bh) = second.minimum();
                if axis == "x" {
                    (aw + bw + 6., ah.max(bh))
                } else {
                    (aw.max(bw), ah + bh + 6.)
                }
            }
        }
    }
    fn graft(&mut self, group: &str, other: &Node, id: &str, axis: &str, before: bool) -> bool {
        match self {
            Self::Group { group_id } if group_id == group => {
                let original = self.clone();
                let (first, second) = if before {
                    (other.clone(), original)
                } else {
                    (original, other.clone())
                };
                *self = Self::Split {
                    id: id.into(),
                    axis: axis.into(),
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.graft(group, other, id, axis, before)
                    || second.graft(group, other, id, axis, before)
            }
            _ => false,
        }
    }
    pub fn groups(&self, out: &mut Vec<String>) {
        match self {
            Self::Group { group_id } => out.push(group_id.clone()),
            Self::Split { first, second, .. } => {
                first.groups(out);
                second.groups(out);
            }
        }
    }
    fn split(&mut self, group: &str, other: &str, id: &str, axis: &str, before: bool) -> bool {
        match self {
            Self::Group { group_id } if group_id == group => {
                let original = self.clone();
                let added = Self::Group {
                    group_id: other.into(),
                };
                let (first, second) = if before {
                    (added, original)
                } else {
                    (original, added)
                };
                *self = Self::Split {
                    id: id.into(),
                    axis: axis.into(),
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split(group, other, id, axis, before)
                    || second.split(group, other, id, axis, before)
            }
            _ => false,
        }
    }
    fn without(self, group: &str) -> Option<Self> {
        match self {
            Self::Group { group_id } => (group_id != group).then_some(Self::Group { group_id }),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => match (first.without(group), second.without(group)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    id,
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (first, second) => first.or(second),
            },
        }
    }
    fn resize(&mut self, split: &str, value: f64) -> bool {
        match self {
            Self::Split { id, ratio, .. } if id == split => {
                *ratio = value;
                true
            }
            Self::Split { first, second, .. } => {
                first.resize(split, value) || second.resize(split, value)
            }
            _ => false,
        }
    }
    fn validate(
        &self,
        depth: usize,
        nodes: &mut HashSet<String>,
        groups: &mut Vec<String>,
    ) -> Result<(), String> {
        if depth > 8 {
            return Err("分屏层级不能超过 8 层".into());
        }
        match self {
            Self::Group { group_id } => groups.push(group_id.clone()),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                if !nodes.insert(id.clone())
                    || !["x", "y"].contains(&axis.as_str())
                    || !ratio.is_finite()
                    || !(0.05..=0.95).contains(ratio)
                {
                    return Err("分屏节点无效".into());
                }
                first.validate(depth + 1, nodes, groups)?;
                second.validate(depth + 1, nodes, groups)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub view_ids: Vec<String>,
    pub active_view_id: Option<String>,
    pub explicit_empty: bool,
}
impl Group {
    fn new(id: String, explicit_empty: bool) -> Self {
        Self {
            id,
            view_ids: vec![],
            active_view_id: None,
            explicit_empty,
        }
    }
    fn remove(&mut self, view: &str) {
        let index = self.view_ids.iter().position(|id| id == view).unwrap_or(0);
        self.view_ids.retain(|id| id != view);
        if self.active_view_id.as_deref() == Some(view) {
            self.active_view_id = self
                .view_ids
                .get(index.min(self.view_ids.len().saturating_sub(1)))
                .cloned();
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolSlot {
    pub definition_key: String,
    pub view_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub entries: Vec<ToolSlot>,
    pub active_view_id: Option<String>,
}
impl Tools {
    pub fn view_ids(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|e| e.view_id.clone())
            .collect()
    }
}
impl Serialize for Tools {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Tools", 3)?;
        s.serialize_field("entries", &self.entries)?;
        s.serialize_field("viewIds", &self.view_ids())?;
        s.serialize_field("activeViewId", &self.active_view_id)?;
        s.end()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub layout_slot: u32,
    pub bounds: Option<crate::workbench_memory::Bounds>,
    pub layout: crate::workspaces::Layout,
    pub center: Node,
    pub groups: BTreeMap<String, Group>,
    pub left: Tools,
    pub right: Tools,
    pub dialogs: Vec<String>,
    pub focused_group_id: String,
    pub zoom_group_id: Option<String>,
    pub revision: u64,
}
impl Host {
    pub(crate) fn preserve_focus(&mut self, before: &Self) {
        if self.groups.contains_key(&before.focused_group_id) {
            self.focused_group_id = before.focused_group_id.clone();
        }
        for (id, g) in &mut self.groups {
            if let Some(b) = before.groups.get(id) {
                if b.active_view_id
                    .as_ref()
                    .is_none_or(|v| g.view_ids.contains(v))
                {
                    g.active_view_id = b.active_view_id.clone();
                }
            }
        }
        self.left.active_view_id = before.left.active_view_id.clone();
        self.right.active_view_id = before.right.active_view_id.clone();
    }
    pub fn insert_portion(
        &mut self,
        other: Self,
        group: &str,
        axis: &str,
        before: bool,
        split_id: String,
    ) -> Result<(), String> {
        let mut next = self.clone();
        if !["x", "y"].contains(&axis)
            || next.groups.keys().any(|id| other.groups.contains_key(id))
            || !next
                .center
                .graft(group, &other.center, &split_id, axis, before)
        {
            return Err("标签组落点无效".into());
        }
        next.groups.extend(other.groups);
        next.focused_group_id = other.focused_group_id;
        next.zoom_group_id = None;
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn relocate_group(
        &mut self,
        source: &str,
        target: &str,
        edge: &str,
        split_id: String,
    ) -> Result<(), String> {
        if source == target {
            return Ok(());
        }
        if ["center", "tab"].contains(&edge) {
            return self.merge_group(source, target);
        }
        let mut next = self.clone();
        let g = next.remove_group(source)?;
        let mut portion = Host::new(format!("portion-{split_id}"));
        portion.groups.clear();
        portion.groups.insert(source.into(), g);
        portion.center = Node::Group {
            group_id: source.into(),
        };
        portion.focused_group_id = source.into();
        next.insert_portion(
            portion,
            target,
            if ["left", "right"].contains(&edge) {
                "x"
            } else {
                "y"
            },
            ["left", "top"].contains(&edge),
            split_id,
        )?;
        *self = next;
        Ok(())
    }
    pub fn new(id: String) -> Self {
        let group = format!("{id}-root");
        Self {
            id,
            layout_slot: 0,
            bounds: None,
            layout: Default::default(),
            center: Node::Group {
                group_id: group.clone(),
            },
            groups: BTreeMap::from([(group.clone(), Group::new(group.clone(), true))]),
            left: Tools::default(),
            right: Tools::default(),
            dialogs: vec![],
            focused_group_id: group,
            zoom_group_id: None,
            revision: 0,
        }
    }
    pub fn ordered_groups(&self) -> Vec<String> {
        let mut groups = vec![];
        self.center.groups(&mut groups);
        groups
    }
    pub fn view_ids(&self) -> Vec<String> {
        self.ordered_groups()
            .iter()
            .flat_map(|id| self.groups[id].view_ids.clone())
            .chain(self.left.view_ids())
            .chain(self.right.view_ids())
            .chain(self.dialogs.clone())
            .collect()
    }
    pub fn contains(&self, view: &str) -> bool {
        self.view_ids().iter().any(|id| id == view)
    }
    pub fn group_of(&self, view: &str) -> Option<String> {
        self.groups
            .values()
            .find(|g| g.view_ids.iter().any(|id| id == view))
            .map(|g| g.id.clone())
    }
    pub fn active_view(&self) -> Option<String> {
        self.groups
            .get(&self.focused_group_id)
            .and_then(|g| g.active_view_id.clone())
    }
    pub fn split(
        &mut self,
        group: &str,
        axis: &str,
        new_group: String,
        split_id: String,
        before: bool,
    ) -> Result<(), String> {
        if !["x", "y"].contains(&axis)
            || !self.groups.contains_key(group)
            || self.groups.contains_key(&new_group)
            || self.groups.len() >= 16
        {
            return Err("分屏目标、方向或组数量无效".into());
        }
        let mut next = self.clone();
        next.center
            .split(group, &new_group, &split_id, axis, before);
        next.groups
            .insert(new_group.clone(), Group::new(new_group.clone(), true));
        next.focused_group_id = new_group;
        next.zoom_group_id = None;
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn remove(&mut self, view: &str) {
        self.dialogs.retain(|id| id != view);
        for g in self.groups.values_mut() {
            g.remove(view);
        }
        for tools in [&mut self.left, &mut self.right] {
            tools.entries.retain(|e| e.view_id.as_deref() != Some(view));
            if tools.active_view_id.as_deref() == Some(view) {
                tools.active_view_id = None;
            }
        }
        let empty = self
            .groups
            .values()
            .filter(|g| g.view_ids.is_empty() && !g.explicit_empty)
            .map(|g| g.id.clone())
            .collect::<Vec<_>>();
        for id in empty {
            if self.groups.len() > 1 {
                let _ = self.remove_group(&id);
            }
        }
        self.revision += 1;
    }
    pub fn place(
        &mut self,
        view: &str,
        group: &str,
        index: usize,
        activate: bool,
    ) -> Result<(), String> {
        if !self.groups.contains_key(group) {
            return Err("目标标签组已关闭".into());
        }
        // Keep the destination while removing its existing occurrence.
        for g in self.groups.values_mut() {
            g.remove(view);
        }
        for tools in [&mut self.left, &mut self.right] {
            tools.entries.retain(|e| e.view_id.as_deref() != Some(view));
            if tools.active_view_id.as_deref() == Some(view) {
                tools.active_view_id = None;
            }
        }
        let g = self.groups.get_mut(group).unwrap();
        g.view_ids.insert(index.min(g.view_ids.len()), view.into());
        g.explicit_empty = false;
        if activate || g.active_view_id.is_none() {
            g.active_view_id = Some(view.into());
        }
        if activate {
            self.focused_group_id = group.into();
        }
        self.prune();
        self.revision += 1;
        Ok(())
    }
    pub fn tool_key(&self, view: &str) -> Option<&str> {
        self.left
            .entries
            .iter()
            .chain(&self.right.entries)
            .find(|e| e.view_id.as_deref() == Some(view))
            .map(|e| e.definition_key.as_str())
    }
    pub fn tool_side(&self, key: &str) -> Option<&str> {
        if self.left.entries.iter().any(|e| e.definition_key == key) {
            Some("left")
        } else if self.right.entries.iter().any(|e| e.definition_key == key) {
            Some("right")
        } else {
            None
        }
    }
    pub fn dock(
        &mut self,
        view: &str,
        key: &str,
        side: &str,
        activate: bool,
    ) -> Result<(), String> {
        if !["left", "right"].contains(&side) {
            return Err("无效停靠侧".into());
        }
        if key.is_empty()
            || key.len() > 256
            || (self.tool_side(key).is_none()
                && self.left.entries.len() + self.right.entries.len() >= 128)
        {
            return Err("工具槽位无效或超出额度".into());
        }
        let index = if side == "left" {
            &self.left
        } else {
            &self.right
        }
        .entries
        .iter()
        .position(|e| e.definition_key == key);
        self.remove(view);
        self.left.entries.retain(|e| e.definition_key != key);
        self.right.entries.retain(|e| e.definition_key != key);
        let tools = if side == "left" {
            &mut self.left
        } else {
            &mut self.right
        };
        tools.entries.insert(
            index
                .unwrap_or(tools.entries.len())
                .min(tools.entries.len()),
            ToolSlot {
                definition_key: key.into(),
                view_id: Some(view.into()),
            },
        );
        if activate {
            tools.active_view_id = Some(view.into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn move_tool(&mut self, key: &str, side: &str, before: Option<&str>) -> Result<(), String> {
        let mut next = self.clone();
        next.move_tool_inner(key, side, before)?;
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn move_tool_inner(
        &mut self,
        key: &str,
        side: &str,
        before: Option<&str>,
    ) -> Result<(), String> {
        if !["left", "right"].contains(&side) {
            return Err("无效停靠侧".into());
        }
        let source = self.tool_side(key).ok_or("工具位置已改变")?.to_owned();
        let original = self.clone();
        let from = if source == "left" {
            &mut self.left
        } else {
            &mut self.right
        };
        let at = from
            .entries
            .iter()
            .position(|e| e.definition_key == key)
            .unwrap();
        let entry = from.entries.remove(at);
        let active = entry.view_id.is_some() && from.active_view_id == entry.view_id;
        if source != side && active {
            from.active_view_id = None;
        }
        let target = if side == "left" {
            &mut self.left
        } else {
            &mut self.right
        };
        let at = match before {
            Some(b) if b == key => at.min(target.entries.len()),
            Some(b) => target
                .entries
                .iter()
                .position(|e| e.definition_key == b)
                .ok_or("目标工具顺序已改变")?,
            None => target.entries.len(),
        };
        if source != side && active {
            target.active_view_id = entry.view_id.clone();
        }
        target.entries.insert(at, entry);
        if self.left != original.left || self.right != original.right {
            self.revision += 1;
        }
        self.validate()
    }
    pub fn select(&mut self, view: &str) -> Result<(), String> {
        if let Some(group) = self.group_of(view) {
            if self.groups[&group].active_view_id.as_deref() == Some(view)
                && self.focused_group_id == group
            {
                return Ok(());
            }
            self.groups.get_mut(&group).unwrap().active_view_id = Some(view.into());
            self.focused_group_id = group;
        } else if self.left.view_ids().iter().any(|id| id == view) {
            if self.left.active_view_id.as_deref() == Some(view) {
                return Ok(());
            }
            self.left.active_view_id = Some(view.into());
        } else if self.right.view_ids().iter().any(|id| id == view) {
            if self.right.active_view_id.as_deref() == Some(view) {
                return Ok(());
            }
            self.right.active_view_id = Some(view.into());
        } else if !self.dialogs.iter().any(|id| id == view) {
            return Err("页面不属于此工作台".into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn remove_group(&mut self, group: &str) -> Result<Group, String> {
        if self.groups.len() == 1 {
            return Err("工作台需保留一个标签组".into());
        }
        let root = self
            .center
            .clone()
            .without(group)
            .ok_or("不能移除最后一个组")?;
        let removed = self.groups.remove(group).ok_or("标签组已关闭")?;
        self.center = root;
        if !self.groups.contains_key(&self.focused_group_id) {
            self.focused_group_id = self.ordered_groups()[0].clone();
        }
        if self.zoom_group_id.as_deref() == Some(group) {
            self.zoom_group_id = None;
        }
        self.revision += 1;
        Ok(removed)
    }
    fn prune(&mut self) {
        let empty = self
            .groups
            .values()
            .filter(|g| g.view_ids.is_empty() && !g.explicit_empty)
            .map(|g| g.id.clone())
            .collect::<Vec<_>>();
        for id in empty {
            if self.groups.len() > 1 {
                let _ = self.remove_group(&id);
            }
        }
    }
    pub fn resize(&mut self, split: &str, ratio: f64) -> Result<(), String> {
        if !ratio.is_finite()
            || !(0.05..=0.95).contains(&ratio)
            || !self.center.resize(split, ratio)
        {
            return Err("分屏比例或节点无效".into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn merge_group(&mut self, source: &str, target: &str) -> Result<(), String> {
        if source == target || !self.groups.contains_key(target) {
            return Err("合并目标无效".into());
        }
        let mut next = self.clone();
        let g = next.remove_group(source)?;
        let to = next.groups.get_mut(target).unwrap();
        to.view_ids.extend(g.view_ids);
        to.explicit_empty = false;
        if g.active_view_id.is_some() {
            to.active_view_id = g.active_view_id;
        }
        next.focused_group_id = target.into();
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn merge_host(&mut self, other: Self, split_id: String) -> Result<(), String> {
        let mut next = self.clone();
        if next.view_ids().is_empty() && next.groups.len() == 1 {
            next.center = other.center;
            next.groups = other.groups;
            next.focused_group_id = other.focused_group_id;
        } else {
            if next.groups.keys().any(|id| other.groups.contains_key(id)) {
                return Err("标签组标识重复".into());
            }
            next.center = Node::Split {
                id: split_id,
                axis: "x".into(),
                ratio: 0.5,
                first: Box::new(next.center),
                second: Box::new(other.center),
            };
            next.groups.extend(other.groups);
        }
        for (a, b) in [(&mut next.left, other.left), (&mut next.right, other.right)] {
            for entry in b.entries {
                if let Some(existing) = a
                    .entries
                    .iter()
                    .position(|e| e.definition_key == entry.definition_key)
                {
                    if a.entries[existing].view_id.is_some() && entry.view_id.is_some() {
                        return Err("工具存在重复布局位置".into());
                    }
                    if entry.view_id.is_none() {
                        continue;
                    }
                    a.entries.remove(existing);
                }
                a.entries.push(entry);
            }
            if b.active_view_id.is_some() {
                a.active_view_id = b.active_view_id;
            }
        }
        next.zoom_group_id = None;
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut nodes = HashSet::new();
        let mut ids = vec![];
        self.center.validate(0, &mut nodes, &mut ids)?;
        if ids.len() > 16
            || ids.len() != self.groups.len()
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            || ids.iter().any(|id| !self.groups.contains_key(id))
            || !self.groups.contains_key(&self.focused_group_id)
        {
            return Err("标签组归属无效".into());
        }
        let views = self.view_ids();
        if views.iter().collect::<HashSet<_>>().len() != views.len() {
            return Err("页面存在重复布局位置".into());
        }
        if self
            .zoom_group_id
            .as_ref()
            .is_some_and(|id| !self.groups.contains_key(id))
        {
            return Err("放大标签组已关闭".into());
        }
        for (id, g) in &self.groups {
            if id != &g.id
                || g.active_view_id
                    .as_ref()
                    .is_some_and(|id| !g.view_ids.contains(id))
            {
                return Err("活动页面不在标签组中".into());
            }
        }
        for tools in [&self.left, &self.right] {
            if tools
                .active_view_id
                .as_ref()
                .is_some_and(|id| !tools.view_ids().contains(id))
            {
                return Err("活动工具不在停靠组中".into());
            }
        }
        let slots: Vec<_> = self
            .left
            .entries
            .iter()
            .chain(&self.right.entries)
            .collect();
        if slots.len() > 128
            || slots
                .iter()
                .any(|e| e.definition_key.is_empty() || e.definition_key.len() > 256)
            || slots
                .iter()
                .map(|e| &e.definition_key)
                .collect::<HashSet<_>>()
                .len()
                != slots.len()
        {
            return Err("工具槽位无效或重复".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_split_move_and_merge_keep_unique_instances() {
        let mut h = Host::new("h".into());
        let root = h.focused_group_id.clone();
        h.place("a", &root, 0, true).unwrap();
        h.place("b", &root, 1, true).unwrap();
        h.split(&root, "x", "right".into(), "s1".into(), false)
            .unwrap();
        h.place("b", "right", 0, true).unwrap();
        h.split("right", "y", "bottom".into(), "s2".into(), false)
            .unwrap();
        h.place("c", "bottom", 0, true).unwrap();
        h.resize("s2", 0.7).unwrap();
        h.validate().unwrap();
        assert_eq!(
            h.ordered_groups(),
            vec![root.clone(), "right".into(), "bottom".into()]
        );
        h.merge_group("bottom", "right").unwrap();
        assert_eq!(h.groups["right"].view_ids, vec!["b", "c"]);
        assert_eq!(h.active_view().as_deref(), Some("c"));
        h.validate().unwrap();
    }
    #[test]
    fn empty_split_survives_and_close_selects_next_then_previous() {
        let mut h = Host::new("h".into());
        let root = h.focused_group_id.clone();
        for id in ["a", "b", "c"] {
            h.place(id, &root, usize::MAX, true).unwrap();
        }
        h.select("b").unwrap();
        h.remove("b");
        assert_eq!(h.active_view().as_deref(), Some("c"));
        h.split(&root, "y", "empty".into(), "s".into(), false)
            .unwrap();
        h.remove("c");
        assert!(h.groups.contains_key("empty"));
        assert_eq!(h.groups[&root].active_view_id.as_deref(), Some("a"));
        h.validate().unwrap();
    }
    #[test]
    fn docking_moves_without_duplication_and_merge_failure_is_atomic() {
        let mut h = Host::new("h".into());
        let root = h.focused_group_id.clone();
        h.place("tool", &root, 0, true).unwrap();
        h.dock("tool", "test:tool", "right", true).unwrap();
        assert_eq!(h.view_ids(), vec!["tool"]);
        assert!(h.active_view().is_none());
        h.validate().unwrap();
        let before = h.clone();
        assert!(h
            .split(&root, "z", "bad".into(), "s".into(), false)
            .is_err());
        assert_eq!(h, before);
        assert!(h.resize("s", f64::NAN).is_err());
        assert_eq!(h, before);
    }
    #[test]
    fn a_returning_host_replaces_its_empty_origin_with_the_same_group_id() {
        let mut origin = Host::new("origin".into());
        let mut returning = origin.clone();
        let root = returning.focused_group_id.clone();
        returning.place("terminal", &root, 0, true).unwrap();
        returning
            .split(&root, "y", "bottom".into(), "split".into(), false)
            .unwrap();
        returning.place("editor", "bottom", 0, true).unwrap();
        origin
            .merge_host(returning.clone(), "merge".into())
            .unwrap();
        assert_eq!(origin.center, returning.center);
        assert_eq!(origin.view_ids(), returning.view_ids());
        let before = origin.clone();
        assert!(origin.merge_host(returning, "duplicate".into()).is_err());
        assert_eq!(origin, before);
    }
    #[test]
    fn slot_order_and_activity_are_atomic_and_noop_keeps_revision() {
        let mut h = Host::new("h".into());
        h.left.entries.push(ToolSlot {
            definition_key: "a".into(),
            view_id: None,
        });
        h.dock("b-view", "b", "left", true).unwrap();
        h.dock("c-view", "c", "right", true).unwrap();
        let before = h.clone();
        assert!(h.move_tool("a", "right", Some("missing")).is_err());
        assert_eq!(h, before);
        h.move_tool("a", "left", Some("b")).unwrap();
        assert_eq!(h, before);
        h.move_tool("a", "right", Some("c")).unwrap();
        assert_eq!(h.right.active_view_id.as_deref(), Some("c-view"));
        h.move_tool("b", "right", Some("c")).unwrap();
        assert!(h.left.active_view_id.is_none());
        assert_eq!(h.right.active_view_id.as_deref(), Some("b-view"));
        assert_eq!(
            h.right
                .entries
                .iter()
                .map(|e| e.definition_key.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        let before = h.clone();
        h.move_tool("c", "right", None).unwrap();
        assert_eq!(h, before);
    }
}
