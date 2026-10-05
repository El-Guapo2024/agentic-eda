//! The schematic editor's edit and drawing tools that act on items already on the sheet, ported from
//! KiCad's `eeschema/tools` (`SCH_EDIT_TOOL`, `SCH_DRAWING_TOOLS`, `SCH_POINT_EDITOR`, ...).
//!
//! One `Cmd` variant carries all of them -- `Cmd::SchEdit(SchCmd)`, `{"op": "sch_edit", "verb": ...}` on the
//! wire -- so this family can grow without touching the flat verb list. Every verb is a pure edit of
//! `design.schematic` (or one of the `sheet_contents` screens) and therefore undoable like any other; each one
//! cites the KiCad function it ports (eeschema at 8303b2ad).

use crate::{Board, Cmd};
use eda_model::ir::{Point, SchematicSection};
use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
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
    /// Draw a rectangle, circle, arc, bezier, polygon, text box, rule area or directive label
    /// (`SCH_DRAWING_TOOLS::DrawShape` / `DrawRuleArea` / the class-label branch of `TwoClickPlace`): appended to
    /// the drawing order; its id is assigned here. Degenerate shapes (zero size, fewer points than the kind
    /// needs, collinear arc points) are refused.
    AddGraphic { graphic: SchGraphic },
    /// Delete one drawn graphic (`SCH_EDIT_TOOL::DoDelete`).
    DeleteGraphic { id: String },
    /// Replace a drawn graphic in place, keeping its place in the drawing order and its lock (what the
    /// properties dialogs and the point editor commit).
    EditGraphic { id: String, graphic: SchGraphic },
    /// Delete a placed hierarchical sheet (`SCH_EDIT_TOOL::DoDelete` on a `SCH_SHEET`): the sheet symbol and its
    /// pins go; the file's content stays in the project (another sheet may use it, or it may be placed again).
    DeleteSheet { id: String },
}

impl SchCmd {
    /// The ids this verb names, for the activity log.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            SchCmd::SetLocked { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddGraphic { .. } => vec!["graphic"],
            SchCmd::DeleteGraphic { id } | SchCmd::EditGraphic { id, .. } | SchCmd::DeleteSheet { id } => vec![id.as_str()],
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        match self {
            SchCmd::SetLocked { ids, locked } => format!("schematic {} {}", if *locked { "lock" } else { "unlock" }, ids.join(" ")),
            SchCmd::AddGraphic { graphic } => format!("schematic draw {}", graphic_kind_name(&graphic.shape)),
            SchCmd::DeleteGraphic { id } => format!("schematic delete-graphic {id}"),
            SchCmd::EditGraphic { id, .. } => format!("schematic edit-graphic {id}"),
            SchCmd::DeleteSheet { id } => format!("schematic delete-sheet {id}"),
        }
    }

    /// The activity kind this verb files under (`crates/cli/src/board.rs`).
    pub fn kind(&self) -> &'static str {
        match self {
            SchCmd::SetLocked { .. } => "schematic-lock",
            SchCmd::AddGraphic { .. } | SchCmd::DeleteGraphic { .. } | SchCmd::EditGraphic { .. } => "schematic-graphic",
            SchCmd::DeleteSheet { .. } => "schematic-sheet",
        }
    }
}

impl<'a> Board<'a> {
    /// Apply one [`SchCmd`] to the schematic.
    pub(crate) fn apply_sch_edit(&mut self, cmd: &SchCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            SchCmd::SetLocked { ids, locked } => set_locked(self.schematic_mut()?, ids, *locked),
            SchCmd::AddGraphic { graphic } => add_graphic(self.schematic_mut_or_create(), graphic.clone()),
            SchCmd::DeleteGraphic { id } => delete_graphic(self.schematic_mut()?, id),
            SchCmd::EditGraphic { id, graphic } => edit_graphic(self.schematic_mut()?, id, graphic.clone()),
            SchCmd::DeleteSheet { id } => delete_sheet(self.schematic_mut()?, id),
        }
    }
}

impl Cmd {
    /// A `Cmd` for one [`SchCmd`] (the wrapper callers and tests build).
    pub fn sch(cmd: SchCmd) -> Cmd {
        Cmd::SchEdit(cmd)
    }
}

fn graphic_kind_name(s: &SchGraphicKind) -> &'static str {
    match s {
        SchGraphicKind::Rectangle { .. } => "rectangle",
        SchGraphicKind::Circle { .. } => "circle",
        SchGraphicKind::Arc { .. } => "arc",
        SchGraphicKind::Bezier { .. } => "bezier",
        SchGraphicKind::Polygon { .. } => "polygon",
        SchGraphicKind::TextBox { .. } => "text-box",
        SchGraphicKind::RuleArea { .. } => "rule-area",
        SchGraphicKind::Directive { .. } => "directive-label",
    }
}

/// Is `a`, `b`, `c` a real arc (three non-collinear points)?
fn non_collinear(a: Point, b: Point, c: Point) -> bool {
    let cross = (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128;
    cross != 0
}

/// Refuse a graphic that could never be drawn: the shape-specific sanity `EndEdit` implies.
fn check_graphic(g: &SchGraphic) -> Result<(), Vec<CheckResult>> {
    let bad = |what: &str| Err(vec![CheckResult::fail("ops_bad_graphic", graphic_kind_name(&g.shape), what.to_string())]);
    if g.width_um < 0 {
        return bad("a stroke width cannot be negative");
    }
    match &g.shape {
        SchGraphicKind::Rectangle { start, end, corner_radius_um } => {
            if start.x == end.x || start.y == end.y {
                return bad("a rectangle needs two corners that differ in both x and y");
            }
            if *corner_radius_um < 0 {
                return bad("a corner radius cannot be negative");
            }
        }
        SchGraphicKind::Circle { radius_um, .. } => {
            if *radius_um <= 0 {
                return bad("a circle needs a positive radius");
            }
        }
        SchGraphicKind::Arc { start, mid, end } => {
            if !non_collinear(*start, *mid, *end) {
                return bad("an arc needs three points that are not in a line");
            }
        }
        SchGraphicKind::Bezier { start, c1, c2, end } => {
            if start == end && start == c1 && start == c2 {
                return bad("a bezier needs at least two different points");
            }
        }
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => {
            let mut distinct = pts.clone();
            distinct.sort();
            distinct.dedup();
            if distinct.len() < 3 {
                return bad("a polygon needs at least three different points");
            }
        }
        SchGraphicKind::TextBox { start, end, size_um, .. } => {
            if start.x == end.x || start.y == end.y {
                return bad("a text box needs a width and a height");
            }
            if *size_um <= 0 {
                return bad("a text box needs a positive text size");
            }
        }
        SchGraphicKind::Directive { pin_length_um, .. } => {
            if *pin_length_um < 0 {
                return bad("a directive label's pole cannot have a negative length");
            }
        }
    }
    Ok(())
}

fn add_graphic(sch: &mut SchematicSection, mut graphic: SchGraphic) -> Result<(), Vec<CheckResult>> {
    check_graphic(&graphic)?;
    graphic.id = String::new();
    sch.extras.graphics.push(graphic);
    sch.assign_missing_ids();
    Ok(())
}

fn delete_graphic(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let before = sch.extras.graphics.len();
    sch.extras.graphics.retain(|g| g.id != id);
    if sch.extras.graphics.len() == before {
        return Err(vec![CheckResult::fail("ops_unknown_graphic", id, "no drawn graphic with this id")]);
    }
    sch.extras.set_locked(id, false);
    Ok(())
}

fn edit_graphic(sch: &mut SchematicSection, id: &str, mut graphic: SchGraphic) -> Result<(), Vec<CheckResult>> {
    check_graphic(&graphic)?;
    let Some(i) = sch.extras.graphics.iter().position(|g| g.id == id) else {
        return Err(vec![CheckResult::fail("ops_unknown_graphic", id, "no drawn graphic with this id")]);
    };
    graphic.id = id.to_string();
    if sch.extras.graphics[i] == graphic {
        return Err(vec![CheckResult::fail("ops_graphic_unchanged", id, "the graphic already has these properties")]);
    }
    sch.extras.graphics[i] = graphic;
    Ok(())
}

fn delete_sheet(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let before = sch.sheets.len();
    sch.sheets.retain(|s| s.id != id);
    if sch.sheets.len() == before {
        return Err(vec![CheckResult::fail("ops_unknown_sheet", id, "no sheet with this id")]);
    }
    sch.extras.set_locked(id, false);
    Ok(())
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
    use eda_model::ir::{Design, Provenance, SchematicText, Wire};
    use eda_model::sch_extras::SchFill;
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

    fn rectangle(x0: i64, y0: i64, x1: i64, y1: i64) -> SchGraphic {
        SchGraphic::new(SchGraphicKind::Rectangle { start: p(x0, y0), end: p(x1, y1), corner_radius_um: 0 })
    }

    #[test]
    fn graphics_are_added_edited_locked_and_deleted() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, 100, 300);
        b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: rectangle(0, 0, 10_000, 5_000) })).unwrap();
        b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: SchGraphic::new(SchGraphicKind::Circle { center: p(1, 1), radius_um: 3_000 }) })).unwrap();
        let ids: Vec<String> = b.design().schematic.as_ref().unwrap().extras.graphics.iter().map(|g| g.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids[0].starts_with("shp_") && ids[0] != ids[1]);

        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec![ids[0].clone()], locked: true })).unwrap();
        let mut edited = rectangle(0, 0, 20_000, 5_000);
        edited.fill = SchFill::Background;
        b.apply(&Cmd::sch(SchCmd::EditGraphic { id: ids[0].clone(), graphic: edited.clone() })).unwrap();
        let sch = b.design().schematic.as_ref().unwrap();
        assert_eq!(sch.extras.graphics[0].id, ids[0], "an edit keeps the id");
        assert_eq!(sch.extras.graphics[0].fill, SchFill::Background);
        assert!(sch.extras.is_locked(&ids[0]), "and the lock");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditGraphic { id: ids[0].clone(), graphic: edited })).unwrap_err()[0].check, "ops_graphic_unchanged");

        b.apply(&Cmd::sch(SchCmd::DeleteGraphic { id: ids[0].clone() })).unwrap();
        let sch = b.design().schematic.as_ref().unwrap();
        assert_eq!(sch.extras.graphics.len(), 1);
        assert!(!sch.extras.is_locked(&ids[0]), "a deleted item's lock goes with it");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteGraphic { id: ids[0].clone() })).unwrap_err()[0].check, "ops_unknown_graphic");
    }

    #[test]
    fn degenerate_graphics_are_refused() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, 100, 300);
        let refuse = |b: &mut Board, g: SchGraphic| assert_eq!(b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: g })).unwrap_err()[0].check, "ops_bad_graphic");
        refuse(&mut b, rectangle(0, 0, 0, 5_000));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Circle { center: p(0, 0), radius_um: 0 }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Arc { start: p(0, 0), mid: p(1, 1), end: p(2, 2) }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Polygon { pts: vec![p(0, 0), p(1, 0), p(1, 0)] }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(0, 0), p(1, 0)], exclude_from_sim: false, exclude_from_bom: false, exclude_from_board: false, dnp: false }));
        let mut negative = rectangle(0, 0, 1, 1);
        negative.width_um = -5;
        refuse(&mut b, negative);
        assert!(b.design().schematic.as_ref().unwrap().extras.graphics.is_empty());
    }

    #[test]
    fn deleting_a_sheet_removes_its_symbol_and_pins_but_not_the_file_content() {
        use eda_model::ir::{LabelShape, SheetInstance, SheetPin};
        let mut sch = empty_sheet();
        sch.sheets.push(SheetInstance { id: "sheet_a".into(), name: "A".into(), file: "a.kicad_sch".into(), at: p(0, 0), size: (10_000, 10_000), pins: vec![SheetPin { id: "pin_a".into(), name: "X".into(), shape: LabelShape::Input, at: p(0, 5_000) }] });
        let model = ConstraintModel::default();
        let mut design = design_with_schematic(sch);
        design.sheet_contents = Some([("a.kicad_sch".to_string(), empty_sheet())].into_iter().collect());
        let mut b = Board::new(design, &model, 100, 300);
        b.apply(&Cmd::sch(SchCmd::DeleteSheet { id: "sheet_a".into() })).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().sheets.is_empty());
        assert!(b.design().sheet_contents.as_ref().unwrap().contains_key("a.kicad_sch"));
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteSheet { id: "sheet_a".into() })).unwrap_err()[0].check, "ops_unknown_sheet");
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
