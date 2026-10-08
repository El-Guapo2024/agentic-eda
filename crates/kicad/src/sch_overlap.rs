//! Does anything on a schematic sheet overlap anything else, the way KiCad itself draws it?
//!
//! The checker reads the `.kicad_sch` text (the file kicad-cli renders), builds every item eeschema would draw -- symbol bodies,
//! pins, pin names and numbers, fields, labels with their flags, power symbols, sheet symbols and their pins, wires, junctions,
//! no-connect flags -- gives each the bounding box eeschema gives it, and lists the pairs whose boxes overlap, except where
//! two items are meant to touch (a wire, a label or a power symbol on a pin's end; a symbol's own pin texts inside its body).
//!
//! The box rules are ported from the KiCad source at 8303b2ad (10.99), not remembered:
//!
//! * text: `EDA_TEXT::GetTextBox` and `STROKE_FONT::GetTextAsGlyphs` (`eda_model::kicad_font`);
//! * pins: `PIN_LAYOUT_CACHE` (`GetPinNameInfo`, `GetPinNumberInfo`, `getUntransformedPin*Box`), `SCH_PIN::GetBoundingBox`;
//! * fields: `SCH_FIELD::GetBoundingBox` (the text box turned about its own anchor, then the symbol's mirror and rotation);
//! * symbols: `SCH_SYMBOL::SetOrientation` (the transform), `LIB_SYMBOL::GetBodyBoundingBox` (body graphics, no fields, no pins);
//! * labels: `SCH_LABEL::GetBodyBoundingBox`, `SCH_HIERLABEL::GetBodyBoundingBox` (sheet pins are hierarchical labels) and the
//!   spin style the parser reads off the angle and the justification;
//! * sheets: `SCH_SHEET::GetBodyBoundingBox`, `SCH_SHEET_PIN::SetSide`.
//!
//! Units: micrometres, y down, as `f64`.

use std::collections::BTreeMap;

use eda_model::kicad_font::{default_pen_iu, kiround, mm_to_iu, string_boundary_limits, text_box_iu, HJustify, VJustify, DEFAULT_TEXT_SIZE_IU};

use crate::sexpr::{self, find, find_all, num, tag, txt, Sexpr};

/// Boxes that overlap by less than this (micrometres) in either direction only touch.
const TOL: f64 = 3.0;

/// Schematic internal units (0.1 um) to micrometres.
fn um(iu: i64) -> f64 {
    iu as f64 / 10.0
}

// ------------------------------------------------------------------------------------------------------- geometry

pub type Pt = (f64, f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(a: Pt, b: Pt) -> Rect {
        Rect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
    }
    pub fn point(p: Pt) -> Rect {
        Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 }
    }
    pub fn merge(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn merge_point(self, p: Pt) -> Rect {
        self.merge(Rect::point(p))
    }
    pub fn inflate(self, d: f64) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    pub fn offset(self, dx: f64, dy: f64) -> Rect {
        Rect { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    /// The size of the part the two boxes share, when it has an area.
    fn shared(&self, o: &Rect) -> Option<(f64, f64)> {
        let w = self.x1.min(o.x1) - self.x0.max(o.x0);
        let h = self.y1.min(o.y1) - self.y0.max(o.y0);
        (w > TOL && h > TOL).then_some((w, h))
    }
}

/// Which way a point turns for KiCad's `RotatePoint` with a positive angle: counter-clockwise on the y-down page, so a quarter
/// turn takes (x, y) to (y, -x).
fn rotate_about(p: Pt, c: Pt, quarter_turns: i32) -> Pt {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    let (rx, ry) = match quarter_turns.rem_euclid(4) {
        0 => (dx, dy),
        1 => (dy, -dx),
        2 => (-dx, -dy),
        _ => (-dy, dx),
    };
    (c.0 + rx, c.1 + ry)
}

/// `TRANSFORM`: where a symbol's own frame (y down, origin at the symbol) goes on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Xf {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Xf {
    const IDENTITY: Xf = Xf { x1: 1, y1: 0, x2: 0, y2: 1 };

    fn apply(&self, p: Pt) -> Pt {
        (self.x1 as f64 * p.0 + self.y1 as f64 * p.1, self.x2 as f64 * p.0 + self.y2 as f64 * p.1)
    }

    /// The inverse of a rotation or mirror is its transpose.
    fn unapply(&self, p: Pt) -> Pt {
        (self.x1 as f64 * p.0 + self.x2 as f64 * p.1, self.y1 as f64 * p.0 + self.y2 as f64 * p.1)
    }

    /// `SetOrientation`'s incremental step: the new matrix is the old one times the requested change.
    fn then(self, t: Xf) -> Xf {
        Xf {
            x1: self.x1 * t.x1 + self.x2 * t.y1,
            y1: self.y1 * t.x1 + self.y2 * t.y1,
            x2: self.x1 * t.x2 + self.x2 * t.y2,
            y2: self.y1 * t.x2 + self.y2 * t.y2,
        }
    }

    /// The transform of an instance written `(at x y angle)` and `(mirror x|y)`.
    fn of_instance(angle: f64, mirror: Option<&str>) -> Xf {
        const CCW: Xf = Xf { x1: 0, y1: 1, x2: -1, y2: 0 };
        const CW: Xf = Xf { x1: 0, y1: -1, x2: 1, y2: 0 };
        let mut t = Xf::IDENTITY;
        match (angle.round() as i64).rem_euclid(360) {
            90 => t = t.then(CCW),
            180 => t = t.then(CCW).then(CCW),
            270 => t = t.then(CW),
            _ => {}
        }
        match mirror {
            Some("x") => t = t.then(Xf { x1: 1, y1: 0, x2: 0, y2: -1 }),
            Some("y") => t = t.then(Xf { x1: -1, y1: 0, x2: 0, y2: 1 }),
            _ => {}
        }
        t
    }
}

// ------------------------------------------------------------------------------------------------------- items

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Body,
    Pin,
    PinName,
    PinNumber,
    Field,
    Label,
    Text,
    SheetBody,
    SheetPin,
    Wire,
    Junction,
    NoConnect,
}

impl Kind {
    /// Items that hang on a connection point and are meant to touch whatever else is there.
    fn is_connector(self) -> bool {
        matches!(self, Kind::Wire | Kind::Pin | Kind::Junction | Kind::NoConnect)
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    /// The symbol, power symbol, sheet or label the item belongs to.
    pub owner: String,
    pub what: String,
    pub rect: Rect,
    /// The item itself is a line (a wire, a pin): its box is only for broad checks.
    pub seg: Option<(Pt, Pt)>,
    /// The connection points the item is anchored to.
    pub anchors: Vec<Pt>,
    /// For a pin's name and number: the pin's connection point.
    pub pin_tip: Option<Pt>,
}

impl Item {
    fn boxed(kind: Kind, owner: &str, what: String, rect: Rect) -> Item {
        Item { kind, owner: owner.to_string(), what, rect, seg: None, anchors: Vec::new(), pin_tip: None }
    }
}

// ------------------------------------------------------------------------------------------------------- text

/// One piece of text as eeschema lays it out: where its anchor is, how it is justified, how big it is.
#[derive(Debug, Clone)]
struct Effects {
    /// KiCad writes `(size height width)`; the box only needs the height.
    size_iu: i64,
    thickness_iu: i64,
    h: HJustify,
    v: VJustify,
    hide: bool,
}

fn has_hide(list: &[Sexpr]) -> bool {
    list.iter().any(|it| match it {
        Sexpr::Atom(a) => a == "hide",
        Sexpr::List(l) => tag(l) == Some("hide") && !matches!(txt(l, 1), Some("no") | Some("false")),
    })
}

/// Read an `(effects ...)` list (or the lack of one): the file format's default justification is centred both ways.
fn effects_of(parent: &[Sexpr], default_h: HJustify, default_v: VJustify) -> Effects {
    let mut e = Effects { size_iu: DEFAULT_TEXT_SIZE_IU, thickness_iu: 0, h: default_h, v: default_v, hide: has_hide(parent) };
    let Some(fx) = find(parent, "effects") else { return e };
    e.hide |= has_hide(fx);
    e.h = HJustify::Center;
    e.v = VJustify::Center;
    if let Some(font) = find(fx, "font") {
        if let Some(sz) = find(font, "size").and_then(|s| num(s, 1)) {
            e.size_iu = mm_to_iu(sz);
        }
        if let Some(t) = find(font, "thickness").and_then(|s| num(s, 1)) {
            e.thickness_iu = mm_to_iu(t);
        }
    }
    if let Some(j) = find(fx, "justify") {
        for a in j.iter().skip(1) {
            match a.text() {
                Some("left") => e.h = HJustify::Left,
                Some("right") => e.h = HJustify::Right,
                Some("top") => e.v = VJustify::Top,
                Some("bottom") => e.v = VJustify::Bottom,
                _ => {}
            }
        }
    }
    e
}

impl Effects {
    fn pen(&self) -> i64 {
        if self.thickness_iu > 1 {
            self.thickness_iu.min(kiround(self.size_iu as f64 * 0.25))
        } else {
            default_pen_iu(self.size_iu)
        }
    }

    /// `EDA_TEXT::GetTextBox` at `anchor`, then turned about the anchor by `quarter_turns` (`GetTextAngle`).
    fn text_box(&self, text: &str, anchor: Pt, quarter_turns: i32) -> Rect {
        let (x0, y0, x1, y1) = text_box_iu(text, self.size_iu, self.pen(), self.h, self.v);
        let a = (anchor.0 + um(x0), anchor.1 + um(y0));
        let b = (anchor.0 + um(x1), anchor.1 + um(y1));
        let (a, b) = (rotate_about(a, anchor, quarter_turns), rotate_about(b, anchor, quarter_turns));
        Rect::new(a, b)
    }

    /// The box of text centred (or justified as given) on `anchor` without the 17 % fudge `GetTextBox` adds: what the pin
    /// layout cache measures (`StringBoundaryLimits`).
    fn extents(&self, text: &str) -> (f64, f64) {
        let (w, h) = string_boundary_limits(text, self.size_iu, self.pen());
        (um(w), um(h))
    }
}

fn quarter_turns_of(angle: f64) -> i32 {
    ((angle / 90.0).round() as i32).rem_euclid(4)
}

/// `KeepUpright`: texts are only ever horizontal or vertical, and never upside down.
fn upright_quarter_turns(angle: f64) -> i32 {
    quarter_turns_of(angle) % 2
}

// ----------------------------------------------------------------------------------------------------- library

#[derive(Debug, Clone)]
struct LibPin {
    number: String,
    name: String,
    x: f64,
    y: f64,
    angle: f64,
    length: f64,
    hide: bool,
    name_fx: Effects,
    number_fx: Effects,
    unit: u32,
    style: u32,
}

#[derive(Debug, Clone)]
struct Gfx {
    unit: u32,
    style: u32,
    /// In the library's frame (mm, y up).
    bbox: Rect,
}

#[derive(Debug, Clone, Default)]
struct LibSym {
    power: bool,
    names_hidden: bool,
    numbers_hidden: bool,
    /// `(pin_names (offset x))`, mm; the library default is 0.508.
    name_offset: f64,
    pins: Vec<LibPin>,
    gfx: Vec<Gfx>,
}

fn point_of(list: &[Sexpr]) -> Option<Pt> {
    Some((num(list, 1)?, num(list, 2)?))
}

fn pts_of(list: &[Sexpr]) -> Vec<Pt> {
    find(list, "pts").map(|p| find_all(p, "xy").filter_map(point_of).collect()).unwrap_or_default()
}

fn stroke_pen_mm(list: &[Sexpr]) -> f64 {
    let w = find(list, "stroke").and_then(|s| find(s, "width")).and_then(|w| num(w, 1)).unwrap_or(0.0);
    if w > 0.0 {
        w
    } else {
        0.1524
    }
}

/// The three points of an arc and the box the arc covers: the ends, plus the extreme points of its circle the arc passes.
fn arc_bbox(a: Pt, m: Pt, b: Pt) -> Rect {
    let mut r = Rect::point(a).merge_point(m).merge_point(b);
    let d = 2.0 * (a.0 * (m.1 - b.1) + m.0 * (b.1 - a.1) + b.0 * (a.1 - m.1));
    if d.abs() < 1e-9 {
        return r;
    }
    let (a2, m2, b2) = (a.0 * a.0 + a.1 * a.1, m.0 * m.0 + m.1 * m.1, b.0 * b.0 + b.1 * b.1);
    let cx = (a2 * (m.1 - b.1) + m2 * (b.1 - a.1) + b2 * (a.1 - m.1)) / d;
    let cy = (a2 * (b.0 - m.0) + m2 * (a.0 - b.0) + b2 * (m.0 - a.0)) / d;
    let rad = ((a.0 - cx).powi(2) + (a.1 - cy).powi(2)).sqrt();
    let ang = |p: Pt| (p.1 - cy).atan2(p.0 - cx);
    let tau = std::f64::consts::TAU;
    let norm = |x: f64| x.rem_euclid(tau);
    let (sa, ma, ea) = (norm(ang(a)), norm(ang(m)), norm(ang(b)));
    // Is angle `t` on the way from a to b through m?
    let ccw_span = norm(ea - sa);
    let ccw_mid = norm(ma - sa);
    let passes = |t: f64| {
        if ccw_mid <= ccw_span {
            norm(t - sa) <= ccw_span
        } else {
            norm(sa - t) <= norm(sa - ea)
        }
    };
    for (k, p) in [(0.0, (cx + rad, cy)), (std::f64::consts::FRAC_PI_2, (cx, cy + rad)), (std::f64::consts::PI, (cx - rad, cy)), (3.0 * std::f64::consts::FRAC_PI_2, (cx, cy - rad))] {
        if passes(k) {
            r = r.merge_point(p);
        }
    }
    r
}

fn parse_lib_symbol(def: &[Sexpr]) -> LibSym {
    let mut sym = LibSym { name_offset: 0.508, ..Default::default() };
    sym.power = find(def, "power").is_some() || def.iter().any(|i| i.text() == Some("power"));
    if let Some(pn) = find(def, "pin_names") {
        sym.names_hidden = has_hide(pn);
        if let Some(o) = find(pn, "offset").and_then(|o| num(o, 1)) {
            sym.name_offset = o;
        }
    }
    if let Some(pn) = find(def, "pin_numbers") {
        sym.numbers_hidden = has_hide(pn);
    }
    for sub in find_all(def, "symbol") {
        // `NAME_<unit>_<style>`
        let name = txt(sub, 1).unwrap_or("");
        let mut parts = name.rsplit('_');
        let style: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let unit: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        for g in sub.iter().filter_map(Sexpr::as_list) {
            let pen = stroke_pen_mm(g);
            let bbox = match tag(g) {
                Some("rectangle") => {
                    let (s, e) = (find(g, "start").and_then(point_of), find(g, "end").and_then(point_of));
                    s.zip(e).map(|(s, e)| Rect::new(s, e).inflate(pen / 2.0))
                }
                Some("polyline") | Some("bezier") => {
                    let pts = pts_of(g);
                    pts.iter().skip(1).fold(pts.first().map(|p| Rect::point(*p)), |acc, p| acc.map(|r| r.merge_point(*p))).map(|r| r.inflate(pen / 2.0))
                }
                Some("circle") => {
                    let c = find(g, "center").and_then(point_of);
                    let r = find(g, "radius").and_then(|r| num(r, 1));
                    c.zip(r).map(|(c, r)| Rect::new((c.0 - r, c.1 - r), (c.0 + r, c.1 + r)).inflate(pen / 2.0))
                }
                Some("arc") => {
                    let (s, m, e) = (find(g, "start").and_then(point_of), find(g, "mid").and_then(point_of), find(g, "end").and_then(point_of));
                    match (s, m, e) {
                        (Some(s), Some(m), Some(e)) => Some(arc_bbox(s, m, e).inflate(pen / 2.0)),
                        _ => None,
                    }
                }
                Some("text") => {
                    let at = find(g, "at");
                    let fx = effects_of(g, HJustify::Center, VJustify::Center);
                    at.and_then(point_of).map(|p| {
                        let (w, h) = fx.extents(txt(g, 1).unwrap_or(""));
                        let (w, h) = (w / 1000.0, h / 1000.0);
                        Rect::new((p.0 - w / 2.0, p.1 - h / 2.0), (p.0 + w / 2.0, p.1 + h / 2.0))
                    })
                }
                Some("pin") => {
                    let at = find(g, "at");
                    let (Some(p), Some(angle)) = (at.and_then(point_of), at.and_then(|a| num(a, 3))) else { continue };
                    let length = find(g, "length").and_then(|l| num(l, 1)).unwrap_or(0.0);
                    let name_list = find(g, "name");
                    let number_list = find(g, "number");
                    let name = name_list.and_then(|n| txt(n, 1)).unwrap_or("").to_string();
                    let number = number_list.and_then(|n| txt(n, 1)).unwrap_or("").to_string();
                    sym.pins.push(LibPin {
                        number,
                        name,
                        x: p.0,
                        y: p.1,
                        angle,
                        length,
                        hide: has_hide(g),
                        name_fx: name_list.map(|n| effects_of(n, HJustify::Center, VJustify::Center)).unwrap_or_else(|| effects_of(&[], HJustify::Center, VJustify::Center)),
                        number_fx: number_list.map(|n| effects_of(n, HJustify::Center, VJustify::Center)).unwrap_or_else(|| effects_of(&[], HJustify::Center, VJustify::Center)),
                        unit,
                        style,
                    });
                    None
                }
                _ => None,
            };
            if let Some(bbox) = bbox {
                sym.gfx.push(Gfx { unit, style, bbox });
            }
        }
    }
    sym
}

// ------------------------------------------------------------------------------------------------------- sheet

/// What the checker found on one sheet.
#[derive(Debug, Clone)]
pub struct Overlap {
    pub a: String,
    pub b: String,
    pub kinds: (Kind, Kind),
    /// How deep the two boxes run into one another, micrometres.
    pub depth_um: f64,
    pub at: Pt,
}

#[derive(Debug, Clone)]
pub struct Outside {
    pub what: String,
    pub rect: Rect,
    pub why: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct SheetReport {
    pub file: String,
    pub items: usize,
    pub overlaps: Vec<Overlap>,
    pub outside: Vec<Outside>,
}

impl SheetReport {
    pub fn count(&self) -> usize {
        self.overlaps.len() + self.outside.len()
    }

    pub fn render(&self) -> String {
        let mut s = format!("{}: {} items, {} overlap(s), {} outside the frame\n", self.file, self.items, self.overlaps.len(), self.outside.len());
        for o in &self.overlaps {
            s.push_str(&format!("  overlap {:.2} mm at ({:.2}, {:.2}): {} / {}\n", o.depth_um / 1000.0, o.at.0 / 1000.0, o.at.1 / 1000.0, o.a, o.b));
        }
        for o in &self.outside {
            s.push_str(&format!("  outside ({}): {} [{:.2},{:.2} - {:.2},{:.2}]\n", o.why, o.what, o.rect.x0 / 1000.0, o.rect.y0 / 1000.0, o.rect.x1 / 1000.0, o.rect.y1 / 1000.0));
        }
        s
    }
}

/// The paper sizes KiCad names, landscape, in micrometres.
fn paper_um(name: &str) -> Option<(f64, f64)> {
    Some(match name {
        "A5" => (210_000.0, 148_000.0),
        "A4" => (297_000.0, 210_000.0),
        "A3" => (420_000.0, 297_000.0),
        "A2" => (594_000.0, 420_000.0),
        "A1" => (841_000.0, 594_000.0),
        "A0" => (1_189_000.0, 841_000.0),
        "A" => (279_400.0, 215_900.0),
        "B" => (431_800.0, 279_400.0),
        _ => return None,
    })
}

/// KiCad's default drawing sheet: the frame is 10 mm in, a second line 2 mm inside it, and the title block sits against the inner frame's bottom-right corner.
const FRAME_INNER: f64 = 12_000.0;
const TITLE_W: f64 = 108_000.0;
const TITLE_H: f64 = 32_000.0;

struct Builder {
    libs: BTreeMap<String, LibSym>,
    items: Vec<Item>,
}

fn mm_pt(p: Pt) -> Pt {
    (p.0 * 1000.0, p.1 * 1000.0)
}

/// `(at x y angle)` of an item, as millimetres.
fn at_of(list: &[Sexpr]) -> Option<(Pt, f64)> {
    let at = find(list, "at")?;
    Some(((num(at, 1)?, num(at, 2)?), num(at, 3).unwrap_or(0.0)))
}

/// A label's spin: `SCH_LABEL_BASE::GetSpinStyle` from the file's angle (kept upright) and justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spin {
    Right,
    Up,
    Left,
    Bottom,
}

fn spin_of(angle: f64, h: HJustify) -> Spin {
    let vertical = upright_quarter_turns(angle) == 1;
    match (vertical, h == HJustify::Right) {
        (true, true) => Spin::Bottom,
        (true, false) => Spin::Up,
        (false, true) => Spin::Left,
        (false, false) => Spin::Right,
    }
}

impl Builder {
    fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    // -------------------------------------------------------------------------------------------- symbols

    fn symbol(&mut self, inst: &[Sexpr]) {
        let Some(lib_id) = find(inst, "lib_id").and_then(|l| txt(l, 1)) else { return };
        let Some(lib) = self.libs.get(lib_id).cloned() else { return };
        let Some((pos_mm, angle)) = at_of(inst) else { return };
        let pos = mm_pt(pos_mm);
        let xf = Xf::of_instance(angle, find(inst, "mirror").and_then(|m| txt(m, 1)));
        let unit: u32 = find(inst, "unit").and_then(|u| num(u, 1)).map(|u| u as u32).unwrap_or(1);
        let style: u32 = find(inst, "body_style").or_else(|| find(inst, "convert")).and_then(|u| num(u, 1)).map(|u| u as u32).unwrap_or(1);
        let mut reference = String::new();
        for p in find_all(inst, "property") {
            if txt(p, 1) == Some("Reference") {
                reference = txt(p, 2).unwrap_or("").to_string();
            }
        }
        let owner = if reference.is_empty() { lib_id.to_string() } else { reference.clone() };
        let to_page = |lib_pt: Pt| -> Pt {
            // the library frame is y up; the page's is y down
            let p = xf.apply(mm_pt((lib_pt.0, -lib_pt.1)));
            (pos.0 + p.0, pos.1 + p.1)
        };
        let on_unit = |u: u32, s: u32| (u == 0 || u == unit) && (s == 0 || s == style);

        // the body: the drawn graphics of the unit, no pins and no fields
        let mut body: Option<Rect> = None;
        for g in lib.gfx.iter().filter(|g| on_unit(g.unit, g.style)) {
            let (a, b) = (to_page((g.bbox.x0, g.bbox.y0)), to_page((g.bbox.x1, g.bbox.y1)));
            let r = Rect::new(a, b);
            body = Some(body.map_or(r, |acc| acc.merge(r)));
        }
        let pins: Vec<&LibPin> = lib.pins.iter().filter(|p| on_unit(p.unit, p.style)).collect();
        // wires, junctions and no-connect flags on a pin's end touch the symbol there: a body's box can reach past its own pin tips (the arrows
        // of an LED do)
        let anchors: Vec<Pt> = pins.iter().map(|p| to_page((p.x, p.y))).collect();
        if let Some(rect) = body {
            let mut it = Item::boxed(Kind::Body, &owner, format!("{owner} body"), rect);
            it.anchors = anchors;
            self.push(it);
        }

        // the pins
        for p in pins.iter().filter(|p| !p.hide) {
            self.pin(&owner, &lib, &xf, pos, p);
        }

        // the fields
        for prop in find_all(inst, "property") {
            let (Some(name), Some(value)) = (txt(prop, 1), txt(prop, 2)) else { continue };
            self.field(&owner, name, value, prop, &xf, pos);
        }
    }

    fn pin(&mut self, owner: &str, lib: &LibSym, xf: &Xf, pos: Pt, p: &LibPin) {
        let to_page = |lib_pt: Pt| -> Pt {
            let q = xf.apply(mm_pt((lib_pt.0, -lib_pt.1)));
            (pos.0 + q.0, pos.1 + q.1)
        };
        let tip = to_page((p.x, p.y));
        // the pin's own direction in the symbol's (y down) frame, then on the page: PinDrawOrient
        let dir_local = match (p.angle.round() as i64).rem_euclid(360) {
            0 => (1.0, 0.0),
            90 => (0.0, -1.0),
            180 => (-1.0, 0.0),
            _ => (0.0, 1.0),
        };
        let d = xf.apply(dir_local);
        let len = p.length * 1000.0;
        let end = (tip.0 + d.0 * len, tip.1 + d.1 * len);
        if len > 0.0 {
            let mut it = Item::boxed(Kind::Pin, owner, format!("{owner} pin {} line", p.number), Rect::new(tip, end).inflate(0.1));
            it.seg = Some((tip, end));
            it.anchors = vec![tip];
            self.push(it);
        }
        // the orientation names and numbers are laid out in: the pin's own, as drawn
        #[derive(PartialEq, Clone, Copy)]
        enum O {
            Right,
            Left,
            Up,
            Down,
        }
        let orient = if d.0.abs() < 0.5 {
            if d.1 > 0.0 {
                O::Down
            } else {
                O::Up
            }
        } else if d.0 < 0.0 {
            O::Left
        } else {
            O::Right
        };
        let vertical = matches!(orient, O::Up | O::Down);
        let shown_name = if p.name == "~" { "" } else { p.name.as_str() };
        let name_visible = !lib.names_hidden && !shown_name.is_empty();
        let name_inside = lib.name_offset > 0.0;
        // `getPinTextOffset()`: 24 mils scaled by the 0.15 ratio, rounded to whole mils; `PIN_TEXT_MARGIN`: 4 mils
        let mil = 254.0;
        let text_offset = 4.0 * mil;
        let clearance = text_offset + 4.0 * mil;
        // the pen the painter hands the layout cache: the 6 mil line width
        let thickness = 6.0 * mil;
        let half_len = len / 2.0;

        // the number
        if !lib.numbers_hidden && !p.number.is_empty() {
            let fx = &p.number_fx;
            let (w, h) = fx.extents(&p.number);
            let size = um(fx.size_iu);
            let both = name_visible && !name_inside;
            let (cx, cy) = if vertical {
                // the QA box the cache estimates the width with: 0.6 em a character
                let perp = um(clearance as i64) + size / 2.0 + um(thickness as i64);
                let cx = if both { tip.0 + perp } else { tip.0 - perp };
                (cx, if orient == O::Down { tip.1 + half_len } else { tip.1 - half_len })
            } else {
                let half_h = size / 2.0;
                let off = um(clearance as i64) + half_h + um(thickness as i64);
                let cy = if both { tip.1 + off } else { tip.1 - off };
                (if orient == O::Left { tip.0 - half_len } else { tip.0 + half_len }, cy)
            };
            let (bw, bh) = if vertical { (h, w) } else { (w, h) };
            let rect = Rect::new((cx - bw / 2.0, cy - bh / 2.0), (cx + bw / 2.0, cy + bh / 2.0));
            let mut it = Item::boxed(Kind::PinNumber, owner, format!("{owner} pin {} number '{}'", p.number, p.number), rect);
            it.pin_tip = Some(tip);
            self.push(it);
        }

        // the name
        if name_visible {
            let fx = &p.name_fx;
            let (w, h) = fx.extents(shown_name);
            let size = um(fx.size_iu);
            let rect = if name_inside {
                // inside the body, left-justified against the end of the pin (+ the offset) in the pin's own frame
                let inset = len + lib.name_offset * 1000.0;
                let (bw, bh) = if vertical { (h, w) } else { (w, h) };
                match orient {
                    O::Right => Rect::new((tip.0 + inset, tip.1 - bh / 2.0), (tip.0 + inset + bw, tip.1 + bh / 2.0)),
                    O::Left => Rect::new((tip.0 - inset - bw, tip.1 - bh / 2.0), (tip.0 - inset, tip.1 + bh / 2.0)),
                    O::Up => Rect::new((tip.0 - bw / 2.0, tip.1 - inset - bh), (tip.0 + bw / 2.0, tip.1 - inset)),
                    O::Down => Rect::new((tip.0 - bw / 2.0, tip.1 + inset), (tip.0 + bw / 2.0, tip.1 + inset + bh)),
                }
            } else {
                // outside: over the pin, or at its left when the pin is vertical
                let off = um(clearance as i64) + size / 2.0 + um(thickness as i64);
                if vertical {
                    let cx = tip.0 - off;
                    let cy = if orient == O::Down { tip.1 + half_len } else { tip.1 - half_len };
                    Rect::new((cx - h / 2.0, cy - w / 2.0), (cx + h / 2.0, cy + w / 2.0))
                } else {
                    let cx = if orient == O::Left { tip.0 - half_len } else { tip.0 + half_len };
                    let cy = tip.1 - off;
                    Rect::new((cx - w / 2.0, cy - h / 2.0), (cx + w / 2.0, cy + h / 2.0))
                }
            };
            let mut it = Item::boxed(Kind::PinName, owner, format!("{owner} pin {} name '{shown_name}'", p.number), rect);
            it.pin_tip = Some(tip);
            self.push(it);
        }
    }

    /// `SCH_FIELD::GetBoundingBox`: the text box turned about its anchor by the field's angle, then carried through the symbol's
    /// transform about the symbol's origin.
    fn field(&mut self, owner: &str, name: &str, value: &str, prop: &[Sexpr], xf: &Xf, pos: Pt) {
        let fx = effects_of(prop, HJustify::Left, VJustify::Center);
        if fx.hide || value.is_empty() {
            return;
        }
        let Some((at_mm, angle)) = at_of(prop) else { return };
        let at_page = mm_pt(at_mm);
        // the file stores the page position; the box is built in the symbol's own, untransformed frame
        let local = xf.unapply((at_page.0 - pos.0, at_page.1 - pos.1));
        let anchor = (pos.0 + local.0, pos.1 + local.1);
        let turns = upright_quarter_turns(angle) as i32;
        let r = fx.text_box(value, anchor, turns);
        let a = xf.apply((r.x0 - pos.0, r.y0 - pos.1));
        let b = xf.apply((r.x1 - pos.0, r.y1 - pos.1));
        let rect = Rect::new((pos.0 + a.0, pos.1 + a.1), (pos.0 + b.0, pos.1 + b.1));
        self.push(Item::boxed(Kind::Field, owner, format!("{owner} {name} '{value}'"), rect));
    }

    // -------------------------------------------------------------------------------------------- labels

    fn label(&mut self, l: &[Sexpr]) {
        let Some(text) = txt(l, 1) else { return };
        let Some((at_mm, angle)) = at_of(l) else { return };
        let at = mm_pt(at_mm);
        let kind = tag(l).unwrap_or("label");
        let hier = matches!(kind, "hierarchical_label" | "global_label");
        let fx = effects_of(l, HJustify::Left, if hier { VJustify::Center } else { VJustify::Bottom });
        let spin = spin_of(angle, fx.h);
        let rect = if hier { hier_label_rect(&fx, text, at, spin) } else { local_label_rect(&fx, text, at, upright_quarter_turns(angle) as i32) };
        let what = format!("{} '{text}'", if hier { "hierarchical label" } else { "label" });
        let mut it = Item::boxed(Kind::Label, text, what, rect);
        it.anchors = vec![at];
        self.push(it);
    }

    fn free_text(&mut self, t: &[Sexpr]) {
        let Some(text) = txt(t, 1) else { return };
        let Some((at_mm, angle)) = at_of(t) else { return };
        let fx = effects_of(t, HJustify::Left, VJustify::Bottom);
        let rect = fx.text_box(text, mm_pt(at_mm), upright_quarter_turns(angle) as i32);
        self.push(Item::boxed(Kind::Text, text, format!("text '{text}'"), rect));
    }

    // -------------------------------------------------------------------------------------------- sheets

    fn sheet(&mut self, s: &[Sexpr]) {
        let (Some(at), Some(size)) = (find(s, "at").and_then(point_of), find(s, "size").and_then(point_of)) else { return };
        let (p, sz) = (mm_pt(at), mm_pt(size));
        let name = find_all(s, "property").find(|p| txt(p, 1) == Some("Sheetname")).and_then(|p| txt(p, 2)).unwrap_or("sheet").to_string();
        // `SCH_SHEET::GetBodyBoundingBox`: the rectangle, half a 6 mil line out
        let body = Rect::new(p, (p.0 + sz.0, p.1 + sz.1)).inflate(3.0 * 25.4);
        self.push(Item::boxed(Kind::SheetBody, &name, format!("sheet '{name}' body"), body));
        for prop in find_all(s, "property") {
            let (Some(n), Some(v)) = (txt(prop, 1), txt(prop, 2)) else { continue };
            self.field(&name, n, v, prop, &Xf::IDENTITY, p);
        }
        let mut pin_anchors = Vec::new();
        for pin in find_all(s, "pin") {
            let Some(pname) = txt(pin, 1) else { continue };
            let Some((pat_mm, side_angle)) = at_of(pin) else { continue };
            let pp = mm_pt(pat_mm);
            let fx = effects_of(pin, HJustify::Left, VJustify::Center);
            // `SetSide` turns the pin's text horizontal for a pin on the left or right edge and vertical for one on the top or
            // bottom; the justification the file gives decides which way it reads (`GetSpinStyle`)
            let vertical = matches!((side_angle.round() as i64).rem_euclid(360), 90 | 270);
            let spin = spin_of(if vertical { 90.0 } else { 0.0 }, fx.h);
            let rect = hier_label_rect(&fx, pname, pp, spin);
            let mut it = Item::boxed(Kind::SheetPin, &name, format!("sheet '{name}' pin '{pname}'"), rect);
            it.anchors = vec![pp];
            pin_anchors.push(pp);
            self.push(it);
        }
        // a wire that meets the sheet at one of its pins touches the sheet's edge there
        if let Some(body_item) = self.items.iter_mut().rev().find(|i| i.kind == Kind::SheetBody && i.owner == name) {
            body_item.anchors = pin_anchors;
        }
    }
}

/// `SCH_LABEL::GetBodyBoundingBox`: the text box lifted by the text offset, grown by the pen, turned about the anchor, with the
/// anchor itself in it.
fn local_label_rect(fx: &Effects, text: &str, at: Pt, turns: i32) -> Rect {
    let (x0, y0, x1, y1) = text_box_iu(text, fx.size_iu, fx.pen(), fx.h, fx.v);
    let text_offset = um(kiround(0.15 * fx.size_iu as f64));
    let pen = um(fx.pen());
    let r = Rect::new((at.0 + um(x0), at.1 + um(y0) - text_offset), (at.0 + um(x1), at.1 + um(y1) - text_offset)).inflate(pen);
    let (a, b) = (rotate_about((r.x0, r.y0), at, turns), rotate_about((r.x1, r.y1), at, turns));
    Rect::new(a, b).merge_point(at)
}

/// `SCH_HIERLABEL::GetBodyBoundingBox`: the flag and the text, from just behind the anchor.
fn hier_label_rect(fx: &Effects, text: &str, at: Pt, spin: Spin) -> Rect {
    let pen = um(fx.pen());
    let margin = um(kiround(0.15 * fx.size_iu as f64));
    let (x0, _, x1, _) = text_box_iu(text, fx.size_iu, fx.pen(), fx.h, fx.v);
    let height = um(fx.size_iu) + pen + margin;
    let length = um(x1 - x0) + height;
    let dangling = 12.0 * 25.4;
    let (x, y) = at;
    match spin {
        Spin::Left => Rect::new((x + dangling, y - height / 2.0), (x + dangling - length, y + height / 2.0)),
        Spin::Right => Rect::new((x - dangling, y - height / 2.0), (x - dangling + length, y + height / 2.0)),
        Spin::Up => Rect::new((x - height / 2.0, y + dangling), (x + height / 2.0, y + dangling - length)),
        Spin::Bottom => Rect::new((x - height / 2.0, y - dangling), (x + height / 2.0, y - dangling + length)),
    }
}

// ------------------------------------------------------------------------------------------------------- reading

/// Every item eeschema draws on one sheet's text, and the paper it is on.
pub fn items_of_sheet(text: &str) -> Result<(Vec<Item>, Option<(f64, f64)>), String> {
    let root = sexpr::parse(text)?;
    let root = root.as_list().ok_or("not a list")?;
    if tag(root) != Some("kicad_sch") {
        return Err("not a kicad_sch".to_string());
    }
    let mut b = Builder { libs: BTreeMap::new(), items: Vec::new() };
    if let Some(libs) = find(root, "lib_symbols") {
        for def in find_all(libs, "symbol") {
            if let Some(name) = txt(def, 1) {
                b.libs.insert(name.to_string(), parse_lib_symbol(def));
            }
        }
    }
    for it in root.iter().filter_map(Sexpr::as_list) {
        match tag(it) {
            Some("symbol") => b.symbol(it),
            Some("wire") | Some("bus") => {
                let pts = pts_of(it);
                for pair in pts.windows(2) {
                    let (a, c) = (mm_pt(pair[0]), mm_pt(pair[1]));
                    let mut item = Item::boxed(Kind::Wire, "", format!("wire ({:.2},{:.2})-({:.2},{:.2})", a.0 / 1000.0, a.1 / 1000.0, c.0 / 1000.0, c.1 / 1000.0), Rect::new(a, c).inflate(0.1));
                    item.seg = Some((a, c));
                    item.anchors = vec![a, c];
                    b.push(item);
                }
            }
            Some("junction") => {
                if let Some(p) = find(it, "at").and_then(point_of) {
                    let p = mm_pt(p);
                    let mut item = Item::boxed(Kind::Junction, "", "junction".to_string(), Rect::new(p, p).inflate(457.2));
                    item.anchors = vec![p];
                    b.push(item);
                }
            }
            Some("no_connect") => {
                if let Some(p) = find(it, "at").and_then(point_of) {
                    let p = mm_pt(p);
                    let mut item = Item::boxed(Kind::NoConnect, "", "no-connect flag".to_string(), Rect::new(p, p).inflate(609.6));
                    item.anchors = vec![p];
                    b.push(item);
                }
            }
            Some("label") | Some("global_label") | Some("hierarchical_label") => b.label(it),
            Some("text") => b.free_text(it),
            Some("sheet") => b.sheet(it),
            _ => {}
        }
    }
    let paper = find(root, "paper").and_then(|p| txt(p, 1)).and_then(paper_um);
    Ok((b.items, paper))
}

// ------------------------------------------------------------------------------------------------------- checking

fn same_point(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0
}

fn share_anchor(a: &Item, b: &Item) -> bool {
    a.anchors.iter().any(|p| b.anchors.iter().any(|q| same_point(*p, *q)))
}

/// Pairs that are meant to touch.
fn exempt(a: &Item, b: &Item) -> bool {
    if a.kind == Kind::Wire && b.kind == Kind::Wire {
        return true;
    }
    if !a.owner.is_empty() && a.owner == b.owner {
        let pin_like = |k: Kind| matches!(k, Kind::Pin | Kind::PinName | Kind::PinNumber);
        if (a.kind == Kind::Body && pin_like(b.kind)) || (b.kind == Kind::Body && pin_like(a.kind)) {
            return true;
        }
        if (a.kind == Kind::SheetBody && b.kind == Kind::SheetPin) || (a.kind == Kind::SheetPin && b.kind == Kind::SheetBody) {
            return true;
        }
    }
    // two pins of one symbol never meet: they are two pins at one spot
    if a.kind == Kind::Pin && b.kind == Kind::Pin && a.owner == b.owner {
        return false;
    }
    if share_anchor(a, b) && (a.kind.is_connector() || b.kind.is_connector()) {
        return true;
    }
    // a no-connect flag sits on its pin's end, over that pin's own texts
    let nc_on_texts = |nc: &Item, t: &Item| nc.kind == Kind::NoConnect && matches!(t.kind, Kind::PinName | Kind::PinNumber) && t.pin_tip.is_some_and(|tip| nc.anchors.iter().any(|p| same_point(*p, tip)));
    nc_on_texts(a, b) || nc_on_texts(b, a)
}

/// How far the segment runs through the inside of the box (shrunk by the tolerance), if it does.
fn seg_through(seg: (Pt, Pt), r: &Rect) -> Option<f64> {
    let r = Rect { x0: r.x0 + TOL, y0: r.y0 + TOL, x1: r.x1 - TOL, y1: r.y1 - TOL };
    if r.x0 >= r.x1 || r.y0 >= r.y1 {
        return None;
    }
    // Liang-Barsky
    let (p, q) = seg;
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [(-dx, p.0 - r.x0), (dx, r.x1 - p.0), (-dy, p.1 - r.y0), (dy, r.y1 - p.1)] {
        if pk.abs() < 1e-12 {
            if qk < 0.0 {
                return None;
            }
        } else {
            let t = qk / pk;
            if pk < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    let len = ((dx * dx + dy * dy).sqrt()) * (t1 - t0);
    (len > TOL).then_some(len)
}

/// Two lines (pins, or a pin and a wire) that run along one another or cross, other than meeting end to end.
fn segs_conflict(a: (Pt, Pt), b: (Pt, Pt)) -> Option<f64> {
    let (p, r) = (a.0, (a.1 .0 - a.0 .0, a.1 .1 - a.0 .1));
    let (q, s) = (b.0, (b.1 .0 - b.0 .0, b.1 .1 - b.0 .1));
    let cross = |u: Pt, v: Pt| u.0 * v.1 - u.1 * v.0;
    let rxs = cross(r, s);
    let qp = (q.0 - p.0, q.1 - p.1);
    if rxs.abs() < 1e-9 {
        // parallel: collinear overlap?
        if cross(qp, r).abs() > TOL * (r.0.hypot(r.1)).max(1.0) {
            return None;
        }
        let rr = r.0 * r.0 + r.1 * r.1;
        if rr < 1e-9 {
            return None;
        }
        let t0 = (qp.0 * r.0 + qp.1 * r.1) / rr;
        let t1 = t0 + (s.0 * r.0 + s.1 * r.1) / rr;
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
        let len = (hi - lo) * rr.sqrt();
        return (len > TOL).then_some(len);
    }
    let t = cross(qp, s) / rxs;
    let u = cross(qp, r) / rxs;
    let end_tol_t = TOL / r.0.hypot(r.1).max(1.0);
    let end_tol_u = TOL / s.0.hypot(s.1).max(1.0);
    if t < -end_tol_t || t > 1.0 + end_tol_t || u < -end_tol_u || u > 1.0 + end_tol_u {
        return None;
    }
    // meeting at an end of both is a joint, not a conflict; at the end of one and the inside of the other is a T onto a line
    let at_end_t = t < end_tol_t || t > 1.0 - end_tol_t;
    let at_end_u = u < end_tol_u || u > 1.0 - end_tol_u;
    if at_end_t && at_end_u {
        return None;
    }
    Some(TOL * 2.0)
}

fn conflict(a: &Item, b: &Item) -> Option<f64> {
    match (a.seg, b.seg) {
        (None, None) => a.rect.shared(&b.rect).map(|(w, h)| w.min(h)),
        (Some(s), None) => seg_through(s, &b.rect),
        (None, Some(s)) => seg_through(s, &a.rect),
        (Some(s), Some(t)) => segs_conflict(s, t),
    }
}

/// Check the items of one sheet: every overlapping pair, and everything outside the frame or on the title block.
pub fn check_items(file: &str, items: &[Item], paper: Option<(f64, f64)>) -> SheetReport {
    let mut report = SheetReport { file: file.to_string(), items: items.len(), ..Default::default() };
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            let (a, b) = (&items[i], &items[j]);
            if a.rect.shared(&b.rect).is_none() && a.seg.is_none() && b.seg.is_none() {
                continue;
            }
            // a cheap broad phase for the lines
            let (ra, rb) = (a.rect.inflate(TOL), b.rect.inflate(TOL));
            if ra.x1 < rb.x0 || rb.x1 < ra.x0 || ra.y1 < rb.y0 || rb.y1 < ra.y0 {
                continue;
            }
            if exempt(a, b) {
                continue;
            }
            if let Some(depth) = conflict(a, b) {
                let shared_x = (a.rect.x0.max(b.rect.x0) + a.rect.x1.min(b.rect.x1)) / 2.0;
                let shared_y = (a.rect.y0.max(b.rect.y0) + a.rect.y1.min(b.rect.y1)) / 2.0;
                report.overlaps.push(Overlap { a: a.what.clone(), b: b.what.clone(), kinds: (a.kind, b.kind), depth_um: depth, at: (shared_x, shared_y) });
            }
        }
    }
    if let Some((w, h)) = paper {
        let frame = Rect { x0: FRAME_INNER, y0: FRAME_INNER, x1: w - FRAME_INNER, y1: h - FRAME_INNER };
        let title = Rect { x0: frame.x1 - TITLE_W, y0: frame.y1 - TITLE_H, x1: frame.x1, y1: frame.y1 };
        for it in items {
            let r = it.rect;
            if r.x0 < frame.x0 - TOL || r.y0 < frame.y0 - TOL || r.x1 > frame.x1 + TOL || r.y1 > frame.y1 + TOL {
                report.outside.push(Outside { what: it.what.clone(), rect: r, why: "outside the frame" });
            } else if r.shared(&title).is_some() {
                report.outside.push(Outside { what: it.what.clone(), rect: r, why: "on the title block" });
            }
        }
    }
    report
}

/// Check one `.kicad_sch` text.
pub fn check_sheet(file: &str, text: &str) -> Result<SheetReport, String> {
    let (items, paper) = items_of_sheet(text).map_err(|e| format!("{file}: {e}"))?;
    Ok(check_items(file, &items, paper))
}

/// Check every file of an exported tree (`export_kicad_sch_tree`): `(file name, text)` pairs.
pub fn check_tree(files: &[(String, String)]) -> Result<Vec<SheetReport>, String> {
    files.iter().map(|(f, t)| check_sheet(f, t)).collect()
}

#[cfg(test)]
mod tests;

