//! The schematic editor's edit and drawing tools that act on items already on the sheet, ported from
//! KiCad's `eeschema/tools` (`SCH_EDIT_TOOL`, `SCH_DRAWING_TOOLS`, `SCH_POINT_EDITOR`, ...).
//!
//! One `Cmd` variant carries all of them -- `Cmd::SchEdit(SchCmd)`, `{"op": "sch_edit", "verb": ...}` on the
//! wire -- so this family can grow without touching the flat verb list. Every verb is a pure edit of
//! `design.schematic` (or one of the `sheet_contents` screens) and therefore undoable like any other; each one
//! cites the KiCad function it ports (eeschema at 8303b2ad).

use crate::{Board, Cmd};
use eda_model::ir::SchematicSection;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One schematic edit verb -- see the module doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum SchCmd {
    /// Lock / Unlock / Toggle Lock (`SCH_EDIT_TOOL::modifyLockSelected`): set `SCH_ITEM::SetLocked` on every
    /// listed item. KiCad skips pins, fields and sheet pins ("they inherit from parent"); so does this (a sheet
    /// pin id is accepted and ignored). The toggle's own rule -- unlock when any selected item is locked, else
    /// lock -- is the caller's: it picks `locked` and sends one verb.
    SetLocked { ids: Vec<String>, locked: bool },
}

impl SchCmd {
    /// The ids this verb names, for the activity log.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            SchCmd::SetLocked { ids, .. } => ids.iter().map(String::as_str).collect(),
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        match self {
            SchCmd::SetLocked { ids, locked } => format!("schematic {} {}", if *locked { "lock" } else { "unlock" }, ids.join(" ")),
        }
    }

    /// The activity kind this verb files under (`crates/cli/src/board.rs`).
    pub fn kind(&self) -> &'static str {
        match self {
            SchCmd::SetLocked { .. } => "schematic-lock",
        }
    }
}

impl<'a> Board<'a> {
    /// Apply one [`SchCmd`] to the schematic.
    pub(crate) fn apply_sch_edit(&mut self, cmd: &SchCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            SchCmd::SetLocked { ids, locked } => set_locked(self.schematic_mut()?, ids, *locked),
        }
    }
}

impl Cmd {
    /// A `Cmd` for one [`SchCmd`] (the wrapper callers and tests build).
    pub fn sch(cmd: SchCmd) -> Cmd {
        Cmd::SchEdit(cmd)
    }
}

/// Every item id of this sheet that Lock can act on (everything but pins and fields; sheet pins are listed
/// separately because they are accepted-and-ignored).
fn lockable_ids(sch: &SchematicSection) -> BTreeSet<&str> {
    let mut out: BTreeSet<&str> = BTreeSet::new();
    out.extend(sch.symbols.iter().map(|s| s.id.as_str()));
    out.extend(sch.power_symbols.iter().map(|p| p.id.as_str()));
    out.extend(sch.wires.iter().map(|w| w.id.as_str()));
    out.extend(sch.labels.iter().map(|l| l.id.as_str()));
    out.extend(sch.texts.iter().map(|t| t.id.as_str()));
    out.extend(sch.no_connects.iter().map(|n| n.id.as_str()));
    out.extend(sch.bus_entries.iter().map(|b| b.id.as_str()));
    out.extend(sch.junctions.iter().map(|j| j.id.as_str()));
    out.extend(sch.lines.iter().map(|l| l.id.as_str()));
    out.extend(sch.sheets.iter().map(|s| s.id.as_str()));
    out.extend(sch.extras.graphics.iter().map(|g| g.id.as_str()));
    out.remove("");
    out
}

fn set_locked(sch: &mut SchematicSection, ids: &[String], locked: bool) -> Result<(), Vec<CheckResult>> {
    if ids.is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_lock", "lock", "no items given")]);
    }
    let known = lockable_ids(sch);
    let sheet_pins: BTreeSet<&str> = sch.sheets.iter().flat_map(|s| s.pins.iter()).map(|p| p.id.as_str()).collect();
    let mut targets: Vec<String> = Vec::new();
    for id in ids {
        if known.contains(id.as_str()) {
            targets.push(id.clone());
        } else if !sheet_pins.contains(id.as_str()) {
            return Err(vec![CheckResult::fail("ops_unknown_item", id, "no symbol, wire, label, text, junction, line, sheet or graphic with this id to lock")]);
        }
    }
    let mut changed = false;
    for id in &targets {
        changed |= sch.extras.set_locked(id, locked);
    }
    if !changed && !targets.is_empty() {
        // Nothing to do (already in that state): refuse, so the undo stack never records a step that changed nothing.
        return Err(vec![CheckResult::fail("ops_lock_unchanged", &targets[0], format!("every listed item is already {}", if locked { "locked" } else { "unlocked" }))]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Design, Point, Provenance, SchematicText, Wire};
    use eda_model::ConstraintModel;

    pub(super) fn design_with_schematic(sch: SchematicSection) -> Design {
        Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    pub(super) fn empty_sheet() -> SchematicSection {
        crate::empty_schematic_section()
    }

    pub(super) fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    #[test]
    fn lock_unlock_round_trips_and_refuses_no_ops() {
        let mut sch = empty_sheet();
        sch.wires.push(Wire { id: "wire_a".into(), net: "N".into(), pins: vec![], pts: vec![p(0, 0), p(1000, 0)], bus: false });
        sch.texts.push(SchematicText { id: "txt_a".into(), content: "hi".into(), at: p(0, 0), angle: 0, size_um: 1270 });
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(sch), &model, 100, 300);
        assert_eq!(Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: true }).domain(), crate::Domain::Schematic);
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: true })).unwrap();
        assert_eq!(b.design().schematic.as_ref().unwrap().extras.locked, vec!["txt_a".to_string(), "wire_a".to_string()]);
        // Locking again changes nothing: refused.
        let err = b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: true })).unwrap_err();
        assert_eq!(err[0].check, "ops_lock_unchanged");
        // One of two already locked still counts as a change.
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: false })).unwrap();
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: true })).unwrap();
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: false })).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().extras.locked.is_empty());
    }

    #[test]
    fn lock_refuses_unknown_and_empty() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, 100, 300);
        assert_eq!(b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec![], locked: true })).unwrap_err()[0].check, "ops_bad_lock");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["nope".into()], locked: true })).unwrap_err()[0].check, "ops_unknown_item");
    }

    #[test]
    fn sch_edit_json_shape_is_op_plus_verb() {
        let c = Cmd::sch(SchCmd::SetLocked { ids: vec!["a".into()], locked: true });
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json, serde_json::json!({ "op": "sch_edit", "verb": "set_locked", "ids": ["a"], "locked": true }));
        let back: Cmd = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }
}
