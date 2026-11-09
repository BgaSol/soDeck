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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub view_ids: Vec<String>,
    pub active_view_id: Option<String>,
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
            .chain(self.left.view_ids.clone())
            .chain(self.right.view_ids.clone())
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
            tools.view_ids.retain(|id| id != view);
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
            tools.view_ids.retain(|id| id != view);
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
    pub fn dock(&mut self, view: &str, side: &str, activate: bool) -> Result<(), String> {
        if !["left", "right"].contains(&side) {
            return Err("无效停靠侧".into());
        }
        self.remove(view);
        let tools = if side == "left" {
            &mut self.left
        } else {
            &mut self.right
        };
        tools.view_ids.push(view.into());
        if activate {
            tools.active_view_id = Some(view.into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn select(&mut self, view: &str) -> Result<(), String> {
        if let Some(group) = self.group_of(view) {
            self.groups.get_mut(&group).unwrap().active_view_id = Some(view.into());
            self.focused_group_id = group;
        } else if self.left.view_ids.iter().any(|id| id == view) {
            self.left.active_view_id = Some(view.into());
        } else if self.right.view_ids.iter().any(|id| id == view) {
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
            a.view_ids.extend(b.view_ids);
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
                .is_some_and(|id| !tools.view_ids.contains(id))
            {
                return Err("活动工具不在停靠组中".into());
            }
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
        h.dock("tool", "right", true).unwrap();
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
}
