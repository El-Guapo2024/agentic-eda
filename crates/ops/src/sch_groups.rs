//! Groups on the schematic: Group, Ungroup, Add Items, Remove Items and Group Properties as `SchCmd` verbs, the upkeep that keeps a group honest when
//! what it holds leaves the sheet, and the items made while a group is entered.
//!
//! Ports `SCH_GROUP_TOOL::Group` (`eeschema/tools/sch_group_tool.cpp`), `GROUP_TOOL` (`common/tool/group_tool.cpp`: Ungroup, AddToGroup, RemoveFromGroup),
//! `DIALOG_GROUP_PROPERTIES::TransferDataFromWindow` and `EDA_GROUP` (`common/eda_group.cpp`) onto the sheet in view. The group list is
//! [`SchExtras::groups`](eda_model::sch_extras::SchExtras::groups) and the list edits are `eda_model::groups::GroupHost`'s, the code the board's groups
//! (`pcb_groups.rs`) run too; what is the sheet's own is which ids may be members, and how the sheet keeps the list honest.
//!
//! # What can be a member
//!
//! `SCH_ITEM::IsGroupableType`: a symbol, a shape, a text, a text box, a line (a wire, a bus, a graphic line), a junction, a no-connect, a bus entry, a label of any
//! kind, a rule area, a directive label, a sheet, and another group. Not a sheet pin ("they go along with an `SCH_SHEET`"), and `SCH_GROUP_TOOL::Group`
//! takes the items that belong to a symbol (`GetParentSymbol()`: its pins and fields) out of the selection, so they are no members. A symbol is its reference,
//! every placed unit of it; a wire is the whole polyline the studio keeps for it (KiCad has one `SCH_LINE` per segment, and the file lists every segment).
//!
//! # Items made while a group is entered
//!
//! `SCH_COMMIT::Push` adds every item a command made (`CHT_ADD`) to the group the selection tool has entered, when the item is groupable and in no group
//! (`if( enteredGroup && schItem->IsGroupableType() && !schItem->GetParentGroup() ) enteredGroup->AddItem( schItem )`; `SCH_MOVE_TOOL` does the same earlier for
//! what it carries). The entered group lives in the editor, so the editor wraps a command in [`Cmd::InGroup`] while one is entered and the new items join it.
//!
//! # Items that leave the sheet
//!
//! `SCH_COMMIT::Push` takes an item that is removed out of its group (`group->RemoveItem( schItem )`). The delete verbs do not know about groups, so
//! [`Board::apply`](super::Board::apply) compares the ids of the sheet before and after a command and [`Board::tidy_sch_groups`] drops from every group the members that
//! were there and are not now. Like the board's groups, a group left with fewer than two members dissolves (KiCad keeps a group of one until it is ungrouped and
//! does not write an empty one; one rule for both editors is the simpler one to work with).

use super::{Board, Cmd, Domain};
use crate::pcb_groups::refused;
use crate::sch_edit::lockable_ids;
use eda_model::groups::GroupHost;
use eda_model::ir::SchematicSection;
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// Every id a group of this sheet may list as a member: the groupable items (`IsGroupableType`) and the sheet's groups.
fn memberable(sch: &SchematicSection) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = lockable_ids(sch).into_iter().map(String::from).collect();
    out.extend(sch.extras.groups.iter().map(|g| g.id.clone()));
    out
}

/// The ids a group of this sheet may list as a member (`IsGroupableType`; a pin, a field and a sheet pin are not): what the studio's server keeps of a selection that is to be
/// linked to a design block.
pub fn groupable_ids(sch: &SchematicSection) -> BTreeSet<String> {
    memberable(sch)
}

/// The ids on the sheet before and after a command, to see what left: items, and groups.
pub(crate) fn sheet_ids(sch: &SchematicSection) -> BTreeSet<String> {
    memberable(sch)
}

fn not_groupable(id: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_unknown_item", id, "no groupable item with this id on the sheet (a pin, a field and a sheet pin go along with the item that has them)")]
}

impl Board<'_> {
    /// The listed ids as members of a group of this sheet, or the first that cannot be one.
    fn check_members(&self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let known = memberable(self.schematic()?);
        match ids.iter().find(|id| !known.contains(id.as_str())) {
            Some(bad) => Err(not_groupable(bad)),
            None => Ok(()),
        }
    }

    /// `SchCmd::Group` (Ctrl+G, `SCH_GROUP_TOOL::Group`): a new group over `ids`. See [`GroupHost::make_group`].
    pub(crate) fn sch_group_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        self.check_members(ids)?;
        self.schematic_mut()?.extras.make_group(ids).map_err(refused)
    }

    /// `SchCmd::Ungroup` (Ctrl+Shift+G, `GROUP_TOOL::Ungroup`): every named group dissolves, its members freed. A listed id that is no group is skipped, and
    /// at least one has to be one ("nothing to ungroup" would leave an empty undo step).
    pub(crate) fn sch_ungroup_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !ids.iter().any(|id| sch.extras.is_group(id)) {
            return Err(vec![CheckResult::fail("ops_unknown_group", ids.first().map(String::as_str).unwrap_or("ungroup"), "no group among the listed items")]);
        }
        sch.extras.dissolve_groups(ids);
        Ok(())
    }

    /// `SchCmd::AddToGroup` (`GROUP_TOOL::AddToGroup`): `ids` join `group_id`. See [`GroupHost::add_members`].
    pub(crate) fn sch_add_to_group(&mut self, group_id: &str, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        self.check_members(ids)?;
        self.schematic_mut()?.extras.add_members(group_id, ids).map_err(refused)
    }

    /// `SchCmd::RemoveFromGroup` (`GROUP_TOOL::RemoveFromGroup`): each id leaves its group; at least one has to be in one.
    pub(crate) fn sch_remove_from_group(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !ids.iter().any(|id| sch.extras.parent_group(id).is_some()) {
            return Err(vec![CheckResult::fail("ops_not_in_group", ids.first().map(String::as_str).unwrap_or("remove"), "none of the listed items is in a group")]);
        }
        sch.extras.remove_members(ids);
        Ok(())
    }

    /// `SchCmd::EditGroup` (`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`): a name, the member list and the design block link. See [`GroupHost::set_group`].
    pub(crate) fn sch_edit_group(&mut self, id: &str, name: &str, member_ids: &[String], design_block: Option<&str>) -> Result<(), Vec<CheckResult>> {
        self.check_members(member_ids)?;
        self.schematic_mut()?.extras.set_group(id, name, member_ids, design_block).map_err(refused)
    }

    /// `SchCmd::LinkDesignBlock`: see [`GroupHost::link_design_block`]. The ids must be groupable items of the sheet (`SaveSelectionAsDesignBlock` leaves out an item that belongs to
    /// a symbol -- its pins and fields -- and one that `IsGroupableType` refuses), or groups.
    pub(crate) fn sch_link_design_block(&mut self, ids: &[String], block: &str) -> Result<(), Vec<CheckResult>> {
        self.check_members(ids)?;
        self.schematic_mut()?.extras.link_design_block(ids, block).map(|_| ()).map_err(refused)
    }

    /// The ids of the sheet in view before a command, when it has groups to keep honest ([`Board::tidy_sch_groups`]); `None` without any.
    pub(crate) fn sch_group_universe(&self) -> Option<BTreeSet<String>> {
        let sch = self.schematic().ok()?;
        (!sch.extras.groups.is_empty()).then(|| sheet_ids(sch))
    }

    /// Takes out of every group of the sheet the members that were on it before the command (`before`) and are not now, and dissolves a group left with
    /// fewer than two (`SCH_COMMIT::Push`'s `group->RemoveItem( schItem )` for a removed item).
    pub(crate) fn tidy_sch_groups(&mut self, before: Option<BTreeSet<String>>) {
        let Some(before) = before else { return };
        let Ok(sch) = self.schematic_mut() else { return };
        if sch.extras.groups.is_empty() {
            return;
        }
        let now = sheet_ids(sch);
        let gone: BTreeSet<&String> = before.difference(&now).collect();
        if gone.is_empty() {
            return;
        }
        for g in sch.extras.groups.iter_mut() {
            g.member_ids.retain(|m| !gone.contains(m));
        }
        // `SCH_COMMIT::Push` takes a removed item out of its group and dissolves nothing: a group of one stays, an empty one goes.
        eda_model::groups::drop_empty_groups(&mut sch.extras.groups);
    }

    /// `Cmd::InGroup` for a schematic command: `cmd`, then every item it added that is in no group joins `group` (`SCH_COMMIT::Push`'s
    /// `enteredGroup->AddItem( schItem )`). A group that is no longer there is simply not joined.
    pub(crate) fn sch_in_group(&mut self, group: &str, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        debug_assert_eq!(cmd.domain(), Domain::Schematic);
        let before = self.schematic().map(sheet_ids).unwrap_or_default();
        self.apply(cmd)?;
        let Ok(sch) = self.schematic_mut() else { return Ok(()) };
        // what the command made has no id yet (a verb leaves that to the next load): name it, as that would
        sch.assign_missing_ids();
        if !sch.extras.is_group(group) {
            return Ok(());
        }
        let fresh: Vec<String> = sheet_ids(sch).difference(&before).cloned().collect();
        for id in fresh {
            if id == group || sch.extras.parent_group(&id).is_some() || (sch.extras.is_group(&id) && sch.extras.group_holds(&id, group)) {
                continue;
            }
            if let Some(g) = sch.extras.groups.iter_mut().find(|g| g.id == group) {
                g.member_ids.push(id);
            }
        }
        Ok(())
    }
}

/// `SCH_ITEM::IsLocked` for a group member: the item, or any group above it, is locked.
pub(crate) fn locked_in_tree(sch: &SchematicSection, id: &str) -> bool {
    let reference = id.split('#').next().unwrap_or(id);
    if sch.extras.is_locked(id) || sch.extras.is_locked(reference) {
        return true;
    }
    let mut at = sch.extras.parent_group(id);
    let mut hops = 0;
    while let Some(g) = at {
        if sch.extras.is_locked(&g.id) {
            return true;
        }
        hops += 1;
        if hops > sch.extras.groups.len() {
            return false; // a loop made by hand ends the walk
        }
        at = sch.extras.parent_group(&g.id);
    }
    false
}

/// `FilterSelectionForLockedItems` for a group: it is no selection when it, a group below it or an item below it is locked.
pub(crate) fn group_has_locked(sch: &SchematicSection, group: &str) -> bool {
    sch.extras.group_tree().group_and_descendants(group).iter().any(|g| sch.extras.is_locked(g)) || sch.extras.group_leaves(group).iter().any(|l| locked_in_tree(sch, l))
}

/// A symbol reference renamed (`SCH_REFERENCE::SetRef`): the groups that list it list the new one.
pub(crate) fn rename_member(sch: &mut SchematicSection, old: &str, new: &str) {
    if old == new {
        return;
    }
    for g in sch.extras.groups.iter_mut() {
        if g.member_ids.iter().any(|m| m == old) {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            g.member_ids = g.member_ids.iter().map(|m| if m == old { new.to_string() } else { m.clone() }).filter(|m| seen.insert(m.clone())).collect();
        }
    }
}
