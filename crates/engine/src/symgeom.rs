//! A placed symbol as KiCad draws it, in sheet coordinates: where its body is, where each pin ends, which way it runs, and how long it is.
//!
//! `placed.rs` answers "where does a wire meet this pin" in the engine's box-and-stub terms; the texts around a symbol (its pins'
//! names and numbers, its fields) depend on the symbol's real drawn geometry instead, so they are measured here, with the rules of
//! `eda_model::kicad_geom`. The geometry comes from `placed::corner_symbol`, the same symbol the `.kicad_sch` writer embeds, moved by
//! the instance's turn and mirror the way `placed::bake_offset` moves a point (KiCad's `TRANSFORM`), so a pin's tip here is the point a wire ends on.

use eda_model::ir::{Point, SymbolInstance};
use eda_model::kicad_geom::{arc_bbox, PinOrient, PinTexts, Rect, TextStyle};
use eda_model::symbol::{LibSymbol, SymbolGraphic};
use eda_model::Part;

use crate::placed::{corner_symbol, part_box};

/// A placed symbol's orientation folded into a turn and a left-right mirror: KiCad's `SYM_MIRROR_X` (this IR's `mirror_y`, a top-bottom flip) is a half
/// turn and a left-right flip (`GetOrientation`'s twelve have "180 + mirror x" equal to "0 + mirror y"), so every orientation is one of the eight a
/// turn and `mirrored` give.
pub fn kicad_turn(rot_millideg: u32, mirrored: bool, mirror_y: bool) -> (u32, bool) {
    if mirror_y {
        ((rot_millideg % 360_000 + 180_000) % 360_000, !mirrored)
    } else {
        (rot_millideg % 360_000, mirrored)
    }
}

/// Where a point of the symbol's unturned frame (nanometres from the corner the symbol is placed by, y down) lands on the sheet, as an offset from
/// `SymbolInstance::at`: KiCad's `TRANSFORM` of the orientation (`SCH_SYMBOL::SetOrientation`), the turn first and then the mirror, each about `at` --
/// what the editor (`sch_scene::Matrix`) and the studio's painter (`transform.ts`) draw, so a pin is where the user wires it. A turn of 90 degrees is
/// counter-clockwise on the sheet (`(x, y)` goes to `(y, -x)`, y down); `mirrored` negates x. `width` is not used: KiCad mirrors about the symbol's
/// own origin, not about its box. (`mirror_y` is folded in by [`kicad_turn`].)
pub fn bake(rot_millideg: u32, mirrored: bool, width: f64, lx: f64, ly: f64) -> (f64, f64) {
    let _ = width;
    let (x, y) = match rot_millideg % 360_000 {
        0 => (lx, ly),
        90_000 => (ly, -lx),
        180_000 => (-lx, -ly),
        270_000 => (-ly, lx),
        deg => {
            let (s, c) = (deg as f64 / 1000.0).to_radians().sin_cos();
            (lx * c + ly * s, -lx * s + ly * c)
        }
    };
    (if mirrored { -x } else { x }, y)
}

/// The inverse of [`bake`]: the unturned-frame point that lands `(px, py)` from `at`.
pub fn unbake(rot_millideg: u32, mirrored: bool, width: f64, px: f64, py: f64) -> (f64, f64) {
    let _ = width;
    let px = if mirrored { -px } else { px };
    match rot_millideg % 360_000 {
        0 => (px, py),
        90_000 => (-py, px),
        180_000 => (-px, -py),
        270_000 => (py, -px),
        deg => {
            let (s, c) = (deg as f64 / 1000.0).to_radians().sin_cos();
            (px * c - py * s, px * s + py * c)
        }
    }
}

/// The turn and mirror alone, for a direction rather than a point.
pub fn bake_dir(rot_millideg: u32, mirrored: bool, v: (f64, f64)) -> (f64, f64) {
    bake(rot_millideg, mirrored, 0.0, v.0, v.1)
}

#[derive(Debug, Clone)]
pub struct PinGeom {
    pub number: String,
    /// The name KiCad shows: empty for none.
    pub name: String,
    /// The pin's connection point, nanometres.
    pub tip: (f64, f64),
    pub orient: PinOrient,
    /// The pin line's length, nanometres.
    pub length: f64,
}

#[derive(Debug, Clone)]
pub struct SymbolGeom {
    /// The drawn graphics' box (no pins, no fields): `GetBodyBoundingBox`.
    pub body: Option<Rect>,
    pub pins: Vec<PinGeom>,
    pub texts: PinTexts,
    pub power: bool,
    /// The box's width in the unturned frame, nanometres: what a mirror is about.
    pub width: f64,
}

/// A body graphic's box in the symbol's frame (millimetres, y up), pen included.
fn graphic_bbox(g: &SymbolGraphic) -> Rect {
    let pen = |w: f64| if w > 0.0 { w } else { 0.1524 };
    match g {
        SymbolGraphic::Rectangle { start, end, stroke_mm, .. } => Rect::new((start.x, start.y), (end.x, end.y)).inflate(pen(*stroke_mm) / 2.0),
        SymbolGraphic::Polyline { pts, stroke_mm, .. } => {
            let mut it = pts.iter();
            let first = it.next().map(|p| Rect::point((p.x, p.y))).unwrap_or(Rect::point((0.0, 0.0)));
            it.fold(first, |r, p| r.merge_point((p.x, p.y))).inflate(pen(*stroke_mm) / 2.0)
        }
        SymbolGraphic::Circle { center, radius_mm, stroke_mm, .. } => Rect::new((center.x - radius_mm, center.y - radius_mm), (center.x + radius_mm, center.y + radius_mm)).inflate(pen(*stroke_mm) / 2.0),
        SymbolGraphic::Arc { start, mid, end, stroke_mm, .. } => arc_bbox((start.x, start.y), (mid.x, mid.y), (end.x, end.y)).inflate(pen(*stroke_mm) / 2.0),
        SymbolGraphic::Text { at, .. } => Rect::point((at.x, at.y)),
    }
}

impl SymbolGeom {
    /// The geometry of the placed `sym`, a part drawn from `resolved` (a library symbol) or as the generic box.
    pub fn of(sym: &SymbolInstance, part: &Part, resolved: Option<&LibSymbol>) -> SymbolGeom {
        let cs = corner_symbol(&sym.lib_id, part, resolved, sym.unit);
        let width = part_box(part, resolved, sym.unit).width() as f64;
        let (rot, mirrored) = kicad_turn(sym.rot, sym.mirrored, sym.mirror_y);
        Self::of_corner_symbol(sym.at, rot, mirrored, width, &cs, sym.unit, Some(part))
    }

    /// The geometry of a symbol whose library frame has its origin at the symbol's `at` (a power symbol, a sheet-less glyph): no
    /// box corner to measure from, no mirror about a box.
    pub fn of_origin_symbol(at: Point, rot: u32, lib: &LibSymbol) -> SymbolGeom {
        let mut g = Self::of_corner_symbol(at, rot, false, 0.0, lib, 1, None);
        g.power = lib.power;
        g
    }

    fn of_corner_symbol(at: Point, rot: u32, mirrored: bool, width: f64, cs: &LibSymbol, unit: u32, part: Option<&Part>) -> SymbolGeom {
        let on_unit = |u: u32| u == 0 || u == unit;
        let page = |x_mm: f64, y_mm_up: f64| -> (f64, f64) {
            let (dx, dy) = bake(rot, mirrored, width, x_mm * 1e6, -y_mm_up * 1e6);
            (at.x.raw_shim() as f64 + dx, at.y.raw_shim() as f64 + dy)
        };
        let mut body: Option<Rect> = None;
        for g in cs.graphics.iter().filter(|g| on_unit(g.unit())) {
            let b = graphic_bbox(g);
            let r = Rect::new(page(b.x0, b.y0), page(b.x1, b.y1));
            body = Some(body.map_or(r, |acc| acc.merge(r)));
        }
        let mut pins = Vec::new();
        for p in cs.pins.iter().filter(|p| on_unit(p.unit)) {
            let tip = page(p.at.x, p.at.y);
            // the direction from the pin's tip toward the body, in the unturned frame (y down)
            let local = match (p.angle_deg.round() as i64).rem_euclid(360) {
                0 => (1.0, 0.0),
                90 => (0.0, -1.0),
                180 => (-1.0, 0.0),
                _ => (0.0, 1.0),
            };
            let d = bake_dir(rot, mirrored, local);
            let name = part.and_then(|pt| pt.pins.iter().find(|x| x.number == p.number)).map(|x| x.name.clone().unwrap_or_default()).unwrap_or_else(|| p.name.clone());
            pins.push(PinGeom { number: p.number.clone(), name: eda_model::kicad_geom::shown_name(&name).to_string(), tip, orient: PinOrient::of_dir(d), length: p.length_mm * 1e6 });
        }
        let texts = PinTexts { names_hidden: cs.pin_names_hidden, numbers_hidden: cs.pin_numbers_hidden, name_offset_mm: cs.pin_name_offset_mm };
        SymbolGeom { body, pins, texts, power: cs.power, width }
    }

    /// The default style pin texts are set in.
    pub fn pin_style() -> TextStyle {
        TextStyle::new(eda_model::kicad_font::HJustify::Center, eda_model::kicad_font::VJustify::Center)
    }

    /// Every box eeschema draws for the pins: the line, the name and the number, as the checker measures them.
    pub fn pin_rects(&self) -> Vec<(Rect, &PinGeom)> {
        use eda_model::kicad_geom::{pin_name_rect, pin_number_rect};
        let st = Self::pin_style();
        let mut out = Vec::new();
        for p in &self.pins {
            if p.length > 0.0 {
                let end = (p.tip.0 + p.orient.dir().0 * p.length, p.tip.1 + p.orient.dir().1 * p.length);
                out.push((Rect::new(p.tip, end).inflate(100.0), p));
            }
            let name_visible = !self.texts.names_hidden && !p.name.is_empty();
            if !self.texts.numbers_hidden && !p.number.is_empty() {
                let both = name_visible && !self.texts.name_inside();
                out.push((pin_number_rect(p.tip, p.orient, p.length, &p.number, &st, both), p));
            }
            if name_visible {
                out.push((pin_name_rect(p.tip, p.orient, p.length, &p.name, &st, &self.texts), p));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;
    use eda_model::{Pin, PinKind};

    fn two_pin(reference: &str) -> Part {
        Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: Some("330".into()),
            package: None,
            footprint: None,
            symbol: None,
            datasheet: None,
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_nm: None,
            edge: None,
        }
    }

    fn instance(at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
        SymbolInstance { id: "R1".into(), at, rot, mirrored, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "330".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
    }

    #[test]
    fn bake_and_unbake_are_inverses() {
        for rot in [0, 90_000, 180_000, 270_000] {
            for mirrored in [false, true] {
                let (px, py) = bake(rot, mirrored, 5_080_000.0, 1_234_000.0, 2_345_000.0);
                let (lx, ly) = unbake(rot, mirrored, 5_080_000.0, px, py);
                assert!((lx - 1_234_000.0).abs() < 1e-6 && (ly - 2_345_000.0).abs() < 1e-6, "{rot} {mirrored}: {lx} {ly}");
            }
        }
    }

    #[test]
    fn a_standing_resistor_has_its_pins_above_and_below_a_box_between_them() {
        let r = eda_model::symbol::builtin("Device:R");
        let at = Point { x: mm(50.8), y: mm(50.8) };
        let g = SymbolGeom::of(&instance(at, 0, false), &two_pin("R1"), r.as_ref());
        assert_eq!(g.pins.len(), 2);
        let (top, bottom) = (&g.pins[0], &g.pins[1]);
        // the box is the pins' tips less one stub (1.27 mm): 7.62 tall, its corner at `at`
        assert_eq!(top.tip.1, 50_800_000.0 - 1_270_000.0);
        assert_eq!(bottom.tip.1, 50_800_000.0 + part_box(&two_pin("R1"), r.as_ref(), 1).height() as f64 + 1_270_000.0);
        assert_eq!(top.orient, PinOrient::Down);
        assert_eq!(bottom.orient, PinOrient::Up);
        let body = g.body.expect("the body");
        assert!((body.w() - 2_286_000.0).abs() < 1.0 && (body.h() - 5_334_000.0).abs() < 1.0, "{body:?}");
        // turned half a turn, pin 1 is at the bottom
        let g2 = SymbolGeom::of(&instance(Point { x: mm(50.8), y: Nm::from_raw_shim(50_800_000 + 7_620_000) }, 180_000, false), &two_pin("R1"), r.as_ref());
        assert!(g2.pins[0].tip.1 > g2.pins[1].tip.1);
        assert_eq!(g2.pins[0].orient, PinOrient::Up);
    }
}
