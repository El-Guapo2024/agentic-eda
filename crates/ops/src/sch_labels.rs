//! Labels and text as they are placed: a global or hierarchical label that rotates itself to what it is connected to, and a free text placed with its
//! justification. Ported from eeschema at 8303b2ad:
//!
//! - `SCH_EDIT_FRAME::AutoRotateItem` (sch_edit_frame.cpp), which `SCH_DRAWING_TOOLS::TwoClickPlace` runs when a label is put down, `SCH_MOVE_TOOL`
//!   runs on every moved item and `DIALOG_LABEL_PROPERTIES::TransferDataFromWindow` runs when its "Auto" check is on;
//! - `SCH_SCREEN::GetLabelOrientationForPoint` (sch_screen.cpp), the choice of a spin from what overlaps the label's anchor, and `GetPinSpinStyle`
//!   (symb_transforms_utils.cpp), the spin a pin asks for;
//! - `SCH_DRAWING_TOOLS::createNewLabel` and `createNewText` (the properties a label or text is given as it is placed).
//!
//! The flag itself is `LabelLook::auto_rotate` (`SCH_LABEL_BASE::AutoRotateOnPlacement`): only a global label and a hierarchical label have it
//! (`AutoRotateOnPlacementSupported`), and KiCad does not write it to the file.

use crate::sch_move::effective_spin;
use crate::sch_scene::{on_segment, Item, Matrix, Scene};
use crate::Board;
use eda_model::ir::{LabelKind, Millideg, NetLabel, Point, SchematicText, TextJustify, TextVAlign, Nm};
use eda_model::sch_extras::{LabelLook, LabelSpin, SchTextJustify};
use eda_model::CheckResult;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM` of the label and text dialogs: "Don't allow text to disappear".
const TEXT_SIZE_MIN_NM: Nm = 10_000;
const TEXT_SIZE_MAX_NM: Nm = 1_000_000_000;

/// `AutoRotateOnPlacementSupported`: a global label and a hierarchical label rotate themselves, a plain label and a directive label do not.
pub(crate) fn auto_rotate_supported(kind: &LabelKind) -> bool {
    !kind.is_local()
}

/// The spin a pin asks of a label on its connection point (`GetPinSpinStyle`): away from the symbol's body. `body` is the way the pin's line runs from its
/// connection point to the body, already turned by the symbol's orientation, as a step on the sheet's axes (y down).
fn pin_spin_style(body: (i64, i64)) -> Option<LabelSpin> {
    // "if( aPin.GetOrientation() == PIN_ORIENTATION::PIN_RIGHT ) ret = SPIN_STYLE::LEFT" ... then the symbol's orientation and mirrors
    match (body.0.signum(), body.1.signum()) {
        (1, 0) => Some(LabelSpin::Left),
        (-1, 0) => Some(LabelSpin::Right),
        (0, -1) => Some(LabelSpin::Bottom),
        (0, 1) => Some(LabelSpin::Up),
        _ => None,
    }
}

impl<'m> Scene<'m> {
    /// `SCH_SCREEN::GetLabelOrientationForPoint( aPosition, aDefaultOrientation )`: the spin a label at `at` should have given what overlaps that point, `default`
    /// when nothing does. The C++ walks `Items().Overlapping( aPosition )`, which visits the item types in `KICAD_T` order (bus wire entries, lines, symbols) and
    /// lets each match overwrite the answer, so the last one wins: a pin over a wire end, a wire over a bus entry.
    pub(crate) fn label_orientation_for_point(&self, at: Point, default: LabelSpin) -> LabelSpin {
        let mut ret = default;

        // SCH_BUS_WIRE_ENTRY_T
        for b in &self.sch.bus_entries {
            let end = Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y };
            let (lo, hi) = (Point { x: b.at.x.min(end.x), y: b.at.y.min(end.y) }, Point { x: b.at.x.max(end.x), y: b.at.y.max(end.y) });
            if at.x < lo.x || at.x > hi.x || at.y < lo.y || at.y > hi.y {
                continue;
            }
            // `m_connected_bus_item`: the bus that one of the entry's two ends is on
            let Some(bus) = self.segs.iter().find(|s| !s.dead && s.bus && (on_segment(s.a, s.b, b.at) || on_segment(s.a, s.b, end))) else { continue };
            // "bus->Angle().AsDegrees() == 90.0": the angle of the line as drawn, not normalised, so only a bus drawn downwards or to the right counts
            let (dx, dy) = (bus.b.x - bus.a.x, bus.b.y - bus.a.y);
            if dx == 0 && dy > 0 {
                // "bus is vertical -> label shall be horizontal and shall be placed to the side where the bus entry is"
                if at.x < bus.a.x {
                    ret = LabelSpin::Left;
                } else if at.x > bus.a.x {
                    ret = LabelSpin::Right;
                }
            } else if dy == 0 && dx >= 0 {
                // "bus is horizontal -> label shall be vertical and shall be placed to the side where the bus entry is"
                if at.y < bus.a.y {
                    ret = LabelSpin::Up;
                } else if at.y > bus.a.y {
                    ret = LabelSpin::Bottom;
                }
            }
        }

        // SCH_LINE_T: wires, buses and graphic lines. `Items().Overlapping( aPosition )` is the R-tree of `SCH_LINE::GetBoundingBox`, the box of the two ends
        // grown by half the pen (`GetPenWidth() / 2`, and 1 IU on the far side): a point anywhere in that box counts, on the line or not.
        let mut lines: Vec<(Point, Point, Nm)> = self.segs.iter().filter(|s| !s.dead).map(|s| (s.a, s.b, if s.bus { HALF_BUS_PEN_NM } else { HALF_WIRE_PEN_NM })).collect();
        for l in &self.sch.lines {
            let half = if l.width_nm > 0 { l.width_nm / 2 } else { HALF_WIRE_PEN_NM };
            lines.extend(l.pts.windows(2).map(|w| (w[0], w[1], half)));
        }
        for (start, end, half) in lines {
            if at.x < start.x.min(end.x) - half || at.x > start.x.max(end.x) + half || at.y < start.y.min(end.y) - half || at.y > start.y.max(end.y) + half {
                continue;
            }
            // "line angles goes between -90 and 90 degrees, but normalize": `Angle().Normalize90()` is in (-90, 90], so -45 < angle <= 45 is
            // -dx < dy <= dx for the direction turned to point right (or down, for a vertical line)
            let (mut dx, mut dy) = (end.x - start.x, end.y - start.y);
            if dx < 0 || (dx == 0 && dy < 0) {
                (dx, dy) = (-dx, -dy);
            }
            let at_end = end == at;
            if (dx == 0 && dy == 0) || (dx > 0 && -dx < dy && dy <= dx) {
                ret = if start.x <= end.x {
                    if at_end { LabelSpin::Right } else { LabelSpin::Left }
                } else if at_end {
                    LabelSpin::Left
                } else {
                    LabelSpin::Right
                };
            } else {
                ret = if start.y <= end.y {
                    if at_end { LabelSpin::Bottom } else { LabelSpin::Up }
                } else if at_end {
                    LabelSpin::Up
                } else {
                    LabelSpin::Bottom
                };
            }
        }

        // SCH_SYMBOL_T: placed symbols and power symbols (a power symbol is a symbol)
        for (i, s) in self.sch.symbols.iter().enumerate() {
            let m = Matrix::of(s.rot, s.mirrored, s.mirror_y);
            for (k, (_, pos)) in self.pins_of(i).iter().enumerate() {
                if *pos != at {
                    continue;
                }
                if let Some(spin) = self.shapes[i].dirs.get(k).and_then(|d| pin_spin_style(m.apply(d.0, d.1))) {
                    ret = spin;
                }
                break;
            }
        }
        for (i, p) in self.sch.power_symbols.iter().enumerate() {
            let m = Matrix::of(p.rot, false, false);
            let shape = &self.power_shapes[i];
            for (k, (px, py)) in shape.pins.iter().map(|(_, at)| *at).enumerate() {
                let (x, y) = m.apply(px, py);
                if (Point { x: p.at.x + x, y: p.at.y + y }) != at {
                    continue;
                }
                if let Some(spin) = shape.dirs.get(k).and_then(|d| pin_spin_style(m.apply(d.0, d.1))) {
                    ret = spin;
                }
                break;
            }
        }
        ret
    }

    /// `SCH_EDIT_FRAME::AutoRotateItem`: a global or hierarchical label whose "Auto" check is on takes the spin of what it is connected to. Returns whether the
    /// spin changed.
    pub(crate) fn auto_rotate_item(&mut self, it: Item) -> bool {
        let Item::Label(i) = it else { return false };
        let l = &self.sch.labels[i];
        if !auto_rotate_supported(&l.kind) || !self.sch.extras.label_looks.get(&l.id).is_some_and(|k| k.auto_rotate) {
            return false;
        }
        let current = effective_spin(self, i);
        let spin = self.label_orientation_for_point(l.at, current);
        if spin == current {
            return false;
        }
        let id = l.id.clone();
        self.sch.extras.label_spins.insert(id, spin);
        true
    }
}

/// Half the pen `SCH_LINE::GetPenWidth` gives a wire with nothing set (the Default net class's 6 mil; a graphic line's `m_DefaultLineWidth` is 6 mil too) and a bus
/// (12 mil), in nanometres.
const HALF_WIRE_PEN_NM: Nm = 76_200;
const HALF_BUS_PEN_NM: Nm = 152_400;

/// What Place Label sets on the label it puts down (`createNewLabel`: the properties dialog's result).
#[derive(Debug, Clone, Default)]
pub(crate) struct PlacedLabel {
    pub spin: Option<LabelSpin>,
    pub size_nm: Option<Nm>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub auto_rotate: Option<bool>,
}

impl<'a> Board<'a> {
    /// Place Label, Global Label and Hierarchical Label ([`SchCmd::PlaceLabel`](crate::sch_edit::SchCmd::PlaceLabel)): `TwoClickPlace`'s second click, with the properties the dialog gave the label
    /// and `AutoRotateItem` run on it.
    pub(crate) fn place_label(&mut self, net: &str, at: Point, kind: &LabelKind, props: &PlacedLabel) -> Result<(), Vec<CheckResult>> {
        if net.trim().is_empty() {
            return Err(fail("ops_bad_label", "label", "a label needs a net name"));
        }
        if let Some(size) = props.size_nm {
            if !(TEXT_SIZE_MIN_NM..=TEXT_SIZE_MAX_NM).contains(&size) {
                return Err(fail("ops_bad_text_size", net, "The text size must be between 0.01 and 1000 mm."));
            }
        }
        if props.auto_rotate == Some(true) && !auto_rotate_supported(kind) {
            return Err(fail("ops_label_no_auto_rotate", net, "only a global or a hierarchical label can rotate itself on placement"));
        }
        let model = self.model;
        let sch = self.schematic_mut_or_create();
        sch.labels.push(NetLabel { id: String::new(), net: net.trim().to_string(), at, kind: kind.clone() });
        sch.assign_missing_ids();
        let id = sch.labels.last().map(|l| l.id.clone()).unwrap_or_default();

        let mut look = LabelLook::default();
        if let Some(size) = props.size_nm {
            look.size_nm = if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM { 0 } else { size };
        }
        look.bold = props.bold.unwrap_or(false);
        look.italic = props.italic.unwrap_or(false);
        look.auto_rotate = props.auto_rotate.unwrap_or(false) && auto_rotate_supported(kind);
        if !look.is_default() {
            sch.extras.label_looks.insert(id.clone(), look);
        }
        // "if( !m_currentLabel->AutoRotateOnPlacement() && spin != selected ) SetSpinStyle( selected )", after the placement's AutoRotateItem. A label always
        // has a spin of its own (`createNewLabel`: `SetSpinStyle( m_lastTextOrientation )`, which the tool starts as `SPIN_STYLE::RIGHT`), so none given is
        // RIGHT and is kept: it is not read again off the wire it ends at whenever the wire moves.
        sch.extras.label_spins.insert(id.clone(), props.spin.unwrap_or(LabelSpin::Right));
        if look.auto_rotate {
            let mut scene = Scene::new(sch, model);
            let index = scene.sch.labels.iter().position(|l| l.id == id).unwrap_or(0);
            if scene.auto_rotate_item(Item::Label(index)) {
                let spin = scene.sch.extras.label_spins.get(&id).copied();
                if let Some(spin) = spin {
                    sch.extras.label_spins.insert(id, spin);
                }
            }
        }
        Ok(())
    }

    /// Place Text ([`SchCmd::PlaceText`](crate::sch_edit::SchCmd::PlaceText)): `createNewText`'s result put down at `at`. The justification is KiCad's last-used one, centre and centre, unless the
    /// dialog gave another; the angle is made upright (along the sheet or up it).
    pub(crate) fn place_text(&mut self, content: &str, at: Point, angle: Millideg, size_nm: Nm, h: Option<TextJustify>, v: Option<TextVAlign>) -> Result<(), Vec<CheckResult>> {
        if content.trim().is_empty() {
            return Err(fail("ops_bad_text", "text", "Text can not be empty."));
        }
        if !(TEXT_SIZE_MIN_NM..=TEXT_SIZE_MAX_NM).contains(&size_nm) {
            return Err(fail("ops_bad_text_size", content, "The text size must be between 0.01 and 1000 mm."));
        }
        let angle = if eda_model::kicad_geom::keep_upright_is_vertical(angle as f64 / 1000.0) { 90_000 } else { 0 };
        let sch = self.schematic_mut_or_create();
        sch.texts.push(SchematicText { id: String::new(), content: content.to_string(), at, angle, size_nm });
        sch.assign_missing_ids();
        let id = sch.texts.last().map(|t| t.id.clone()).unwrap_or_default();
        let file_default = SchTextJustify::FILE_DEFAULT;
        sch.extras.set_text_justify(&id, SchTextJustify { h: h.unwrap_or(file_default.h), v: v.unwrap_or(file_default.v) });
        Ok(())
    }
}
