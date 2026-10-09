//! The group tree: `EDA_GROUP`'s parent-and-members relation (`common/eda_group.cpp`, `pcbnew/pcb_group.cpp`) read off
//! `DrawingsSection::groups`.
//!
//! A [`Group`] lists the ids of its direct members, and a member may be another group (`EDA_GROUP::m_items` holds any
//! `EDA_ITEM`, groups included), so groups nest. An item has at most one parent group (`EDA_GROUP::AddItem` takes it out of
//! the one it was in), and the parent chain never loops; the readers here do not rely on the second rule -- a loop in a file
//! made by hand ends the walk instead of hanging it.

use crate::ir::{DrawingsSection, Group};
use std::collections::BTreeSet;

impl DrawingsSection {
    /// The group with this id.
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Whether `id` names a group.
    pub fn is_group(&self, id: &str) -> bool {
        self.groups.iter().any(|g| g.id == id)
    }

    /// `EDA_ITEM::GetParentGroup()`: the group that lists `id` as one of its members.
    pub fn parent_group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.member_ids.iter().any(|m| m == id))
    }

    /// Every item below the group `id`, nested groups opened: each member that is not itself a group, in member order, once.
    /// `id` that names no group gives nothing. (`PCB_GROUP::RunOnChildren( .., RECURSE )` without the groups themselves.)
    pub fn group_leaves(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        self.collect_leaves(id, &mut seen, &mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, id: &'a str, seen: &mut BTreeSet<&'a str>, out: &mut Vec<String>) {
        let Some(g) = self.group(id) else { return };
        if !seen.insert(g.id.as_str()) {
            return;
        }
        for m in &g.member_ids {
            if self.is_group(m) {
                self.collect_leaves(m, seen, out);
            } else if !out.contains(m) {
                out.push(m.clone());
            }
        }
    }

    /// `ids` with every group replaced by the items below it: what a move, a turn, a flip or a delete of those ids acts on.
    /// An id that is not a group stays as it is; each item once, in order.
    pub fn expand_groups(&self, ids: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for id in ids {
            if self.is_group(id) {
                for leaf in self.group_leaves(id) {
                    if !out.contains(&leaf) {
                        out.push(leaf);
                    }
                }
            } else if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }

    /// Whether the group `inner` is the group `outer` or lies somewhere below it.
    pub fn group_holds(&self, outer: &str, inner: &str) -> bool {
        let mut at = inner;
        let mut hops = 0;
        loop {
            if at == outer {
                return true;
            }
            match self.parent_group(at) {
                Some(p) if hops <= self.groups.len() => {
                    at = p.id.as_str();
                    hops += 1;
                }
                _ => return false,
            }
        }
    }

    /// The outermost group above `id` (`PCB_GROUP::TopLevelGroup` with no group entered), or `None` for an item in no group.
    pub fn top_group(&self, id: &str) -> Option<&Group> {
        let mut top = self.parent_group(id)?;
        let mut hops = 0;
        while let Some(p) = self.parent_group(&top.id) {
            if hops > self.groups.len() {
                break;
            }
            top = p;
            hops += 1;
        }
        Some(top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(id: &str, members: &[&str]) -> Group {
        Group { id: id.into(), name: String::new(), member_ids: members.iter().map(|m| m.to_string()).collect() }
    }

    fn drawings(groups: Vec<Group>) -> DrawingsSection {
        DrawingsSection { groups, ..Default::default() }
    }

    #[test]
    fn leaves_open_nested_groups_in_member_order() {
        let d = drawings(vec![g("outer", &["a", "inner", "b"]), g("inner", &["c", "d"])]);
        assert_eq!(d.group_leaves("outer"), vec!["a", "c", "d", "b"]);
        assert_eq!(d.group_leaves("inner"), vec!["c", "d"]);
        assert!(d.group_leaves("a").is_empty(), "an item is not a group");
    }

    #[test]
    fn a_loop_made_by_hand_ends_the_walk() {
        let d = drawings(vec![g("x", &["a", "y"]), g("y", &["b", "x"])]);
        assert_eq!(d.group_leaves("x"), vec!["a", "b"]);
        assert!(d.group_holds("x", "y") && d.group_holds("y", "x"));
        assert!(d.top_group("a").is_some());
    }

    #[test]
    fn parents_and_the_top_group() {
        let d = drawings(vec![g("outer", &["a", "inner"]), g("inner", &["c", "d"]), g("lone", &["e", "f"])]);
        assert_eq!(d.parent_group("c").map(|p| p.id.as_str()), Some("inner"));
        assert_eq!(d.parent_group("inner").map(|p| p.id.as_str()), Some("outer"));
        assert!(d.parent_group("outer").is_none());
        assert_eq!(d.top_group("c").map(|p| p.id.as_str()), Some("outer"));
        assert_eq!(d.top_group("e").map(|p| p.id.as_str()), Some("lone"));
        assert!(d.top_group("zzz").is_none());
        assert!(d.group_holds("outer", "inner") && d.group_holds("outer", "outer") && !d.group_holds("inner", "outer"));
    }

    #[test]
    fn expanding_replaces_each_group_by_its_items_once() {
        let d = drawings(vec![g("outer", &["a", "inner"]), g("inner", &["c", "d"])]);
        let ids: Vec<String> = ["x", "outer", "c", "y"].iter().map(|s| s.to_string()).collect();
        assert_eq!(d.expand_groups(&ids), vec!["x", "a", "c", "d", "y"]);
    }
}
