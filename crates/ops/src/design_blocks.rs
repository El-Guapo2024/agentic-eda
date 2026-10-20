//! Design blocks in the design: what placing a block does to the sheet and to the board.
//!
//! A design block (`common/design_block.cpp`, the library is `eda_kicad::design_block`) is placed by the code that brings any file in, and a block's own marks on the
//! result are its **group**: the items that came in are put in one group named after the block and linked to it (`EDA_GROUP::SetDesignBlockLibId`), and that link is what
//! Place Linked Design Block, Save to Linked Design Block and Apply Design Block Layout read afterwards.
//!
//! * On the sheet, Place Design Block *is* Import Sheet (`SCH_DRAWING_TOOLS::ImportSheet` runs for `placeDesignBlock` with `placingDesignBlock` set): [`Cmd::ImportSheet`]
//!   carries the block's `Lib:Name` and Place as group. With the file's contents the group holds what the paste put at the top; with the file as a sheet, the sheet alone, in
//!   a group of one (`DrawSheet`: `sheetGroup->AddItem( sheet )`) -- and only for a block: a sheet drawn from a plain file is not grouped.
//! * On the board, Place Design Block is Append Board (`PCB_CONTROL::AppendDesignBlock` -> `AppendBoard( .., designBlock )`): [`Cmd::PlaceDesignBlock`] is the paste of
//!   the block's board file, and `placeAsGroup` puts what came in a group when there are at least two things at its top (`groupableCount >= 2`).
//!
//! The group's name is the block's (`designBlock->GetLibId().GetLibItemName()`), or, for a plain file, the file's stem (`wxFileName( sheetFileName ).GetName()`); "Place repeated
//! copies" numbers the names (`group->GetName() + wxString::Format( "%d", suffix++ )`), which the caller does by passing `group_name`.

use super::Board;
use eda_model::groups::GroupHost;
use eda_model::ir::{Group, Point};
use eda_model::CheckResult;

/// How the schematic file a sheet import reads is being placed: grouped or not, under what name, and as which design block.
pub(crate) struct Placement<'a> {
    /// The file (`ImportSheet`'s `source`): its stem names a group when nothing else does.
    pub source: &'a str,
    pub as_group: bool,
    pub group_name: &'a str,
    /// `Lib:Name`, or empty for a file that is not a design block.
    pub block: &'a str,
}

impl Placement<'_> {
    /// The group's name: the one asked for, else the block's, else the file's stem.
    pub(crate) fn name(&self) -> String {
        if !self.group_name.trim().is_empty() {
            return self.group_name.trim().to_string();
        }
        if let Some((_, item)) = self.block.split_once(':') {
            return item.to_string();
        }
        let file = self.source.rsplit(['/', '\\']).next().unwrap_or(self.source);
        file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string()
    }
}

impl Board<'_> {
    /// Put `ids` -- what an import put at the top of the sheet in view -- in a group named and linked as `placement` says. A group needs two items, unless it is a design
    /// block's (a block placed as a sheet puts the sheet alone in its group; `prune_groups` keeps it). Nothing is done when the placement is not Place as group.
    pub(crate) fn group_placed(&mut self, ids: &[String], placement: &Placement<'_>) -> Result<(), Vec<CheckResult>> {
        if !placement.as_group || ids.is_empty() || (ids.len() < 2 && placement.block.is_empty()) {
            return Ok(());
        }
        let sch = self.schematic_mut()?;
        sch.extras.groups.push(Group { id: String::new(), name: placement.name(), member_ids: ids.to_vec(), design_block: placement.block.to_string() });
        sch.extras.assign_group_ids();
        Ok(())
    }

    /// `Cmd::PlaceDesignBlock`: see the module doc. The file's text is the clipboard form (`eda_kicad::parse_pcb_clipboard` reads a board file as it reads a copy).
    pub(crate) fn place_design_block(&mut self, text: &str, at: Point, block: &str, as_group: bool, group_name: &str) -> Result<(), Vec<CheckResult>> {
        if let Err(why) = eda_model::groups::check_design_block_id(block) {
            return Err(vec![CheckResult::fail("ops_bad_design_block", block, why)]);
        }
        let clip = eda_kicad::parse_pcb_clipboard(text).map_err(|e| vec![CheckResult::fail("ops_bad_clipboard", "design block", e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; "))])?;
        let copies = self.copies_from_clipboard(&clip, at, super::pcb_paste_special::PasteMode::Keep, false);
        if copies.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_clipboard", "design block", "nothing in the design block's board fits this board: its copper is on layers the board does not have")]);
        }
        let top = self.insert_copies_all(copies)?;
        // `AppendBoard`: `if( groupableCount >= 2 )`
        if as_group && top.len() >= 2 {
            let name = if group_name.trim().is_empty() { block.split_once(':').map_or(block, |(_, n)| n).to_string() } else { group_name.trim().to_string() };
            let dr = self.drawings_mut();
            dr.groups.push(Group { id: String::new(), name, member_ids: top, design_block: block.to_string() });
            dr.assign_group_ids();
        }
        Ok(())
    }
}
