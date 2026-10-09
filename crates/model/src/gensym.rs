//! A symbol for a part no library has one for, drawn the way KiCad's own generated symbols are.
//!
//! `eda:<ref>` (the symbol a part with no library symbol always had) was a box sized and spaced by the layout engine: pins a grid cell
//! apart on the box's edge, pin names and numbers written over one another. A generated symbol (`gen:<ref>`) follows KiCad's
//! conventions instead, so what the `.kicad_sch` writer embeds, what the studio draws and what the layout measures are one thing:
//!
//! * pins 2.54 mm long, 2.54 mm apart, every connection point on the 1.27 mm grid, no two at one spot;
//! * text 1.27 mm; pin names inside the body at KiCad's usual 1.016 mm offset, numbers over the pin;
//! * a body wide enough for the longest names (measured with the stroke font) and tall enough for the pins and for the names of the
//!   pins on its top and bottom, which read down into it and up into it;
//! * power pins on the top, grounds on the bottom, inputs and control pins on the left, outputs on the right, the rest split between
//!   left and right, no-connect pins last on the bottom -- the sides the box layout gave them.
//!
//! Pure and deterministic in the part, so there is nothing to store: [`crate::ConstraintModel::real_symbol_of`] makes it when asked.

use crate::kicad_font::{string_boundary_limits, default_pen_iu, DEFAULT_TEXT_SIZE_IU};
use crate::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use crate::{Part, Pin, PinKind};

/// The nickname generated symbols are filed under.
pub const GENERATED_PREFIX: &str = "gen:";

pub const PIN_LENGTH_MM: f64 = 2.54;
pub const PIN_PITCH_MM: f64 = 2.54;
/// How far inside the body a pin's name starts (`(pin_names (offset 1.016))`, the usual for an IC).
pub const NAME_OFFSET_MM: f64 = 1.016;
/// Clear space kept between two names that face one another, and from a name to the corner of the body.
const GAP_MM: f64 = 1.27;
const MIN_BODY_MM: f64 = 7.62;

pub fn is_generated_lib_id(lib_id: &str) -> bool {
    lib_id.starts_with(GENERATED_PREFIX)
}

fn is_ground_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("GND") || n.contains("VSS")
}

fn is_control_pin_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["EN", "CE", "SHDN", "RESET", "RST", "CS"].iter().any(|p| n.contains(p))
}

/// The four sides' pins (indices into `part.pins`), each in drawing order: left top to bottom, right top to bottom, top and bottom left
/// to right.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Sides {
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    pub top: Vec<usize>,
    pub bottom: Vec<usize>,
}

/// Which side each pin goes on: power on the top, ground on the bottom, inputs and control pins on the left, outputs on the right, the
/// rest half and half, a no-connect pin at the end of the bottom.
pub fn assign_sides(part: &Part) -> Sides {
    let (mut north, mut south, mut west_named, mut west_ctrl, mut east, mut fallback, mut nc) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, pin) in part.pins.iter().enumerate() {
        let name = pin.name.clone().unwrap_or_default();
        let upper = name.to_ascii_uppercase();
        match pin.kind {
            PinKind::Nc => nc.push(i),
            PinKind::Ground => south.push(i),
            PinKind::Power => {
                if is_ground_name(&name) {
                    south.push(i);
                } else if upper.contains("IN") {
                    west_named.push(i);
                } else if upper.contains("OUT") {
                    east.push(i);
                } else {
                    north.push(i);
                }
            }
            PinKind::Signal | PinKind::Passive => {
                if is_ground_name(&name) {
                    south.push(i);
                } else if upper.contains("OUT") {
                    east.push(i);
                } else if upper.contains("IN") {
                    west_named.push(i);
                } else if is_control_pin_name(&name) {
                    west_ctrl.push(i);
                } else {
                    fallback.push(i);
                }
            }
        }
    }
    let west_count = fallback.len() / 2;
    let (fb_west, fb_east) = fallback.split_at(west_count);
    let mut left = west_named;
    left.extend(west_ctrl);
    left.extend_from_slice(fb_west);
    let mut right = east;
    right.extend_from_slice(fb_east);
    south.extend(nc);
    Sides { left, right, top: north, bottom: south }
}

fn electrical_type(pin: &Pin) -> &'static str {
    let name = pin.name.as_deref().unwrap_or("");
    match pin.kind {
        PinKind::Power if name.to_ascii_uppercase().contains("OUT") => "power_out",
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
}

/// The width in millimetres the stroke font gives `name` at 1.27 mm, pen reach included.
fn text_mm(name: &str) -> f64 {
    if name.is_empty() {
        return 0.0;
    }
    let (w, _) = string_boundary_limits(name, DEFAULT_TEXT_SIZE_IU, default_pen_iu(DEFAULT_TEXT_SIZE_IU));
    w as f64 / 10_000.0
}

fn round_up(v: f64, step: f64) -> f64 {
    (v / step - 1e-9).ceil() * step
}

/// The generated symbol of `part`, filed as `lib_id`.
pub fn generate(part: &Part, lib_id: &str) -> LibSymbol {
    let sides = assign_sides(part);
    let name_of = |i: usize| part.pins[i].name.clone().unwrap_or_default();
    let name = |i: usize| {
        let n = name_of(i);
        if n == "~" {
            String::new()
        } else {
            n
        }
    };
    let widest = |side: &[usize]| side.iter().map(|&i| text_mm(&name(i))).fold(0.0_f64, f64::max);
    let (wl, wr, wt, wb) = (widest(&sides.left), widest(&sides.right), widest(&sides.top), widest(&sides.bottom));

    // width: the pins on top and bottom side by side, and the two columns of names that face each other across the body
    let columns = sides.top.len().max(sides.bottom.len()) as f64;
    let mut inner = 0.0;
    if !sides.left.is_empty() || !sides.right.is_empty() {
        inner = 2.0 * NAME_OFFSET_MM + wl + wr + GAP_MM;
    }
    let width = round_up(MIN_BODY_MM.max((columns + 1.0) * PIN_PITCH_MM).max(inner), PIN_PITCH_MM);
    // height: a band above the side pins for the names of the top ones, one under them for the bottom ones
    let band = |side: &[usize], longest: f64| if side.is_empty() { 0.0 } else { round_up(NAME_OFFSET_MM + longest + GAP_MM, PIN_PITCH_MM) };
    let (top_band, bottom_band) = (band(&sides.top, wt), band(&sides.bottom, wb));
    let rows = sides.left.len().max(sides.right.len());
    let side_block = if rows > 0 || (top_band > 0.0 && bottom_band > 0.0) { (rows as f64 + 1.0) * PIN_PITCH_MM } else { 0.0 };
    let height = round_up((top_band + side_block + bottom_band).max(2.0 * PIN_PITCH_MM), PIN_PITCH_MM);

    let (hw, hh) = (width / 2.0, height / 2.0);
    let mut pins: Vec<LibPin> = Vec::with_capacity(part.pins.len());
    let mut push = |i: usize, x: f64, y: f64, angle: f64| {
        let p = &part.pins[i];
        pins.push(LibPin { number: p.number.clone(), name: name(i), electrical_type: electrical_type(p).to_string(), shape: "line".to_string(), at: SPoint::new(x, y), angle_deg: angle, length_mm: PIN_LENGTH_MM, unit: 1 });
    };
    for (k, &i) in sides.left.iter().enumerate() {
        push(i, -hw - PIN_LENGTH_MM, hh - top_band - PIN_PITCH_MM * (k as f64 + 1.0), 0.0);
    }
    for (k, &i) in sides.right.iter().enumerate() {
        push(i, hw + PIN_LENGTH_MM, hh - top_band - PIN_PITCH_MM * (k as f64 + 1.0), 180.0);
    }
    let across = |n: usize, k: usize| (k as f64 - (n as f64 - 1.0) / 2.0) * PIN_PITCH_MM;
    for (k, &i) in sides.top.iter().enumerate() {
        push(i, across(sides.top.len(), k), hh + PIN_LENGTH_MM, 270.0);
    }
    for (k, &i) in sides.bottom.iter().enumerate() {
        push(i, across(sides.bottom.len(), k), -hh - PIN_LENGTH_MM, 90.0);
    }

    let reference_prefix: String = part.reference.chars().take_while(|c| c.is_alphabetic()).collect();
    LibSymbol {
        lib_id: lib_id.to_string(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-hw, hh), end: SPoint::new(hw, -hh), stroke_mm: 0.254, filled: true }],
        pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: part.datasheet.clone().unwrap_or_default(),
        description: String::new(),
        reference_prefix: if reference_prefix.is_empty() { "U".to_string() } else { reference_prefix },
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: NAME_OFFSET_MM,
        alternate: None,
    }
}

#[cfg(test)]
mod tests;
