//! The boxes eeschema gives what it draws, as plain functions: a text's box, a pin's name and number, a label with its flag.
//!
//! One place for the rules, ported from the KiCad source at 8303b2ad (10.99), so everything that has to agree with KiCad about where
//! text goes asks here: the overlap checker (`eda_kicad::sch_overlap`, reading the files kicad-cli draws), the layout of the
//! derived sheets (so a pin's texts, a field and a label are measured as KiCad measures them), and the studio painter (which ports
//! the same numbers).
//!
//! * text: `EDA_TEXT::GetTextBox` ([`TextStyle::text_box`]);
//! * pins: `PIN_LAYOUT_CACHE::GetPinNumberInfo` / `GetPinNameInfo`, where the text is drawn, and `getUntransformedPin*Box`;
//! * labels: `SCH_LABEL::GetBodyBoundingBox`, `SCH_HIERLABEL::GetBodyBoundingBox`, and the spin style `SCH_IO_KICAD_SEXPR_PARSER`
//!   reads off a label's angle and justification;
//! * the symbol transform: `SCH_SYMBOL::SetOrientation`.
//!
//! Units: micrometres as `f64`, y down (the sheet's own frame).

use crate::kicad_font::{default_pen_iu, kiround, string_boundary_limits, text_box_iu, HJustify, VJustify, DEFAULT_TEXT_SIZE_IU};

/// Schematic internal units (0.1 um) to micrometres.
pub fn um(iu: i64) -> f64 {
    iu as f64 / 10.0
}

/// Mils to micrometres (a mil is 25.4 um).
pub const MIL: f64 = 25.4;

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
    pub fn center(&self) -> Pt {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }
    /// The size of the part the two boxes share beyond `tol`, when it has an area.
    pub fn shared(&self, o: &Rect, tol: f64) -> Option<(f64, f64)> {
        let w = self.x1.min(o.x1) - self.x0.max(o.x0);
        let h = self.y1.min(o.y1) - self.y0.max(o.y0);
        (w > tol && h > tol).then_some((w, h))
    }
}

/// `RotatePoint` with a positive angle: counter-clockwise on the y-down sheet, so a quarter turn takes (x, y) to (y, -x).
pub fn rotate_about(p: Pt, c: Pt, quarter_turns: i32) -> Pt {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    let (rx, ry) = match quarter_turns.rem_euclid(4) {
        0 => (dx, dy),
        1 => (dy, -dx),
        2 => (-dx, -dy),
        _ => (-dy, dx),
    };
    (c.0 + rx, c.1 + ry)
}

/// `TRANSFORM`: where a symbol's own frame (y down, origin at the symbol) goes on the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xf {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Xf {
    pub const IDENTITY: Xf = Xf { x1: 1, y1: 0, x2: 0, y2: 1 };

    pub fn apply(&self, p: Pt) -> Pt {
        (self.x1 as f64 * p.0 + self.y1 as f64 * p.1, self.x2 as f64 * p.0 + self.y2 as f64 * p.1)
    }

    /// The inverse of a rotation or mirror is its transpose.
    pub fn unapply(&self, p: Pt) -> Pt {
        (self.x1 as f64 * p.0 + self.x2 as f64 * p.1, self.y1 as f64 * p.0 + self.y2 as f64 * p.1)
    }

    /// `SetOrientation`'s incremental step: the new matrix is the old one times the requested change.
    pub fn then(self, t: Xf) -> Xf {
        Xf {
            x1: self.x1 * t.x1 + self.x2 * t.y1,
            y1: self.y1 * t.x1 + self.y2 * t.y1,
            x2: self.x1 * t.x2 + self.x2 * t.y2,
            y2: self.y1 * t.x2 + self.y2 * t.y2,
        }
    }

    /// True for a quarter turn (`transform.y1` is not zero): a field's text goes from horizontal to vertical.
    pub fn swaps_axes(&self) -> bool {
        self.y1 != 0
    }

    /// The transform of an instance written `(at x y angle)` and `(mirror x|y)`.
    pub fn of_instance(angle: f64, mirror: Option<&str>) -> Xf {
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

// ------------------------------------------------------------------------------------------------------- text

/// How one piece of text is set: its size, the pen, and how it is justified against its anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub size_iu: i64,
    pub thickness_iu: i64,
    pub h: HJustify,
    pub v: VJustify,
}

impl TextStyle {
    pub fn new(h: HJustify, v: VJustify) -> TextStyle {
        TextStyle { size_iu: DEFAULT_TEXT_SIZE_IU, thickness_iu: 0, h, v }
    }

    /// `GetEffectiveTextPenWidth`.
    pub fn pen(&self) -> i64 {
        if self.thickness_iu > 1 {
            self.thickness_iu.min(kiround(self.size_iu as f64 * 0.25))
        } else {
            default_pen_iu(self.size_iu)
        }
    }

    /// `EDA_TEXT::GetTextBox` at `anchor`, then turned about the anchor by `quarter_turns` (`GetTextAngle`).
    pub fn text_box(&self, text: &str, anchor: Pt, quarter_turns: i32) -> Rect {
        let (x0, y0, x1, y1) = text_box_iu(text, self.size_iu, self.pen(), self.h, self.v);
        let a = (anchor.0 + um(x0), anchor.1 + um(y0));
        let b = (anchor.0 + um(x1), anchor.1 + um(y1));
        Rect::new(rotate_about(a, anchor, quarter_turns), rotate_about(b, anchor, quarter_turns))
    }

    /// The size of the box the pin layout measures a text with (`StringBoundaryLimits`, without the 17 % fudge): `(width, height)`.
    pub fn extents(&self, text: &str) -> (f64, f64) {
        let (w, h) = string_boundary_limits(text, self.size_iu, self.pen());
        (um(w), um(h))
    }

    pub fn size_um(&self) -> f64 {
        um(self.size_iu)
    }
}

/// A label's spin: `SCH_LABEL_BASE::GetSpinStyle`, which the parser reads off the file's angle (kept upright) and justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spin {
    Right,
    Up,
    Left,
    Bottom,
}

/// `KeepUpright`: texts are only ever horizontal or vertical, and never upside down: 0 or 1 quarter turns.
pub fn upright_quarter_turns(angle_deg: f64) -> i32 {
    ((angle_deg / 90.0).round() as i32).rem_euclid(4) % 2
}

pub fn spin_of(angle_deg: f64, h: HJustify) -> Spin {
    let vertical = upright_quarter_turns(angle_deg) == 1;
    match (vertical, h == HJustify::Right) {
        (true, true) => Spin::Bottom,
        (true, false) => Spin::Up,
        (false, true) => Spin::Left,
        (false, false) => Spin::Right,
    }
}

impl Spin {
    /// The unit vector the label's text runs along, from its anchor: the way the text reads away from the wire.
    pub fn dir(self) -> (f64, f64) {
        match self {
            Spin::Right => (1.0, 0.0),
            Spin::Left => (-1.0, 0.0),
            Spin::Up => (0.0, -1.0),
            Spin::Bottom => (0.0, 1.0),
        }
    }
}

/// `SCH_LABEL::GetBodyBoundingBox`: the text box lifted by the text offset, grown by the pen, turned about the anchor, with the
/// anchor itself in it.
pub fn local_label_rect(fx: &TextStyle, text: &str, at: Pt, turns: i32) -> Rect {
    let (x0, y0, x1, y1) = text_box_iu(text, fx.size_iu, fx.pen(), fx.h, fx.v);
    let text_offset = um(kiround(0.15 * fx.size_iu as f64));
    let pen = um(fx.pen());
    let r = Rect::new((at.0 + um(x0), at.1 + um(y0) - text_offset), (at.0 + um(x1), at.1 + um(y1) - text_offset)).inflate(pen);
    Rect::new(rotate_about((r.x0, r.y0), at, turns), rotate_about((r.x1, r.y1), at, turns)).merge_point(at)
}

/// `SCH_HIERLABEL::GetBodyBoundingBox`: the flag and the text, from just behind the anchor.
pub fn hier_label_rect(fx: &TextStyle, text: &str, at: Pt, spin: Spin) -> Rect {
    let pen = um(fx.pen());
    let margin = um(kiround(0.15 * fx.size_iu as f64));
    let (x0, _, x1, _) = text_box_iu(text, fx.size_iu, fx.pen(), fx.h, fx.v);
    let height = um(fx.size_iu) + pen + margin;
    let length = um(x1 - x0) + height;
    let dangling = 12.0 * MIL;
    let (x, y) = at;
    match spin {
        Spin::Left => Rect::new((x + dangling, y - height / 2.0), (x + dangling - length, y + height / 2.0)),
        Spin::Right => Rect::new((x - dangling, y - height / 2.0), (x - dangling + length, y + height / 2.0)),
        Spin::Up => Rect::new((x - height / 2.0, y + dangling), (x + height / 2.0, y + dangling - length)),
        Spin::Bottom => Rect::new((x - height / 2.0, y - dangling), (x + height / 2.0, y - dangling + length)),
    }
}

/// The three points of an arc and the box the arc covers: the ends, plus the extreme points of its circle the arc passes.
pub fn arc_bbox(a: Pt, m: Pt, b: Pt) -> Rect {
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

// -------------------------------------------------------------------------------------------------------- pins

/// The way a pin's line runs from its connection point toward the symbol's body, as drawn (`PinDrawOrient`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinOrient {
    Right,
    Left,
    Up,
    Down,
}

impl PinOrient {
    pub fn vertical(self) -> bool {
        matches!(self, PinOrient::Up | PinOrient::Down)
    }

    /// The pin's direction on the sheet (a unit vector, y down).
    pub fn dir(self) -> (f64, f64) {
        match self {
            PinOrient::Right => (1.0, 0.0),
            PinOrient::Left => (-1.0, 0.0),
            PinOrient::Up => (0.0, -1.0),
            PinOrient::Down => (0.0, 1.0),
        }
    }

    pub fn of_dir(d: (f64, f64)) -> PinOrient {
        if d.0.abs() < 0.5 {
            if d.1 > 0.0 {
                PinOrient::Down
            } else {
                PinOrient::Up
            }
        } else if d.0 < 0.0 {
            PinOrient::Left
        } else {
            PinOrient::Right
        }
    }
}

/// The clearance eeschema keeps between a pin's line and its texts: `getPinTextOffset` is 24 mils scaled by the 0.15 text offset
/// ratio and rounded to whole mils (4), and `PIN_TEXT_MARGIN` adds another 4.
const PIN_TEXT_CLEARANCE: f64 = 8.0 * MIL;
/// The pen the painter hands the pin layout (the 6 mil default line width): it sets where the texts' centres sit.
const PIN_TEXT_THICKNESS: f64 = 6.0 * MIL;

/// How the pins of one symbol show their texts (`LIB_SYMBOL::m_showPinNames`, `m_showPinNumbers`, `m_pinNameOffset`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinTexts {
    pub names_hidden: bool,
    pub numbers_hidden: bool,
    /// Millimetres; names are drawn inside the body when it is above zero.
    pub name_offset_mm: f64,
}

impl Default for PinTexts {
    fn default() -> PinTexts {
        PinTexts { names_hidden: false, numbers_hidden: false, name_offset_mm: 0.508 }
    }
}

impl PinTexts {
    pub fn name_inside(&self) -> bool {
        self.name_offset_mm > 0.0
    }
}

/// The name KiCad shows for a pin: `~` stands for none.
pub fn shown_name(name: &str) -> &str {
    if name == "~" {
        ""
    } else {
        name
    }
}

/// Where a pin's number is drawn: over the pin, or under it when the name is over it; to the left or right of a vertical one
/// (`GetPinNumberInfo`). `tip` is the pin's connection point.
pub fn pin_number_rect(tip: Pt, orient: PinOrient, length: f64, number: &str, style: &TextStyle, both: bool) -> Rect {
    let (w, h) = style.extents(number);
    let size = style.size_um();
    let half_len = length / 2.0;
    let (cx, cy) = if orient.vertical() {
        let perp = PIN_TEXT_CLEARANCE + size / 2.0 + PIN_TEXT_THICKNESS;
        (if both { tip.0 + perp } else { tip.0 - perp }, if orient == PinOrient::Down { tip.1 + half_len } else { tip.1 - half_len })
    } else {
        let off = PIN_TEXT_CLEARANCE + size / 2.0 + PIN_TEXT_THICKNESS;
        (if orient == PinOrient::Left { tip.0 - half_len } else { tip.0 + half_len }, if both { tip.1 + off } else { tip.1 - off })
    };
    let (bw, bh) = if orient.vertical() { (h, w) } else { (w, h) };
    Rect::new((cx - bw / 2.0, cy - bh / 2.0), (cx + bw / 2.0, cy + bh / 2.0))
}

/// Where a pin's name is drawn: inside the body, from the pin's inner end plus the offset, or over the pin (`GetPinNameInfo`).
pub fn pin_name_rect(tip: Pt, orient: PinOrient, length: f64, name: &str, style: &TextStyle, texts: &PinTexts) -> Rect {
    let (w, h) = style.extents(name);
    let size = style.size_um();
    let (bw, bh) = if orient.vertical() { (h, w) } else { (w, h) };
    if texts.name_inside() {
        let inset = length + texts.name_offset_mm * 1000.0;
        match orient {
            PinOrient::Right => Rect::new((tip.0 + inset, tip.1 - bh / 2.0), (tip.0 + inset + bw, tip.1 + bh / 2.0)),
            PinOrient::Left => Rect::new((tip.0 - inset - bw, tip.1 - bh / 2.0), (tip.0 - inset, tip.1 + bh / 2.0)),
            PinOrient::Up => Rect::new((tip.0 - bw / 2.0, tip.1 - inset - bh), (tip.0 + bw / 2.0, tip.1 - inset)),
            PinOrient::Down => Rect::new((tip.0 - bw / 2.0, tip.1 + inset), (tip.0 + bw / 2.0, tip.1 + inset + bh)),
        }
    } else {
        let off = PIN_TEXT_CLEARANCE + size / 2.0 + PIN_TEXT_THICKNESS;
        if orient.vertical() {
            let cx = tip.0 - off;
            let cy = if orient == PinOrient::Down { tip.1 + length / 2.0 } else { tip.1 - length / 2.0 };
            Rect::new((cx - h / 2.0, cy - w / 2.0), (cx + h / 2.0, cy + w / 2.0))
        } else {
            let cx = if orient == PinOrient::Left { tip.0 - length / 2.0 } else { tip.0 + length / 2.0 };
            let cy = tip.1 - off;
            Rect::new((cx - w / 2.0, cy - h / 2.0), (cx + w / 2.0, cy + h / 2.0))
        }
    }
}

/// `PIN_LAYOUT_CACHE::GetPinBoundingBox` for a pin with its name and number: the line (a pin has no pen of its own), the name and the
/// number each in the box the layout cache keeps them in (`getUntransformedPinNameBox`, `getUntransformedPinNumberBox`), turned to the
/// pin's direction about its connection point. What Autoplace Fields measures the pins on a side of a symbol by.
#[allow(clippy::too_many_arguments)]
pub fn pin_bbox(tip: Pt, orient: PinOrient, length: f64, name: &str, number: &str, name_st: &TextStyle, number_st: &TextStyle, texts: &PinTexts) -> Rect {
    // the pin laid out running right from the origin, as the cache does it
    let offset = 4.0 * MIL;
    let mut r = Rect::new((0.0, 0.0), (length, 0.0)).inflate(0.1);
    let show_name = !texts.names_hidden && !name.is_empty();
    let show_number = !texts.numbers_hidden && !number.is_empty();
    if show_name {
        let (w, h) = name_st.extents(name);
        let b = if texts.name_inside() {
            let x0 = length + texts.name_offset_mm * 1000.0;
            Rect::new((x0, -h / 2.0), (x0 + w, h / 2.0))
        } else {
            // centred over the pin, lifted by half its height and the text offset
            Rect::new((length / 2.0 - w / 2.0, -h - offset), (length / 2.0 + w / 2.0, -offset))
        };
        r = r.merge(b);
    }
    if show_number {
        let (w, h) = number_st.extents(number);
        let both = show_name && !texts.name_inside();
        let dy = if both { h / 2.0 + offset } else { -h / 2.0 - offset };
        r = r.merge(Rect::new((length / 2.0 - w / 2.0, -h / 2.0 + dy), (length / 2.0 + w / 2.0, h / 2.0 + dy)));
    }
    let turn = |p: Pt| -> Pt {
        match orient {
            PinOrient::Right => p,
            PinOrient::Left => (-p.0, p.1),
            PinOrient::Up => (p.1, -p.0),
            PinOrient::Down => (p.1, p.0),
        }
    };
    let (a, b) = (turn((r.x0, r.y0)), turn((r.x1, r.y1)));
    Rect::new((tip.0 + a.0, tip.1 + a.1), (tip.0 + b.0, tip.1 + b.1)).inflate(0.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_box_is_what_eeschema_builds_from_the_text_box() {
        // 1.27 mm text, a label's text box offset up by 0.1905 mm and grown by the 0.1588 mm pen, the anchor in it
        let fx = TextStyle::new(HJustify::Left, VJustify::Bottom);
        let r = local_label_rect(&fx, "A", (0.0, 0.0), 0);
        assert!((r.y0 - -2095.7).abs() < 0.2 && (r.y1 - 265.2).abs() < 0.2, "{r:?}");
        assert!((r.x0 - -158.8).abs() < 0.2, "{r:?}");
        // turned a quarter, the text reads upward
        let up = local_label_rect(&fx, "A", (0.0, 0.0), 1);
        assert!(up.y0 < -500.0 && up.x0 < -2000.0, "{up:?}");
    }

    #[test]
    fn a_hierarchical_label_flag_starts_just_behind_its_anchor() {
        let fx = TextStyle::new(HJustify::Left, VJustify::Center);
        let r = hier_label_rect(&fx, "PA0", (10_000.0, 10_000.0), Spin::Right);
        assert!((r.x0 - (10_000.0 - 304.8)).abs() < 0.2, "{r:?}");
        assert!((r.h() - 1619.3).abs() < 0.2, "the flag is the text height + pen + offset tall: {}", r.h());
        let left = hier_label_rect(&TextStyle::new(HJustify::Right, VJustify::Center), "PA0", (10_000.0, 10_000.0), Spin::Left);
        assert!((left.x1 - (10_000.0 + 304.8)).abs() < 0.2 && left.x0 < 10_000.0 - 3_000.0, "{left:?}");
    }

    #[test]
    fn a_pins_number_sits_over_its_line_and_under_it_when_the_name_is_over_it() {
        let style = TextStyle::new(HJustify::Center, VJustify::Center);
        // a pin on the left of a body: its connection point at the origin, the line running right for 2.54 mm
        let over = pin_number_rect((0.0, 0.0), PinOrient::Right, 2540.0, "7", &style, false);
        let under = pin_number_rect((0.0, 0.0), PinOrient::Right, 2540.0, "7", &style, true);
        assert!(over.y1 < 0.0 && under.y0 > 0.0, "{over:?} {under:?}");
        // centred along the pin
        assert!((over.center().0 - 1270.0).abs() < 0.2);
        // 0.2032 + 0.635 + 0.1524 mm from the line to the centre
        assert!((over.center().1 + 990.6).abs() < 0.2, "{:?}", over.center());
    }

    #[test]
    fn a_pins_name_inside_starts_the_offset_past_the_pins_inner_end() {
        let style = TextStyle::new(HJustify::Center, VJustify::Center);
        let texts = PinTexts { names_hidden: false, numbers_hidden: false, name_offset_mm: 1.016 };
        let r = pin_name_rect((0.0, 0.0), PinOrient::Right, 2540.0, "PA0", &style, &texts);
        assert!((r.x0 - (2540.0 + 1016.0)).abs() < 0.2, "{r:?}");
        // a pin on the right of a body mirrors it
        let l = pin_name_rect((0.0, 0.0), PinOrient::Left, 2540.0, "PA0", &style, &texts);
        assert!((l.x1 - -(2540.0 + 1016.0)).abs() < 0.2, "{l:?}");
        // a pin on the bottom edge runs its name up into the body, vertically
        let u = pin_name_rect((0.0, 0.0), PinOrient::Up, 2540.0, "PA0", &style, &texts);
        assert!((u.y1 - -(2540.0 + 1016.0)).abs() < 0.2 && u.h() > u.w(), "{u:?}");
    }

    #[test]
    fn pin_texts_sit_where_the_studio_painter_has_them() {
        // the same cases, with the same numbers, as web/studio/src/components/schematic/pinText.test.ts
        let style = TextStyle::new(HJustify::Center, VJustify::Center);
        let off = 990.6; // 0.2032 + 0.635 + 0.1524 mm
        let near = |a: (f64, f64), x: f64, y: f64| assert!((a.0 - x).abs() < 0.2 && (a.1 - y).abs() < 0.2, "{a:?} is not ({x}, {y})");
        let inside = PinTexts { names_hidden: false, numbers_hidden: false, name_offset_mm: 0.508 };
        let outside = PinTexts { name_offset_mm: 0.0, ..inside };

        // a pin running right: the name starts the offset past the inner end, the number is over the line
        let name = pin_name_rect((0.0, 0.0), PinOrient::Right, 2540.0, "VIN", &style, &inside);
        assert!((name.x0 - (2540.0 + 508.0)).abs() < 0.2, "{name:?}");
        near(pin_number_rect((0.0, 0.0), PinOrient::Right, 2540.0, "3", &style, false).center(), 1270.0, -off);
        // running left, mirrored
        let name = pin_name_rect((0.0, 0.0), PinOrient::Left, 2540.0, "OUT", &style, &inside);
        assert!((name.x1 - -(2540.0 + 508.0)).abs() < 0.2, "{name:?}");
        near(pin_number_rect((0.0, 0.0), PinOrient::Left, 2540.0, "2", &style, false).center(), -1270.0, -off);
        // running up: the name runs up from the inner end, the number is left of the line
        let name = pin_name_rect((0.0, 0.0), PinOrient::Up, 2540.0, "VDD", &style, &inside);
        assert!((name.y1 - -(2540.0 + 508.0)).abs() < 0.2, "{name:?}");
        near(pin_number_rect((0.0, 0.0), PinOrient::Up, 2540.0, "1", &style, false).center(), -off, -1270.0);
        // running down
        let name = pin_name_rect((0.0, 0.0), PinOrient::Down, 2540.0, "GND", &style, &inside);
        assert!((name.y0 - (2540.0 + 508.0)).abs() < 0.2, "{name:?}");
        near(pin_number_rect((0.0, 0.0), PinOrient::Down, 2540.0, "2", &style, false).center(), -off, 1270.0);

        // names at offset zero are over the pin and the numbers under it
        near(pin_name_rect((0.0, 0.0), PinOrient::Right, 2540.0, "IN", &style, &outside).center(), 1270.0, -off);
        near(pin_number_rect((0.0, 0.0), PinOrient::Right, 2540.0, "1", &style, true).center(), 1270.0, off);
        near(pin_name_rect((0.0, 0.0), PinOrient::Up, 2540.0, "IN", &style, &outside).center(), -off, -1270.0);
        near(pin_number_rect((0.0, 0.0), PinOrient::Up, 2540.0, "1", &style, true).center(), off, -1270.0);
    }

    #[test]
    fn quarter_turns_compose_like_set_orientation() {
        let t = Xf::of_instance(90.0, None);
        // a point to the right of the origin goes up the sheet when the symbol is turned counter-clockwise
        assert_eq!(t.apply((10.0, 0.0)), (0.0, -10.0));
        assert_eq!(Xf::of_instance(180.0, None).apply((3.0, 4.0)), (-3.0, -4.0));
        assert_eq!(Xf::of_instance(0.0, Some("y")).apply((3.0, 4.0)), (-3.0, 4.0));
        assert_eq!(Xf::of_instance(0.0, Some("x")).apply((3.0, 4.0)), (3.0, -4.0));
        let m = Xf::of_instance(270.0, Some("y"));
        assert_eq!(m.unapply(m.apply((5.0, 7.0))), (5.0, 7.0));
    }
}
