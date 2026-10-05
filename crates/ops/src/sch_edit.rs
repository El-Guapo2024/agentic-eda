//! The schematic editor's edit and drawing tools that act on items already on the sheet, ported from
//! KiCad's `eeschema/tools` (`SCH_EDIT_TOOL`, `SCH_DRAWING_TOOLS`, `SCH_POINT_EDITOR`, ...).
//!
//! One `Cmd` variant carries all of them -- `Cmd::SchEdit(SchCmd)`, `{"op": "sch_edit", "verb": ...}` on the
//! wire -- so this family can grow without touching the flat verb list. Every verb is a pure edit of
//! `design.schematic` (or one of the `sheet_contents` screens) and therefore undoable like any other; each one
//! cites the KiCad function it ports (eeschema at 8303b2ad).

use crate::{Board, Cmd};
use eda_model::ir::{LabelShape, Point, SchematicSection, SheetInstance, SheetPin};
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
    /// Put a pin on a placed sheet's border (`SCH_SHEET::AddPin`, as `SCH_DRAWING_TOOLS::TwoClickPlace` and
    /// `AutoPlaceAllSheetPins` do): `name` is the hierarchical label it stands for in the sheet's file, `at` a
    /// point on the sheet's border (`SCH_SHEET_PIN::ConstrainOnEdge` is the caller's). An empty name, an unknown
    /// sheet or a point off the border is refused.
    AddSheetPin { sheet: String, name: String, shape: LabelShape, at: Point },
    /// Delete one sheet pin (`SCH_EDIT_TOOL::DoDelete` on a pin; what `CleanupSheetPins` and the sync dialog send).
    DeleteSheetPin { id: String },
    /// Rename, reshape or move one sheet pin (`SCH_SHEET_PIN` properties and the sync dialog's "update"); a field left
    /// out is unchanged. The new position must still be on the border.
    EditSheetPin {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        shape: Option<LabelShape>,
        #[serde(default)]
        at: Option<Point>,
    },
    /// Change Symbol (`DIALOG_CHANGE_SYMBOLS::processSymbols`, `MODE::CHANGE`): every placed unit of the reference `id`
    /// takes the library symbol `lib_id`; position, orientation, unit and fields stay (fields are the caller's to reset).
    /// Refused when no library symbol has that id, or when the new symbol has fewer units than one of the placed ones
    /// ("new symbol has too few units"), or when the reference already uses it.
    ChangeSymbol { id: String, lib_id: String },
    /// Update Symbol(s) from Library (`DIALOG_CHANGE_SYMBOLS`, `MODE::UPDATE`): the symbols placed with these library ids start
    /// resolving from the project's edited library symbol of that id (`LibrarySymbol::published`, what the Symbol Editor's
    /// "Update Symbol in Schematic" sets). Library ids with no edited symbol, or one already published, are skipped; refused when
    /// none was left to update.
    UpdateLibrarySymbols { lib_ids: Vec<String> },
}

impl SchCmd {
    /// The ids this verb names, for the activity log.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            SchCmd::SetLocked { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddGraphic { .. } => vec!["graphic"],
            SchCmd::UpdateLibrarySymbols { lib_ids } => lib_ids.iter().map(String::as_str).collect(),
            SchCmd::DeleteGraphic { id } | SchCmd::EditGraphic { id, .. } | SchCmd::DeleteSheet { id } | SchCmd::DeleteSheetPin { id } | SchCmd::EditSheetPin { id, .. } | SchCmd::ChangeSymbol { id, .. } => vec![id.as_str()],
            SchCmd::AddSheetPin { sheet, .. } => vec![sheet.as_str()],
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
            SchCmd::AddSheetPin { sheet, name, .. } => format!("schematic sheet-pin add {sheet} {name}"),
            SchCmd::DeleteSheetPin { id } => format!("schematic sheet-pin delete {id}"),
            SchCmd::EditSheetPin { id, .. } => format!("schematic sheet-pin edit {id}"),
            SchCmd::ChangeSymbol { id, lib_id } => format!("schematic change-symbol {id} {lib_id}"),
            SchCmd::UpdateLibrarySymbols { lib_ids } => format!("schematic update-symbols {}", lib_ids.join(" ")),
        }
    }

    /// The activity kind this verb files under (`crates/cli/src/board.rs`).
    pub fn kind(&self) -> &'static str {
        match self {
            SchCmd::SetLocked { .. } => "schematic-lock",
            SchCmd::AddGraphic { .. } | SchCmd::DeleteGraphic { .. } | SchCmd::EditGraphic { .. } => "schematic-graphic",
            SchCmd::DeleteSheet { .. } | SchCmd::AddSheetPin { .. } | SchCmd::DeleteSheetPin { .. } | SchCmd::EditSheetPin { .. } => "schematic-sheet",
            SchCmd::ChangeSymbol { .. } | SchCmd::UpdateLibrarySymbols { .. } => "schematic-symbol",
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
            SchCmd::AddSheetPin { sheet, name, shape, at } => add_sheet_pin(self.schematic_mut()?, sheet, name, *shape, *at),
            SchCmd::DeleteSheetPin { id } => delete_sheet_pin(self.schematic_mut()?, id),
            SchCmd::EditSheetPin { id, name, shape, at } => edit_sheet_pin(self.schematic_mut()?, id, name.as_deref(), *shape, *at),
            SchCmd::ChangeSymbol { id, lib_id } => self.change_symbol(id, lib_id),
            SchCmd::UpdateLibrarySymbols { lib_ids } => self.update_library_symbols(lib_ids),
        }
    }

    fn update_library_symbols(&mut self, lib_ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let mut updated = 0usize;
        if let Some(lib) = self.design.symbol_library.as_mut() {
            for sym in lib.symbols.iter_mut().filter(|s| lib_ids.contains(&s.lib_id) && !s.published) {
                sym.published = true;
                updated += 1;
            }
        }
        if updated == 0 {
            return Err(vec![CheckResult::fail("ops_nothing_to_update", lib_ids.first().map(String::as_str).unwrap_or("symbols"), "no edited library symbol is waiting to be updated from")]);
        }
        Ok(())
    }

    /// How many units the library symbol `lib_id` has: the project's own (edited) symbol first, then the libraries and builtin table
    /// `ConstraintModel::symbol_of` resolves. `None` when no library symbol has that id.
    fn library_unit_count(&self, lib_id: &str) -> Option<u32> {
        if let Some(sym) = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)) {
            return Some(sym.unit_count.max(1));
        }
        self.model.symbol_of(lib_id).map(|s| s.unit_count.max(1))
    }

    fn change_symbol(&mut self, id: &str, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        let lib_id = lib_id.trim();
        if lib_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", id, "a symbol needs a library id")]);
        }
        let Some(unit_count) = self.library_unit_count(lib_id) else {
            return Err(vec![CheckResult::fail("ops_unknown_lib_symbol", lib_id, "symbol not found")]);
        };
        let sch = self.schematic_mut()?;
        let placed: Vec<usize> = sch.symbols.iter().enumerate().filter(|(_, s)| s.id == id).map(|(i, _)| i).collect();
        if placed.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol with this reference on the sheet")]);
        }
        if let Some(&i) = placed.iter().find(|&&i| sch.symbols[i].unit > unit_count) {
            return Err(vec![CheckResult::fail("ops_too_few_units", id, format!("new symbol has too few units: unit {} is placed but {lib_id} has {unit_count}", sch.symbols[i].unit))]);
        }
        if placed.iter().all(|&i| sch.symbols[i].lib_id == lib_id) {
            return Err(vec![CheckResult::fail("ops_symbol_unchanged", id, format!("the symbol already uses {lib_id}"))]);
        }
        for i in placed {
            sch.symbols[i].lib_id = lib_id.to_string();
        }
        Ok(())
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

/// Is `at` on the border of the sheet rectangle (`SCH_SHEET_PIN` is always `ConstrainOnEdge`-clamped there)?
fn on_sheet_border(sheet: &SheetInstance, at: Point) -> bool {
    let (w, h) = sheet.size;
    let (left, top, right, bottom) = (sheet.at.x, sheet.at.y, sheet.at.x + w, sheet.at.y + h);
    let in_x = (left..=right).contains(&at.x);
    let in_y = (top..=bottom).contains(&at.y);
    (in_y && (at.x == left || at.x == right)) || (in_x && (at.y == top || at.y == bottom))
}

fn add_sheet_pin(sch: &mut SchematicSection, sheet: &str, name: &str, shape: LabelShape, at: Point) -> Result<(), Vec<CheckResult>> {
    if name.trim().is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_sheet_pin", sheet, "a sheet pin needs a name")]);
    }
    let Some(s) = sch.sheets.iter_mut().find(|s| s.id == sheet) else {
        return Err(vec![CheckResult::fail("ops_unknown_sheet", sheet, "no sheet with this id")]);
    };
    if !on_sheet_border(s, at) {
        return Err(vec![CheckResult::fail("ops_sheet_pin_off_border", sheet, "a sheet pin sits on the border of its sheet")]);
    }
    s.pins.push(SheetPin { id: String::new(), name: name.to_string(), shape, at });
    sch.assign_missing_ids();
    Ok(())
}

fn find_sheet_pin<'a>(sch: &'a mut SchematicSection, id: &str) -> Result<(&'a mut SheetInstance, usize), Vec<CheckResult>> {
    for s in sch.sheets.iter_mut() {
        if let Some(i) = s.pins.iter().position(|p| p.id == id) {
            return Ok((s, i));
        }
    }
    Err(vec![CheckResult::fail("ops_unknown_sheet_pin", id, "no sheet pin with this id")])
}

fn delete_sheet_pin(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let (s, i) = find_sheet_pin(sch, id)?;
    s.pins.remove(i);
    Ok(())
}

fn edit_sheet_pin(sch: &mut SchematicSection, id: &str, name: Option<&str>, shape: Option<LabelShape>, at: Option<Point>) -> Result<(), Vec<CheckResult>> {
    if name.is_some_and(|n| n.trim().is_empty()) {
        return Err(vec![CheckResult::fail("ops_bad_sheet_pin", id, "a sheet pin needs a name")]);
    }
    let (s, i) = find_sheet_pin(sch, id)?;
    if let Some(p) = at {
        if !on_sheet_border(s, p) {
            return Err(vec![CheckResult::fail("ops_sheet_pin_off_border", id, "a sheet pin sits on the border of its sheet")]);
        }
    }
    let pin = &mut s.pins[i];
    let before = pin.clone();
    if let Some(n) = name {
        pin.name = n.to_string();
    }
    if let Some(sh) = shape {
        pin.shape = sh;
    }
    if let Some(p) = at {
        pin.at = p;
    }
    if pin.name == before.name && pin.shape == before.shape && pin.at == before.at {
        return Err(vec![CheckResult::fail("ops_sheet_pin_unchanged", id, "the pin already has these properties")]);
    }
    // The id follows the pin's name and place (`SheetPin::id_seed`): a renamed or moved pin gets the id its new content hashes to.
    pin.id = String::new();
    sch.assign_missing_ids();
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
    use eda_model::ir::{LabelShape, SheetInstance};
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

    fn board_with_sheet() -> (ConstraintModel, Design) {
        let mut sch = empty_sheet();
        sch.sheets.push(SheetInstance { id: "sheet_a".into(), name: "A".into(), file: "a.kicad_sch".into(), at: p(10_000, 10_000), size: (20_000, 10_000), pins: vec![] });
        (ConstraintModel::default(), design_with_schematic(sch))
    }

    #[test]
    fn sheet_pins_are_added_on_the_border_edited_and_deleted() {
        let (model, design) = board_with_sheet();
        let mut b = Board::new(design, &model, 100, 300);
        // On the left edge, on the top edge (a corner counts for both).
        b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: "sheet_a".into(), name: "VIN".into(), shape: LabelShape::Input, at: p(10_000, 15_000) })).unwrap();
        b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: "sheet_a".into(), name: "CLK".into(), shape: LabelShape::Output, at: p(25_000, 10_000) })).unwrap();
        let pins = b.design().schematic.as_ref().unwrap().sheets[0].pins.clone();
        assert_eq!(pins.len(), 2);
        assert!(pins.iter().all(|pin| pin.id.starts_with("shpin_")));
        let vin = pins.iter().find(|pin| pin.name == "VIN").unwrap().clone();

        // Off the border, an empty name and an unknown sheet are refused.
        let add = |b: &mut Board, sheet: &str, name: &str, at: Point| b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: sheet.into(), name: name.into(), shape: LabelShape::Passive, at })).unwrap_err()[0].check.clone();
        assert_eq!(add(&mut b, "sheet_a", "X", p(15_000, 15_000)), "ops_sheet_pin_off_border");
        assert_eq!(add(&mut b, "sheet_a", " ", p(10_000, 12_000)), "ops_bad_sheet_pin");
        assert_eq!(add(&mut b, "nope", "X", p(10_000, 12_000)), "ops_unknown_sheet");

        // Renaming and reshaping, then a move along the edge; a no-op is refused.
        b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: vin.id.clone(), name: Some("VBUS".into()), shape: Some(LabelShape::Bidirectional), at: None })).unwrap();
        let renamed = b.design().schematic.as_ref().unwrap().sheets[0].pins.iter().find(|pin| pin.name == "VBUS").unwrap().clone();
        assert_eq!(renamed.shape, LabelShape::Bidirectional);
        assert_ne!(renamed.id, vin.id, "the id follows the pin's name and place");
        b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: renamed.id.clone(), name: None, shape: None, at: Some(p(10_000, 18_000)) })).unwrap();
        let moved = b.design().schematic.as_ref().unwrap().sheets[0].pins.iter().find(|pin| pin.name == "VBUS").unwrap().clone();
        assert_eq!(moved.at, p(10_000, 18_000));
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: moved.id.clone(), name: Some("VBUS".into()), shape: None, at: None })).unwrap_err()[0].check, "ops_sheet_pin_unchanged");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: moved.id.clone(), name: None, shape: None, at: Some(p(12_000, 12_000)) })).unwrap_err()[0].check, "ops_sheet_pin_off_border");

        b.apply(&Cmd::sch(SchCmd::DeleteSheetPin { id: moved.id.clone() })).unwrap();
        assert_eq!(b.design().schematic.as_ref().unwrap().sheets[0].pins.len(), 1);
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteSheetPin { id: moved.id })).unwrap_err()[0].check, "ops_unknown_sheet_pin");
    }

    #[test]
    fn change_symbol_swaps_the_library_symbol_of_every_unit_and_refuses_what_cannot_be() {
        use eda_model::ir::SymbolInstance;
        let mut sch = empty_sheet();
        let mk = |id: &str, lib: &str, unit: u32| SymbolInstance { id: id.into(), at: p(0, 0), rot: 0, mirrored: false, mirror_y: false, lib_id: lib.into(), unit, value: "10k".into(), footprint: String::new(), datasheet: String::new() };
        sch.symbols.push(mk("R1", "Device:R", 1));
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(sch), &model, 100, 300);
        b.apply(&Cmd::sch(SchCmd::ChangeSymbol { id: "R1".into(), lib_id: "Device:C".into() })).unwrap();
        let s = &b.design().schematic.as_ref().unwrap().symbols[0];
        assert_eq!((s.lib_id.as_str(), s.value.as_str(), s.unit), ("Device:C", "10k", 1), "the fields stay; resetting them is the caller's");
        let check = |b: &mut Board, id: &str, lib: &str| b.apply(&Cmd::sch(SchCmd::ChangeSymbol { id: id.into(), lib_id: lib.into() })).unwrap_err()[0].check.clone();
        assert_eq!(check(&mut b, "R1", "Device:C"), "ops_symbol_unchanged");
        assert_eq!(check(&mut b, "R1", "nowhere:X"), "ops_unknown_lib_symbol");
        assert_eq!(check(&mut b, "R1", ""), "ops_bad_symbol");
        assert_eq!(check(&mut b, "R9", "Device:R"), "ops_unknown_symbol");
    }

    #[test]
    fn update_library_symbols_publishes_edited_symbols_once() {
        use eda_model::ir::{LibrarySymbol, SymbolLibrarySection};
        let mut design = design_with_schematic(empty_sheet());
        design.symbol_library = Some(SymbolLibrarySection { symbols: vec![LibrarySymbol::new_empty("Device:R"), LibrarySymbol::new_empty("Device:C")] });
        let model = ConstraintModel::default();
        let mut b = Board::new(design, &model, 100, 300);
        let published = |b: &Board, id: &str| b.design().symbol_library.as_ref().unwrap().by_lib_id(id).unwrap().published;
        b.apply(&Cmd::sch(SchCmd::UpdateLibrarySymbols { lib_ids: vec!["Device:R".into(), "Device:Nowhere".into()] })).unwrap();
        assert!(published(&b, "Device:R") && !published(&b, "Device:C"));
        // Already published, unknown and empty requests have nothing to update.
        let err = |b: &mut Board, ids: Vec<&str>| b.apply(&Cmd::sch(SchCmd::UpdateLibrarySymbols { lib_ids: ids.into_iter().map(String::from).collect() })).unwrap_err()[0].check.clone();
        assert_eq!(err(&mut b, vec!["Device:R"]), "ops_nothing_to_update");
        assert_eq!(err(&mut b, vec!["Device:Nowhere"]), "ops_nothing_to_update");
        assert_eq!(err(&mut b, vec![]), "ops_nothing_to_update");
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
