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

use crate::ir::{LabelSide, FootprintInstance, Point, Side, Um};
use crate::{ConstraintModel, Part};
use serde::{Deserialize, Serialize};

/// A pad transformed into board space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedPad {
    pub number: String,
    pub center: Point,
    /// Full axis-aligned extents in board space (w, h).
    pub size: (Um, Um),
    pub through_hole: bool,
    pub shape: PadShape,
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
            PadShape::RoundRect => 0.25 * self.size.0.min(self.size.1) as f64,
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
    /// (negative inside). This is the clearance measure the routing gates
    /// use (`routing_clearance` tests segments against pad rects, corner
    /// rounding ignored), so it is what the router must keep clear of;
    /// [`PlacedPad::signed_distance`] is the true copper edge and is up to
    /// one corner radius more lenient at a rounded corner.
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

/// Axis-aligned board-space extents of a local (w, h) box under `fp`'s
/// rotation. Exact for multiples of 90°, conservative otherwise.
pub fn rotated_extent(fp: &FootprintInstance, size: (Um, Um)) -> (Um, Um) {
    let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0 as f64, size.1 as f64);
    ((w * cos.abs() + h * sin.abs()).round() as Um, (w * sin.abs() + h * cos.abs()).round() as Um)
}

/// Every pad of `fp` in board space, sorted by pad number. `None` when the
/// part has no resolvable footprint.
pub fn placed_pads(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<Vec<PlacedPad>> {
    let footprint = model.footprint_of(part)?;
    let mut pads: Vec<PlacedPad> = footprint
        .pads
        .iter()
        .map(|p| PlacedPad {
            number: p.number.clone(),
            center: to_board(fp, p.at),
            size: rotated_extent(fp, p.size),
            through_hole: p.kind == PadKind::ThroughHole,
            shape: p.shape,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    pub name: String,
    #[serde(default)]
    pub pads: Vec<Pad>,
    /// Courtyard half-extents (w, h) in µm, centred on the origin. When
    /// absent the placer derives one from the pad bounding box plus margin.
    #[serde(default)]
    pub courtyard: Option<(Um, Um)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pad {
    pub number: String,
    /// Centre in the local frame, µm.
    pub at: (Um, Um),
    /// Full (width, height) in µm, un-rotated.
    pub size: (Um, Um),
    #[serde(default)]
    pub shape: PadShape,
    #[serde(default)]
    pub kind: PadKind,
    /// Drill diameter, µm. Only meaningful for `PadKind::ThroughHole`.
    #[serde(default)]
    pub drill: Option<Um>,
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
    pub fn validate(&self) -> Vec<crate::CheckResult> {
        let mut out = Vec::new();
        let mut bad = |what: &str, why: String| {
            out.push(crate::CheckResult::fail("footprint", format!("{}.{what}", self.name), why));
        };
        if self.pads.is_empty() {
            bad("pads", "footprint has no pads; nothing connects it to the board".into());
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.pads {
            if !seen.insert(&p.number) {
                bad("pads", format!("pad {:?} is defined twice", p.number));
            }
            if p.size.0 <= 0 || p.size.1 <= 0 {
                bad(&format!("pad {}", p.number), format!("pad is {} x {} um", p.size.0, p.size.1));
            }
            match (p.kind, p.drill) {
                (PadKind::ThroughHole, None) => bad(
                    &format!("pad {}", p.number),
                    "a through-hole pad with no drill is not a pad; the hole is a dimension of the lead, not something an exporter can derive from the copper".into(),
                ),
                (PadKind::ThroughHole, Some(d)) if d <= 0 => {
                    bad(&format!("pad {}", p.number), format!("through-hole pad drills a {d} um hole"))
                }
                (PadKind::ThroughHole, Some(d)) if d >= p.size.0.min(p.size.1) => bad(
                    &format!("pad {}", p.number),
                    format!("drill {d} um is not smaller than the {} x {} um pad, so there is no annular ring", p.size.0, p.size.1),
                ),
                _ => {}
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

fn two_pad(name: &str, pitch: Um, pw: Um, ph: Um) -> Footprint {
    Footprint {
        name: name.into(),
        pads: vec![
            Pad { number: "1".into(), at: (-pitch / 2, 0), size: (pw, ph), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
            Pad { number: "2".into(), at: (pitch / 2, 0), size: (pw, ph), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
        ],
        courtyard: None,
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
        pads.push(Pad {
            number: (i + 1).to_string(),
            at: (-col_x, y0 + i as Um * pitch),
            size: (pw, ph),
            shape: PadShape::RoundRect,
            kind: PadKind::Smd,
            drill: None,
        });
    }
    for i in 0..per_col {
        pads.push(Pad {
            number: (per_col + i + 1).to_string(),
            at: (col_x, y0 + (per_col - 1 - i) as Um * pitch),
            size: (pw, ph),
            shape: PadShape::RoundRect,
            kind: PadKind::Smd,
            drill: None,
        });
    }
    Footprint { name: name.into(), pads, courtyard: None }
}

/// Single-row 2.54 mm through-hole header, `n` pins along +x, centred.
fn pin_header(name: &str, n: usize) -> Footprint {
    let x0 = -((n as Um - 1) * 2540) / 2;
    let pads = (0..n)
        .map(|i| Pad {
            number: (i + 1).to_string(),
            at: (x0 + i as Um * 2540, 0),
            size: (1700, 1700),
            shape: if i == 0 { PadShape::Rect } else { PadShape::Circle },
            kind: PadKind::ThroughHole,
            drill: Some(1000),
        })
        .collect();
    Footprint { name: name.into(), pads, courtyard: None }
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
                Pad { number: "1".into(), at: (-938, -950), size: (1475, 600), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
                Pad { number: "2".into(), at: (-938, 950), size: (1475, 600), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
                Pad { number: "3".into(), at: (938, 0), size: (1475, 600), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
            ],
            courtyard: None,
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
                Pad { number: "1".into(), at: (-3150, -2300), size: (2000, 1500), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
                Pad { number: "2".into(), at: (3150, 0), size: (2000, 3800), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
                Pad { number: "3".into(), at: (-3150, 2300), size: (2000, 1500), shape: PadShape::RoundRect, kind: PadKind::Smd, drill: None },
            ],
            courtyard: None,
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
