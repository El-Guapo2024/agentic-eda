//! Footprint pad geometry — the physical truth the placer and router work
//! against. Local frame: micrometers, origin at the footprint centre, +x
//! right, +y down (same convention as `ir`), un-rotated, as seen from the
//! top side. Instances apply `rot` then translate; bottom-side instances
//! mirror x first (KiCad's flip convention).
//!
//! Geometry comes from two places, in priority order:
//! 1. `ConstraintModel::footprints` — explicit definitions in the intent.
//! 2. [`builtin`] — a small library of standard packages keyed by the bare
//!    package name (`"0603"`, `"SOT-23"`) or by a KiCad footprint id whose
//!    library-name prefix and metric suffix we strip (`"Capacitor_SMD:
//!    C_0603_1608Metric"` -> `"0603"`).
//!
//! A part with no resolvable footprint is a hard error for every physical
//! stage — there is deliberately no synthetic "pins on a line" stand-in.

use crate::ir::{LabelSide, FootprintInstance, Millideg, Point, Side, Um};
use crate::{ConstraintModel, Part};
use serde::{Deserialize, Serialize};

/// A pad transformed into board space.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedPad {
    /// Pad numbers are not unique: several pads may share one number (one
    /// pin, several copper pads -- a connector's shield, a split thermal
    /// pad). Every caller that maps a "REF.PIN" to a pad must expect more
    /// than one match and use them all, never just the first.
    pub number: String,
    pub center: Point,
    /// Full axis-aligned extents in board space (w, h). Exact for a pad
    /// (and footprint) rotation that is a multiple of 90 degrees,
    /// conservative otherwise -- see `rotated_extent`.
    pub size: (Um, Um),
    pub through_hole: bool,
    pub shape: PadShape,
    /// Corner ratio for `PadShape::RoundRect`; `None` = KiCad's own
    /// default (0.25).
    pub roundrect_ratio: Option<f64>,
}

impl PlacedPad {
    /// Round pads (circle/oval) have no copper in the corners of their
    /// bounding box; treat them as an ellipse for containment.
    pub fn is_round(&self) -> bool {
        matches!(self.shape, PadShape::Circle | PadShape::Oval)
    }
    /// Corner radius of a rounded-rectangle pad (KiCad default ratio 25%
    /// of the shorter side), 0 for plain rectangles.
    pub fn corner_radius(&self) -> f64 {
        match self.shape {
            PadShape::RoundRect => self.roundrect_ratio.unwrap_or(0.25) * self.size.0.min(self.size.1) as f64,
            _ => 0.0,
        }
    }

    /// Signed distance from `p` to the pad's copper edge: negative inside.
    /// Exact for rect/roundrect/circle; ovals use the inscribed ellipse
    /// scaled by the shorter radius (conservative).
    pub fn signed_distance(&self, p: Point) -> f64 {
        let (hw, hh) = (self.size.0 as f64 / 2.0, self.size.1 as f64 / 2.0);
        let (dx, dy) = ((p.x - self.center.x) as f64, (p.y - self.center.y) as f64);
        if self.is_round() {
            let rmin = hw.min(hh);
            let e = ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt();
            (e - 1.0) * rmin
        } else {
            let r = self.corner_radius();
            // Distance to the rect inset by r, then Minkowski-expand by r.
            let (iw, ih) = (hw - r, hh - r);
            let (ox, oy) = ((dx.abs() - iw).max(0.0), (dy.abs() - ih).max(0.0));
            let outside = (ox * ox + oy * oy).sqrt();
            if outside > 0.0 {
                outside - r
            } else {
                (dx.abs() - iw).max(dy.abs() - ih) - r
            }
        }
    }

    /// Distance from `p` to the pad's axis-aligned bounding rectangle
    /// (negative inside). A conservative stand-in for the pad: the grid
    /// router keeps clear of the rectangle, which can only be farther than
    /// the copper. [`PlacedPad::signed_distance`] is the true copper edge
    /// (up to one corner radius more lenient at a rounded corner), and the
    /// `routing_clearance` gate measures the exact outline
    /// (`eda_drc::board::placed_pad_copper`), not this rectangle.
    pub fn rect_distance(&self, p: Point) -> f64 {
        let (hw, hh) = (self.size.0 as f64 / 2.0, self.size.1 as f64 / 2.0);
        let (dx, dy) = (((p.x - self.center.x) as f64).abs(), ((p.y - self.center.y) as f64).abs());
        let (ox, oy) = ((dx - hw).max(0.0), (dy - hh).max(0.0));
        let outside = (ox * ox + oy * oy).sqrt();
        if outside > 0.0 { outside } else { (dx - hw).max(dy - hh) }
    }

    /// True when `p` lies inside the copper with at least `margin` µm to
    /// spare (so copper of half-width `margin` centred there stays inside).
    pub fn contains_with_margin(&self, p: Point, margin: f64) -> bool {
        self.signed_distance(p) <= -margin
    }

    /// True when `p` lies strictly inside the pad's copper.
    pub fn contains(&self, p: Point) -> bool {
        self.signed_distance(p) < 0.0
    }
}

/// Transform a local-frame point of `fp` into board space: bottom-side
/// instances mirror x, then rotate about the origin, then translate.
pub fn to_board(fp: &FootprintInstance, local: (Um, Um)) -> Point {
    let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let mirror = if fp.side == Side::Bottom { -1.0 } else { 1.0 };
    let lx = local.0 as f64 * mirror;
    let ly = local.1 as f64;
    Point { x: fp.at.x + (lx * cos - ly * sin).round() as Um, y: fp.at.y + (lx * sin + ly * cos).round() as Um }
}

/// Axis-aligned extents of a local (w, h) box rotated by `rot_millideg`.
/// Exact for multiples of 90°, conservative otherwise.
fn rotated_extent_by(rot_millideg: i64, size: (Um, Um)) -> (Um, Um) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0 as f64, size.1 as f64);
    ((w * cos.abs() + h * sin.abs()).round() as Um, (w * sin.abs() + h * cos.abs()).round() as Um)
}

/// Axis-aligned board-space extents of a local (w, h) box under `fp`'s
/// rotation. Exact for multiples of 90°, conservative otherwise.
///
/// This is the whole-footprint case (a pad with no rotation of its own);
/// [`placed_pads`] composes a pad's own rotation with `fp.rot` before
/// calling the shared [`rotated_extent_by`], since a reflection (mirroring
/// a bottom-side part) does not change the *magnitude* of a rotated box's
/// extent, only where the mirrored copy sits -- so the order composition
/// happens in does not matter for this conservative bound.
pub fn rotated_extent(fp: &FootprintInstance, size: (Um, Um)) -> (Um, Um) {
    rotated_extent_by(fp.rot as i64, size)
}

/// Every pad of `fp` in board space, sorted by pad number -- **not**
/// necessarily unique: see [`PlacedPad::number`]. `None` when the part has
/// no resolvable footprint.
pub fn placed_pads(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<Vec<PlacedPad>> {
    let footprint = model.footprint_of(part)?;
    let mut pads: Vec<PlacedPad> = footprint
        .pads
        .iter()
        .map(|p| PlacedPad {
            number: p.number.clone(),
            center: to_board(fp, p.at),
            // The pad's own rotation is relative to the footprint, applied
            // before the footprint's own rotation/mirror -- see `Pad::rot`.
            size: rotated_extent_by(fp.rot as i64 + p.rot as i64, p.size),
            through_hole: p.kind != PadKind::Smd,
            shape: p.shape,
            roundrect_ratio: p.roundrect_ratio,
        })
        .collect();
    pads.sort_by(|a, b| a.number.cmp(&b.number));
    Some(pads)
}

/// Board-space courtyard rectangle `(min_x, min_y, max_x, max_y)` of `fp`.
/// Refdes silkscreen font size for a board: 1/40 of the shorter rendered
/// side (outline bbox plus a 2 mm margin), never under 600 µm. Shared by
/// the judge renderer, the gates, the router keep-outs and the placers so
/// every stage agrees on where the label is.
pub fn refdes_font_um(outline: &[Point]) -> Um {
    let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for p in outline {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if outline.is_empty() {
        return 600;
    }
    let m = 2000;
    let (vw, vh) = (x1 - x0 + 2 * m, y1 - y0 + 2 * m);
    (vw.min(vh) / 40).clamp(600, 1000)
}

/// Refdes label box for a placed courtyard `(x0, y0, x1, y1)`: the text
/// is centred horizontally just above the courtyard's top edge — or just
/// below it when "above" would leave the board (`board_top` is the
/// outline's minimum y; pass `Um::MIN` to always label above, e.g. for
/// position-independent node geometry). Pure geometry so placers can
/// evaluate candidate poses without a model.
pub fn refdes_box_for(courtyard: (Um, Um, Um, Um), refdes: &str, font_um: Um, board_top: Um) -> (Um, Um, Um, Um) {
    refdes_box_side(courtyard, refdes, font_um, board_top, LabelSide::Above)
}

/// [`refdes_box_for`] with an explicit side. `Above` still flips below at
/// the board top; `Below` is always below.
pub fn refdes_box_side(courtyard: (Um, Um, Um, Um), refdes: &str, font_um: Um, board_top: Um, side: LabelSide) -> (Um, Um, Um, Um) {
    let (cx0, cy0, cx1, cy1) = courtyard;
    let half_w = (font_um * 6 / 10) * refdes.chars().count() as Um / 2;
    let cx = (cx0 + cx1) / 2;
    let (asc, desc) = (font_um * 3 / 4, font_um / 5);
    let above = cy0 - 200 - asc;
    match side {
        LabelSide::Above if above >= board_top => {
            let baseline = cy0 - 200;
            (cx - half_w, baseline - asc, cx + half_w, baseline + desc)
        }
        LabelSide::Above | LabelSide::Below => {
            let baseline = cy1 + 200 + asc;
            (cx - half_w, baseline - asc, cx + half_w, baseline + desc)
        }
        LabelSide::Left | LabelSide::Right => {
            // Beside the courtyard, text still horizontal, vertically
            // centred on the part.
            let cy = (cy0 + cy1) / 2;
            let baseline = cy + asc / 2;
            let (x0, x1) = if side == LabelSide::Left { (cx0 - 200 - 2 * half_w, cx0 - 200) } else { (cx1 + 200, cx1 + 200 + 2 * half_w) };
            (x0, baseline - asc, x1, baseline + desc)
        }
    }
}

/// Text baseline y for a label box from [`refdes_box_for`].
pub fn refdes_baseline(bx: (Um, Um, Um, Um), font_um: Um) -> Um {
    bx.3 - font_um / 5
}

/// Minimum y of an outline (`Um::MIN` for an empty one).
pub fn outline_top(outline: &[Point]) -> Um {
    outline.iter().map(|p| p.y).min().unwrap_or(Um::MIN)
}

/// Courtyard plus refdes label: the rectangle a part actually needs kept
/// free of other parts so its label stays readable and no neighbour's pad
/// ends up under it (which walls that pad in for routing).
pub fn keepout_for(courtyard: (Um, Um, Um, Um), refdes: &str, font_um: Um, board_top: Um) -> (Um, Um, Um, Um) {
    keepout_side(courtyard, refdes, font_um, board_top, LabelSide::Above)
}

/// [`keepout_for`] with an explicit label side.
pub fn keepout_side(courtyard: (Um, Um, Um, Um), refdes: &str, font_um: Um, board_top: Um, side: LabelSide) -> (Um, Um, Um, Um) {
    let l = refdes_box_side(courtyard, refdes, font_um, board_top, side);
    (courtyard.0.min(l.0), courtyard.1.min(l.1), courtyard.2.max(l.2), courtyard.3.max(l.3))
}

/// Placed refdes label box (see [`refdes_box_for`]).
pub fn placed_refdes_box(model: &ConstraintModel, outline: &[Point], part: &Part, fp: &FootprintInstance) -> Option<(Um, Um, Um, Um)> {
    Some(refdes_box_side(placed_courtyard(model, part, fp)?, &fp.id, model.board.refdes_font(outline), outline_top(outline), fp.label))
}

/// Placed courtyard-plus-label keep-out (see [`keepout_for`]).
pub fn placed_keepout(model: &ConstraintModel, outline: &[Point], part: &Part, fp: &FootprintInstance) -> Option<(Um, Um, Um, Um)> {
    Some(keepout_side(placed_courtyard(model, part, fp)?, &fp.id, model.board.refdes_font(outline), outline_top(outline), fp.label))
}

pub fn placed_courtyard(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<(Um, Um, Um, Um)> {
    let footprint = model.footprint_of(part)?;
    let (hw, hh) = footprint.courtyard_half();
    let (w, h) = rotated_extent(fp, (hw * 2, hh * 2));
    Some((fp.at.x - w / 2, fp.at.y - h / 2, fp.at.x + w / 2, fp.at.y + h / 2))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    pub name: String,
    #[serde(default)]
    pub pads: Vec<Pad>,
    /// Courtyard half-extents (w, h) in µm, centred on the origin. When
    /// absent the placer derives one from the pad bounding box plus margin.
    ///
    /// Always symmetric about the origin: a real footprint's F.CrtYd can be
    /// off-centre (KiCad has no such restriction), and a library footprint
    /// loaded with an asymmetric courtyard is conservatively *enclosed* by
    /// the smallest symmetric box that contains it, not represented
    /// exactly. Extending this to an offset rect is future work.
    #[serde(default)]
    pub courtyard: Option<(Um, Um)>,
    /// The real courtyard (`FOOTPRINT::GetCourtyard`): closed outlines in
    /// the footprint's local frame (same frame as `Pad::at`), built from a
    /// KiCad footprint's `F.CrtYd`/`B.CrtYd` graphics. Empty when unknown
    /// (built-in footprints, or a KiCad footprint without courtyard
    /// graphics), in which case `courtyard`/`courtyard_half` stand in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courtyard_outlines: Vec<Vec<(Um, Um)>>,
    /// KiCad `(model "...")` path for this footprint's 3D model, e.g.
    /// `"${KICAD9_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"`.
    /// `None` when no model is known (this footprint predates 3D model
    /// support, or nothing in the built-in package map matches it) -- the
    /// 3D viewer falls back to its own procedural box for those.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pad {
    /// Not unique within a footprint: several pads may share one number --
    /// one logical pin, several physical copper pads (a connector's
    /// shield, a split thermal pad). All of them go on that pin's net; see
    /// `placed_pads`.
    pub number: String,
    /// Centre in the local frame, µm.
    pub at: (Um, Um),
    /// Full (width, height) in µm, un-rotated.
    pub size: (Um, Um),
    #[serde(default)]
    pub shape: PadShape,
    #[serde(default)]
    pub kind: PadKind,
    /// Round drill diameter, µm. Only meaningful for `PadKind::ThroughHole`/
    /// `NonPlatedHole`, and mutually exclusive with `drill_slot`.
    #[serde(default)]
    pub drill: Option<Um>,
    /// Slot (oval) drill (width, height), µm -- KiCad's `(drill oval w h)`,
    /// e.g. a connector shield tab's mounting leg. When set, this pad's
    /// hole is a slot and `drill` must be absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_slot: Option<(Um, Um)>,
    /// This pad's own rotation, millidegrees, relative to the footprint --
    /// applied before the footprint's own mirror/rotation. 0 for the
    /// overwhelming majority of pads.
    #[serde(default)]
    pub rot: Millideg,
    /// Corner ratio for `PadShape::RoundRect`, KiCad's `roundrect_rratio`
    /// (fraction of the shorter side). `None` = KiCad's own default, 0.25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundrect_ratio: Option<f64>,
    /// An SMD pad flashed on the *other* outer copper layer than its
    /// footprint's own side (KiCad allows a front footprint's pad to be
    /// `(layers "B.Cu")`, e.g. the back-side fingers of a card-edge
    /// connector). `false` -- the usual case -- means the footprint's side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub opposite_side: bool,
}

impl Pad {
    /// Is this pad's copper on the back (`B.Cu`) when its footprint sits
    /// on `fp_side`? Only meaningful for an SMD pad.
    pub fn on_back(&self, fp_side: crate::ir::Side) -> bool {
        (fp_side == crate::ir::Side::Bottom) != self.opposite_side
    }
}

impl Footprint {
    /// Reject a footprint that cannot be fabricated.
    ///
    /// The drill check is the one that matters. A through-hole pad with no
    /// drill is not a pad, and both exporters used to invent one -- the
    /// KiCad writer from `w.min(h) / 2`, the circuit-json writer from
    /// `size.0 / 2`. For a non-square pad those are different holes, so the
    /// same part shipped a different board depending on which exporter ran,
    /// and neither number came from the part's datasheet. There is no right
    /// value to guess here: the hole is a dimension of the physical lead.
    ///
    /// Pad numbers are deliberately *not* checked for uniqueness: KiCad
    /// itself allows a number to repeat (a connector's shield tab is
    /// commonly four physical pads named the same pin), and an empty
    /// number (a mounting hole) is normal too.
    pub fn validate(&self) -> Vec<crate::CheckResult> {
        let mut out = Vec::new();
        let mut bad = |what: &str, why: String| {
            out.push(crate::CheckResult::fail("footprint", format!("{}.{what}", self.name), why));
        };
        if self.pads.is_empty() {
            bad("pads", "footprint has no pads; nothing connects it to the board".into());
        }
        for p in &self.pads {
            if p.size.0 <= 0 || p.size.1 <= 0 {
                bad(&format!("pad {}", p.number), format!("pad is {} x {} um", p.size.0, p.size.1));
            }
            if let Some(r) = p.roundrect_ratio {
                if !(0.0..=0.5).contains(&r) {
                    bad(&format!("pad {}", p.number), format!("roundrect ratio {r} is outside 0.0..=0.5 (a fraction of the shorter side)"));
                }
            }
            if matches!(p.kind, PadKind::ThroughHole | PadKind::NonPlatedHole) {
                // A non-plated hole is normal at size == drill (no annular
                // ring by definition -- there is no copper); only a plated
                // through-hole needs the drill strictly smaller than the pad.
                let plated = p.kind == PadKind::ThroughHole;
                match (p.drill, p.drill_slot) {
                    (None, None) => bad(
                        &format!("pad {}", p.number),
                        "a through-hole or non-plated pad with no drill is not a hole; the hole is a dimension of the lead or fastener, not something an exporter can derive from the copper".into(),
                    ),
                    (Some(_), Some(_)) => bad(&format!("pad {}", p.number), "a pad cannot have both a round drill and a slot drill".into()),
                    (Some(d), None) if d <= 0 => bad(&format!("pad {}", p.number), format!("drills a {d} um hole")),
                    (Some(d), None) if plated && d >= p.size.0.min(p.size.1) => bad(
                        &format!("pad {}", p.number),
                        format!("drill {d} um is not smaller than the {} x {} um pad, so there is no annular ring", p.size.0, p.size.1),
                    ),
                    (None, Some((w, h))) if w <= 0 || h <= 0 => bad(&format!("pad {}", p.number), format!("slot drill is {w} x {h} um")),
                    (None, Some((w, h))) if plated && (w >= p.size.0 || h >= p.size.1) => bad(
                        &format!("pad {}", p.number),
                        format!("slot drill {w} x {h} um is not smaller than the {} x {} um pad, so there is no annular ring", p.size.0, p.size.1),
                    ),
                    _ => {}
                }
            }
        }
        if let Some((w, h)) = self.courtyard {
            if w <= 0 || h <= 0 {
                bad("courtyard", format!("courtyard half-extents are {w} x {h} um"));
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    #[default]
    Rect,
    RoundRect,
    Circle,
    Oval,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    #[default]
    Smd,
    ThroughHole,
    /// A mechanical hole with no copper and no net -- KiCad's
    /// `np_thru_hole` (a mounting hole, a connector's locating peg). Still
    /// occupies space: it counts as a hole for clearance the same way any
    /// other pad does, via the same generic pad-clearance checks.
    NonPlatedHole,
}

impl Footprint {
    /// Axis-aligned bounding box of the pads in the local frame:
    /// `(min_x, min_y, max_x, max_y)`. Zero box for a pad-less footprint.
    pub fn pad_bbox(&self) -> (Um, Um, Um, Um) {
        let mut b = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
        for p in &self.pads {
            b.0 = b.0.min(p.at.0 - p.size.0 / 2);
            b.1 = b.1.min(p.at.1 - p.size.1 / 2);
            b.2 = b.2.max(p.at.0 + p.size.0 / 2);
            b.3 = b.3.max(p.at.1 + p.size.1 / 2);
        }
        if self.pads.is_empty() {
            (0, 0, 0, 0)
        } else {
            b
        }
    }

    /// Courtyard half-extents about the origin: explicit, else the pad
    /// bbox's largest reach from the origin + 250 µm margin (so an
    /// asymmetric pad layout still gets a courtyard that covers it).
    pub fn courtyard_half(&self) -> (Um, Um) {
        if let Some(c) = self.courtyard {
            return c;
        }
        let (x0, y0, x1, y1) = self.pad_bbox();
        let hw = x0.abs().max(x1.abs()) + 250;
        let hh = y0.abs().max(y1.abs()) + 250;
        (hw.max(250), hh.max(250))
    }
}

// ------------------------------------------------------------- built-ins

/// Normalise a footprint or package name to a built-in key.
pub fn normalize_name(name: &str) -> String {
    // Strip KiCad library prefix.
    let s = name.rsplit(':').next().unwrap_or(name);
    let mut s = s.to_ascii_uppercase();
    // Strip KiCad's "<Letter>_" prefix ("C_0603_1608Metric", "R_0402_...").
    if s.len() > 2 && s.as_bytes()[1] == b'_' && s.as_bytes()[0].is_ascii_alphabetic() {
        s = s[2..].to_string();
    }
    // Strip "_1608METRIC" style suffix.
    if let Some(i) = s.find("_") {
        if s[i + 1..].ends_with("METRIC") {
            s = s[..i].to_string();
        }
    }
    // "SOT-23-5" / "SOT23-5" / "SOT_23_5" -> "SOT-23-5"
    s = s.replace('_', "-");
    if s.starts_with("SOT") && !s.starts_with("SOT-") {
        s = format!("SOT-{}", &s[3..]);
    }
    if s.starts_with("SOIC") && !s.starts_with("SOIC-") {
        s = format!("SOIC-{}", &s[4..]);
    }
    if s.starts_with("TSSOP") && !s.starts_with("TSSOP-") {
        s = format!("TSSOP-{}", &s[5..]);
    }
    s
}

impl Pad {
    /// A pad with no per-pad rotation, no slot drill, and KiCad's default
    /// roundrect ratio -- what every built-in package uses.
    fn simple(number: impl Into<String>, at: (Um, Um), size: (Um, Um), shape: PadShape, kind: PadKind, drill: Option<Um>) -> Self {
        Pad { opposite_side: false, number: number.into(), at, size, shape, kind, drill, drill_slot: None, rot: 0, roundrect_ratio: None }
    }
}

fn two_pad(name: &str, pitch: Um, pw: Um, ph: Um) -> Footprint {
    Footprint {
        name: name.into(),
        pads: vec![
            Pad::simple("1", (-pitch / 2, 0), (pw, ph), PadShape::RoundRect, PadKind::Smd, None),
            Pad::simple("2", (pitch / 2, 0), (pw, ph), PadShape::RoundRect, PadKind::Smd, None),
        ],
        courtyard: None,
        model: None,
        courtyard_outlines: vec![],
    }
}

/// Dual-column gull-wing package (SOIC/TSSOP/MSOP-style): `n` pins total in
/// two columns at x = ∓`col_x`, pitch `pitch` along y. Pin 1 is at the top
/// of the left column (min y), numbering increases going down the left
/// column, then continues up the right column from bottom to top — matching
/// KiCad's standard SOIC/TSSOP footprint numbering.
fn dual_row(name: &str, n: usize, pitch: Um, col_x: Um, pw: Um, ph: Um) -> Footprint {
    let per_col = n / 2;
    let y0 = -((per_col as Um - 1) * pitch) / 2;
    let mut pads = Vec::with_capacity(n);
    for i in 0..per_col {
        pads.push(Pad::simple((i + 1).to_string(), (-col_x, y0 + i as Um * pitch), (pw, ph), PadShape::RoundRect, PadKind::Smd, None));
    }
    for i in 0..per_col {
        pads.push(Pad::simple((per_col + i + 1).to_string(), (col_x, y0 + (per_col - 1 - i) as Um * pitch), (pw, ph), PadShape::RoundRect, PadKind::Smd, None));
    }
    Footprint { name: name.into(), pads, courtyard: None, model: None, courtyard_outlines: vec![] }
}

/// Single-row 2.54 mm through-hole header, `n` pins along +x, centred.
fn pin_header(name: &str, n: usize) -> Footprint {
    let x0 = -((n as Um - 1) * 2540) / 2;
    let pads = (0..n)
        .map(|i| Pad::simple((i + 1).to_string(), (x0 + i as Um * 2540, 0), (1700, 1700), if i == 0 { PadShape::Rect } else { PadShape::Circle }, PadKind::ThroughHole, Some(1000)))
        .collect();
    Footprint { name: name.into(), pads, courtyard: None, model: None, courtyard_outlines: vec![] }
}

/// All fixed-key built-ins (excludes the generic `PINHEADER-N` family, which
/// is parameterized by pin count — see [`builtin`]'s fallback branch).
pub fn builtin_names() -> &'static [&'static str] {
    &[
        "0201", "0402", "0603", "0805", "1206", "1210", "SOD-123", "SOD-323", "SMA", "SOT-23", "SOT-23-5", "SOT-23-6", "SOT-223",
        "SOIC-8", "SOIC-14", "SOIC-16", "TSSOP-8", "TSSOP-14", "TSSOP-16", "TSSOP-20", "MSOP-8", "MSOP-10",
    ]
}

/// Built-in package library. Dimensions follow IPC-7351 nominal land
/// patterns as used by the KiCad standard libraries.
pub fn builtin(name: &str) -> Option<Footprint> {
    let key = normalize_name(name);
    let fp = match key.as_str() {
        // Two-pad passives: pitch/size taken from KiCad's Resistor_SMD.pretty
        // (canonical for the shared two-pad shape; nearly identical to
        // Capacitor_SMD).
        "0201" => two_pad(&key, 640, 460, 400),
        "0402" => two_pad(&key, 1020, 540, 640),
        "0603" => two_pad(&key, 1650, 800, 950),
        "0805" => two_pad(&key, 1825, 1025, 1400),
        "1206" => two_pad(&key, 2925, 1125, 1750),
        "1210" => two_pad(&key, 2925, 1125, 2650),
        "SOD-123" => two_pad(&key, 3300, 900, 1200),
        "SOD-323" => two_pad(&key, 2100, 600, 450),
        "SMA" => two_pad(&key, 4000, 2500, 1800),
        // Package_TO_SOT_SMD.pretty/SOT-23.kicad_mod: pins 1,2 on the left
        // column (top/bottom), pin 3 alone on the right column (middle).
        "SOT-23" | "SOT-23-3" => Footprint {
            name: "SOT-23".into(),
            pads: vec![
                Pad::simple("1", (-938, -950), (1475, 600), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("2", (-938, 950), (1475, 600), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("3", (938, 0), (1475, 600), PadShape::RoundRect, PadKind::Smd, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        },
        // Package_TO_SOT_SMD.pretty/SOT-23-5.kicad_mod / SOT-23-6.kicad_mod:
        // both columns at x = ∓1.1375mm, pitch 0.95mm. SOT-23-5 omits the
        // right-column middle pad (built as a 6-pad dual_row, then pin 5 —
        // the right-middle pad — dropped and pin 6 renumbered to 5).
        "SOT-23-5" | "SOT-23-6" => {
            let n = if key == "SOT-23-5" { 5 } else { 6 };
            let mut fp = dual_row(&key, 6, 950, 1138, 1325, 600);
            if n == 5 {
                fp.pads.retain(|p| p.number != "5");
                for p in fp.pads.iter_mut() {
                    if p.number == "6" {
                        p.number = "5".into();
                    }
                }
            }
            fp
        }
        // Package_TO_SOT_SMD.pretty/SOT-223-3_TabPin2.kicad_mod: pins 1 and
        // 3 are the small leads on the left column; pin 2 is the tab — a
        // large pad on the right column (KiCad also draws a small redundant
        // copper pad for pin 2 on the left column at the same net; we merge
        // that into the single tab pad since our Pad numbers must be
        // unique).
        "SOT-223" => Footprint {
            name: key.clone(),
            pads: vec![
                Pad::simple("1", (-3150, -2300), (2000, 1500), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("2", (3150, 0), (2000, 3800), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("3", (-3150, 2300), (2000, 1500), PadShape::RoundRect, PadKind::Smd, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        },
        // Package_SO.pretty SOIC-8/14/16_3.9x*mm_P1.27mm.
        "SOIC-8" => dual_row(&key, 8, 1270, 2475, 1950, 600),
        "SOIC-14" => dual_row(&key, 14, 1270, 2475, 1950, 600),
        "SOIC-16" => dual_row(&key, 16, 1270, 2475, 1950, 600),
        // Package_SO.pretty TSSOP-*_4.4x*mm_P0.65mm.
        "TSSOP-8" => dual_row(&key, 8, 650, 2863, 1475, 400),
        "TSSOP-14" => dual_row(&key, 14, 650, 2863, 1475, 400),
        "TSSOP-16" => dual_row(&key, 16, 650, 2863, 1475, 400),
        "TSSOP-20" => dual_row(&key, 20, 650, 2863, 1475, 400),
        // Package_SO.pretty MSOP-8_3x3mm_P0.65mm / MSOP-10_3x3mm_P0.5mm.
        "MSOP-8" => dual_row(&key, 8, 650, 2113, 1625, 400),
        "MSOP-10" => dual_row(&key, 10, 500, 2100, 1500, 350),
        _ => {
            // Generic headers: "PINHEADER-N" / "PIN_HEADER_1X04" / "1X04".
            let digits: String = key.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
            if (key.starts_with("PINHEADER-") || key.starts_with("PIN-HEADER-") || key.starts_with("1X") || key.contains("-1X"))
                && !digits.is_empty()
            {
                pin_header(&key, digits.parse().ok()?)
            } else {
                return None;
            }
        }
    };
    Some(fp)
}

/// A footprint of KiCad's own libraries that a generic package name stands for, and with it the 3D model of that footprint. This is the small explicit
/// table behind the 3D view: a part whose footprint is a bare `0603`, `SOIC-16` or `PINHEADER-4` (what a netlist says, not a library id) is drawn and
/// exported as the real thing -- a resistor, an LED, an SOIC-16, a 1x04 header -- instead of a placeholder box.
///
/// KiCad's 3D libraries mirror its footprint libraries one to one (`Resistor_SMD.pretty/R_0603_1608Metric.kicad_mod` has
/// `Resistor_SMD.3dshapes/R_0603_1608Metric.step`), so the footprint id is all the table has to say; every entry was checked against the installed KiCad
/// 10.99.0 libraries (both files exist), not guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KicadFootprintRef {
    /// The library nickname: `Resistor_SMD`.
    pub library: String,
    /// The footprint in it: `R_0603_1608Metric`.
    pub name: String,
}

impl KicadFootprintRef {
    fn new(library: &str, name: impl Into<String>) -> Self {
        Self { library: library.to_string(), name: name.into() }
    }

    /// `Lib:Name`, the footprint's id in KiCad's libraries.
    pub fn id(&self) -> String {
        format!("{}:{}", self.library, self.name)
    }

    /// KiCad's own `(model "...")` path for it: `${KICAD10_3DMODEL_DIR}` is the exact variable the installed KiCad 10.99.0 footprint files use (checked with
    /// `grep "(model " .../footprints/**/*.kicad_mod`); older releases name it `KICAD6_3DMODEL_DIR` ... `KICAD9_3DMODEL_DIR`, but `kicad-cli pcb export glb`
    /// resolves whichever variable its own version defines, so this only has to match the version that runs the export.
    pub fn model_path(&self) -> String {
        format!("${{KICAD10_3DMODEL_DIR}}/{}.3dshapes/{}.step", self.library, self.name)
    }
}

/// The KiCad footprint a generic package `key` stands for, given the part's reference designator and value; `None` when nothing in the table matches.
///
/// `key` is the *normalized* name (`normalize_name( part.package )`), so `Resistor_SMD:R_0603_1608Metric` arrives as `0603` too. The two-pad passive
/// shapes (`0402`/`0603`/`0805`) are the same footprint for a resistor, a capacitor, an inductor, an LED or a diode, so those need the reference
/// designator's own R/C/L/D prefix to pick the library -- the same convention the schematic's passive-glyph heuristic uses (web/studio's isTwoPinPassive /
/// schematic/layout.ts); a `D` is an LED unless its value says it is a diode (`LED` or no value stays an LED, which is what every `D` of the studio's example
/// boards is).
pub fn kicad_footprint_for(key: &str, reference: &str, value: Option<&str>) -> Option<KicadFootprintRef> {
    match key {
        "0402" | "0603" | "0805" => {
            let metric = match key {
                "0402" => "1005Metric",
                "0603" => "1608Metric",
                _ => "2012Metric",
            };
            let is_diode = value.is_some_and(|v| {
                let v = v.to_ascii_lowercase();
                !v.contains("led") && (v.starts_with("1n") || v.contains("diode") || v.contains("schottky") || v.contains("zener") || v.starts_with("bat") || v.starts_with("ss") || v.starts_with("bas"))
            });
            let (lib, prefix) = match reference.chars().next().unwrap_or('?').to_ascii_uppercase() {
                'R' => ("Resistor_SMD", "R"),
                'C' => ("Capacitor_SMD", "C"),
                'L' => ("Inductor_SMD", "L"),
                'D' if is_diode => ("Diode_SMD", "D"),
                'D' => ("LED_SMD", "LED"),
                _ => return None,
            };
            // KiCad ships a 0402 diode footprint but no 3D model for it: a model path that names no file would only make the export complain.
            if lib == "Diode_SMD" && key == "0402" {
                return None;
            }
            Some(KicadFootprintRef::new(lib, format!("{prefix}_{key}_{metric}")))
        }
        "SOT-23" => Some(KicadFootprintRef::new("Package_TO_SOT_SMD", "SOT-23")),
        "SOT-23-5" => Some(KicadFootprintRef::new("Package_TO_SOT_SMD", "SOT-23-5")),
        "SOT-23-6" => Some(KicadFootprintRef::new("Package_TO_SOT_SMD", "SOT-23-6")),
        "SOT-223" => Some(KicadFootprintRef::new("Package_TO_SOT_SMD", "SOT-223")),
        "SOIC-8" => Some(KicadFootprintRef::new("Package_SO", "SOIC-8_3.9x4.9mm_P1.27mm")),
        "SOIC-14" => Some(KicadFootprintRef::new("Package_SO", "SOIC-14_3.9x8.7mm_P1.27mm")),
        "SOIC-16" => Some(KicadFootprintRef::new("Package_SO", "SOIC-16_3.9x9.9mm_P1.27mm")),
        "TSSOP-8" => Some(KicadFootprintRef::new("Package_SO", "TSSOP-8_4.4x3mm_P0.65mm")),
        "TSSOP-14" => Some(KicadFootprintRef::new("Package_SO", "TSSOP-14_4.4x5mm_P0.65mm")),
        "TSSOP-16" => Some(KicadFootprintRef::new("Package_SO", "TSSOP-16_4.4x5mm_P0.65mm")),
        "TSSOP-20" => Some(KicadFootprintRef::new("Package_SO", "TSSOP-20_4.4x6.5mm_P0.65mm")),
        "MSOP-8" => Some(KicadFootprintRef::new("Package_SO", "MSOP-8_3x3mm_P0.65mm")),
        "MSOP-10" => Some(KicadFootprintRef::new("Package_SO", "MSOP-10_3x3mm_P0.5mm")),
        _ => {
            // Generic headers -- same trailing-digit extraction `builtin`'s own fallback branch uses, since `key` here can be any of several spellings
            // ("PINHEADER-4", "PIN-HEADER-4", "1X04").
            let digits: String = key.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
            if (key.starts_with("PINHEADER") || key.starts_with("PIN-HEADER") || key.contains("1X")) && !digits.is_empty() {
                let n: u32 = digits.parse().ok()?;
                Some(KicadFootprintRef::new("Connector_PinHeader_2.54mm", format!("PinHeader_1x{n:02}_P2.54mm_Vertical")))
            } else {
                None
            }
        }
    }
}

/// KiCad's own `(model "...")` path for a built-in package: [`kicad_footprint_for`]'s footprint's 3D model. `key` is the *pre-normalized* name (`builtin`'s
/// own `key`, e.g. from `normalize_name(part.package)`), so a caller that already has it (like `ConstraintModel::footprint_of`) does not normalize twice.
/// Without the part's value a `D` is an LED.
pub fn builtin_model_path(key: &str, reference: &str) -> Option<String> {
    kicad_footprint_for(key, reference, None).map(|f| f.model_path())
}

/// The KiCad footprint a part's `footprint` / `package` name stands for through the table (`Part::footprint` first, then `package` -- the order
/// `ConstraintModel::footprint_of` resolves pads in); a real library id (`Resistor_SMD:R_0603_1608Metric`) normalizes to its generic key and maps back to the
/// same footprint.
pub fn kicad_footprint_of_part(part: &crate::Part) -> Option<KicadFootprintRef> {
    [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten().find_map(|name| kicad_footprint_for(&normalize_name(name), &part.reference, part.value.as_deref()))
}

/// The `(model ...)` path an exported footprint of this part gets when the footprint itself carries none -- an explicit definition (a project-library entry, a
/// `.kicad_mod` read for its pads) has no model of its own, and without one `kicad-cli pcb export glb` leaves the part out of the 3D view altogether.
pub fn model_path_for_part(part: &crate::Part) -> Option<String> {
    kicad_footprint_of_part(part).map(|f| f.model_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_kicad_ids() {
        assert_eq!(normalize_name("Capacitor_SMD:C_0603_1608Metric"), "0603");
        assert_eq!(normalize_name("Resistor_SMD:R_0402_1005Metric"), "0402");
        assert_eq!(normalize_name("Package_TO_SOT_SMD:SOT-23-5"), "SOT-23-5");
        assert_eq!(normalize_name("sot23"), "SOT-23");
        assert_eq!(normalize_name("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"), "SOIC-8_3.9X4.9MM_P1.27MM".replace('_', "-"));
    }

    fn part(reference: &str, package: &str, value: Option<&str>) -> crate::Part {
        crate::Part { reference: reference.into(), mpn: None, lcsc: None, value: value.map(String::from), package: Some(package.into()), footprint: Some(package.into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None }
    }

    /// The four footprints of the 3D-view brief, through the table: a bare package name stands for the real KiCad footprint and model.
    #[test]
    fn generic_package_names_map_to_kicads_footprints_and_models() {
        let id = |p: &crate::Part| kicad_footprint_of_part(p).map(|f| f.id());
        assert_eq!(id(&part("R1", "0603", Some("330"))).as_deref(), Some("Resistor_SMD:R_0603_1608Metric"));
        assert_eq!(id(&part("D1", "0603", Some("LED"))).as_deref(), Some("LED_SMD:LED_0603_1608Metric"));
        assert_eq!(id(&part("U1", "SOIC-16", None)).as_deref(), Some("Package_SO:SOIC-16_3.9x9.9mm_P1.27mm"));
        assert_eq!(id(&part("J1", "PINHEADER-4", None)).as_deref(), Some("Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical"));
        assert_eq!(model_path_for_part(&part("R1", "0603", None)).as_deref(), Some("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step"));
        assert_eq!(model_path_for_part(&part("U1", "SOIC-16", None)).as_deref(), Some("${KICAD10_3DMODEL_DIR}/Package_SO.3dshapes/SOIC-16_3.9x9.9mm_P1.27mm.step"));
        assert_eq!(model_path_for_part(&part("J1", "PINHEADER-4", None)).as_deref(), Some("${KICAD10_3DMODEL_DIR}/Connector_PinHeader_2.54mm.3dshapes/PinHeader_1x04_P2.54mm_Vertical.step"));
    }

    #[test]
    fn the_reference_letter_and_value_pick_the_passive_library() {
        let id = |r: &str, v: Option<&str>| kicad_footprint_of_part(&part(r, "0402", v)).map(|f| f.id());
        assert_eq!(id("C3", Some("100nF")).as_deref(), Some("Capacitor_SMD:C_0402_1005Metric"));
        assert_eq!(id("L1", Some("10uH")).as_deref(), Some("Inductor_SMD:L_0402_1005Metric"));
        assert_eq!(id("D2", None).as_deref(), Some("LED_SMD:LED_0402_1005Metric"), "a D with no value is an LED, as every D of the example boards is");
        assert_eq!(id("D2", Some("LED")).as_deref(), Some("LED_SMD:LED_0402_1005Metric"));
        assert_eq!(kicad_footprint_of_part(&part("D3", "0603", Some("1N4148"))).map(|f| f.id()).as_deref(), Some("Diode_SMD:D_0603_1608Metric"));
        assert_eq!(id("D3", Some("1N4148")), None, "KiCad has no 0402 diode 3D model, so no model path rather than one that names no file");
        assert_eq!(id("X1", None), None, "a crystal is not a resistor: no guess from a letter the table does not know");
    }

    #[test]
    fn a_real_kicad_footprint_id_maps_back_to_the_same_footprint() {
        let mut p = part("R2", "0603", None);
        p.footprint = Some("Resistor_SMD:R_0603_1608Metric".into());
        assert_eq!(kicad_footprint_of_part(&p).map(|f| f.id()).as_deref(), Some("Resistor_SMD:R_0603_1608Metric"));
        let mut q = part("U2", "Whatever", None);
        q.footprint = Some("Package_TO_SOT_SMD:SOT-23-5".into());
        assert_eq!(kicad_footprint_of_part(&q).map(|f| f.id()).as_deref(), Some("Package_TO_SOT_SMD:SOT-23-5"));
        assert_eq!(model_path_for_part(&part("U3", "QFN-48", None)), None, "a package the table does not know has no model");
    }

    /// Every entry of the table names files KiCad.app really has -- a footprint and its 3D model -- when it is installed here.
    #[test]
    fn every_table_entry_names_files_kicad_ships() {
        let (fp_root, m3_root) = (std::path::Path::new("/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints"), std::path::Path::new("/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels"));
        if !fp_root.is_dir() || !m3_root.is_dir() {
            return; // no KiCad install on this machine
        }
        let mut parts = Vec::new();
        for key in ["0402", "0603", "0805"] {
            for r in ["R", "C", "L", "D"] {
                parts.push(part(&format!("{r}1"), key, Some(if r == "D" { "LED" } else { "x" })));
            }
        }
        for key in ["SOT-23", "SOT-23-5", "SOT-23-6", "SOT-223", "SOIC-8", "SOIC-14", "SOIC-16", "TSSOP-8", "TSSOP-14", "TSSOP-16", "TSSOP-20", "MSOP-8", "MSOP-10", "PINHEADER-1", "PINHEADER-4", "PINHEADER-12"] {
            parts.push(part("U1", key, None));
        }
        for p in &parts {
            let Some(f) = kicad_footprint_of_part(p) else { continue };
            assert!(fp_root.join(format!("{}.pretty", f.library)).join(format!("{}.kicad_mod", f.name)).is_file(), "{} has no footprint file", f.id());
            assert!(m3_root.join(format!("{}.3dshapes", f.library)).join(format!("{}.step", f.name)).is_file(), "{} has no 3D model", f.id());
        }
        assert!(parts.iter().filter(|p| kicad_footprint_of_part(p).is_some()).count() >= 25, "the table covers the built-in packages");
    }

    #[test]
    fn builtins_have_unique_numbered_pads() {
        for name in ["0402", "0603", "0805", "SOT-23", "SOT-23-5", "SOT-23-6", "SOIC-8", "TSSOP-16", "SOT-223", "PINHEADER-4"] {
            let fp = builtin(name).unwrap_or_else(|| panic!("{name}"));
            let mut nums: Vec<&str> = fp.pads.iter().map(|p| p.number.as_str()).collect();
            let n = nums.len();
            nums.sort();
            nums.dedup();
            assert_eq!(nums.len(), n, "{name} duplicate pad numbers");
            // Pads must not overlap each other.
            for (i, a) in fp.pads.iter().enumerate() {
                for b in fp.pads.iter().skip(i + 1) {
                    let dx = (a.at.0 - b.at.0).abs();
                    let dy = (a.at.1 - b.at.1).abs();
                    assert!(dx * 2 >= a.size.0 + b.size.0 || dy * 2 >= a.size.1 + b.size.1, "{name}: pads {} & {} overlap", a.number, b.number);
                }
            }
        }
        assert_eq!(builtin("SOT-23-5").unwrap().pads.len(), 5);
        assert_eq!(builtin("SOIC-8").unwrap().pads.len(), 8);
        assert!(builtin("NOPE").is_none());
    }

    #[test]
    fn courtyard_derives_from_pads() {
        let fp = builtin("0603").unwrap();
        let (hw, hh) = fp.courtyard_half();
        assert_eq!(hw, 1650 / 2 + 800 / 2 + 250);
        assert_eq!(hh, 950 / 2 + 250);
        let hdr = builtin("PINHEADER-4").unwrap();
        let (x0, _, x1, _) = hdr.pad_bbox();
        assert_eq!(x0, -x1, "headers are centred");
    }

    fn part_with(name: &str, footprint: Footprint) -> (crate::ConstraintModel, crate::Part) {
        let part = crate::Part {
            reference: "U1".into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some(name.into()),
            footprint: Some(name.into()),
            pins: (1..=footprint.pads.len()).map(|i| crate::Pin { number: i.to_string(), name: None, kind: crate::PinKind::Passive }).collect(),
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        };
        let model = crate::ConstraintModel { parts: vec![part.clone()], footprints: vec![footprint], ..Default::default() };
        (model, part)
    }

    fn fp_instance(rot: crate::ir::Millideg, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: 10_000, y: 10_000 }, rot, side, label: Default::default() }
    }

    #[test]
    fn a_pads_own_rotation_composes_with_the_footprints() {
        // A 2000x800 pad, rotated 90 degrees by itself inside an unrotated
        // footprint, must present the same board-space extent as an
        // unrotated pad inside a footprint rotated 90 degrees: rotation
        // composes additively regardless of which side contributes it.
        let mut fp = Footprint { name: "T".into(), pads: vec![Pad::simple("1", (0, 0), (2000, 800), PadShape::Rect, PadKind::Smd, None)], courtyard: None, model: None, courtyard_outlines: vec![] };
        fp.pads[0].rot = 90_000;
        let (model, part) = part_with("T", fp.clone());
        let via_pad_rot = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();

        fp.pads[0].rot = 0;
        let (model2, part2) = part_with("T", fp);
        let via_fp_rot = placed_pads(&model2, &part2, &fp_instance(90_000, Side::Top)).unwrap();

        assert_eq!(via_pad_rot[0].size, (800, 2000));
        assert_eq!(via_pad_rot[0].size, via_fp_rot[0].size, "a pad's own rotation and its footprint's must compose the same way");
    }

    #[test]
    fn placed_pads_keeps_every_pad_of_a_repeated_number() {
        // Four physical "SH" pads, one logical pin: every one of them must
        // come back from `placed_pads`, at its own position, not collapsed
        // to one.
        let shield = Footprint {
            name: "CONN".into(),
            pads: vec![
                Pad::simple("SH", (-4000, -3000), (1000, 2000), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (-4000, 1000), (1000, 1500), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (4000, -3000), (1000, 2000), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (4000, 1000), (1000, 1500), PadShape::Oval, PadKind::ThroughHole, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        };
        let (model, part) = part_with("CONN", shield);
        let pads = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();
        let shs: Vec<_> = pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shs.len(), 4, "every physical pad of the shared number must survive, not just one");
        let centers: std::collections::BTreeSet<(Um, Um)> = shs.iter().map(|p| (p.center.x, p.center.y)).collect();
        assert_eq!(centers.len(), 4, "the four pads must keep their own distinct positions");
    }

    #[test]
    fn roundrect_ratio_flows_into_placed_pads() {
        let fp = Footprint {
            name: "T".into(),
            pads: vec![{
                let mut p = Pad::simple("1", (0, 0), (1000, 2000), PadShape::RoundRect, PadKind::Smd, None);
                p.roundrect_ratio = Some(0.4);
                p
            }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        };
        let (model, part) = part_with("T", fp);
        let pads = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();
        assert_eq!(pads[0].corner_radius(), 0.4 * 1000.0);
    }
}

/// An edge connector: `Part::edge` when set, else a value/mpn naming a
/// thing a cable plugs into (USB, jack, terminal block, receptacle,
/// socket, barrel, RJ45, SMA). Shared by the placement gate, the
/// annealer and the Cypress edge pass so they cannot disagree.
pub fn is_edge_connector(part: &Part) -> bool {
    if let Some(e) = part.edge {
        return e;
    }
    // Keywords only on connector-class references (J, P, X): a "USB ESD
    // array" (D2) or a "USB-UART bridge" (U9) is not a connector.
    let r = part.reference.as_str();
    if !(r.starts_with('J') || r.starts_with('P') || r.starts_with('X')) {
        return false;
    }
    let text = format!("{} {}", part.value.clone().unwrap_or_default(), part.mpn.clone().unwrap_or_default()).to_ascii_lowercase();
    ["usb", "jack", "terminal", "receptacle", "socket", "barrel", "rj45", "sma"].iter().any(|k| text.contains(k))
}

/// Which board edges a connector courtyard may lie along: an elongated
/// connector (aspect > 1.3) only counts the two edges parallel to its long
/// side — a header touching the edge with its short end is not on the
/// edge, its pins point into the board. Returns `(left, right, top, bottom)`
/// usable flags.
pub fn usable_edges(r: (Um, Um, Um, Um)) -> (bool, bool, bool, bool) {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    if w as f64 > 1.3 * h as f64 {
        (false, false, true, true)
    } else if h as f64 > 1.3 * w as f64 {
        (true, true, false, false)
    } else {
        (true, true, true, true)
    }
}

/// Gap from a connector courtyard to the nearest usable board edge (see
/// [`usable_edges`]); `bb` is the outline bounding box.
pub fn edge_connector_gap(r: (Um, Um, Um, Um), bb: (Um, Um, Um, Um)) -> Um {
    let (l, rt, t, b) = (r.0 - bb.0, bb.2 - r.2, r.1 - bb.1, bb.3 - r.3);
    let (ul, ur, ut, ub) = usable_edges(r);
    [(ul, l), (ur, rt), (ut, t), (ub, b)].iter().filter(|(u, _)| *u).map(|(_, g)| *g).min().unwrap_or(l.min(rt).min(t).min(b))
}
