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
//! group can hold neither itself nor a group that holds it, and Remove Items dissolves a group it leaves with fewer than two
//! members (`GROUP_TOOL::RemoveFromGroup`'s `affectedGroups`); a group of one is otherwise kept, as KiCad keeps it.
//!
//! # Items made while a group is entered
//!
//! `BOARD_COMMIT::Push` adds every item a command made (`CHT_ADD`) to the group the selection tool has entered, when the item can be in a group
//! and is not in one yet (`if( enteredGroup && boardItem->IsGroupableType() && !boardItem->GetParentGroup() ) enteredGroup->AddItem( boardItem );`);
//! `PCB_CONTROL::placeBoardItems` does the same earlier for what a Paste brings, so the preview while it is carried is right. The entered group
//! lives in the editor, not in the design, so the editor wraps a command in [`Cmd::InGroup`] while one is entered and the new items join it.
//!
//! # Items that leave the board
//!
//! `BOARD_COMMIT::Push` takes an item that is removed out of its parent group. The delete verbs here do not know about groups, so
//! [`Board::apply`](super::Board::apply) compares the board before and after a command ([`Board::group_universe`]) and
//! [`Board::tidy_groups`] drops from every group the members that were on the board and no longer are, dropping a group
//! that is left empty (a group of one stays). A member the board never had (a group made over ids no item answers to) stays, as it did.

use super::{Board, Cmd};
use eda_model::groups::{GroupHost, GroupRefusal};
use eda_model::ir::Design;
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// A refusal of a group edit as the command's failure (`CheckResult`).
pub(crate) fn refused(r: GroupRefusal) -> Vec<CheckResult> {
    vec![CheckResult::fail(r.check, &r.subject, r.why)]
}

/// Every id an item of the board can be listed under: the groupable kinds (`BOARD_ITEM::IsGroupableType`: placed footprints, copper, graphics, texts, text boxes,
/// reference images, barcodes, dimensions, tables, generators -- tuning patterns and via stitchings -- and the groups themselves; not a point).
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
        out.extend(dr.tables.iter().map(|t| t.id.clone()));
        out.extend(dr.barcodes.iter().map(|b| b.id.clone()));
        out.extend(dr.board_items.text_boxes.iter().map(|t| t.id.clone()));
        out.extend(dr.board_items.images.iter().map(|i| i.id.clone()));
        out.extend(dr.board_items.via_stitches.iter().map(|v| v.id.clone()));
        out.extend(dr.tuning_patterns.iter().map(|p| p.id.clone()));
        out.extend(dr.groups.iter().map(|g| g.id.clone()));
    }
    out
}

impl Board<'_> {
    /// `Cmd::Group` (Ctrl+G): a new group over `ids`. Groups among them become members of it (they nest). Each id leaves the group
    /// it was in. When every id shared one group, the new group takes their place in it -- what a group made while that group is
    /// entered does (`BOARD_COMMIT::Push` adds an item made with a group entered to that group, and the selection tool only
    /// offers the entered group's own members then). The list edit is `GroupHost::make_group`, the sheet's as well.
    ///
    /// `PCB_GROUP_TOOL::Group`: "if( static_cast<BOARD_ITEM*>( eda_item )->IsLocked() ) group->SetLocked( true );" -- a locked item makes the new group locked.
    pub(crate) fn group_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let dr = self.drawings_mut();
        let before: BTreeSet<String> = dr.groups.iter().map(|g| g.id.clone()).collect();
        // `BOARD_ITEM::IsLocked`: the item's own lock, or that of a group above it (`GetParentGroup()->IsLocked()`), before it leaves that group
        let locked_or_above = |id: &str| {
            let mut at = id.to_string();
            for _ in 0..64 {
                if dr.locked_ids.contains(&at) {
                    return true;
                }
                match dr.parent_group(&at) {
                    Some(g) => at = g.id.clone(),
                    None => return false,
                }
            }
            false
        };
        let any_locked = ids.iter().any(|id| locked_or_above(id));
        dr.make_group(ids).map_err(refused)?;
        if any_locked {
            let new: Vec<String> = dr.groups.iter().map(|g| g.id.clone()).filter(|g| !before.contains(g)).collect();
            dr.locked_ids.extend(new);
            dr.locked_ids.sort();
            dr.locked_ids.dedup();
        }
        Ok(())
    }

    /// `Cmd::Ungroup` (Ctrl+Shift+G): dissolve every named group. Its members are free of it -- also of the group above it, if
    /// any (`GROUP_TOOL::Ungroup`: `group->RemoveAll()`) -- and an id that names no group is skipped.
    pub(crate) fn ungroup_items(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.dissolve_groups(ids);
        }
        Ok(())
    }

    /// `Cmd::AddToGroup` (`GROUP_TOOL::AddToGroup`): `ids` join `group_id`, each leaving the group it was in. A group cannot be
    /// added to itself or to a group below it.
    pub(crate) fn add_to_group(&mut self, group_id: &str, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        self.drawings_mut().add_members(group_id, ids).map_err(refused)
    }

    /// `Cmd::RemoveFromGroup` (`GROUP_TOOL::RemoveFromGroup`): each id leaves the group it is in (a no-op for one that is in none),
    /// free of every group above it too.
    pub(crate) fn remove_from_group(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.remove_members(ids);
        }
        Ok(())
    }

    /// `Cmd::EditGroup` (`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`): rename the group and make its members exactly
    /// `member_ids`, each pulled out of the group it was in, the previous members not listed released. A member may be a group, but
    /// not this one or one that holds it.
    pub(crate) fn edit_group(&mut self, id: &str, name: &str, member_ids: &[String], design_block: Option<&str>) -> Result<(), Vec<CheckResult>> {
        self.drawings_mut().set_group(id, name, member_ids, design_block).map_err(refused)
    }

    /// `Cmd::LinkDesignBlock` (`PCB_EDIT_FRAME::SaveSelectionAsDesignBlock`, `UpdateDesignBlockFromSelection`): `ids` are linked to the block `block` (`Lib:Name`). A lone
    /// group with no link takes it; anything else goes in a new group named after the block and linked to it, each item leaving the group it was in. The ids must be items of
    /// the board (a footprint, a track, a via, a zone, a graphic, a text, a dimension) or groups.
    pub(crate) fn link_design_block_items(&mut self, ids: &[String], block: &str) -> Result<(), Vec<CheckResult>> {
        let known = universe(&self.design);
        if let Some(bad) = ids.iter().find(|id| !known.contains(id.as_str())) {
            return Err(vec![CheckResult::fail("ops_unknown_item", bad, "no item of the board has this id")]);
        }
        self.drawings_mut().link_design_block(ids, block).map(|_| ()).map_err(refused)
    }

    /// `Cmd::InGroup`: `cmd`, then every item it added that is in no group joins `group` -- the group the editor has entered
    /// (`BOARD_COMMIT::Push`'s `enteredGroup->AddItem( boardItem )`, `placeBoardItems`' earlier copy of it). The command is the editor's
    /// own, so it is all or nothing like it; a group that is no longer there (dissolved, deleted) is simply not joined.
    pub(crate) fn in_group(&mut self, group: &str, cmd: &Cmd) -> Result<(), Vec<CheckResult>> {
        // a schematic command joins a group of the sheet (`sch_groups.rs`)
        if cmd.domain() == super::Domain::Schematic {
            return self.sch_in_group(group, cmd);
        }
        let before = universe(&self.design);
        self.apply(cmd)?;
        self.join_group(group, &before);
        Ok(())
    }

    /// Adds to `group` what the board has now and did not have in `before`, when it is in no group: new items, and a new group (a pasted tree joins
    /// as its top group, its members staying in it). A group cannot join itself or one below it.
    fn join_group(&mut self, group: &str, before: &BTreeSet<String>) {
        let fresh: Vec<String> = universe(&self.design).difference(before).cloned().collect();
        let Some(dr) = self.design.drawings.as_mut() else { return };
        if !dr.is_group(group) {
            return;
        }
        for id in fresh {
            if id == group || dr.parent_group(&id).is_some() || (dr.is_group(&id) && dr.group_holds(&id, group)) {
                continue;
            }
            if let Some(g) = dr.groups.iter_mut().find(|g| g.id == group) {
                g.member_ids.push(id);
            }
        }
    }

    /// The ids on the board before a command, when there are groups to keep honest ([`Board::tidy_groups`]); `None` without any.
    pub(crate) fn group_universe(&self) -> Option<BTreeSet<String>> {
        let dr = self.design.drawings.as_ref()?;
        (!dr.groups.is_empty()).then(|| universe(&self.design))
    }

    /// Takes out of every group the members that were on the board before the command (`before`) and are not now
    /// (`BOARD_COMMIT::Push`'s `parentGroup->RemoveItem( boardItem )` for a removed item). Nothing dissolves a group there: a group of one
    /// member stays on the board and in the file; a group left with no member is gone (`PCB_IO_KICAD_SEXPR::format( const PCB_GROUP* )`:
    /// `if( memberIds.empty() ) return;`), and the group above it loses it in turn. (`GROUP_TOOL::RemoveFromGroup`'s `< 2` rule is that
    /// verb's own.)
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
            loop {
                let empty: Vec<String> = dr.groups.iter().filter(|g| g.member_ids.is_empty()).map(|g| g.id.clone()).collect();
                if empty.is_empty() {
                    break;
                }
                dr.groups.retain(|g| !g.member_ids.is_empty());
                for g in dr.groups.iter_mut() {
                    g.member_ids.retain(|m| !empty.contains(m));
                }
            }
        }
    }
}
