//! The measuring kit the schematic layouts share: rectangles, the boxes KiCad draws text in, the footprint of a drawn symbol, the paper
//! sizes and the drawing sheet's frame.
//!
//! What a layout promises ("no overlapping text") is only worth what its measuring is: a symbol's pin names and numbers, its fields,
//! a label with its flag and a power symbol with its value are measured here with eeschema's own rules (`eda_model::kicad_geom`, the
//! stroke font's widths, `crate::fields` for where Autoplace Fields puts the fields), the same ones the overlap checker
//! (`eda_kicad::sch_overlap`) holds the written `.kicad_sch` files to. Everything is micrometres, rounded outward to whole ones.

use eda_layout::{Point as LPoint, Side};
use eda_model::ir::{Point, PowerSymbol, SymbolInstance};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::kicad_geom::{self as kg, TextStyle};
use eda_model::symbol::LibSymbol;
use eda_model::Part;

use crate::geometry::GRID;
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

/// Every schematic text is 50 mils.
pub const TEXT_FONT: i64 = 1270;
pub const SHEET_NAME_FONT: i64 = TEXT_FONT;
pub const SHEET_FILE_FONT: i64 = TEXT_FONT;
pub const SHEET_PIN_FONT: i64 = TEXT_FONT;

/// KiCad's box (floating point) as a whole-micrometre one that holds it.
pub fn outward(r: kg::Rect) -> Rect {
    Rect { x0: r.x0.floor() as i64, y0: r.y0.floor() as i64, x1: r.x1.ceil() as i64, y1: r.y1.ceil() as i64 }
}

fn fpt(p: Point) -> (f64, f64) {
    (p.x as f64, p.y as f64)
}

/// `EDA_TEXT::GetTextBox` of one line of 1.27 mm text at `anchor`, justified `h`/`v`, turned a quarter when `vertical`.
pub fn text_rect(text: &str, anchor: (f64, f64), h: HJustify, v: VJustify, vertical: bool) -> Rect {
    outward(TextStyle::new(h, v).text_box(text, anchor, vertical as i32))
}

/// [`text_rect`] for a text set in `style` (its size and pen; the justification is the style's own).
pub fn text_rect_styled(text: &str, anchor: (f64, f64), style: &TextStyle, vertical: bool) -> Rect {
    outward(style.text_box(text, anchor, vertical as i32))
}

/// The width of a text's box, pen included: `GetTextBox`'s width.
pub fn text_w(_font: i64, s: &str) -> i64 {
    text_rect(s, (0.0, 0.0), HJustify::Left, VJustify::Center, false).w()
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

    /// This symbol as an instance on the sheet, for measuring.
    pub fn instance(&self) -> SymbolInstance {
        SymbolInstance {
            id: self.part.reference.clone(),
            at: self.at(),
            rot: self.rot(),
            mirrored: false,
            mirror_y: false,
            lib_id: self.lib_id.clone(),
            unit: 1,
            value: self.value.clone(),
            footprint: self.footprint.clone(),
            datasheet: String::new(),
            dnp: false,
            exclude_from_bom: false,
            exclude_from_board: false,
            exclude_from_sim: false,
        }
    }

    /// Where its body, its pins and their texts are, as KiCad draws them.
    pub fn geom(&self) -> crate::symgeom::SymbolGeom {
        crate::symgeom::SymbolGeom::of(&self.instance(), &self.part, self.resolved.as_ref())
    }

    /// The fields where Autoplace Fields puts them (`crate::fields`), on the sheet.
    pub fn page_fields(&self) -> Vec<crate::fields::PageField> {
        let sym = self.instance();
        let geom = self.geom();
        let specs = crate::fields::symbol_specs(&sym, Some(&self.part), self.resolved.as_ref().map(|r| r.datasheet.as_str()).unwrap_or(""));
        let placed = crate::fields::autoplace_symbol(&sym, &geom, &specs);
        specs.iter().zip(placed.iter()).map(|(spec, p)| crate::fields::page_field(sym.at, sym.rot, sym.mirrored, geom.width, p, &spec.text)).collect()
    }

    /// The box of each drawn field's text.
    pub fn field_rects(&self) -> Vec<Rect> {
        self.page_fields().iter().filter(|f| f.visible && !f.text.is_empty()).map(|f| text_rect(&f.text, f.at, f.h, f.v, f.vertical)).collect()
    }

    /// The boxes of the pins' names and numbers.
    pub fn pin_text_rects(&self) -> Vec<Rect> {
        let g = self.geom();
        g.pin_rects().into_iter().filter(|(_, p)| p.length >= 0.0).map(|(r, _)| outward(r)).collect()
    }

    /// The body and the pin lines: what the painter's own bounding box for the symbol is.
    pub fn body_rect(&self) -> Rect {
        let g = self.geom();
        let mut r = g.body.map(outward).unwrap_or_else(|| self.box_rect());
        for p in &g.pins {
            let end = (p.tip.0 + p.orient.dir().0 * p.length, p.tip.1 + p.orient.dir().1 * p.length);
            r = r.union(outward(kg::Rect::new(p.tip, end)));
        }
        r
    }

    /// The body with its pins, and every text it draws, the fields where `sch` keeps them (else where Autoplace Fields puts them).
    pub fn keepout_in(&self, sch: &eda_model::ir::SchematicSection) -> Rect {
        let mut r = self.body_rect();
        for t in self.pin_text_rects() {
            r = r.union(t);
        }
        let (inst, geom) = (self.instance(), self.geom());
        for f in crate::fields::symbol_fields(sch, &inst, Some(&self.part), self.resolved.as_ref(), &geom) {
            if f.visible && !f.text.is_empty() {
                r = r.union(text_rect(&f.text, f.at, f.h, f.v, f.vertical));
            }
        }
        r
    }

    /// The body with its pins, and every text it draws.
    pub fn keepout(&self) -> Rect {
        let mut r = self.body_rect();
        for f in self.field_rects() {
            r = r.union(f);
        }
        for t in self.pin_text_rects() {
            r = r.union(t);
        }
        r
    }
}

/// A power symbol with its value: `rot` turns it (0 hangs a ground symbol down and raises a supply one), `lib_id` says which glyph.
pub fn power_rect(at: Point, rot: u32, net: &str, lib_id: &str) -> Rect {
    let ps = PowerSymbol { id: String::new(), lib_id: lib_id.to_string(), at, rot, net: net.to_string(), pin: String::new() };
    let lib = eda_model::symbol::builtin(lib_id);
    let mut r = outward(kg::Rect::point(fpt(at)));
    if let Some(lib) = &lib {
        if let Some(b) = crate::symgeom::SymbolGeom::of_origin_symbol(at, rot, lib).body {
            r = r.union(outward(b));
        }
    }
    let sch = eda_model::ir::SchematicSection::default();
    for f in crate::fields::power_fields(&sch, &ps) {
        if f.visible && !f.text.is_empty() {
            r = r.union(text_rect(&f.text, f.at, f.h, f.v, f.vertical));
        }
    }
    r
}

/// A label at the end of a wire that leaves along `dir`: its text runs on away from the wire, a hierarchical one with its flag
/// (`SCH_HIERLABEL::GetBodyBoundingBox`), a local one as `SCH_LABEL::GetBodyBoundingBox` has it.
pub fn label_rect(at: Point, dir: (i64, i64), net: &str, hierarchical: bool) -> Rect {
    let (spin, turns) = match dir {
        (-1, 0) => (kg::Spin::Left, 0),
        (0, -1) => (kg::Spin::Up, 1),
        (0, 1) => (kg::Spin::Bottom, 1),
        _ => (kg::Spin::Right, 0),
    };
    let h = if matches!(spin, kg::Spin::Left | kg::Spin::Bottom) { HJustify::Right } else { HJustify::Left };
    if hierarchical {
        outward(kg::hier_label_rect(&TextStyle::new(h, VJustify::Center), net, fpt(at), spin))
    } else {
        outward(kg::local_label_rect(&TextStyle::new(h, VJustify::Bottom), net, fpt(at), turns))
    }
}

/// A sheet's pin on its left edge (`left`) or right edge: the flag and the name read into the sheet.
pub fn sheet_pin_rect(pin: Point, left: bool, name: &str) -> Rect {
    let (h, spin) = if left { (HJustify::Left, kg::Spin::Right) } else { (HJustify::Right, kg::Spin::Left) };
    outward(kg::hier_label_rect(&TextStyle::new(h, VJustify::Center), name, fpt(pin), spin))
}

/// A no-connect flag.
pub fn nc_rect(at: Point) -> Rect {
    Rect { x0: at.x - 610, y0: at.y - 610, x1: at.x + 610, y1: at.y + 610 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::STUB;
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
