//! The group verbs (`Cmd::Group`, `Ungroup`, `AddToGroup`, `RemoveFromGroup`, `EditGroup`), with groups that nest, and the
//! upkeep that keeps a group honest when what it holds is taken off the board.
//!
//! Ports `GROUP_TOOL` (`common/tool/group_tool.cpp`), `PCB_GROUP_TOOL::Group` (`pcbnew/tools/pcb_group_tool.cpp`),
//! `EDA_GROUP` (`common/eda_group.cpp`) and `DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`. The group tree itself is read with
//! `DrawingsSection::group_leaves` and friends (`crates/model/src/groups.rs`).
//!
//! # What nests
//!
//! A group's members are item ids, and an id may be another group's: grouping a selection that holds groups makes those groups
//! members of the new one (`PCB_GROUP_TOOL::Group` adds every selected item, a group included, to the new group), where this
//! model used to flatten them. An item is in one group at a time (`EDA_GROUP::AddItem` takes it out of the one it was in), a
//! group can hold neither itself nor a group that holds it, and a group of fewer than two members dissolves
//! (`GROUP_TOOL::RemoveFromGroup`), a nested one taking its place in the group above with it.
//!
//! # Items that leave the board
//!
//! `BOARD_COMMIT::Push` takes an item that is removed out of its parent group. The delete verbs here do not know about groups, so
//! [`Board::apply`](super::Board::apply) compares the board before and after a command ([`Board::group_universe`]) and
//! [`Board::tidy_groups`] drops from every group the members that were on the board and no longer are, dissolving a group
//! that is left with fewer than two. A member the board never had (a group made over ids no item answers to) stays, as it did.

use super::Board;
use eda_model::ir::{Design, DrawingsSection, Group};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn bad_group(subject: &str, why: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_group", subject, why)]
}

/// A group with fewer than two members is meaningless and dissolves (`GROUP_TOOL::RemoveFromGroup`'s
/// `if( group->GetItems().size() < 2 )`); the group above it loses it as a member, and may fall under two in turn.
pub(crate) fn prune_groups(dr: &mut DrawingsSection) {
    loop {
        let doomed: Vec<String> = dr.groups.iter().filter(|g| g.member_ids.len() < 2).map(|g| g.id.clone()).collect();
        if doomed.is_empty() {
            return;
        }
        dr.groups.retain(|g| !doomed.contains(&g.id));
        for g in dr.groups.iter_mut() {
            g.member_ids.retain(|m| !doomed.contains(m));
        }
    }
}

/// Every id an item of the board can be listed under: placed footprints, copper, graphics, dimensions and the groups themselves.
fn universe(design: &Design) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = design.placement.iter().flat_map(|p| p.footprints.iter().map(|f| f.id.clone())).collect();
    if let Some(rt) = &design.routing {
        out.extend(rt.tracks.iter().map(|t| t.id.clone()));
        out.extend(rt.vias.iter().map(|v| v.id.clone()));
        out.extend(rt.zones.iter().map(|z| z.id.clone()));
    }
    if let Some(dr) = &design.drawings {
        out.extend(dr.shapes.iter().map(|s| s.id().to_string()));
        out.extend(dr.texts.iter().map(|t| t.id.clone()));
        out.extend(dr.dimensions.iter().map(|d| d.id.clone()));
        out.extend(dr.groups.iter().map(|g| g.id.clone()));
    }
    out
}

impl Board<'_> {
    /// `Cmd::Group` (Ctrl+G): a new group over `ids`. Groups among them become members of it (they nest). Each id leaves the group
    /// it was in. When every id shared one group, the new group takes their place in it -- what a group made while that group is
    /// entered does (`BOARD_COMMIT::Push` adds an item made with a group entered to that group, and the selection tool only
    /// offers the entered group's own members then).
    pub(crate) fn group_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let mut members: Vec<String> = Vec::new();
        for id in ids {
            if !members.contains(id) {
                members.push(id.clone());
            }
        }
        if members.len() < 2 {
            return Err(bad_group("group", "a group needs at least two items"));
        }
        let dr = self.drawings_mut();
        let shared: Option<String> = dr.parent_group(&members[0]).map(|p| p.id.clone()).filter(|first| members.iter().all(|m| dr.parent_group(m).is_some_and(|p| &p.id == first)));
        for g in dr.groups.iter_mut() {
            g.member_ids.retain(|m| !members.contains(m));
        }
        dr.groups.push(Group { id: String::new(), name: String::new(), member_ids: members });
        dr.assign_missing_ids();
        let new_id = dr.groups.last().map(|g| g.id.clone()).unwrap_or_default();
        if let Some(above) = shared {
            if let Some(g) = dr.groups.iter_mut().find(|g| g.id == above) {
                g.member_ids.push(new_id);
            }
        }
        prune_groups(dr);
        Ok(())
    }

    /// `Cmd::Ungroup` (Ctrl+Shift+G): dissolve every named group. Its members are free of it -- also of the group above it, if
    /// any (`GROUP_TOOL::Ungroup`: `group->RemoveAll()`) -- and an id that names no group is skipped.
    pub(crate) fn ungroup_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.groups.retain(|g| !ids.contains(&g.id));
            for g in dr.groups.iter_mut() {
                g.member_ids.retain(|m| !ids.contains(m));
            }
            prune_groups(dr);
        }
        Ok(())
    }

    /// `Cmd::AddToGroup` (`GROUP_TOOL::AddToGroup`): `ids` join `group_id`, each leaving the group it was in. A group cannot be
    /// added to itself or to a group below it.
    pub(crate) fn add_to_group(&mut self, group_id: &str, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let dr = self.drawings_mut();
        if !dr.is_group(group_id) {
            return Err(vec![CheckResult::fail("ops_unknown_group", group_id, "no group with this id")]);
        }
        if let Some(outer) = ids.iter().find(|id| dr.is_group(id) && dr.group_holds(id, group_id)) {
            return Err(bad_group(outer, "a group cannot hold itself or a group that holds it"));
        }
        for g in dr.groups.iter_mut().filter(|g| g.id != group_id) {
            g.member_ids.retain(|m| !ids.contains(m));
        }
        if let Some(g) = dr.groups.iter_mut().find(|g| g.id == group_id) {
            for id in ids {
                if !g.member_ids.contains(id) {
                    g.member_ids.push(id.clone());
                }
            }
        }
        prune_groups(dr);
        Ok(())
    }

    /// `Cmd::RemoveFromGroup` (`GROUP_TOOL::RemoveFromGroup`): each id leaves the group it is in (a no-op for one that is in none),
    /// free of every group above it too.
    pub(crate) fn remove_from_group(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if let Some(dr) = self.design.drawings.as_mut() {
            for g in dr.groups.iter_mut() {
                g.member_ids.retain(|m| !ids.contains(m));
            }
            prune_groups(dr);
        }
        Ok(())
    }

    /// `Cmd::EditGroup` (`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`): rename the group and make its members exactly
    /// `member_ids`, each pulled out of the group it was in, the previous members not listed released. A member may be a group, but
    /// not this one or one that holds it.
    pub(crate) fn edit_group(&mut self, id: &str, name: &str, member_ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let dr = self.drawings_mut();
        if !dr.is_group(id) {
            return Err(vec![CheckResult::fail("ops_unknown_group", id, "no group with this id")]);
        }
        if member_ids.iter().any(|m| m == id || (dr.is_group(m) && dr.group_holds(m, id))) {
            return Err(bad_group(id, "a group cannot hold itself or a group that holds it"));
        }
        let mut members: Vec<String> = Vec::new();
        for m in member_ids {
            if !members.contains(m) {
                members.push(m.clone());
            }
        }
        for g in dr.groups.iter_mut().filter(|g| g.id != id) {
            g.member_ids.retain(|m| !members.contains(m));
        }
        if let Some(g) = dr.groups.iter_mut().find(|g| g.id == id) {
            g.name = name.to_string();
            g.member_ids = members;
        }
        prune_groups(dr);
        Ok(())
    }

    /// The ids on the board before a command, when there are groups to keep honest ([`Board::tidy_groups`]); `None` without any.
    pub(crate) fn group_universe(&self) -> Option<BTreeSet<String>> {
        let dr = self.design.drawings.as_ref()?;
        (!dr.groups.is_empty()).then(|| universe(&self.design))
    }

    /// Takes out of every group the members that were on the board before the command (`before`) and are not now, and dissolves a
    /// group that is left with fewer than two members (`BOARD_COMMIT::Push`'s `parentGroup->RemoveItem( boardItem )` for a removed
    /// item, then `GROUP_TOOL::RemoveFromGroup`'s dissolving rule).
    pub(crate) fn tidy_groups(&mut self, before: Option<BTreeSet<String>>) {
        let Some(before) = before else { return };
        if self.design.drawings.as_ref().is_none_or(|d| d.groups.is_empty()) {
            return;
        }
        let now = universe(&self.design);
        let gone: BTreeSet<&String> = before.difference(&now).collect();
        if gone.is_empty() {
            return;
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            for g in dr.groups.iter_mut() {
                g.member_ids.retain(|m| !gone.contains(m));
            }
            prune_groups(dr);
        }
    }
}
