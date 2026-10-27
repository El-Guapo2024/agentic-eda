//! Footprint pad geometry — the physical truth the placer and router work
//! against. Local frame: nanometres, origin at the footprint centre, +x
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

use crate::ir::{LabelSide, FootprintInstance, Millideg, Point, Side, Nm};
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
    pub size: (Nm, Nm),
    pub through_hole: bool,
    pub shape: PadShape,
    /// Corner ratio for `PadShape::RoundRect`; `None` = KiCad's own
    /// default (0.25).
    pub roundrect_ratio: Option<f64>,
    /// The pad's copper in board space when `shape` and `size` do not give it -- a trapezoid, a chamfered rectangle, a custom pad, a
    /// pad whose copper is off its position ([`crate::pad_geom::pad_pieces`]); empty for the rest. [`PlacedPad::signed_distance`]
    /// measures to it, so a connection and a clearance see the real copper and not the box round it.
    pub pieces: Vec<crate::pad_geom::PadPiece>,
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
            PadShape::RoundRect => self.roundrect_ratio.unwrap_or(0.25) * (self.size.0.min(self.size.1)).as_f64(),
            _ => 0.0,
        }
    }

    /// Signed distance from `p` to the pad's copper edge: negative inside.
    /// Exact for rect/roundrect/circle; ovals use the inscribed ellipse
    /// scaled by the shorter radius (conservative).
    pub fn signed_distance(&self, p: Point) -> f64 {
        if !self.pieces.is_empty() {
            return self.pieces.iter().map(|q| q.signed_distance((p.x.as_f64(), p.y.as_f64()))).fold(f64::INFINITY, f64::min);
        }
        let (hw, hh) = (self.size.0.as_f64() / 2.0, self.size.1.as_f64() / 2.0);
        let (dx, dy) = ((p.x - self.center.x).as_f64(), (p.y - self.center.y).as_f64());
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
        let (hw, hh) = (self.size.0.as_f64() / 2.0, self.size.1.as_f64() / 2.0);
        let (dx, dy) = (((p.x - self.center.x).as_f64()).abs(), ((p.y - self.center.y).as_f64()).abs());
        let (ox, oy) = ((dx - hw).max(0.0), (dy - hh).max(0.0));
        let outside = (ox * ox + oy * oy).sqrt();
        if outside > 0.0 { outside } else { (dx - hw).max(dy - hh) }
    }

    /// True when `p` lies inside the copper with at least `margin` nm to
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
pub fn to_board(fp: &FootprintInstance, local: (Nm, Nm)) -> Point {
    let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let mirror = if fp.side == Side::Bottom { -1.0 } else { 1.0 };
    let lx = local.0.as_f64() * mirror;
    let ly = local.1.as_f64();
    Point { x: fp.at.x + Nm::kiround(lx * cos - ly * sin), y: fp.at.y + Nm::kiround(lx * sin + ly * cos) }
}

/// Axis-aligned extents of a local (w, h) box rotated by `rot_millideg`.
/// Exact for multiples of 90°, conservative otherwise.
fn rotated_extent_by(rot_millideg: i64, size: (Nm, Nm)) -> (Nm, Nm) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0.as_f64(), size.1.as_f64());
    (Nm::kiround(w * cos.abs() + h * sin.abs()), Nm::kiround(w * sin.abs() + h * cos.abs()))
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
pub fn rotated_extent(fp: &FootprintInstance, size: (Nm, Nm)) -> (Nm, Nm) {
    rotated_extent_by(fp.rot as i64, size)
}

/// Every pad of `fp` in board space, sorted by pad number -- **not**
/// necessarily unique: see [`PlacedPad::number`]. `None` when the part has
/// no resolvable footprint.
pub fn placed_pads(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<Vec<PlacedPad>> {
    let footprint = model.footprint_of(part)?;
    let mut pads: Vec<PlacedPad> = footprint.pads.iter().map(|p| placed_pad_of(fp, p)).collect();
    pads.sort_by(|a, b| a.number.cmp(&b.number));
    Some(pads)
}

/// The pad `p` of `fp` in board space. The pad's own rotation is relative to the footprint, applied before the footprint's own rotation/mirror -- see
/// `Pad::rot`. A pad whose copper differs from layer to layer ([`crate::padstack`]) is one box and one set of pieces that hold the copper of every layer, so a
/// consumer that knows nothing of layers keeps clear of all of it; the ones that do use [`Pad::shape_groups`] and this function for each layer's shape.
pub fn placed_pad_of(fp: &FootprintInstance, p: &Pad) -> PlacedPad {
    if p.has_stack() {
        let mut size = (Nm::ZERO, Nm::ZERO);
        let mut pieces = Vec::new();
        for shape in p.unique_shapes() {
            let v = shape.lay_on(p);
            let e = placed_extent(fp, &v);
            size = (size.0.max(e.0), size.1.max(e.1));
            let frame = crate::pad_geom::PadFrame::of(fp, &v, to_board(fp, v.at));
            pieces.extend(crate::pad_geom::pad_pieces(&v).iter().map(|q| q.map(&|w| frame.to_board(w))));
        }
        return PlacedPad { number: p.number.clone(), center: to_board(fp, p.at), size, through_hole: p.kind != PadKind::Smd, shape: p.shape, roundrect_ratio: p.roundrect_ratio, pieces };
    }
    PlacedPad {
        number: p.number.clone(),
        center: to_board(fp, p.at),
        size: placed_extent(fp, p),
        through_hole: p.kind != PadKind::Smd,
        shape: p.shape,
        roundrect_ratio: p.roundrect_ratio,
        pieces: placed_pieces(fp, p),
    }
}

/// The extents of pad `p` of `fp` about the pad's position: its size turned by the pad's and the footprint's rotation. A trapezoid, a
/// chamfered rectangle, a custom pad and a pad whose copper is off its position are measured by their true outline
/// ([`crate::pad_geom::pad_pieces`]), so the box the placer and the grid router keep clear always holds the copper.
fn placed_extent(fp: &FootprintInstance, p: &Pad) -> (Nm, Nm) {
    let pieces = placed_pieces(fp, p);
    if pieces.is_empty() {
        return rotated_extent_by(fp.rot as i64 + p.rot as i64, p.size);
    }
    let centre = to_board(fp, p.at);
    let Some((x0, y0, x1, y1)) = crate::pad_geom::pieces_bbox(&pieces) else { return rotated_extent_by(fp.rot as i64 + p.rot as i64, p.size) };
    let (cx, cy) = (centre.x.as_f64(), centre.y.as_f64());
    (2 * Nm::new((x1 - cx).max(cx - x0).ceil() as i64), 2 * Nm::new((y1 - cy).max(cy - y0).ceil() as i64))
}

/// The copper of pad `p` of `fp` in board space as pieces ([`crate::pad_geom::pad_pieces`]) when its shape and size do not give it (a
/// trapezoid, a chamfered rectangle, a custom pad, copper off the pad's position); empty for the rest.
pub fn placed_pieces(fp: &FootprintInstance, p: &Pad) -> Vec<crate::pad_geom::PadPiece> {
    if !(p.has_complex_outline() || p.ext.offset.is_some()) {
        return Vec::new();
    }
    let frame = crate::pad_geom::PadFrame::of(fp, p, to_board(fp, p.at));
    crate::pad_geom::pad_pieces(p).iter().map(|q| q.map(&|v| frame.to_board(v))).collect()
}

/// Board-space courtyard rectangle `(min_x, min_y, max_x, max_y)` of `fp`.
/// Refdes silkscreen font size for a board: 1/40 of the shorter rendered
/// side (outline bbox plus a 2 mm margin), never under 0.6 mm. Shared by
/// the judge renderer, the gates, the router keep-outs and the placers so
/// every stage agrees on where the label is.
pub fn refdes_font_nm(outline: &[Point]) -> Nm {
    let (mut x0, mut y0, mut x1, mut y1) = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
    for p in outline {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if outline.is_empty() {
        return Nm::new(600_000);
    }
    let m = 2_000_000;
    let (vw, vh) = (x1 - x0 + Nm::new(2 * m), y1 - y0 + Nm::new(2 * m));
    (vw.min(vh) / 40).clamp(Nm::new(600_000), Nm::new(1_000_000))
}

/// Refdes label box for a placed courtyard `(x0, y0, x1, y1)`: the text
/// is centred horizontally just above the courtyard's top edge — or just
/// below it when "above" would leave the board (`board_top` is the
/// outline's minimum y; pass `Nm::MIN` to always label above, e.g. for
/// position-independent node geometry). Pure geometry so placers can
/// evaluate candidate poses without a model.
pub fn refdes_box_for(courtyard: (Nm, Nm, Nm, Nm), refdes: &str, font_nm: Nm, board_top: Nm) -> (Nm, Nm, Nm, Nm) {
    refdes_box_side(courtyard, refdes, font_nm, board_top, LabelSide::Above)
}

/// [`refdes_box_for`] with an explicit side. `Above` still flips below at
/// the board top; `Below` is always below.
pub fn refdes_box_side(courtyard: (Nm, Nm, Nm, Nm), refdes: &str, font_nm: Nm, board_top: Nm, side: LabelSide) -> (Nm, Nm, Nm, Nm) {
    let (cx0, cy0, cx1, cy1) = courtyard;
    let half_w = (font_nm * 6 / 10) * refdes.chars().count() as i64 / 2;
    let cx = (cx0 + cx1) / 2;
    let (asc, desc) = (font_nm * 3 / 4, font_nm / 5);
    let above = cy0 - Nm::new(200_000) - asc;
    match side {
        LabelSide::Above if above >= board_top => {
            let baseline = cy0 - Nm::new(200_000);
            (cx - half_w, baseline - asc, cx + half_w, baseline + desc)
        }
        LabelSide::Above | LabelSide::Below => {
            let baseline = cy1 + Nm::new(200_000) + asc;
            (cx - half_w, baseline - asc, cx + half_w, baseline + desc)
        }
        LabelSide::Left | LabelSide::Right => {
            // Beside the courtyard, text still horizontal, vertically
            // centred on the part.
            let cy = (cy0 + cy1) / 2;
            let baseline = cy + asc / 2;
            let (x0, x1) = if side == LabelSide::Left { (cx0 - Nm::new(200_000) - 2 * half_w, cx0 - Nm::new(200_000)) } else { (cx1 + Nm::new(200_000), cx1 + Nm::new(200_000) + 2 * half_w) };
            (x0, baseline - asc, x1, baseline + desc)
        }
    }
}

/// Text baseline y for a label box from [`refdes_box_for`].
pub fn refdes_baseline(bx: (Nm, Nm, Nm, Nm), font_nm: Nm) -> Nm {
    bx.3 - font_nm / 5
}

/// Minimum y of an outline (`Nm::MIN` for an empty one).
pub fn outline_top(outline: &[Point]) -> Nm {
    outline.iter().map(|p| p.y).min().unwrap_or(Nm::MIN)
}

/// Courtyard plus refdes label: the rectangle a part actually needs kept
/// free of other parts so its label stays readable and no neighbour's pad
/// ends up under it (which walls that pad in for routing).
pub fn keepout_for(courtyard: (Nm, Nm, Nm, Nm), refdes: &str, font_nm: Nm, board_top: Nm) -> (Nm, Nm, Nm, Nm) {
    keepout_side(courtyard, refdes, font_nm, board_top, LabelSide::Above)
}

/// [`keepout_for`] with an explicit label side.
pub fn keepout_side(courtyard: (Nm, Nm, Nm, Nm), refdes: &str, font_nm: Nm, board_top: Nm, side: LabelSide) -> (Nm, Nm, Nm, Nm) {
    let l = refdes_box_side(courtyard, refdes, font_nm, board_top, side);
    (courtyard.0.min(l.0), courtyard.1.min(l.1), courtyard.2.max(l.2), courtyard.3.max(l.3))
}

/// Placed refdes label box (see [`refdes_box_for`]).
pub fn placed_refdes_box(model: &ConstraintModel, outline: &[Point], part: &Part, fp: &FootprintInstance) -> Option<(Nm, Nm, Nm, Nm)> {
    Some(refdes_box_side(placed_courtyard(model, part, fp)?, &fp.id, model.board.refdes_font(outline), outline_top(outline), fp.label))
}

/// Placed courtyard-plus-label keep-out (see [`keepout_for`]).
pub fn placed_keepout(model: &ConstraintModel, outline: &[Point], part: &Part, fp: &FootprintInstance) -> Option<(Nm, Nm, Nm, Nm)> {
    Some(keepout_side(placed_courtyard(model, part, fp)?, &fp.id, model.board.refdes_font(outline), outline_top(outline), fp.label))
}

pub fn placed_courtyard(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<(Nm, Nm, Nm, Nm)> {
    let footprint = model.footprint_of(part)?;
    let (hw, hh) = footprint.courtyard_half();
    let (w, h) = rotated_extent(fp, (hw * 2, hh * 2));
    Some((fp.at.x - w / 2, fp.at.y - h / 2, fp.at.x + w / 2, fp.at.y + h / 2))
}

/// The box of the placed footprint's courtyard as KiCad has it: `FOOTPRINT::GetBoundingBox` merges the footprint's `F.CrtYd`/`B.CrtYd`
/// drawings as they are, so an off-centre courtyard (a pin header with its origin at pin 1) gives an off-centre box. That is the real
/// outlines ([`Footprint::courtyard_outlines`]) put on the board, else the rectangle the board file is written with (`courtyard_half`,
/// symmetric, turned with the footprint: the box of its four corners). [`placed_courtyard`] is the placer's symmetric box. Each drawing's box is grown by half its
/// line (`PCB_SHAPE::GetBoundingBox`): the library's courtyard lines' widest, else KiCad's 0.05 mm courtyard line, which the writer draws the box with.
pub fn placed_courtyard_box(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<(Nm, Nm, Nm, Nm)> {
    let footprint = model.footprint_of(part)?;
    let pts: Vec<Point> = if footprint.courtyard_outlines.iter().any(|o| !o.is_empty()) {
        footprint.courtyard_outlines.iter().flatten().map(|&p| to_board(fp, p)).collect()
    } else {
        let (hw, hh) = footprint.courtyard_half();
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].into_iter().map(|p| to_board(fp, p)).collect()
    };
    let half = footprint.art.courtyard.iter().map(|s| s.shape.stroke_width()).max().unwrap_or(Nm::new(50_000)) / 2;
    let (x0, y0) = (pts.iter().map(|p| p.x).min()? - half, pts.iter().map(|p| p.y).min()? - half);
    let (x1, y1) = (pts.iter().map(|p| p.x).max()? + half, pts.iter().map(|p| p.y).max()? + half);
    Some((x0, y0, x1, y1))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    pub name: String,
    #[serde(default)]
    pub pads: Vec<Pad>,
    /// Courtyard half-extents (w, h) in nm, centred on the origin. When
    /// absent the placer derives one from the pad bounding box plus margin.
    ///
    /// Always symmetric about the origin: a real footprint's F.CrtYd can be
    /// off-centre (KiCad has no such restriction), and a library footprint
    /// loaded with an asymmetric courtyard is conservatively *enclosed* by
    /// the smallest symmetric box that contains it, not represented
    /// exactly. Extending this to an offset rect is future work.
    #[serde(default)]
    #[serde(deserialize_with = "crate::units::de")]
    pub courtyard: Option<(Nm, Nm)>,
    /// The real courtyard (`FOOTPRINT::GetCourtyard`): closed outlines in
    /// the footprint's local frame (same frame as `Pad::at`), built from a
    /// KiCad footprint's `F.CrtYd`/`B.CrtYd` graphics. Empty when unknown
    /// (built-in footprints, or a KiCad footprint without courtyard
    /// graphics), in which case `courtyard`/`courtyard_half` stand in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(deserialize_with = "crate::units::de")]
    pub courtyard_outlines: Vec<Vec<(Nm, Nm)>>,
    /// KiCad `(model "...")` path for this footprint's 3D model, e.g.
    /// `"${KICAD9_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl"`.
    /// `None` when no model is known (this footprint predates 3D model
    /// support, or nothing in the built-in package map matches it) -- the
    /// 3D viewer falls back to its own procedural box for those.
    #[serde(default)]
    pub model: Option<String>,
    /// Every `(model ...)` of the footprint with the placement KiCad gives it (`FP_3DMODEL`: offset, scale, rotation, opacity, hidden), as the footprint
    /// file writes them. Empty = nothing but `model`'s path, placed as it comes (offset 0, scale 1, rotation 0): a footprint of an older design, or a
    /// table entry. When it is not empty it is the whole list and `model` is only its first path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models3d: Vec<Model3d>,
    /// The silkscreen, the fab body, the assembly drawings and the texts of the footprint (`fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly`, `fp_curve`,
    /// `fp_text user`), in the footprint's frame: [`crate::fp_art`]. Empty for a built-in footprint, and for a design from before this existed.
    #[serde(default, skip_serializing_if = "crate::fp_art::FpArt::is_empty")]
    pub art: crate::fp_art::FpArt,
}

/// One `(model "path" (offset (xyz ..)) (scale (xyz ..)) (rotate (xyz ..)))` of a footprint: KiCad's `FP_3DMODEL` (`pcbnew/footprint.h`).
/// The placement is applied to the model's own frame (millimetres, z up from the footprint's mounting surface) before the footprint's position,
/// rotation and side: `T(offset) * Rz(-rotate.z) * Ry(-rotate.y) * Rx(-rotate.x) * S(scale)`
/// (`RENDER_3D_OPENGL::get3dModelsFromFootprint`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model3d {
    /// The path the footprint file writes, `${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`.
    pub path: String,
    /// `(offset (xyz x y z))`, millimetres. A legacy `(at (xyz ..))` is inches and is converted when it is read.
    #[serde(default = "zero3", skip_serializing_if = "is_zero3")]
    pub offset: [f64; 3],
    /// `(scale (xyz x y z))`.
    #[serde(default = "one3", skip_serializing_if = "is_one3")]
    pub scale: [f64; 3],
    /// `(rotate (xyz x y z))`, degrees.
    #[serde(default = "zero3", skip_serializing_if = "is_zero3")]
    pub rotate: [f64; 3],
    /// `(opacity v)`: 1 is opaque.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f64,
    /// `(hide yes)` clears it: KiCad does not draw a model that is not shown.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub show: bool,
}

fn zero3() -> [f64; 3] {
    [0.0; 3]
}
fn one3() -> [f64; 3] {
    [1.0; 3]
}
fn one() -> f64 {
    1.0
}
fn yes() -> bool {
    true
}
fn is_zero3(v: &[f64; 3]) -> bool {
    *v == [0.0; 3]
}
fn is_one3(v: &[f64; 3]) -> bool {
    *v == [1.0; 3]
}
fn is_one(v: &f64) -> bool {
    *v == 1.0
}
fn is_yes(v: &bool) -> bool {
    *v
}

impl Model3d {
    /// A model placed as it comes: no offset, no rotation, scale 1.
    pub fn identity(path: impl Into<String>) -> Self {
        Model3d { path: path.into(), offset: [0.0; 3], scale: [1.0; 3], rotate: [0.0; 3], opacity: 1.0, show: true }
    }

    /// The same model turned over onto the other side of the board, in this studio's convention for a bottom-side footprint.
    ///
    /// The studio keeps a footprint in the frame of the part seen from the top and mirrors x for a bottom-side instance (`to_board`), where KiCad
    /// stores a flipped footprint mirrored in y and turns its 3D model with `Ry(pi) Rz(pi)` (`get3dModelsFromFootprint`: a half turn about x). The two
    /// differ by a half turn about z, so a model that sits right on a top footprint sits half a turn off on a bottom one unless that turn is put in
    /// the model's own placement: `offset (x, y, z) -> (-x, -y, z)` and `rotate.z - 180`. A half turn twice is none, so the same call converts a
    /// placement read from a KiCad board file (a bottom-side footprint) to the studio's frame and back again when the board is written.
    pub fn flipped_frame(&self) -> Model3d {
        Model3d { offset: [-self.offset[0], -self.offset[1], self.offset[2]], rotate: [self.rotate[0], self.rotate[1], wrap_deg(self.rotate[2] - 180.0)], ..self.clone() }
    }
}

/// An angle in degrees brought into (-180, 180], so a half turn read back is the one written (`0 - 180` is `-180`, which is `180`).
fn wrap_deg(a: f64) -> f64 {
    let m = a.rem_euclid(360.0);
    if m > 180.0 {
        m - 360.0
    } else {
        m
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pad {
    /// Not unique within a footprint: several pads may share one number --
    /// one logical pin, several physical copper pads (a connector's
    /// shield, a split thermal pad). All of them go on that pin's net; see
    /// `placed_pads`.
    pub number: String,
    /// Centre in the local frame, nm.
    #[serde(deserialize_with = "crate::units::de")]
    pub at: (Nm, Nm),
    /// Full (width, height) in nm, un-rotated.
    #[serde(deserialize_with = "crate::units::de")]
    pub size: (Nm, Nm),
    #[serde(default)]
    pub shape: PadShape,
    #[serde(default)]
    pub kind: PadKind,
    /// Round drill diameter, nm. Only meaningful for `PadKind::ThroughHole`/
    /// `NonPlatedHole`, and mutually exclusive with `drill_slot`.
    #[serde(default)]
    #[serde(deserialize_with = "crate::units::de")]
    pub drill: Option<Nm>,
    /// Slot (oval) drill (width, height), nm -- KiCad's `(drill oval w h)`,
    /// e.g. a connector shield tab's mounting leg. When set, this pad's
    /// hole is a slot and `drill` must be absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub drill_slot: Option<(Nm, Nm)>,
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
    /// The offset of the copper, a trapezoid's deltas, a chamfered rectangle's corners and a custom pad's primitives.
    #[serde(default, skip_serializing_if = "PadShapeExt::is_default")]
    pub ext: PadShapeExt,
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
            if p.size.0 <= Nm::ZERO || p.size.1 <= Nm::ZERO {
                bad(&format!("pad {}", p.number), format!("pad is {} x {} nm", p.size.0, p.size.1));
            }
            // Each copper layer's own shape (`PAD::CheckPad` runs `doCheckPad` for every unique layer of the padstack).
            for shape in p.unique_shapes() {
                if let Some(r) = shape.roundrect_ratio {
                    if !(0.0..=0.5).contains(&r) {
                        bad(&format!("pad {}", p.number), format!("roundrect ratio {r} is outside 0.0..=0.5 (a fraction of the shorter side)"));
                    }
                }
                // `PAD::doCheckPad`: a chamfer cannot be negative nor cut more than the side there is; a trapezoid cannot narrow a side past nothing.
                if shape.shape == PadShape::ChamferedRect {
                    let r = shape.chamfer_ratio.unwrap_or(0.2);
                    let adjacent = {
                        let c = shape.chamfer_corners;
                        (c.top_left && (c.top_right || c.bottom_left)) || (c.bottom_right && (c.top_right || c.bottom_left))
                    };
                    if r < 0.0 {
                        bad(&format!("pad {}", p.number), "negative corner chamfer is not allowed".into());
                    } else if r > if adjacent { 0.5 } else { 1.0 } {
                        bad(&format!("pad {}", p.number), "corner chamfer is too large".into());
                    }
                }
                if shape.shape == PadShape::Trapezoid {
                    if let Some((dx, dy)) = shape.trapezoid_delta {
                        if dx.abs() > shape.size.1 || dy.abs() > shape.size.0 {
                            bad(&format!("pad {}", p.number), "trapezoid delta is too large".into());
                        }
                    }
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
                    (Some(d), None) if d <= Nm::ZERO => bad(&format!("pad {}", p.number), format!("drills a {d} nm hole")),
                    (Some(d), None) if plated && d >= p.size.0.min(p.size.1) => bad(
                        &format!("pad {}", p.number),
                        format!("drill {d} nm is not smaller than the {} x {} nm pad, so there is no annular ring", p.size.0, p.size.1),
                    ),
                    (None, Some((w, h))) if w <= Nm::ZERO || h <= Nm::ZERO => bad(&format!("pad {}", p.number), format!("slot drill is {w} x {h} nm")),
                    (None, Some((w, h))) if plated && (w >= p.size.0 || h >= p.size.1) => bad(
                        &format!("pad {}", p.number),
                        format!("slot drill {w} x {h} nm is not smaller than the {} x {} nm pad, so there is no annular ring", p.size.0, p.size.1),
                    ),
                    _ => {}
                }
            }
        }
        if let Some((w, h)) = self.courtyard {
            if w <= Nm::ZERO || h <= Nm::ZERO {
                bad("courtyard", format!("courtyard half-extents are {w} x {h} nm"));
            }
        }
        out
    }
}

/// `PAD_SHAPE` (`pcbnew/padstack.h`). A pad's size is its bounding box before the pad's own rotation: for a trapezoid or a chamfered
/// rectangle that is the rectangle the shape is cut from, for a custom pad the size of its anchor (the primitives are laid over it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    #[default]
    Rect,
    RoundRect,
    Circle,
    Oval,
    /// `PAD_SHAPE::TRAPEZOID`: a rectangle whose opposite sides differ in length by [`PadShapeExt::trapezoid_delta`].
    Trapezoid,
    /// `PAD_SHAPE::CHAMFERED_RECT`: a rectangle with the corners of [`PadShapeExt::chamfer_corners`] cut off (by
    /// [`PadShapeExt::chamfer_ratio`] of the shorter side) and the others rounded by `roundrect_ratio`. The file writes it as `roundrect`.
    ChamferedRect,
    /// `PAD_SHAPE::CUSTOM`: an anchor (a circle or a rectangle of the pad's size) and the primitives of [`PadCustom`].
    Custom,
}

/// `PAD::GetAnchorPadShape`: a custom pad's anchor is one of the two basic shapes (`(options (anchor circle|rect))`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadAnchor {
    #[default]
    Circle,
    Rect,
}

/// `CUSTOM_SHAPE_ZONE_MODE`: what a zone keeps clear of for a custom pad -- the pad's shape, or the convex hull of it
/// (`(options (clearance outline|convexhull))`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomZoneMode {
    #[default]
    Outline,
    ConvexHull,
}

/// One basic shape of a custom pad (a `PCB_SHAPE` in `PAD::GetPrimitives`): `gr_line`, `gr_arc`, `gr_circle`, `gr_rect`, `gr_poly` or
/// `gr_curve`, in the pad's own frame (the pad's position is the origin, its rotation not applied). Stroked at `stroke_width`, and
/// filled for a circle, a rectangle or a polygon when `filled`. Reuses [`crate::ir::Shape`]: its `layer` and `id` mean nothing here
/// and stay empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadPrimitive {
    pub shape: crate::ir::Shape,
    /// `PCB_SHAPE::IsProxyItem`: a `gr_bbox` (the box the pad's number is placed in) or a `gr_vector` (a thermal spoke template).
    /// Not copper.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proxy: bool,
}

/// What a custom pad adds to its anchor (`PADSTACK` of a `CUSTOM` pad, `(options ..)` and `(primitives ..)`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadCustom {
    #[serde(default)]
    pub anchor: PadAnchor,
    #[serde(default)]
    pub zone: CustomZoneMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primitives: Vec<PadPrimitive>,
}

/// The rest of one layer of a pad's `PADSTACK` (`padstack.h`, `COPPER_LAYER_PROPS`) beyond the shape, the size and the corner
/// ratio the pad has fields for: the offset of the copper from the pad's position, a trapezoid's deltas, a chamfered rectangle's
/// ratio and corners, and a custom pad's anchor and primitives. All of it in the footprint's frame as seen from the top, like
/// `Pad::at` (a bottom-side footprint mirrors x when it is placed, and the file mirrors it back).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadShapeExt {
    /// `PAD::GetOffset`: where the copper's centre is from the pad's position (where the hole is), in the pad's own frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub offset: Option<(Nm, Nm)>,
    /// `PAD::GetDelta` (`(rect_delta dx dy)`): a trapezoid's difference between its two parallel sides, KiCad keeps one of the two
    /// zero. `dx` narrows the sides left and right (the top and bottom ones differ), `dy` the top and bottom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub trapezoid_delta: Option<(Nm, Nm)>,
    /// `PAD::GetChamferRectRatio` (`(chamfer_ratio r)`): the size of the cut as a fraction of the shorter side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer_ratio: Option<f64>,
    /// `PAD::GetChamferPositions` (`(chamfer top_left ..)`).
    #[serde(default, skip_serializing_if = "crate::ir::ChamferCorners::is_none")]
    pub chamfer_corners: crate::ir::ChamferCorners,
    /// The anchor and primitives of a custom pad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<PadCustom>,
    /// The padstack: the shape the pad has on its other layers (`PADSTACK::MODE`) and what becomes of the layers nothing connects to
    /// (`UNCONNECTED_LAYER_MODE`). The fields above and [`Pad::shape`], [`Pad::size`] and [`Pad::roundrect_ratio`] are the front shape; see
    /// [`crate::padstack`]. Default: one shape on every layer, every layer kept.
    #[serde(default, skip_serializing_if = "crate::padstack::PadStack::is_default")]
    pub stack: crate::padstack::PadStack,
}

impl PadShapeExt {
    /// Nothing beyond the plain shape (the field is then not written).
    pub fn is_default(&self) -> bool {
        *self == PadShapeExt::default()
    }
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
    pub fn pad_bbox(&self) -> (Nm, Nm, Nm, Nm) {
        let mut b = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
        for pad in &self.pads {
            // Every copper layer's shape of a padstack ([`crate::padstack`]); a pad of one shape is that shape.
            let views: Vec<Pad> = if pad.has_stack() { pad.unique_shapes().iter().map(|s| s.lay_on(pad)).collect() } else { vec![pad.clone()] };
            for p in &views {
                // A trapezoid, a custom pad or copper off the pad's position reaches where its outline does, not where its size says.
                let (x0, y0, x1, y1) = if p.has_complex_outline() || p.ext.offset.is_some() {
                    let (x0, y0, x1, y1) = p.local_bbox();
                    (p.at.0 + Nm::new(x0.floor() as i64), p.at.1 + Nm::new(y0.floor() as i64), p.at.0 + Nm::new(x1.ceil() as i64), p.at.1 + Nm::new(y1.ceil() as i64))
                } else {
                    (p.at.0 - p.size.0 / 2, p.at.1 - p.size.1 / 2, p.at.0 + p.size.0 / 2, p.at.1 + p.size.1 / 2)
                };
                b.0 = b.0.min(x0);
                b.1 = b.1.min(y0);
                b.2 = b.2.max(x1);
                b.3 = b.3.max(y1);
            }
        }
        if self.pads.is_empty() {
            (Nm::ZERO, Nm::ZERO, Nm::ZERO, Nm::ZERO)
        } else {
            b
        }
    }

    /// Courtyard half-extents about the origin: explicit, else the pad
    /// bbox's largest reach from the origin + 0.25 mm margin (so an
    /// asymmetric pad layout still gets a courtyard that covers it).
    pub fn courtyard_half(&self) -> (Nm, Nm) {
        if let Some(c) = self.courtyard {
            return c;
        }
        let (x0, y0, x1, y1) = self.pad_bbox();
        let hw = x0.abs().max(x1.abs()) + Nm::new(250_000);
        let hh = y0.abs().max(y1.abs()) + Nm::new(250_000);
        (hw.max(Nm::new(250_000)), hh.max(Nm::new(250_000)))
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
    fn simple(number: impl Into<String>, at: (Nm, Nm), size: (Nm, Nm), shape: PadShape, kind: PadKind, drill: Option<Nm>) -> Self {
        Pad { opposite_side: false, ext: Default::default(), number: number.into(), at, size, shape, kind, drill, drill_slot: None, rot: 0, roundrect_ratio: None }
    }
}

fn two_pad(name: &str, pitch: Nm, pw: Nm, ph: Nm) -> Footprint {
    // Half the pitch, to the whole micrometre (0805's 1.825 mm pitch puts its pads 0.912 mm out), as this library has always placed them.
    let half = pitch / 2 / crate::units::NM_PER_UM * crate::units::NM_PER_UM;
    Footprint {
        name: name.into(),
        pads: vec![
            Pad::simple("1", (-half, Nm::ZERO), (pw, ph), PadShape::RoundRect, PadKind::Smd, None),
            Pad::simple("2", (half, Nm::ZERO), (pw, ph), PadShape::RoundRect, PadKind::Smd, None),
        ],
        courtyard: None,
        model: None,
        courtyard_outlines: vec![],
        models3d: vec![],
        art: Default::default(),
    }
}

/// Dual-column gull-wing package (SOIC/TSSOP/MSOP-style): `n` pins total in
/// two columns at x = ∓`col_x`, pitch `pitch` along y. Pin 1 is at the top
/// of the left column (min y), numbering increases going down the left
/// column, then continues up the right column from bottom to top — matching
/// KiCad's standard SOIC/TSSOP footprint numbering.
fn dual_row(name: &str, n: usize, pitch: Nm, col_x: Nm, pw: Nm, ph: Nm) -> Footprint {
    let per_col = n / 2;
    let y0 = -((per_col as i64 - 1) * pitch) / 2;
    let mut pads = Vec::with_capacity(n);
    for i in 0..per_col {
        pads.push(Pad::simple((i + 1).to_string(), (-col_x, y0 + i as i64 * pitch), (pw, ph), PadShape::RoundRect, PadKind::Smd, None));
    }
    for i in 0..per_col {
        pads.push(Pad::simple((per_col + i + 1).to_string(), (col_x, y0 + (per_col - 1 - i) as i64 * pitch), (pw, ph), PadShape::RoundRect, PadKind::Smd, None));
    }
    Footprint { name: name.into(), pads, courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
}

/// Single-row 2.54 mm through-hole header, `n` pins along +x, centred.
fn pin_header(name: &str, n: usize) -> Footprint {
    let x0 = -((n as i64 - 1) * Nm::new(2_540_000)) / 2;
    let pads = (0..n)
        .map(|i| Pad::simple((i + 1).to_string(), (x0 + i as i64 * Nm::new(2_540_000), Nm::ZERO), (Nm::new(1_700_000), Nm::new(1_700_000)), if i == 0 { PadShape::Rect } else { PadShape::Circle }, PadKind::ThroughHole, Some(Nm::new(1_000_000))))
        .collect();
    Footprint { name: name.into(), pads, courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
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
/// patterns as used by the KiCad standard libraries, to the micrometre (KiCad's SOT-23 column at 0.9375 mm is 0.938 here): an
/// approximation of the library, kept as it was, so a board placed with it before keeps its pads where they were and the FreeRouting
/// dumps of `crates/freeroute/tests/design` stay the boards of these values.
pub fn builtin(name: &str) -> Option<Footprint> {
    let key = normalize_name(name);
    let fp = match key.as_str() {
        // Two-pad passives: pitch/size taken from KiCad's Resistor_SMD.pretty
        // (canonical for the shared two-pad shape; nearly identical to
        // Capacitor_SMD).
        "0201" => two_pad(&key, Nm::new(640_000), Nm::new(460_000), Nm::new(400_000)),
        "0402" => two_pad(&key, Nm::new(1_020_000), Nm::new(540_000), Nm::new(640_000)),
        "0603" => two_pad(&key, Nm::new(1_650_000), Nm::new(800_000), Nm::new(950_000)),
        "0805" => two_pad(&key, Nm::new(1_825_000), Nm::new(1_025_000), Nm::new(1_400_000)),
        "1206" => two_pad(&key, Nm::new(2_925_000), Nm::new(1_125_000), Nm::new(1_750_000)),
        "1210" => two_pad(&key, Nm::new(2_925_000), Nm::new(1_125_000), Nm::new(2_650_000)),
        "SOD-123" => two_pad(&key, Nm::new(3_300_000), Nm::new(900_000), Nm::new(1_200_000)),
        "SOD-323" => two_pad(&key, Nm::new(2_100_000), Nm::new(600_000), Nm::new(450_000)),
        "SMA" => two_pad(&key, Nm::new(4_000_000), Nm::new(2_500_000), Nm::new(1_800_000)),
        // Package_TO_SOT_SMD.pretty/SOT-23.kicad_mod: pins 1,2 on the left
        // column (top/bottom), pin 3 alone on the right column (middle).
        "SOT-23" | "SOT-23-3" => Footprint {
            name: "SOT-23".into(),
            pads: vec![
                Pad::simple("1", (Nm::new(-938_000), Nm::new(-950_000)), (Nm::new(1_475_000), Nm::new(600_000)), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("2", (Nm::new(-938_000), Nm::new(950_000)), (Nm::new(1_475_000), Nm::new(600_000)), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("3", (Nm::new(938_000), Nm::ZERO), (Nm::new(1_475_000), Nm::new(600_000)), PadShape::RoundRect, PadKind::Smd, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        },
        // Package_TO_SOT_SMD.pretty/SOT-23-5.kicad_mod / SOT-23-6.kicad_mod:
        // both columns at x = ∓1.1375mm, pitch 0.95mm. SOT-23-5 omits the
        // right-column middle pad (built as a 6-pad dual_row, then pin 5 —
        // the right-middle pad — dropped and pin 6 renumbered to 5).
        "SOT-23-5" | "SOT-23-6" => {
            let n = if key == "SOT-23-5" { 5 } else { 6 };
            let mut fp = dual_row(&key, 6, Nm::new(950_000), Nm::new(1_138_000), Nm::new(1_325_000), Nm::new(600_000));
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
                Pad::simple("1", (Nm::new(-3_150_000), Nm::new(-2_300_000)), (Nm::new(2_000_000), Nm::new(1_500_000)), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("2", (Nm::new(3_150_000), Nm::ZERO), (Nm::new(2_000_000), Nm::new(3_800_000)), PadShape::RoundRect, PadKind::Smd, None),
                Pad::simple("3", (Nm::new(-3_150_000), Nm::new(2_300_000)), (Nm::new(2_000_000), Nm::new(1_500_000)), PadShape::RoundRect, PadKind::Smd, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        },
        // Package_SO.pretty SOIC-8/14/16_3.9x*mm_P1.27mm.
        "SOIC-8" => dual_row(&key, 8, Nm::new(1_270_000), Nm::new(2_475_000), Nm::new(1_950_000), Nm::new(600_000)),
        "SOIC-14" => dual_row(&key, 14, Nm::new(1_270_000), Nm::new(2_475_000), Nm::new(1_950_000), Nm::new(600_000)),
        "SOIC-16" => dual_row(&key, 16, Nm::new(1_270_000), Nm::new(2_475_000), Nm::new(1_950_000), Nm::new(600_000)),
        // Package_SO.pretty TSSOP-*_4.4x*mm_P0.65mm.
        "TSSOP-8" => dual_row(&key, 8, Nm::new(650_000), Nm::new(2_863_000), Nm::new(1_475_000), Nm::new(400_000)),
        "TSSOP-14" => dual_row(&key, 14, Nm::new(650_000), Nm::new(2_863_000), Nm::new(1_475_000), Nm::new(400_000)),
        "TSSOP-16" => dual_row(&key, 16, Nm::new(650_000), Nm::new(2_863_000), Nm::new(1_475_000), Nm::new(400_000)),
        "TSSOP-20" => dual_row(&key, 20, Nm::new(650_000), Nm::new(2_863_000), Nm::new(1_475_000), Nm::new(400_000)),
        // Package_SO.pretty MSOP-8_3x3mm_P0.65mm / MSOP-10_3x3mm_P0.5mm.
        "MSOP-8" => dual_row(&key, 8, Nm::new(650_000), Nm::new(2_113_000), Nm::new(1_625_000), Nm::new(400_000)),
        "MSOP-10" => dual_row(&key, 10, Nm::new(500_000), Nm::new(2_100_000), Nm::new(1_500_000), Nm::new(350_000)),
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
    /// The 3D model's file in the library's `.3dshapes` folder, without `.step`: the footprint's name, but for one that shares another's model
    /// (`SOT-223-3_TabPin2` has `SOT-223.step`).
    pub model: String,
}

impl KicadFootprintRef {
    fn new(library: &str, name: impl Into<String>) -> Self {
        let name = name.into();
        Self { library: library.to_string(), model: name.clone(), name }
    }

    /// `Lib:Name`, the footprint's id in KiCad's libraries.
    pub fn id(&self) -> String {
        format!("{}:{}", self.library, self.name)
    }

    /// KiCad's own `(model "...")` path for it: `${KICAD10_3DMODEL_DIR}` is the exact variable the installed KiCad 10.99.0 footprint files use (checked with
    /// `grep "(model " .../footprints/**/*.kicad_mod`); older releases name it `KICAD6_3DMODEL_DIR` ... `KICAD9_3DMODEL_DIR`, but `kicad-cli pcb export glb`
    /// resolves whichever variable its own version defines, so this only has to match the version that runs the export.
    pub fn model_path(&self) -> String {
        format!("${{KICAD10_3DMODEL_DIR}}/{}.3dshapes/{}.step", self.library, self.model)
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
        // the built-in SOT-223 is KiCad's three-pin one with the tab on pin 2 (its two pads "2" one), not `SOT-223`, whose tab is a pad 4
        "SOT-223" => Some(KicadFootprintRef { model: "SOT-223".into(), ..KicadFootprintRef::new("Package_TO_SOT_SMD", "SOT-223-3_TabPin2") }),
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

/// The one name a part's footprint goes by in every file written here -- the board's `(footprint "..")`, the derived symbol's Footprint field, what Update PCB from
/// Schematic compares (`BOARD_NETLIST_UPDATER::replaceFootprint`: `GetFPID()` against the netlist component's): the footprint the part names, as named (`Lib:Name`;
/// a bare name, which a board read from a file can have, stays bare: `LIB_ID::Format` writes the item name alone), else the installed KiCad footprint its package
/// stands for ([`kicad_footprint_of_part`]: `package: 0603` on an `R` is `Resistor_SMD:R_0603_1608Metric`), else the package's name. Empty for a part with no
/// footprint.
pub fn footprint_lib_id(part: &crate::Part) -> String {
    if let Some(named) = part.footprint.as_deref().filter(|f| !f.is_empty()) {
        return named.to_string();
    }
    if let Some(twin) = kicad_footprint_of_part(part) {
        return twin.id();
    }
    part.package.clone().unwrap_or_default()
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
        crate::Part { reference: reference.into(), mpn: None, lcsc: None, value: value.map(String::from), package: Some(package.into()), footprint: Some(package.into()), pins: vec![], body_nm: None, symbol: None, datasheet: None, edge: None }
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
        assert_eq!(hw, Nm::new(1_650_000 / 2 + 800_000 / 2 + 250_000));
        assert_eq!(hh, Nm::new(950_000 / 2 + 250_000));
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
            body_nm: None, symbol: None, datasheet: None,
            edge: None,
        };
        let model = crate::ConstraintModel { parts: vec![part.clone()], footprints: vec![footprint], ..Default::default() };
        (model, part)
    }

    fn fp_instance(rot: crate::ir::Millideg, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at: Point { x: Nm::new(10_000_000), y: Nm::new(10_000_000) }, rot, side, label: Default::default() }
    }

    /// `FOOTPRINT::GetBoundingBox` merges the courtyard drawings as they are: a `PinHeader_1x04_P2.54mm_Vertical` (origin at pin 1, courtyard
    /// y from -1.77 to 9.39 mm) has a box from -1.77 mm, not the symmetric -9.39 mm the placer's box is.
    #[test]
    fn an_off_centre_courtyard_gives_kicads_box_not_a_symmetric_one() {
        let ring = vec![(Nm::new(-1_770_000), Nm::new(-1_770_000)), (Nm::new(1_770_000), Nm::new(-1_770_000)), (Nm::new(1_770_000), Nm::new(9_390_000)), (Nm::new(-1_770_000), Nm::new(9_390_000))];
        let header = Footprint { name: "HDR".into(), pads: vec![Pad::simple("1", (Nm::ZERO, Nm::ZERO), (Nm::new(1_700_000), Nm::new(1_700_000)), PadShape::Rect, PadKind::ThroughHole, None)], courtyard: Some((Nm::new(1_770_000), Nm::new(9_390_000))), model: None, courtyard_outlines: vec![ring], models3d: vec![], art: Default::default() };
        let (model, part) = part_with("HDR", header);
        let (x, y) = (Nm::new(10_000_000), Nm::new(10_000_000));
        // grown by half the 0.05 mm courtyard line (r2-pcb-edit R8: `PCB_SHAPE::GetBoundingBox` counts the stroke)
        assert_eq!(placed_courtyard_box(&model, &part, &fp_instance(0, Side::Top)), Some((x - Nm::new(1_795_000), y - Nm::new(1_795_000), x + Nm::new(1_795_000), y + Nm::new(9_415_000))));
        // turned a quarter (clockwise on the screen, `to_board`), the long side points left
        assert_eq!(placed_courtyard_box(&model, &part, &fp_instance(90_000, Side::Top)), Some((x - Nm::new(9_415_000), y - Nm::new(1_795_000), x + Nm::new(1_795_000), y + Nm::new(1_795_000))));
        // the placer's box (the importer's symmetric enclosure) stays symmetric
        let sym = placed_courtyard(&model, &part, &fp_instance(0, Side::Top)).unwrap();
        assert_eq!(sym.1, y - Nm::new(9_390_000));
    }

    #[test]
    fn a_pads_own_rotation_composes_with_the_footprints() {
        // A 2x0.8 mm pad, rotated 90 degrees by itself inside an unrotated
        // footprint, must present the same board-space extent as an
        // unrotated pad inside a footprint rotated 90 degrees: rotation
        // composes additively regardless of which side contributes it.
        let mut fp = Footprint { name: "T".into(), pads: vec![Pad::simple("1", (Nm::ZERO, Nm::ZERO), (Nm::new(2_000_000), Nm::new(800_000)), PadShape::Rect, PadKind::Smd, None)], courtyard: None, model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        fp.pads[0].rot = 90_000;
        let (model, part) = part_with("T", fp.clone());
        let via_pad_rot = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();

        fp.pads[0].rot = 0;
        let (model2, part2) = part_with("T", fp);
        let via_fp_rot = placed_pads(&model2, &part2, &fp_instance(90_000, Side::Top)).unwrap();

        assert_eq!(via_pad_rot[0].size, (Nm::new(800_000), Nm::new(2_000_000)));
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
                Pad::simple("SH", (Nm::new(-4_000_000), Nm::new(-3_000_000)), (Nm::new(1_000_000), Nm::new(2_000_000)), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (Nm::new(-4_000_000), Nm::new(1_000_000)), (Nm::new(1_000_000), Nm::new(1_500_000)), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (Nm::new(4_000_000), Nm::new(-3_000_000)), (Nm::new(1_000_000), Nm::new(2_000_000)), PadShape::Oval, PadKind::ThroughHole, None),
                Pad::simple("SH", (Nm::new(4_000_000), Nm::new(1_000_000)), (Nm::new(1_000_000), Nm::new(1_500_000)), PadShape::Oval, PadKind::ThroughHole, None),
            ],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        };
        let (model, part) = part_with("CONN", shield);
        let pads = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();
        let shs: Vec<_> = pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shs.len(), 4, "every physical pad of the shared number must survive, not just one");
        let centers: std::collections::BTreeSet<(Nm, Nm)> = shs.iter().map(|p| (p.center.x, p.center.y)).collect();
        assert_eq!(centers.len(), 4, "the four pads must keep their own distinct positions");
    }

    #[test]
    fn roundrect_ratio_flows_into_placed_pads() {
        let fp = Footprint {
            name: "T".into(),
            pads: vec![{
                let mut p = Pad::simple("1", (Nm::ZERO, Nm::ZERO), (Nm::new(1_000_000), Nm::new(2_000_000)), PadShape::RoundRect, PadKind::Smd, None);
                p.roundrect_ratio = Some(0.4);
                p
            }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        };
        let (model, part) = part_with("T", fp);
        let pads = placed_pads(&model, &part, &fp_instance(0, Side::Top)).unwrap();
        assert_eq!(pads[0].corner_radius(), 0.4 * 1_000_000.0);
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
pub fn usable_edges(r: (Nm, Nm, Nm, Nm)) -> (bool, bool, bool, bool) {
    let (w, h) = (r.2 - r.0, r.3 - r.1);
    if w.as_f64() > 1.3 * h.as_f64() {
        (false, false, true, true)
    } else if h.as_f64() > 1.3 * w.as_f64() {
        (true, true, false, false)
    } else {
        (true, true, true, true)
    }
}

/// Gap from a connector courtyard to the nearest usable board edge (see
/// [`usable_edges`]); `bb` is the outline bounding box.
pub fn edge_connector_gap(r: (Nm, Nm, Nm, Nm), bb: (Nm, Nm, Nm, Nm)) -> Nm {
    let (l, rt, t, b) = (r.0 - bb.0, bb.2 - r.2, r.1 - bb.1, bb.3 - r.3);
    let (ul, ur, ut, ub) = usable_edges(r);
    [(ul, l), (ur, rt), (ut, t), (ub, b)].iter().filter(|(u, _)| *u).map(|(_, g)| *g).min().unwrap_or(l.min(rt).min(t).min(b))
}
