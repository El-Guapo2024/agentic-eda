//! Where a derived symbol's pins really are.
//!
//! `derive_schematic` places every symbol by the top-left corner of its box: `SymbolInstance::at` is that corner, ports sit on the
//! box edge and every wire ends on a pin's *stub tip*, one grid cell outside it (`eda_layout::Node::stub_tip`). The `.kicad_sch`
//! writer bakes exactly this geometry into a per-instance library symbol, so KiCad sees the pins where the wires are.
//!
//! Everything else that has to know where a pin is -- the studio's symbol drawing, the net tracer after a schematic edit, the
//! layout of a module sheet -- asks here, so there is one answer. A symbol turned half a turn (`rot` 180) is the box mirrored
//! through the corner `at`; a quarter turn is the same rotation about the corner; `mirrored` flips the box about its own vertical
//! axis first. That is `eda_kicad`'s `baked_local`, which this file reproduces for the integer-micrometre world.

use eda_layout::{Node, Point as LPoint, Side};
use eda_model::ir::{Point, SymbolInstance};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{Part, PinKind};

use crate::geometry::{build_ports, nc_pin_local_points, node_size, real_symbol_bbox, STUB};

/// One part's box, ports and the pin -> port map, for one unit.
#[derive(Debug, Clone)]
pub struct PartBox {
    pub node: Node,
    /// Pin index (into `Part::pins`) -> index into `node.ports`; `None` for a pin with no port (a no-connect pin, or one that
    /// belongs to another unit).
    pub pin_port: Vec<Option<usize>>,
}

impl PartBox {
    pub fn width(&self) -> i64 {
        self.node.width
    }
    pub fn height(&self) -> i64 {
        self.node.height
    }
}

/// The box `derive_schematic` lays `part` out as, resolved against `resolved` (a real or built-in library symbol, when one speaks
/// for the part).
pub fn part_box(part: &Part, resolved: Option<&LibSymbol>, unit: u32) -> PartBox {
    let (width, height) = node_size(part, resolved, unit);
    let (ports, pin_port) = build_ports(part, width, height, resolved, unit);
    PartBox { node: Node { id: 0, width, height, ports }, pin_port }
}

/// Where a box-local point lands after the instance's mirror and rotation, as an offset from `at` (um). Mirror first, then
/// rotation about the corner -- `eda_kicad`'s `baked_local`.
pub fn bake_offset(rot_millideg: u32, mirrored: bool, width: i64, lx: i64, ly: i64) -> (i64, i64) {
    let lx = if mirrored { width - lx } else { lx };
    match rot_millideg % 360_000 {
        0 => (lx, ly),
        90_000 => (-ly, lx),
        180_000 => (-lx, -ly),
        270_000 => (ly, -lx),
        deg => {
            let theta = (deg as f64 / 1000.0).to_radians();
            let (s, c) = theta.sin_cos();
            (((lx as f64) * c - (ly as f64) * s).round() as i64, ((lx as f64) * s + (ly as f64) * c).round() as i64)
        }
    }
}

/// Every pin of a placed symbol, by pin number, at the point a wire connects to: the stub tip of a pin with a port, the flag
/// point of a no-connect pin. A pin that belongs to another unit of the part is left out.
pub fn pin_points(sym: &SymbolInstance, part: &Part, resolved: Option<&LibSymbol>) -> Vec<(String, Point)> {
    let b = part_box(part, resolved, sym.unit);
    let (w, h) = (b.width(), b.height());
    let at = |lx: i64, ly: i64| -> Point {
        let (dx, dy) = bake_offset(sym.rot, sym.mirrored, w, lx, ly);
        Point { x: sym.at.x + dx, y: sym.at.y + dy }
    };
    let mut out = Vec::new();
    for (i, pin) in part.pins.iter().enumerate() {
        let Some(pi) = b.pin_port[i] else { continue };
        let tip = b.node.stub_tip(LPoint { x: 0, y: 0 }, pi);
        out.push((pin.number.clone(), at(tip.x, tip.y)));
    }
    for (i, local) in nc_pin_local_points(part, w, h, resolved, sym.unit) {
        out.push((part.pins[i].number.clone(), at(local.x, local.y)));
    }
    out
}

/// KiCad's electrical type for a pin that no library symbol describes (`eda_kicad`'s `electrical_type`, which the writer uses).
fn electrical_type(kind: PinKind, name: &str) -> &'static str {
    match kind {
        PinKind::Power if name.to_ascii_uppercase().contains("OUT") => "power_out",
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
}

fn mm(um: i64) -> f64 {
    um as f64 / 1000.0
}

/// KiCad pin angle for a pin whose box side is `side`: the direction from the pin's outer end toward the body.
fn angle_for(side: Side) -> f64 {
    match side {
        Side::Left => 0.0,
        Side::Bottom => 90.0,
        Side::Right => 180.0,
        Side::Top => 270.0,
    }
}

fn shift_point(p: SPoint, dx: f64, dy: f64) -> SPoint {
    SPoint::new(p.x + dx, p.y + dy)
}

fn shift_graphic(g: &SymbolGraphic, dx: f64, dy: f64) -> SymbolGraphic {
    use SymbolGraphic::*;
    match g {
        Rectangle { unit, start, end, stroke_mm, filled } => Rectangle { unit: *unit, start: shift_point(*start, dx, dy), end: shift_point(*end, dx, dy), stroke_mm: *stroke_mm, filled: *filled },
        Polyline { unit, pts, stroke_mm, filled } => Polyline { unit: *unit, pts: pts.iter().map(|p| shift_point(*p, dx, dy)).collect(), stroke_mm: *stroke_mm, filled: *filled },
        Circle { unit, center, radius_mm, stroke_mm, filled } => Circle { unit: *unit, center: shift_point(*center, dx, dy), radius_mm: *radius_mm, stroke_mm: *stroke_mm, filled: *filled },
        Arc { unit, start, mid, end, stroke_mm, filled } => Arc { unit: *unit, start: shift_point(*start, dx, dy), mid: shift_point(*mid, dx, dy), end: shift_point(*end, dx, dy), stroke_mm: *stroke_mm, filled: *filled },
        Text { unit, text, at, angle_deg, size_mm } => Text { unit: *unit, text: text.clone(), at: shift_point(*at, dx, dy), angle_deg: *angle_deg, size_mm: *size_mm },
    }
}

/// The symbol a derived instance is drawn from, with its origin moved to the top-left corner of the box.
///
/// A library symbol's own origin is somewhere inside it (a resistor's is its centre); a derived `SymbolInstance::at` is the box
/// corner. Drawing the library symbol at `at` as it is puts every pin off its wire. This returns the symbol the `.kicad_sch`
/// writer bakes instead: a real or built-in symbol shifted so the corner of `real_symbol_bbox` is the origin, or, for a part with
/// no library symbol, the generic box (`node_size`) with every pin on the port `build_ports` gave it and its tip where the wire
/// ends. Library frame: mm, +y up, so the box hangs below the origin.
pub fn corner_symbol(lib_id: &str, part: &Part, resolved: Option<&LibSymbol>, unit: u32) -> LibSymbol {
    if let Some(sym) = resolved {
        let (x0, _y0, _x1, y1) = real_symbol_bbox(sym, unit);
        let (dx, dy) = (-x0, -y1);
        return LibSymbol {
            lib_id: lib_id.to_string(),
            graphics: sym.graphics.iter().map(|g| shift_graphic(g, dx, dy)).collect(),
            pins: sym.pins.iter().map(|p| LibPin { at: shift_point(p.at, dx, dy), ..p.clone() }).collect(),
            ..sym.clone()
        };
    }
    let b = part_box(part, None, unit);
    let (w, h) = (b.width(), b.height());
    let mut pins: Vec<LibPin> = Vec::new();
    for (i, pin) in part.pins.iter().enumerate() {
        let Some(pi) = b.pin_port[i] else { continue };
        let tip = b.node.stub_tip(LPoint { x: 0, y: 0 }, pi);
        let name = pin.name.clone().unwrap_or_default();
        pins.push(LibPin {
            number: pin.number.clone(),
            name: name.clone(),
            electrical_type: electrical_type(pin.kind, &name).to_string(),
            shape: "line".to_string(),
            at: SPoint::new(mm(tip.x), -mm(tip.y)),
            angle_deg: angle_for(b.node.ports[pi].side),
            length_mm: mm(STUB),
            unit: 1,
        });
    }
    for (i, local) in nc_pin_local_points(part, w, h, None, unit) {
        let pin = &part.pins[i];
        pins.push(LibPin {
            number: pin.number.clone(),
            name: pin.name.clone().unwrap_or_default(),
            electrical_type: "no_connect".to_string(),
            shape: "line".to_string(),
            at: SPoint::new(mm(local.x), -mm(local.y)),
            angle_deg: 90.0,
            length_mm: 0.0,
            unit: 1,
        });
    }
    let reference_prefix: String = part.reference.chars().take_while(|c| c.is_alphabetic()).collect();
    LibSymbol {
        lib_id: lib_id.to_string(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(0.0, 0.0), end: SPoint::new(mm(w), -mm(h)), stroke_mm: 0.254, filled: false }],
        pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix,
        unit_count: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::{ConstraintModel, Pin};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some("v".into()), package: None, footprint: None, symbol: None, datasheet: None, pins, body_um: None, edge: None }
    }
    fn instance(reference: &str, lib_id: &str, at: Point, rot: u32) -> SymbolInstance {
        SymbolInstance { id: reference.into(), at, rot, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() }
    }

    /// A synthetic IC and a real resistor: the point a pin is reported at is the point `derive_schematic` ends a wire on, and the
    /// symbol the studio draws has its pin on that very point.
    #[test]
    fn pin_points_agree_with_the_drawn_symbol() {
        let model = ConstraintModel::default();
        let u1 = part("U1", vec![pin("1", "VDD", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "IN", PinKind::Signal), pin("4", "OUT", PinKind::Signal), pin("5", "NC", PinKind::Nc)]);
        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let _ = &model;
        let resolved_r = eda_model::symbol::builtin("Device:R");
        for (part, lib_id, resolved) in [(&u1, "eda:U1", None), (&r1, "Device:R", resolved_r.as_ref())] {
            let at = Point { x: 12_700, y: 25_400 };
            let sym = instance(&part.reference, lib_id, at, 0);
            let pts = pin_points(&sym, part, resolved);
            let drawn = corner_symbol(lib_id, part, resolved, 1);
            for (number, p) in &pts {
                let lp = drawn.pin_by_number(number).unwrap_or_else(|| panic!("{} has no pin {number}", part.reference));
                // library frame (mm, y up) -> sheet frame (um, y down), from the corner
                let sheet = Point { x: at.x + (lp.at.x * 1000.0).round() as i64, y: at.y - (lp.at.y * 1000.0).round() as i64 };
                assert_eq!(*p, sheet, "{} pin {number}", part.reference);
            }
            assert_eq!(pts.len(), part.pins.len(), "every pin of {} has a point", part.reference);
        }
    }

    #[test]
    fn a_resistor_turned_half_a_turn_has_its_pins_swapped_top_to_bottom() {
        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let resolved = eda_model::symbol::builtin("Device:R");
        let b = part_box(&r1, resolved.as_ref(), 1);
        let at = Point { x: 10_000, y: 10_000 };
        let up = pin_points(&instance("R1", "Device:R", at, 0), &r1, resolved.as_ref());
        // Turned half a turn about the corner, the box hangs up-left of `at`: put `at` at its far corner to compare.
        let flipped_at = Point { x: at.x + b.width(), y: at.y + b.height() };
        let down = pin_points(&instance("R1", "Device:R", flipped_at, 180_000), &r1, resolved.as_ref());
        let get = |v: &[(String, Point)], n: &str| v.iter().find(|(k, _)| k == n).unwrap().1;
        assert_eq!(get(&up, "1").y, get(&down, "2").y, "pin 2 now sits where pin 1 was");
        assert_eq!(get(&up, "2").y, get(&down, "1").y);
        assert_eq!(get(&up, "1").x, get(&down, "1").x, "the column does not move");
    }
}
