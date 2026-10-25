//! The measuring kit the schematic layouts share: rectangles, the boxes KiCad draws text in, the footprint of a drawn symbol, the paper
//! sizes and the drawing sheet's frame.
//!
//! What a layout promises ("no overlapping text") is only worth what its measuring is: a symbol's pin names and numbers, its fields,
//! a label with its flag and a power symbol with its value are measured here with eeschema's own rules (`eda_model::kicad_geom`, the
//! stroke font's widths, `crate::fields` for where Autoplace Fields puts the fields), the same ones the overlap checker
//! (`eda_kicad::sch_overlap`) holds the written `.kicad_sch` files to. Everything is nanometres, rounded outward to whole ones.

use eda_model::units::Nm;
use eda_layout::{Point as LPoint, Side};
use eda_model::ir::{Point, PowerSymbol, SymbolInstance};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::kicad_geom::{self as kg, TextStyle};
use eda_model::symbol::LibSymbol;
use eda_model::Part;

use crate::geometry::GRID;
use crate::placed::{part_box, PartBox};

pub const G: i64 = GRID;

/// One grid cell as a length.
pub const G_NM: Nm = crate::geometry::GRID_NM;

pub fn snap_up(v: Nm) -> Nm {
    G_NM * v.div_euclid(G_NM) + if v.rem_euclid(G_NM) == Nm::ZERO { Nm::ZERO } else { G_NM }
}

pub fn snap_down(v: Nm) -> Nm {
    G_NM * v.div_euclid(G_NM)
}

/// An axis-aligned rectangle, `x0 <= x1`, `y0 <= y1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: Nm,
    pub y0: Nm,
    pub x1: Nm,
    pub y1: Nm,
}

impl Rect {
    pub fn new(ax: Nm, ay: Nm, bx: Nm, by: Nm) -> Rect {
        Rect { x0: ax.min(bx), y0: ay.min(by), x1: ax.max(bx), y1: ay.max(by) }
    }
    /// The box with corners `a` and `b` (in either order).
    pub fn of(a: Point, b: Point) -> Rect {
        Rect::new(a.x, a.y, b.x, b.y)
    }
    /// The box of `w` by `h` whose corner is `at`.
    pub fn sized(at: Point, w: Nm, h: Nm) -> Rect {
        Rect::new(at.x, at.y, at.x + w, at.y + h)
    }
    /// The empty box at `p`.
    pub fn at(p: Point) -> Rect {
        Rect { x0: p.x, y0: p.y, x1: p.x, y1: p.y }
    }
    /// The box with corners `a` and `b`, grown by `m` on every side.
    pub fn around(a: Point, b: Point, m: Nm) -> Rect {
        Rect::of(a, b).inflate(m)
    }
    pub fn w(&self) -> Nm {
        self.x1 - self.x0
    }
    pub fn h(&self) -> Nm {
        self.y1 - self.y0
    }
    pub fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn inflate(self, m: Nm) -> Rect {
        Rect { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m }
    }
    pub fn translate(self, dx: Nm, dy: Nm) -> Rect {
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
pub const TEXT_FONT: Nm = Nm::new(1_270_000);
pub const SHEET_NAME_FONT: Nm = TEXT_FONT;
pub const SHEET_FILE_FONT: Nm = TEXT_FONT;
pub const SHEET_PIN_FONT: Nm = TEXT_FONT;

/// KiCad's box (floating point) as a whole-nanometre one that holds it.
pub fn outward(r: kg::Rect) -> Rect {
    Rect { x0: Nm::new(r.x0.floor() as i64), y0: Nm::new(r.y0.floor() as i64), x1: Nm::new(r.x1.ceil() as i64), y1: Nm::new(r.y1.ceil() as i64) }
}

fn fpt(p: Point) -> (f64, f64) {
    (p.x.as_f64(), p.y.as_f64())
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
pub fn text_w(_font: Nm, s: &str) -> Nm {
    text_rect(s, (0.0, 0.0), HJustify::Left, VJustify::Center, false).w()
}

// ------------------------------------------------------------------------------------------------------ frame

/// A paper size: KiCad's names, landscape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paper {
    pub name: &'static str,
    pub w: Nm,
    pub h: Nm,
}

pub const PAPERS: [Paper; 5] = [
    Paper { name: "A4", w: Nm::new(297_000_000), h: Nm::new(210_000_000) },
    Paper { name: "A3", w: Nm::new(420_000_000), h: Nm::new(297_000_000) },
    Paper { name: "A2", w: Nm::new(594_000_000), h: Nm::new(420_000_000) },
    Paper { name: "A1", w: Nm::new(841_000_000), h: Nm::new(594_000_000) },
    Paper { name: "A0", w: Nm::new(1_189_000_000), h: Nm::new(841_000_000) },
];

pub fn paper_named(name: &str) -> Paper {
    PAPERS.iter().copied().find(|p| p.name.eq_ignore_ascii_case(name)).unwrap_or(PAPERS[0])
}

/// KiCad's default drawing sheet: a frame 10 mm in from the paper, a second line 2 mm inside it, and the title block flush with
/// the inner frame's bottom-right corner (108 x 32 mm).
pub const FRAME_OUTER: Nm = Nm::new(10_000_000);
pub const FRAME_INNER: Nm = Nm::new(12_000_000);
pub const TITLE_W: Nm = Nm::new(108_000_000);
pub const TITLE_H: Nm = Nm::new(32_000_000);
/// Clear space kept between anything drawn and the inner frame line or the title block.
pub const FRAME_CLEAR: Nm = Nm::new(5_000_000);

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
pub fn smallest_paper(w: Nm, h: Nm) -> Paper {
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
    pub x: Nm,
    pub y: Nm,
    pub flip: bool,
    /// The text the painter draws under or beside it.
    pub value: String,
    pub footprint: String,
    /// Which unit of its library symbol it draws (1 for a symbol of one unit).
    pub unit: u32,
    /// Where the user placed its fields, when they did: they are measured there, not where Autoplace Fields would put them.
    pub kept_fields: Option<Vec<eda_model::ir::FieldPlacement>>,
}

impl Placed {
    pub fn new(part: &Part, lib_id: &str, resolved: Option<LibSymbol>, value: &str, footprint: &str) -> Placed {
        let b = part_box(part, resolved.as_ref(), 1);
        Placed { part: part.clone(), lib_id: lib_id.to_string(), resolved, b, x: Nm::ZERO, y: Nm::ZERO, flip: false, value: value.to_string(), footprint: footprint.to_string(), unit: 1, kept_fields: None }
    }

    /// The same symbol drawn as its unit `unit`: the box and the ports of that unit's body and pins.
    pub fn in_unit(mut self, unit: u32) -> Placed {
        self.unit = unit;
        self.b = part_box(&self.part, self.resolved.as_ref(), unit);
        self
    }

    pub fn w(&self) -> Nm {
        Nm::new(self.b.width())
    }
    pub fn h(&self) -> Nm {
        Nm::new(self.b.height())
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
            Some((Point { x: self.x + self.w() - Nm::new(l.x), y: self.y + self.h() - Nm::new(l.y) }, flip_side(side)))
        } else {
            Some((Point { x: self.x + Nm::new(l.x), y: self.y + Nm::new(l.y) }, side))
        }
    }

    /// Move the symbol so that `pin_number`'s tip lands on `target`.
    pub fn place_tip(&mut self, pin_number: &str, target: Point) {
        self.x = Nm::ZERO;
        self.y = Nm::ZERO;
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
            unit: self.unit,
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

    /// The fields where the user placed them, else where Autoplace Fields puts them (`crate::fields`), on the sheet.
    pub fn page_fields(&self) -> Vec<crate::fields::PageField> {
        let sym = self.instance();
        let geom = self.geom();
        if let Some(kept) = &self.kept_fields {
            // the user's own places, kept: the same lookup a stored section makes
            let sch = eda_model::ir::SchematicSection { field_layout: [(eda_model::ir::field_key(&sym.id, sym.unit), kept.clone())].into_iter().collect(), ..Default::default() };
            return crate::fields::symbol_fields(&sch, &sym, Some(&self.part), self.resolved.as_ref(), &geom);
        }
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

/// Half the side of a no-connect flag: `m_size = schIUScale.MilsToIU( DEFAULT_NOCONNECT_SIZE )` (48 mil, `eeschema/default_values.h`), halved.
/// (It was written `610` when the IR was micrometres, 0.61 mm, and was left at 610 nm by the move to nanometres.)
const NC_HALF_NM: Nm = Nm::new(609_600);

/// A no-connect flag: the cross's own square, `SCH_NO_CONNECT`'s size about its position (KiCad's `GetBoundingBox` adds half the pen).
pub fn nc_rect(at: Point) -> Rect {
    Rect::at(at).inflate(NC_HALF_NM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::{mil, mm};
    use crate::geometry::STUB;
    use eda_model::{Pin, PinKind};

    /// The flag is KiCad's 48 mil square, not a thousandth of it (the micrometre `610` read as nanometres).
    #[test]
    fn a_no_connect_flag_is_48_mil_across() {
        let at = Point { x: Nm::new(10_160_000), y: Nm::new(5_080_000) };
        let r = nc_rect(at);
        assert_eq!((r.x1 - r.x0, r.y1 - r.y0), (mil(48.0), mil(48.0)));
        assert_eq!((r.x0 + r.x1, r.y0 + r.y1), (at.x * 2, at.y * 2), "centred on the flag");
    }

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
            body_nm: None,
            edge: None,
        }
    }

    #[test]
    fn paper_usable_areas_are_on_grid_and_inside_the_frame() {
        for p in PAPERS {
            let u = p.usable();
            for v in [u.x0, u.y0, u.x1, u.y1] {
                assert_eq!(v % G_NM, Nm::ZERO, "{} {u:?}", p.name);
            }
            assert!(p.inner_frame().contains(&u));
            assert!(u.x1 - u.x0 > mm(100.0) && u.y1 - u.y0 > mm(80.0), "{} {u:?}", p.name);
        }
        assert_eq!(smallest_paper(mm(200.0), mm(100.0)).name, "A4");
        assert_eq!(smallest_paper(mm(300.0), mm(100.0)).name, "A3");
    }

    #[test]
    fn a_standing_resistor_keeps_its_fields_beside_it_and_a_flipped_one_swaps_its_tips() {
        let resolved = eda_model::symbol::builtin("Device:R");
        let mut r = Placed::new(&two_pin("R1", "330"), "Device:R", resolved, "330", "0603");
        r.x = G_NM * 10;
        r.y = G_NM * 10;
        let (top, side) = r.tip("1").unwrap();
        assert_eq!(side, Side::Top);
        assert_eq!(top.y, r.y - Nm::new(STUB));
        let (bottom, _) = r.tip("2").unwrap();
        assert_eq!(bottom.y, r.y + r.h() + Nm::new(STUB));
        let fields = r.field_rects();
        assert!(fields.iter().all(|f| f.x0 >= r.body_rect().x1), "fields sit to the right of the body: {fields:?}");
        // Turned half a turn, pin 2 is the top one, in the same column.
        r.flip = true;
        let (top2, _) = r.tip("2").unwrap();
        let (bottom1, _) = r.tip("1").unwrap();
        assert_eq!(top2.y, r.y - Nm::new(STUB));
        assert_eq!(bottom1.y, r.y + r.h() + Nm::new(STUB));
        assert_eq!(top2.x, top.x);
        assert_eq!(r.at().x, r.x + r.w());
        // place_tip puts a chosen tip where asked.
        r.place_tip("2", Point { x: G_NM * 5, y: G_NM * 7 });
        assert_eq!(r.tip("2").unwrap().0, Point { x: G_NM * 5, y: G_NM * 7 });
    }

    #[test]
    fn rect_overlap_is_strict() {
        let r = |x0: i64, y0: i64, x1: i64, y1: i64| Rect::new(Nm::new(x0), Nm::new(y0), Nm::new(x1), Nm::new(y1));
        let a = r(0, 0, 10, 10);
        assert!(!a.overlaps(&r(10, 0, 20, 10)), "touching edges do not overlap");
        assert!(a.overlaps(&r(9, 9, 20, 20)));
        assert!(a.contains(&r(1, 1, 9, 9)));
    }
}
