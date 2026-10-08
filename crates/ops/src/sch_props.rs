//! Properties (`E`, a double-click) of the schematic items that had no way to be edited after they were placed: labels, free texts, hierarchical
//! sheets, wires, buses, bus entries, graphic lines and junctions. Ported from `SCH_EDIT_TOOL::Properties` and the dialogs it opens
//! (eeschema/tools/sch_edit_tool.cpp, eeschema/dialogs/dialog_label_properties.cpp, dialog_text_properties.cpp, dialog_sheet_properties.cpp,
//! dialog_wire_bus_properties.cpp, dialog_junction_props.cpp at 8303b2ad).
//!
//! Each dialog is one verb here -- [`SchCmd::EditLabel`], [`SchCmd::EditText`], [`SchCmd::EditSheet`], [`SchCmd::SetStroke`] -- so an OK is one undo
//! step; the properties of a symbol, a sheet pin, a text box, a shape, a rule area and a directive label already have theirs (`Cmd::SetSymbolFields`,
//! [`SchCmd::EditSheetPin`], [`SchCmd::EditGraphic`]). A field left out of a verb is unchanged, and a verb that would change nothing is refused, so the undo
//! stack never records a step that did nothing.
//!
//! What the dialogs have that the drawing model has no place for is not here: fonts, bold and italic, text colours, a sheet's border and fill, its extra fields and
//! its exclusion flags, a label's fields (netclass, intersheet references).

use crate::{empty_schematic_section, Board};
use eda_model::ir::{LabelKind, LabelShape, Millideg, SchematicSection, Um};
use eda_model::sch_extras::{JunctionLook, LabelSpin, SchColor, SchLineStyle, SchStroke};
use eda_model::CheckResult;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM` in `DIALOG_LABEL_PROPERTIES` and `DIALOG_TEXT_PROPERTIES`: "Don't allow text to disappear; it can be difficult to
/// correct if you can't select it" (`m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`).
const TEXT_SIZE_MIN_UM: Um = 10;
const TEXT_SIZE_MAX_UM: Um = 1_000_000;

/// `COLOR4D::UNSPECIFIED` is all four channels zero: the colour of the item's layer.
fn specified(c: SchColor) -> Option<SchColor> {
    (c != SchColor { r: 0, g: 0, b: 0, a: 0 }).then_some(c)
}

/// A sheet's file as `EnsureFileExtension( name, KiCadSchematicFileExtension )` makes it: a bare file name (no folders, as `Cmd::AddSheet` requires)
/// ending in `.kicad_sch`.
fn sheet_file_name(file: &str) -> Result<String, Vec<CheckResult>> {
    let file = file.trim();
    if file.is_empty() {
        return Err(fail("ops_bad_sheet", "sheet", "A sheet must have a valid file name."));
    }
    if file.contains('/') || file.contains('\\') {
        return Err(fail("ops_bad_sheet", file, "a sheet file is a bare file name (no folders)"));
    }
    Ok(if file.ends_with(".kicad_sch") { file.to_string() } else { format!("{file}.kicad_sch") })
}

/// Does the screen `from` hold (through its sheets, and theirs) a placement of the screen `target`? How a sheet is kept from showing a file that shows it back.
fn holds(contents: &BTreeMap<String, SchematicSection>, from: &str, target: &str, seen: &mut BTreeSet<String>) -> bool {
    if !seen.insert(from.to_string()) {
        return false;
    }
    let Some(screen) = contents.get(from) else { return false };
    screen.sheets.iter().any(|s| s.file == target || holds(contents, &s.file, target, seen))
}

impl<'a> Board<'a> {
    /// Label Properties: see [`SchCmd::EditLabel`](crate::sch_edit::SchCmd::EditLabel).
    pub(crate) fn edit_sch_label(&mut self, id: &str, text: Option<&str>, shape: Option<LabelShape>, spin: Option<LabelSpin>) -> Result<(), Vec<CheckResult>> {
        let text = text.map(str::trim);
        if text == Some("") {
            // DIALOG_LABEL_PROPERTIES::TransferDataFromWindow: `if( text.IsEmpty() && !m_currentLabel->IsNew() )`
            return Err(fail("ops_bad_label", id, "Label can not be empty."));
        }
        let sch = self.schematic_mut()?;
        let Some(label) = sch.labels.iter_mut().find(|l| l.id == id) else {
            return Err(fail("ops_unknown_item", id, "no label with this id"));
        };
        // the shape sizer is hidden for a plain label (`m_shapeSizer`): refused before anything changes
        if shape.is_some() && label.kind == LabelKind::Local {
            return Err(fail("ops_bad_label_shape", id, "a local label has no shape"));
        }
        let mut changed = false;
        if let Some(t) = text {
            if label.net != t {
                label.net = t.to_string();
                changed = true;
            }
        }
        if let (Some(new_shape), LabelKind::Global { shape } | LabelKind::Hierarchical { shape }) = (shape, &mut label.kind) {
            if *shape != new_shape {
                *shape = new_shape;
                changed = true;
            }
        }
        if let Some(new_spin) = spin {
            // `if( m_currentLabel->GetSpinStyle() != selectedSpinStyle ) SetSpinStyle( selectedSpinStyle )`
            if sch.extras.label_spins.insert(id.to_string(), new_spin) != Some(new_spin) {
                changed = true;
            }
        }
        if !changed {
            return Err(fail("ops_label_unchanged", id, "the label already has these properties"));
        }
        Ok(())
    }

    /// Text Properties: see [`SchCmd::EditText`](crate::sch_edit::SchCmd::EditText).
    pub(crate) fn edit_sch_text(&mut self, id: &str, text: Option<&str>, size_um: Option<Um>, angle: Option<Millideg>) -> Result<(), Vec<CheckResult>> {
        if text == Some("") {
            return Err(fail("ops_bad_text", id, "Text can not be empty."));
        }
        if let Some(size) = size_um {
            if !(TEXT_SIZE_MIN_UM..=TEXT_SIZE_MAX_UM).contains(&size) {
                return Err(fail("ops_bad_text_size", id, "The text size must be between 0.01 and 1000 mm."));
            }
        }
        if angle.is_some_and(|a| a >= 360_000) {
            return Err(fail("ops_bad_text_angle", id, "a text angle is below 360 degrees"));
        }
        let sch = self.schematic_mut()?;
        let Some(t) = sch.texts.iter_mut().find(|t| t.id == id) else {
            return Err(fail("ops_unknown_item", id, "no text with this id"));
        };
        let before = (t.content.clone(), t.size_um, t.angle);
        if let Some(text) = text {
            t.content = text.to_string();
        }
        if let Some(size) = size_um {
            t.size_um = size;
        }
        if let Some(angle) = angle {
            t.angle = angle;
        }
        if before == (t.content.clone(), t.size_um, t.angle) {
            return Err(fail("ops_text_unchanged", id, "the text already has these properties"));
        }
        Ok(())
    }

    /// Sheet Properties: see [`SchCmd::EditSheet`](crate::sch_edit::SchCmd::EditSheet).
    pub(crate) fn edit_sch_sheet(&mut self, id: &str, name: Option<&str>, file: Option<&str>) -> Result<(), Vec<CheckResult>> {
        let new_name = match name.map(str::trim) {
            Some("") => return Err(fail("ops_bad_sheet", id, "a sheet needs a name")),
            other => other.map(str::to_string),
        };
        let new_file = file.map(sheet_file_name).transpose()?;
        let (cur_name, cur_file, taken) = {
            let sch = self.schematic()?;
            let Some(s) = sch.sheets.iter().find(|s| s.id == id) else {
                return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
            };
            (s.name.clone(), s.file.clone(), sch.sheets.iter().filter(|o| o.id != id).map(|o| o.name.clone()).collect::<Vec<_>>())
        };
        let renamed = new_name.as_ref().is_some_and(|n| *n != cur_name);
        let relinked = new_file.as_ref().is_some_and(|f| *f != cur_file);
        if !renamed && !relinked {
            return Err(fail("ops_sheet_unchanged", id, "the sheet already has these properties"));
        }
        if let Some(n) = new_name.as_ref().filter(|_| renamed) {
            if taken.contains(n) {
                return Err(fail("ops_sheet_name_taken", n, format!("a sheet named '{n}' already exists on this sheet")));
            }
        }
        if let Some(new) = new_file.as_ref().filter(|_| relinked) {
            // `SCH_EDIT_FRAME::ChangeSheetFile`: a sheet may not show a file that shows this sheet's own screen back.
            if let Some(here) = &self.focus {
                let none = BTreeMap::new();
                let contents = self.design.sheet_contents.as_ref().unwrap_or(&none);
                if new == here || holds(contents, new, here, &mut BTreeSet::new()) {
                    return Err(fail("ops_sheet_recursion", new, format!("'{new}' shows this sheet's own screen: a sheet cannot hold itself")));
                }
            }
            // The other placements of the file being left, on the root and on every screen.
            let others = {
                let root = self.design.schematic.iter();
                let screens = self.design.sheet_contents.iter().flat_map(|c| c.values());
                root.chain(screens).flat_map(|s| s.sheets.iter()).filter(|s| s.file == cur_file && s.id != id).count()
            };
            let contents = self.design.sheet_contents.get_or_insert_with(BTreeMap::new);
            if !contents.contains_key(new) {
                // A name the project does not have: "Create new file with the contents of ..." when another sheet still shows the old file, else the
                // file is renamed (`renameFile`); a screen that was never read starts empty, as `Cmd::AddSheet` makes one.
                let content = match contents.get(&cur_file).cloned() {
                    Some(old) => {
                        if others == 0 && self.focus.as_deref() != Some(cur_file.as_str()) {
                            contents.remove(&cur_file);
                        }
                        old
                    }
                    None => empty_schematic_section(),
                };
                contents.insert(new.clone(), content);
            }
        }
        let sch = self.schematic_mut()?;
        let Some(s) = sch.sheets.iter_mut().find(|s| s.id == id) else {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        };
        if let Some(n) = new_name {
            s.name = n;
        }
        if let Some(f) = new_file {
            s.file = f;
        }
        Ok(())
    }

    /// Wire/Bus, Line and Junction Properties: see [`SchCmd::SetStroke`](crate::sch_edit::SchCmd::SetStroke).
    pub(crate) fn set_sch_stroke(&mut self, ids: &[String], width_um: Option<Um>, style: Option<SchLineStyle>, color: Option<SchColor>, diameter_um: Option<Um>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "stroke", "no items given"));
        }
        let sch = self.schematic_mut()?;
        // What each item is, before anything changes: all or nothing.
        enum Kind {
            /// A wire, a bus or a bus entry (`SCH_LINE`, `SCH_BUS_WIRE_ENTRY`): all of the stroke is in `extras.strokes`.
            Stroked,
            /// A graphic line on the notes layer: its width is `SchLine::width_um`, the rest is in `extras.strokes`.
            Notes(usize),
            Junction,
        }
        let mut kinds: Vec<Kind> = Vec::with_capacity(ids.len());
        for id in ids {
            kinds.push(if sch.wires.iter().any(|w| &w.id == id) || sch.bus_entries.iter().any(|b| &b.id == id) {
                Kind::Stroked
            } else if let Some(i) = sch.lines.iter().position(|l| &l.id == id) {
                Kind::Notes(i)
            } else if sch.junctions.iter().any(|j| &j.id == id) {
                Kind::Junction
            } else {
                return Err(fail("ops_unknown_item", id, "no wire, bus, bus entry, graphic line or junction with this id"));
            });
        }
        let color = color.map(specified);
        let mut changed = false;
        for (id, kind) in ids.iter().zip(&kinds) {
            match kind {
                Kind::Stroked | Kind::Notes(_) => {
                    let mut stroke: SchStroke = sch.extras.strokes.get(id).copied().unwrap_or_default();
                    let before = stroke;
                    if let (Some(w), Kind::Stroked) = (width_um, kind) {
                        // `int width = std::max( 0, m_wireWidth.GetIntValue() )`
                        stroke.width_um = w.max(0);
                    }
                    if let Some(s) = style {
                        stroke.style = s;
                    }
                    if let Some(c) = color {
                        stroke.color = c;
                    }
                    if let (Some(w), Kind::Notes(i)) = (width_um, kind) {
                        let w = w.max(0);
                        changed |= sch.lines[*i].width_um != w;
                        sch.lines[*i].width_um = w;
                    }
                    changed |= stroke != before;
                    if stroke.is_default() {
                        sch.extras.strokes.remove(id);
                    } else {
                        sch.extras.strokes.insert(id.clone(), stroke);
                    }
                }
                Kind::Junction => {
                    let mut look: JunctionLook = sch.extras.junction_looks.get(id).copied().unwrap_or_default();
                    let before = look;
                    if let Some(d) = diameter_um {
                        look.diameter_um = d.max(0);
                    }
                    if let Some(c) = color {
                        look.color = c;
                    }
                    changed |= look != before;
                    if look.is_default() {
                        sch.extras.junction_looks.remove(id);
                    } else {
                        sch.extras.junction_looks.insert(id.clone(), look);
                    }
                }
            }
        }
        if !changed {
            // The entries touched above are exactly what they were (an unchanged value is re-inserted as it was).
            return Err(fail("ops_stroke_unchanged", &ids[0], "the items already have these properties"));
        }
        // Entries of items that are gone (deleted, merged into a neighbour) go with them.
        let live: BTreeSet<&str> = sch.wires.iter().map(|w| w.id.as_str()).chain(sch.bus_entries.iter().map(|b| b.id.as_str())).chain(sch.lines.iter().map(|l| l.id.as_str())).collect();
        sch.extras.strokes.retain(|id, _| live.contains(id.as_str()));
        let junctions: BTreeSet<&str> = sch.junctions.iter().map(|j| j.id.as_str()).collect();
        sch.extras.junction_looks.retain(|id, _| junctions.contains(id.as_str()));
        Ok(())
    }
}
