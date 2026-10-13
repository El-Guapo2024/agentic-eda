//! The group tree: `EDA_GROUP`'s parent-and-members relation (`common/eda_group.cpp`, `pcbnew/pcb_group.cpp`, `eeschema/sch_group.cpp`) read off a
//! list of [`Group`]s, and the five group verbs (`common/tool/group_tool.cpp`) on that list.
//!
//! A [`Group`] lists the ids of its direct members, and a member may be another group (`EDA_GROUP::m_items` holds any
//! `EDA_ITEM`, groups included), so groups nest. An item has at most one parent group (`EDA_GROUP::AddItem` takes it out of
//! the one it was in), and the parent chain never loops; the readers here do not rely on the second rule -- a loop in a file
//! made by hand ends the walk instead of hanging it.
//!
//! The same tree lives in two places: the board's drawings ([`DrawingsSection::groups`], `PCB_GROUP`) and a schematic sheet's extras
//! ([`SchExtras::groups`], `SCH_GROUP`). [`GroupTree`] reads either; [`GroupHost`] is what both are, so Group, Ungroup, Add Items, Remove Items and
//! Group Properties are written once (`GROUP_TOOL`) and the board's and the sheet's verbs only say which list they act on.

use crate::ir::{DrawingsSection, Group};
use crate::sch_extras::SchExtras;
use std::collections::BTreeSet;

/// The groups of one list read as a tree: who holds whom, what lies below a group, which group a click on an item selects.
#[derive(Clone, Copy)]
pub struct GroupTree<'a>(pub &'a [Group]);

impl<'a> GroupTree<'a> {
    /// The group with this id.
    pub fn group(&self, id: &str) -> Option<&'a Group> {
        self.0.iter().find(|g| g.id == id)
    }

    /// Whether `id` names a group.
    pub fn is_group(&self, id: &str) -> bool {
        self.0.iter().any(|g| g.id == id)
    }

    /// `EDA_ITEM::GetParentGroup()`: the group that lists `id` as one of its members.
    pub fn parent_group(&self, id: &str) -> Option<&'a Group> {
        self.0.iter().find(|g| g.member_ids.iter().any(|m| m == id))
    }

    /// Every item below the group `id`, nested groups opened: each member that is not itself a group, in member order, once.
    /// `id` that names no group gives nothing. (`PCB_GROUP::RunOnChildren( .., RECURSE )` without the groups themselves.)
    pub fn group_leaves(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        self.collect_leaves(id, &mut seen, &mut out);
        out
    }

    fn collect_leaves(&self, id: &str, seen: &mut BTreeSet<&'a str>, out: &mut Vec<String>) {
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

    /// Every group at or below the group `id`, the group itself first (`RunOnChildren( .., RECURSE )` on groups only).
    pub fn group_and_descendants(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut stack = vec![id.to_string()];
        while let Some(at) = stack.pop() {
            let Some(g) = self.group(&at) else { continue };
            if out.contains(&g.id) {
                continue;
            }
            out.push(g.id.clone());
            for m in g.member_ids.iter().rev().filter(|m| self.is_group(m)) {
                stack.push(m.clone());
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
                Some(p) if hops <= self.0.len() => {
                    at = p.id.as_str();
                    hops += 1;
                }
                _ => return false,
            }
        }
    }

    /// The outermost group above `id` (`PCB_GROUP::TopLevelGroup` with no group entered), or `None` for an item in no group.
    pub fn top_group(&self, id: &str) -> Option<&'a Group> {
        let mut top = self.parent_group(id)?;
        let mut hops = 0;
        while let Some(p) = self.parent_group(&top.id) {
            if hops > self.0.len() {
                break;
            }
            top = p;
            hops += 1;
        }
        Some(top)
    }
}

impl DrawingsSection {
    /// The group with this id.
    pub fn group(&self, id: &str) -> Option<&Group> {
        GroupTree(&self.groups).group(id)
    }

    /// Whether `id` names a group.
    pub fn is_group(&self, id: &str) -> bool {
        GroupTree(&self.groups).is_group(id)
    }

    /// `EDA_ITEM::GetParentGroup()`: the group that lists `id` as one of its members.
    pub fn parent_group(&self, id: &str) -> Option<&Group> {
        GroupTree(&self.groups).parent_group(id)
    }

    /// See [`GroupTree::group_leaves`].
    pub fn group_leaves(&self, id: &str) -> Vec<String> {
        GroupTree(&self.groups).group_leaves(id)
    }

    /// See [`GroupTree::expand_groups`].
    pub fn expand_groups(&self, ids: &[String]) -> Vec<String> {
        GroupTree(&self.groups).expand_groups(ids)
    }

    /// See [`GroupTree::group_holds`].
    pub fn group_holds(&self, outer: &str, inner: &str) -> bool {
        GroupTree(&self.groups).group_holds(outer, inner)
    }

    /// See [`GroupTree::top_group`].
    pub fn top_group(&self, id: &str) -> Option<&Group> {
        GroupTree(&self.groups).top_group(id)
    }
}

impl SchExtras {
    /// The groups of this sheet as a tree.
    pub fn group_tree(&self) -> GroupTree<'_> {
        GroupTree(&self.groups)
    }

    /// The group with this id.
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.group_tree().group(id)
    }

    /// Whether `id` names a group of this sheet.
    pub fn is_group(&self, id: &str) -> bool {
        self.group_tree().is_group(id)
    }

    /// `EDA_ITEM::GetParentGroup()`: the group of this sheet that lists `id` as one of its members.
    pub fn parent_group(&self, id: &str) -> Option<&Group> {
        self.group_tree().parent_group(id)
    }

    /// See [`GroupTree::group_leaves`].
    pub fn group_leaves(&self, id: &str) -> Vec<String> {
        self.group_tree().group_leaves(id)
    }

    /// See [`GroupTree::expand_groups`].
    pub fn expand_groups(&self, ids: &[String]) -> Vec<String> {
        self.group_tree().expand_groups(ids)
    }

    /// See [`GroupTree::group_holds`].
    pub fn group_holds(&self, outer: &str, inner: &str) -> bool {
        self.group_tree().group_holds(outer, inner)
    }

    /// See [`GroupTree::top_group`].
    pub fn top_group(&self, id: &str) -> Option<&Group> {
        self.group_tree().top_group(id)
    }

    /// Give every group of the sheet that has no id the deterministic one `next_item_id( "grp", .. )` makes of its members. A group is named
    /// after the groups inside it, so those are named first; ids only have to differ from other groups' (the prefix keeps them off every other item's).
    pub fn assign_missing_group_ids(&mut self) {
        let mut existing: BTreeSet<String> = self.groups.iter().map(|g| g.id.clone()).filter(|s| !s.is_empty()).collect();
        let mut order: Vec<usize> = (0..self.groups.len()).collect();
        order.sort_by(|&a, &b| self.groups[a].id_seed().cmp(&self.groups[b].id_seed()));
        for i in order {
            if self.groups[i].id.is_empty() {
                let id = crate::ir::next_item_id("grp", &self.groups[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.groups[i].id = id;
            }
        }
    }
}

// ---------------------------------------------------------------- the verbs

/// Why a group edit was refused: the check name, the subject it is about and the reason (the ops layer turns it into a `CheckResult`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRefusal {
    pub check: &'static str,
    pub subject: String,
    pub why: String,
}

impl GroupRefusal {
    fn bad_group(subject: &str, why: &str) -> GroupRefusal {
        GroupRefusal { check: "ops_bad_group", subject: subject.to_string(), why: why.to_string() }
    }

    fn unknown_group(id: &str) -> GroupRefusal {
        GroupRefusal { check: "ops_unknown_group", subject: id.to_string(), why: "no group with this id".to_string() }
    }
}

/// A group with fewer than two members is meaningless and dissolves (`GROUP_TOOL::RemoveFromGroup`'s `if( group->GetItems().size() < 2 )`); the
/// group above it loses it as a member, and may fall under two in turn.
pub fn prune_groups(groups: &mut Vec<Group>) {
    loop {
        let doomed: Vec<String> = groups.iter().filter(|g| g.member_ids.len() < 2).map(|g| g.id.clone()).collect();
        if doomed.is_empty() {
            return;
        }
        groups.retain(|g| !doomed.contains(&g.id));
        for g in groups.iter_mut() {
            g.member_ids.retain(|m| !doomed.contains(m));
        }
    }
}

/// Something that keeps a list of groups: the board's drawings and a sheet's extras. The provided methods are `GROUP_TOOL`'s verbs on that list.
pub trait GroupHost {
    fn group_list(&self) -> &[Group];
    fn group_list_mut(&mut self) -> &mut Vec<Group>;
    /// Name the groups that have no id yet.
    fn assign_group_ids(&mut self);

    /// `GROUP_TOOL::Group` (Ctrl+G): a new group over `ids`. Groups among them become members of it (they nest). Each id leaves the group it was
    /// in. When every id shared one group, the new group takes their place in it -- what a group made while that group is entered does
    /// (`BOARD_COMMIT::Push` adds an item made with a group entered to that group, and the selection tool only offers the entered group's own members then).
    fn make_group(&mut self, ids: &[String]) -> Result<(), GroupRefusal> {
        let mut members: Vec<String> = Vec::new();
        for id in ids {
            if !members.contains(id) {
                members.push(id.clone());
            }
        }
        if members.len() < 2 {
            return Err(GroupRefusal::bad_group("group", "a group needs at least two items"));
        }
        let tree = GroupTree(self.group_list());
        let shared: Option<String> = tree.parent_group(&members[0]).map(|p| p.id.clone()).filter(|first| members.iter().all(|m| tree.parent_group(m).is_some_and(|p| &p.id == first)));
        let groups = self.group_list_mut();
        for g in groups.iter_mut() {
            g.member_ids.retain(|m| !members.contains(m));
        }
        groups.push(Group { id: String::new(), name: String::new(), member_ids: members });
        self.assign_group_ids();
        let groups = self.group_list_mut();
        let new_id = groups.last().map(|g| g.id.clone()).unwrap_or_default();
        if let Some(above) = shared {
            if let Some(g) = groups.iter_mut().find(|g| g.id == above) {
                g.member_ids.push(new_id);
            }
        }
        prune_groups(groups);
        Ok(())
    }

    /// `GROUP_TOOL::Ungroup` (Ctrl+Shift+G): dissolve every named group. Its members are free of it -- also of the group above it, if
    /// any (`group->RemoveAll()`) -- and an id that names no group is skipped.
    fn dissolve_groups(&mut self, ids: &[String]) {
        let groups = self.group_list_mut();
        groups.retain(|g| !ids.contains(&g.id));
        for g in groups.iter_mut() {
            g.member_ids.retain(|m| !ids.contains(m));
        }
        prune_groups(groups);
    }

    /// `GROUP_TOOL::AddToGroup`: `ids` join `group_id`, each leaving the group it was in. A group cannot be added to itself or to a group below it.
    fn add_members(&mut self, group_id: &str, ids: &[String]) -> Result<(), GroupRefusal> {
        let tree = GroupTree(self.group_list());
        if !tree.is_group(group_id) {
            return Err(GroupRefusal::unknown_group(group_id));
        }
        if let Some(outer) = ids.iter().find(|id| tree.is_group(id) && tree.group_holds(id, group_id)) {
            return Err(GroupRefusal::bad_group(outer, "a group cannot hold itself or a group that holds it"));
        }
        let groups = self.group_list_mut();
        for g in groups.iter_mut().filter(|g| g.id != group_id) {
            g.member_ids.retain(|m| !ids.contains(m));
        }
        if let Some(g) = groups.iter_mut().find(|g| g.id == group_id) {
            for id in ids {
                if !g.member_ids.contains(id) {
                    g.member_ids.push(id.clone());
                }
            }
        }
        prune_groups(groups);
        Ok(())
    }

    /// `GROUP_TOOL::RemoveFromGroup`: each id leaves the group it is in (a no-op for one that is in none), free of every group above it too.
    fn remove_members(&mut self, ids: &[String]) {
        let groups = self.group_list_mut();
        for g in groups.iter_mut() {
            g.member_ids.retain(|m| !ids.contains(m));
        }
        prune_groups(groups);
    }

    /// `DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`: rename the group and make its members exactly `member_ids`, each pulled out of the group it
    /// was in, the previous members not listed released. A member may be a group, but not this one or one that holds it.
    fn set_group(&mut self, id: &str, name: &str, member_ids: &[String]) -> Result<(), GroupRefusal> {
        let tree = GroupTree(self.group_list());
        if !tree.is_group(id) {
            return Err(GroupRefusal::unknown_group(id));
        }
        if member_ids.iter().any(|m| m == id || (tree.is_group(m) && tree.group_holds(m, id))) {
            return Err(GroupRefusal::bad_group(id, "a group cannot hold itself or a group that holds it"));
        }
        let mut members: Vec<String> = Vec::new();
        for m in member_ids {
            if !members.contains(m) {
                members.push(m.clone());
            }
        }
        let groups = self.group_list_mut();
        for g in groups.iter_mut().filter(|g| g.id != id) {
            g.member_ids.retain(|m| !members.contains(m));
        }
        if let Some(g) = groups.iter_mut().find(|g| g.id == id) {
            g.name = name.to_string();
            g.member_ids = members;
        }
        prune_groups(groups);
        Ok(())
    }
}

impl GroupHost for DrawingsSection {
    fn group_list(&self) -> &[Group] {
        &self.groups
    }
    fn group_list_mut(&mut self) -> &mut Vec<Group> {
        &mut self.groups
    }
    fn assign_group_ids(&mut self) {
        self.assign_missing_ids();
    }
}

impl GroupHost for SchExtras {
    fn group_list(&self) -> &[Group] {
        &self.groups
    }
    fn group_list_mut(&mut self) -> &mut Vec<Group> {
        &mut self.groups
    }
    fn assign_group_ids(&mut self) {
        self.assign_missing_group_ids();
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

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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

    #[test]
    fn the_groups_below_a_group_come_with_the_group_first() {
        let t = GroupTree(&[]);
        assert!(t.group_and_descendants("x").is_empty());
        let groups = vec![g("outer", &["a", "inner", "other"]), g("inner", &["c", "deep"]), g("deep", &["d", "e"]), g("other", &["f", "h"])];
        let t = GroupTree(&groups);
        assert_eq!(t.group_and_descendants("outer"), vec!["outer", "inner", "deep", "other"]);
        assert_eq!(t.group_and_descendants("deep"), vec!["deep"]);
    }

    /// The sheet's groups are read and edited by the same code as the board's.
    #[test]
    fn a_sheets_groups_are_a_group_host_like_the_boards() {
        let mut e = SchExtras::default();
        assert!(e.make_group(&strs(&["R1"])).is_err(), "a group needs two items");
        e.make_group(&strs(&["R1", "R2"])).unwrap();
        e.make_group(&strs(&["C1", "C2"])).unwrap();
        assert_eq!(e.groups.len(), 2);
        assert!(e.groups.iter().all(|g| g.id.starts_with("grp_")), "{:?}", e.groups);
        // a group of two groups and an item nests them
        let (a, b) = (e.groups[0].id.clone(), e.groups[1].id.clone());
        e.make_group(&[a.clone(), b.clone(), "L1".to_string()]).unwrap();
        assert_eq!(e.groups.len(), 3);
        let top = e.groups[2].id.clone();
        assert_eq!(e.top_group("R1").map(|p| p.id.clone()), Some(top.clone()));
        assert_eq!(e.group_leaves(&top), strs(&["R1", "R2", "C1", "C2", "L1"]));
        // removing one member of an inner group dissolves it: the survivor is free, and the group above loses the inner group
        e.remove_members(&strs(&["R1"]));
        assert!(!e.is_group(&a), "an inner group with one member is not a group");
        assert!(e.parent_group("R2").is_none() && e.parent_group("R1").is_none());
        assert_eq!(e.group_leaves(&top), strs(&["C1", "C2", "L1"]));
        // a group cannot hold the group that holds it
        assert!(e.add_members(&b, &[top.clone()]).is_err());
        // properties: a name and a member list
        e.set_group(&b, "caps", &strs(&["C1", "C2", "C3"])).unwrap();
        assert_eq!(e.group(&b).map(|g| (g.name.as_str(), g.member_ids.len())), Some(("caps", 3)));
        // ungrouping the outer group frees the inner one
        e.dissolve_groups(&[top]);
        assert_eq!(e.groups.len(), 1);
        assert!(e.parent_group(&b).is_none());
    }

    #[test]
    fn group_ids_come_from_their_members_and_survive_a_second_pass() {
        let mut e = SchExtras::default();
        e.groups = vec![g("", &["x", "y"]), g("", &["y2", "x2"])];
        e.assign_missing_group_ids();
        let ids: Vec<String> = e.groups.iter().map(|g| g.id.clone()).collect();
        assert!(ids[0].starts_with("grp_") && ids[1].starts_with("grp_") && ids[0] != ids[1]);
        e.assign_missing_group_ids();
        assert_eq!(e.groups.iter().map(|g| g.id.clone()).collect::<Vec<_>>(), ids, "ids that are set stay");
    }
}
