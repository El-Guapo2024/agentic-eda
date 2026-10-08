//! The measuring kit the schematic layouts share: rectangles, text widths, the footprint of a drawn symbol, the paper sizes and the
//! drawing sheet's frame.
//!
//! Text is not stored anywhere in `design.json` (a symbol's Reference and Value are drawn where the painter puts them), so a layout
//! that promises "no overlapping text" has to *model* the painter. The rules below are the studio's own
//! (`web/studio/src/components/schematic/painter.ts`: `drawFieldsAbout`, `drawSheet`, `drawPowerSymbolText`, `labelShape.ts`), in
//! micrometres, with a margin for the stroke font being wider than a character count says. The same functions measure the output
//! in the tests, so a layout and its check cannot drift apart.

use eda_layout::{Point as LPoint, Side};
use eda_model::ir::Point;
use eda_model::symbol::LibSymbol;
use eda_model::Part;

use crate::geometry::{GRID, STUB};
use crate::placed::{part_box, PartBox};

pub const G: i64 = GRID;

pub fn snap_up(v: i64) -> i64 {
    v.div_euclid(G) * G + if v.rem_euclid(G) == 0 { 0 } else { G }
}

pub fn snap_down(v: i64) -> i64 {
    v.div_euclid(G) * G
}

/// An axis-aligned rectangle, micrometres, `x0 <= x1`, `y0 <= y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Rect {
    pub fn new(ax: i64, ay: i64, bx: i64, by: i64) -> Rect {
        Rect { x0: ax.min(bx), y0: ay.min(by), x1: ax.max(bx), y1: ay.max(by) }
    }
    pub fn w(&self) -> i64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> i64 {
        self.y1 - self.y0
    }
    pub fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn inflate(self, m: i64) -> Rect {
        Rect { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m }
    }
    pub fn translate(self, dx: i64, dy: i64) -> Rect {
        Rect { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    /// Interiors intersect (rectangles that only touch do not overlap).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    pub fn contains(&self, o: &Rect) -> bool {
        self.x0 <= o.x0 && self.y0 <= o.y0 && o.x1 <= self.x1 && o.y1 <= self.y1
    }
}

pub fn union_all(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().reduce(Rect::union)
}

// ------------------------------------------------------------------------------------------------------- text

pub const REF_FONT: i64 = 1600;
pub const VALUE_FONT: i64 = 1400;
pub const FIELD_FONT: i64 = 1000;
pub const LABEL_FONT: i64 = 1270;
pub const SHEET_NAME_FONT: i64 = 1270;
pub const SHEET_FILE_FONT: i64 = 1016;
pub const SHEET_PIN_FONT: i64 = 1000;

/// Advance of one character as a fraction of the font size. KiCad's stroke font averages a little over 0.6; 0.75 leaves room for
/// the wide capitals of a net name.
const ADVANCE: f64 = 0.75;

pub fn text_w(font: i64, s: &str) -> i64 {
    (s.chars().count() as f64 * font as f64 * ADVANCE).ceil() as i64
}

/// A glyph's height above its baseline.
pub fn cap(font: i64) -> i64 {
    font * 95 / 100
}

// ------------------------------------------------------------------------------------------------------ frame

/// A paper size: KiCad's names, landscape, micrometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paper {
    pub name: &'static str,
    pub w: i64,
    pub h: i64,
}

pub const PAPERS: [Paper; 5] = [
    Paper { name: "A4", w: 297_000, h: 210_000 },
    Paper { name: "A3", w: 420_000, h: 297_000 },
    Paper { name: "A2", w: 594_000, h: 420_000 },
    Paper { name: "A1", w: 841_000, h: 594_000 },
    Paper { name: "A0", w: 1_189_000, h: 841_000 },
];

pub fn paper_named(name: &str) -> Paper {
    PAPERS.iter().copied().find(|p| p.name.eq_ignore_ascii_case(name)).unwrap_or(PAPERS[0])
}

/// KiCad's default drawing sheet: a frame 10 mm in from the paper, a second line 2 mm inside it, and the title block flush with
/// the inner frame's bottom-right corner (108 x 32 mm).
pub const FRAME_OUTER: i64 = 10_000;
pub const FRAME_INNER: i64 = 12_000;
pub const TITLE_W: i64 = 108_000;
pub const TITLE_H: i64 = 32_000;
/// Clear space kept between anything drawn and the inner frame line or the title block.
pub const FRAME_CLEAR: i64 = 5_000;

impl Paper {
    /// Where content may go: inside the inner frame with `FRAME_CLEAR` to spare, above the title block, on the grid.
    pub fn usable(&self) -> Rect {
        Rect {
            x0: snap_up(FRAME_INNER + FRAME_CLEAR),
            y0: snap_up(FRAME_INNER + FRAME_CLEAR),
            x1: snap_down(self.w - FRAME_INNER - FRAME_CLEAR),
            y1: snap_down(self.h - FRAME_INNER - TITLE_H - FRAME_CLEAR),
        }
    }
    /// The inner frame, for checks that something is not on the line itself.
    pub fn inner_frame(&self) -> Rect {
        Rect { x0: FRAME_INNER, y0: FRAME_INNER, x1: self.w - FRAME_INNER, y1: self.h - FRAME_INNER }
    }
}

/// The smallest paper whose usable area holds `content` (width x height); the largest when none does.
pub fn smallest_paper(w: i64, h: i64) -> Paper {
    PAPERS.iter().copied().find(|p| p.usable().w() >= w && p.usable().h() >= h).unwrap_or(PAPERS[PAPERS.len() - 1])
}

// ------------------------------------------------------------------------------------------- symbol footprint

/// Which sides a point-reflection swaps (a symbol turned half a turn).
pub fn flip_side(s: Side) -> Side {
    match s {
        Side::Top => Side::Bottom,
        Side::Bottom => Side::Top,
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

pub fn side_dir(s: Side) -> (i64, i64) {
    match s {
        Side::Top => (0, -1),
        Side::Bottom => (0, 1),
        Side::Left => (-1, 0),
        Side::Right => (1, 0),
    }
}

/// One symbol laid out: its box, where it sits, and whether it is turned half a turn.
#[derive(Debug, Clone)]
pub struct Placed {
    pub part: Part,
    pub lib_id: String,
    pub resolved: Option<LibSymbol>,
    pub b: PartBox,
    /// Top-left corner of the box as drawn (after any half turn).
    pub x: i64,
    pub y: i64,
    pub flip: bool,
    /// The text the painter draws under or beside it.
    pub value: String,
    pub footprint: String,
}

impl Placed {
    pub fn new(part: &Part, lib_id: &str, resolved: Option<LibSymbol>, value: &str, footprint: &str) -> Placed {
        let b = part_box(part, resolved.as_ref(), 1);
        Placed { part: part.clone(), lib_id: lib_id.to_string(), resolved, b, x: 0, y: 0, flip: false, value: value.to_string(), footprint: footprint.to_string() }
    }

    pub fn w(&self) -> i64 {
        self.b.width()
    }
    pub fn h(&self) -> i64 {
        self.b.height()
    }

    /// `SymbolInstance::at` for the current pose.
    pub fn at(&self) -> Point {
        if self.flip {
            Point { x: self.x + self.w(), y: self.y + self.h() }
        } else {
            Point { x: self.x, y: self.y }
        }
    }

    pub fn rot(&self) -> u32 {
        if self.flip {
            180_000
        } else {
            0
        }
    }

    fn port_of(&self, pin_number: &str) -> Option<usize> {
        let i = self.part.pins.iter().position(|p| p.number == pin_number)?;
        self.b.pin_port[i]
    }

    /// Where a wire meets `pin_number`, and the way the pin points out of the box, as drawn.
    pub fn tip(&self, pin_number: &str) -> Option<(Point, Side)> {
        let pi = self.port_of(pin_number)?;
        let l = self.b.node.stub_tip(LPoint { x: 0, y: 0 }, pi);
        let side = self.b.node.ports[pi].side;
        if self.flip {
            Some((Point { x: self.x + self.w() - l.x, y: self.y + self.h() - l.y }, flip_side(side)))
        } else {
            Some((Point { x: self.x + l.x, y: self.y + l.y }, side))
        }
    }

    /// Move the symbol so that `pin_number`'s tip lands on `target`.
    pub fn place_tip(&mut self, pin_number: &str, target: Point) {
        self.x = 0;
        self.y = 0;
        if let Some((t, _)) = self.tip(pin_number) {
            self.x = target.x - t.x;
            self.y = target.y - t.y;
        }
    }

    pub fn box_rect(&self) -> Rect {
        Rect { x0: self.x, y0: self.y, x1: self.x + self.w(), y1: self.y + self.h() }
    }

    /// Sides that carry a drawn pin.
    fn sides(&self) -> Vec<Side> {
        let mut v: Vec<Side> = self.b.node.ports.iter().map(|p| if self.flip { flip_side(p.side) } else { p.side }).collect();
        v.sort_by_key(|s| *s as u8);
        v.dedup();
        v
    }

    /// The box with its pin stubs: what the painter's own bounding box for the symbol is.
    pub fn body_rect(&self) -> Rect {
        let mut r = self.box_rect();
        for s in self.sides() {
            match s {
                Side::Top => r.y0 -= STUB,
                Side::Bottom => r.y1 += STUB,
                Side::Left => r.x0 -= STUB,
                Side::Right => r.x1 += STUB,
            }
        }
        r
    }

    /// Exactly two pins, both at the top and bottom: a resistor or capacitor standing up. The painter puts its fields beside it.
    pub fn is_vertical_two_pin(&self) -> bool {
        let wired: Vec<usize> = self.b.pin_port.iter().flatten().copied().collect();
        wired.len() == 2 && self.part.pins.len() == 2 && wired.iter().all(|&i| matches!(self.b.node.ports[i].side, Side::Top | Side::Bottom))
    }

    /// The painter's Reference / Value / footprint text rectangles (`drawFieldsAbout`, as the studio draws it).
    pub fn field_rects(&self) -> Vec<Rect> {
        let body = self.body_rect();
        let reference = self.part.reference.as_str();
        let mut out = Vec::new();
        if self.is_vertical_two_pin() {
            // Beside the body, ref above the middle line and value below it, left-aligned.
            let x0 = body.x1 + 800;
            let cy = (body.y0 + body.y1) / 2;
            out.push(Rect::new(x0, cy - 300 - cap(REF_FONT), x0 + text_w(REF_FONT, reference), cy - 300));
            if !self.value.is_empty() {
                out.push(Rect::new(x0, cy + 1500 - cap(VALUE_FONT), x0 + text_w(VALUE_FONT, &self.value), cy + 1500));
            }
            if !self.footprint.is_empty() {
                let base = cy + 1500 + 1150;
                out.push(Rect::new(x0, base - cap(FIELD_FONT), x0 + text_w(FIELD_FONT, &self.footprint), base));
            }
        } else {
            // Above and below, centred on the body.
            let cx = (body.x0 + body.x1) / 2;
            let tw = text_w(REF_FONT, reference);
            out.push(Rect::new(cx - tw / 2, body.y0 - 400 - cap(REF_FONT), cx + tw / 2, body.y0 - 400));
            if !self.value.is_empty() {
                let tw = text_w(VALUE_FONT, &self.value);
                out.push(Rect::new(cx - tw / 2, body.y1 + 1800 - cap(VALUE_FONT), cx + tw / 2, body.y1 + 1800));
            }
            if !self.footprint.is_empty() {
                let tw = text_w(FIELD_FONT, &self.footprint);
                let base = body.y1 + 1800 + 1150;
                out.push(Rect::new(cx - tw / 2, base - cap(FIELD_FONT), cx + tw / 2, base));
            }
        }
        out
    }

    /// The body with stubs, and every text it draws.
    pub fn keepout(&self) -> Rect {
        let mut r = self.body_rect();
        for f in self.field_rects() {
            r = r.union(f);
        }
        r
    }
}

/// The painter's rectangle for a power symbol's glyph and its net-name text, `up` when the glyph rises from the tip.
pub fn power_rect(tip: Point, net: &str, up: bool) -> Rect {
    let text = text_w(VALUE_FONT, net);
    let (y0, y1) = if up { (tip.y - 2640, tip.y + 600) } else { (tip.y - 900, tip.y + 2640) };
    Rect { x0: tip.x - 1270, y0, x1: tip.x + 508 + text, y1 }
}

/// A label at the end of a wire that leaves along `dir`: the flag and the text beyond the anchor.
pub fn label_rect(at: Point, dir: (i64, i64), net: &str, hierarchical: bool) -> Rect {
    let tw = text_w(LABEL_FONT, net);
    let len = if hierarchical { 1270 + 600 + tw } else { 400 + tw };
    let across = 900;
    match dir {
        (-1, 0) => Rect { x0: at.x - len, y0: at.y - across, x1: at.x, y1: at.y + across },
        (1, 0) => Rect { x0: at.x, y0: at.y - across, x1: at.x + len, y1: at.y + across },
        (0, -1) => Rect { x0: at.x - across, y0: at.y - len, x1: at.x + across, y1: at.y },
        _ => Rect { x0: at.x - across, y0: at.y, x1: at.x + across, y1: at.y + len },
    }
}

/// A no-connect flag.
pub fn nc_rect(at: Point) -> Rect {
    Rect { x0: at.x - 610, y0: at.y - 610, x1: at.x + 610, y1: at.y + 610 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::{Pin, PinKind};

    fn two_pin(reference: &str, value: &str) -> Part {
        Part {
            reference: reference.into(),
            mpn: None,
            lcsc: None,
            value: Some(value.into()),
            package: None,
            footprint: None,
            symbol: None,
            datasheet: None,
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
            edge: None,
        }
    }

    #[test]
    fn paper_usable_areas_are_on_grid_and_inside_the_frame() {
        for p in PAPERS {
            let u = p.usable();
            for v in [u.x0, u.y0, u.x1, u.y1] {
                assert_eq!(v % G, 0, "{} {u:?}", p.name);
            }
            assert!(p.inner_frame().contains(&u));
            assert!(u.x1 - u.x0 > 100_000 && u.y1 - u.y0 > 80_000, "{} {u:?}", p.name);
        }
        assert_eq!(smallest_paper(200_000, 100_000).name, "A4");
        assert_eq!(smallest_paper(300_000, 100_000).name, "A3");
    }

    #[test]
    fn a_standing_resistor_keeps_its_fields_beside_it_and_a_flipped_one_swaps_its_tips() {
        let resolved = eda_model::symbol::builtin("Device:R");
        let mut r = Placed::new(&two_pin("R1", "330"), "Device:R", resolved, "330", "0603");
        assert!(r.is_vertical_two_pin());
        r.x = 10 * G;
        r.y = 10 * G;
        let (top, side) = r.tip("1").unwrap();
        assert_eq!(side, Side::Top);
        assert_eq!(top.y, r.y - STUB);
        let (bottom, _) = r.tip("2").unwrap();
        assert_eq!(bottom.y, r.y + r.h() + STUB);
        let fields = r.field_rects();
        assert!(fields.iter().all(|f| f.x0 >= r.body_rect().x1), "fields sit to the right of the body: {fields:?}");
        // Turned half a turn, pin 2 is the top one, in the same column.
        r.flip = true;
        let (top2, _) = r.tip("2").unwrap();
        let (bottom1, _) = r.tip("1").unwrap();
        assert_eq!(top2.y, r.y - STUB);
        assert_eq!(bottom1.y, r.y + r.h() + STUB);
        assert_eq!(top2.x, top.x);
        assert_eq!(r.at().x, r.x + r.w());
        // place_tip puts a chosen tip where asked.
        r.place_tip("2", Point { x: 5 * G, y: 7 * G });
        assert_eq!(r.tip("2").unwrap().0, Point { x: 5 * G, y: 7 * G });
    }

    #[test]
    fn rect_overlap_is_strict() {
        let a = Rect::new(0, 0, 10, 10);
        assert!(!a.overlaps(&Rect::new(10, 0, 20, 10)), "touching edges do not overlap");
        assert!(a.overlaps(&Rect::new(9, 9, 20, 20)));
        assert!(a.contains(&Rect::new(1, 1, 9, 9)));
    }
}
