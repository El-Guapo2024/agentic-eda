//! A pad's copper outline in its own frame: `PAD::buildEffectiveShape` (`pcbnew/pad.cpp`) for every `PAD_SHAPE`, a trapezoid, a
//! chamfered rectangle (`TransformRoundChamferedRectToPolygon`, `libs/kimath/src/convert_basic_shapes_to_polygon.cpp`) and a custom
//! pad's anchor and primitives (`EDA_SHAPE::makeEffectiveShapes`, `common/eda_shape.cpp`).
//!
//! The pieces are neutral geometry (circles, stadiums, polygons, strokes), nanometres, in the pad's frame: the origin is the pad's
//! position, +x right, +y down, the pad's own rotation and the footprint's pose not applied. [`PadFrame`] carries a point from there
//! to the board. The DRC turns pieces into its `Shape`s, the zone filler into polygons, and the studio's state into the outline it
//! draws, so there is one place that knows what a pad is made of.

use crate::footprint::{Pad, PadAnchor, PadCustom, PadPrimitive, PadShape};
use crate::ir::{ChamferCorners, FootprintInstance, Point, Shape, Side, Nm};
use crate::outline::{arc_to_segment_count, Arc, MAX_ERROR_NM};

pub type P = (f64, f64);

/// One part of the compound `PAD::GetEffectiveShape` returns.
#[derive(Debug, Clone, PartialEq)]
pub enum PadPiece {
    /// `SHAPE_CIRCLE`.
    Circle { c: P, r: f64 },
    /// `SHAPE_SEGMENT`: the segment `a`-`b` with round ends of radius `r` (an oval pad, a stroked line).
    Stadium { a: P, b: P, r: f64 },
    /// `SHAPE_SIMPLE` / `SHAPE_RECT`: a filled polygon.
    Polygon(Vec<P>),
    /// `SHAPE_ARC` with a width: a stroked polyline (an arc, a circle's ring) of half width `r`.
    Strokes { pts: Vec<P>, r: f64 },
}

impl PadPiece {
    /// The bounding box `(x0, y0, x1, y1)` (`SHAPE::BBox( 0 )`), the half width included.
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        let pts: Vec<P>;
        let (points, r): (&[P], f64) = match self {
            PadPiece::Circle { c, r } => {
                pts = vec![*c];
                (&pts, *r)
            }
            PadPiece::Stadium { a, b, r } => {
                pts = vec![*a, *b];
                (&pts, *r)
            }
            PadPiece::Polygon(p) => (p, 0.0),
            PadPiece::Strokes { pts, r } => (pts, *r),
        };
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in points {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        (x0 - r, y0 - r, x1 + r, y1 + r)
    }

    /// The signed distance from `p` to the piece: negative inside it, the gap to its edge outside.
    pub fn signed_distance(&self, p: P) -> f64 {
        let dist = |a: P, b: P| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
            (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
        };
        match self {
            PadPiece::Circle { c, r } => (p.0 - c.0).hypot(p.1 - c.1) - r,
            PadPiece::Stadium { a, b, r } => dist(*a, *b) - r,
            PadPiece::Strokes { pts, r } => pts.windows(2).map(|w| dist(w[0], w[1])).fold(f64::INFINITY, f64::min) - r,
            PadPiece::Polygon(pts) => {
                let n = pts.len();
                if n < 3 {
                    return f64::INFINITY;
                }
                let edge = (0..n).map(|i| dist(pts[i], pts[(i + 1) % n])).fold(f64::INFINITY, f64::min);
                // Even-odd ray cast to the right.
                let mut inside = false;
                for i in 0..n {
                    let (a, b) = (pts[i], pts[(i + 1) % n]);
                    if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                        inside = !inside;
                    }
                }
                if inside {
                    -edge
                } else {
                    edge
                }
            }
        }
    }

    /// The same piece with every point passed through `f`.
    pub fn map(&self, f: &dyn Fn(P) -> P) -> PadPiece {
        match self {
            PadPiece::Circle { c, r } => PadPiece::Circle { c: f(*c), r: *r },
            PadPiece::Stadium { a, b, r } => PadPiece::Stadium { a: f(*a), b: f(*b), r: *r },
            PadPiece::Polygon(p) => PadPiece::Polygon(p.iter().map(|&q| f(q)).collect()),
            PadPiece::Strokes { pts, r } => PadPiece::Strokes { pts: pts.iter().map(|&q| f(q)).collect(), r: *r },
        }
    }
}

/// How a pad's frame sits on the board: the pad's own rotation (clockwise, relative to its footprint), then the footprint's mirror
/// (a bottom-side footprint mirrors x) and rotation, then the pad's position -- the composition `eda_model::footprint::to_board`
/// applies to a pad's position, and `PAD::Flip` to its shape.
#[derive(Debug, Clone, Copy)]
pub struct PadFrame {
    pub origin: P,
    pub pad_rot_mdeg: i64,
    pub fp_rot_mdeg: i64,
    pub mirror: bool,
}

impl PadFrame {
    /// The frame of `pad` on the footprint instance `fp`, whose position for the pad is `origin` (a board point).
    pub fn of(fp: &FootprintInstance, pad: &Pad, origin: Point) -> PadFrame {
        PadFrame { origin: (origin.x as f64, origin.y as f64), pad_rot_mdeg: pad.rot as i64, fp_rot_mdeg: fp.rot as i64, mirror: fp.side == Side::Bottom }
    }

    /// A point of the pad's frame on the board.
    pub fn to_board(&self, p: P) -> P {
        let rot = |a_mdeg: i64, (x, y): P| {
            let (s, c) = ((a_mdeg as f64) / 1000.0 * std::f64::consts::PI / 180.0).sin_cos();
            (x * c - y * s, x * s + y * c)
        };
        let (x, y) = rot(self.pad_rot_mdeg, p);
        let x = if self.mirror { -x } else { x };
        let (x, y) = rot(self.fp_rot_mdeg, (x, y));
        (self.origin.0 + x, self.origin.1 + y)
    }
}

/// Where the error of a polygon that stands for a curve lies (`ERROR_LOC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorLoc {
    Inside,
    Outside,
}

fn half(v: Nm) -> f64 {
    // `VECTOR2I / 2` truncates toward zero.
    (v / 2) as f64
}

/// `PAD::GetRoundRectCornerRadius`: `KiROUND( ratio * min( size.x, size.y ) )`.
pub fn round_rect_radius(pad: &Pad) -> Nm {
    let ratio = pad.roundrect_ratio.unwrap_or(0.25).clamp(0.0, 0.5);
    (ratio * pad.size.0.min(pad.size.1) as f64).round() as Nm
}

/// The corners of a trapezoid or a rectangle, bottom left, bottom right, top right and top left (`PAD::buildEffectiveShape`); `half`
/// is half the size and `delta` half the pad's trapezoid delta (both integer halves, as KiCad takes them).
pub fn trapezoid_corners(half_size: P, delta: P) -> [P; 4] {
    let (hx, hy) = half_size;
    let (dx, dy) = delta;
    [(-hx - dy, hy + dx), (hx + dy, hy - dx), (hx - dy, -hy + dx), (-hx + dy, -hy - dx)]
}

struct Corner {
    p: P,
    r: f64,
}

fn resize(v: P, len: f64) -> P {
    let n = v.0.hypot(v.1);
    if n == 0.0 {
        (0.0, 0.0)
    } else {
        (v.0 * len / n, v.1 * len / n)
    }
}

/// `CornerListRemoveDuplicates`.
fn corner_list_remove_duplicates(corners: &mut Vec<Corner>) {
    let mut prev = corners[0].p;
    for pos in (0..corners.len()).rev() {
        if corners[pos].p == prev {
            corners.remove(pos);
        } else {
            prev = corners[pos].p;
        }
    }
}

/// `CornerListToPolygon` for an inflate of 0 and `ERROR_INSIDE`: the corner list as a polygon, each corner that has a radius replaced
/// by the arc that rounds it (corners in clockwise order, no duplicates, nothing concave).
fn corner_list_to_polygon(corners: &[Corner], max_error: f64) -> Vec<P> {
    let mut out: Vec<P> = Vec::new();
    let count = corners.len();
    let mut incoming = (corners[0].p.0 - corners[count - 1].p.0, corners[0].p.1 - corners[count - 1].p.1);
    for n in 0..count {
        let cur = &corners[n];
        let next = &corners[(n + 1) % count];
        let outgoing = (next.p.0 - cur.p.0, next.p.1 - cur.p.1);
        if cur.r == 0.0 {
            out.push(cur.p);
        } else {
            let radius = cur.r;
            let corner = cur.p;
            let (end_angle, tan_angle2) = if (incoming.0 == 0.0 && outgoing.1 == 0.0) || (incoming.1 == 0.0 && outgoing.0 == 0.0) {
                (std::f64::consts::FRAC_PI_2, 1.0)
            } else {
                let cos_num = incoming.0 * outgoing.0 + incoming.1 * outgoing.1;
                let cos_den = incoming.0.hypot(incoming.1) * outgoing.0.hypot(outgoing.1);
                let angle = (cos_num / cos_den).clamp(-1.0, 1.0).acos();
                (angle, ((std::f64::consts::PI - angle) / 2.0).tan())
            };
            // Ensure 16+ segments per 360 degrees and the first and last segment the same size.
            let num_segs = 16.max(arc_to_segment_count(radius, max_error, std::f64::consts::TAU));
            let ang_delta = std::f64::consts::TAU / num_segs as f64;
            let mut last_seg = end_angle;
            if last_seg > 0.0 {
                while last_seg > ang_delta {
                    last_seg -= ang_delta;
                }
            } else {
                while last_seg < -ang_delta {
                    last_seg += ang_delta;
                }
            }
            let mut ang_pos = if last_seg == 0.0 { ang_delta } else { (ang_delta + last_seg) / 2.0 };
            let transition = if tan_angle2 > 0.0 { radius / tan_angle2 } else { 0.0 };
            let inc_r = resize(incoming, transition);
            let arc_start = (corner.0 - inc_r.0, corner.1 - inc_r.1);
            let perp = (-incoming.1, incoming.0);
            let pr = resize(perp, radius);
            let arc_center = (arc_start.0 + pr.0, arc_start.1 + pr.1);
            // `SEG( cornerPosition, arcCenter ).ReflectPoint( arcStart )`
            let arc_end = {
                let (ax, ay) = corner;
                let (bx, by) = arc_center;
                let (dx, dy) = (bx - ax, by - ay);
                let d2 = dx * dx + dy * dy;
                if d2 == 0.0 {
                    arc_start
                } else {
                    let t = ((arc_start.0 - ax) * dx + (arc_start.1 - ay) * dy) / d2;
                    let foot = (ax + t * dx, ay + t * dy);
                    (2.0 * foot.0 - arc_start.0, 2.0 * foot.1 - arc_start.1)
                }
            };
            let origin = (arc_start.0 - arc_center.0, arc_start.1 - arc_center.1);
            out.push(arc_start);
            while ang_pos < end_angle {
                // `RotatePoint( pt, -angPos )`
                let (s, c) = ang_pos.sin_cos();
                out.push((arc_center.0 + origin.0 * c - origin.1 * s, arc_center.1 + origin.0 * s + origin.1 * c));
                ang_pos += ang_delta;
            }
            out.push(arc_end);
        }
        incoming = outgoing;
    }
    out
}

/// `TransformRoundChamferedRectToPolygon` with no inflate, at the origin and not rotated, `ERROR_INSIDE`: a `size` rectangle whose
/// corners of `corners` are cut off by `chamfer_ratio` of the shorter side and the others rounded by `corner_radius`.
pub fn chamfered_rect_polygon(size: (Nm, Nm), corner_radius: Nm, chamfer_ratio: f64, corners: ChamferCorners, max_error: f64) -> Vec<P> {
    let (sx, sy) = (half(size.0), half(size.1));
    let mask = corners.bits();
    let radius = corner_radius as f64;
    let mut list = vec![Corner { p: (-sx, -sy), r: radius }, Corner { p: (sx, -sy), r: radius }, Corner { p: (sx, sy), r: radius }, Corner { p: (-sx, sy), r: radius }];
    if mask != 0 {
        let shorter = size.0.min(size.1);
        let chamfer = ((chamfer_ratio * shorter as f64).round() as i64).max(0) as f64;
        // top left, top right, bottom right, bottom left
        let cham_id = [1u8, 2, 8, 4];
        let sign: [f64; 8] = [0.0, 1.0, -1.0, 0.0, 0.0, -1.0, 1.0, 0.0];
        let mut pos = 0usize;
        for cc in 0..4usize {
            if mask & cham_id[cc] != 0 {
                list[pos].r = 0.0;
                if chamfer != 0.0 {
                    let dup = Corner { p: list[pos].p, r: 0.0 };
                    list.insert(pos + 1, dup);
                    list[pos].p.0 += sign[(2 * cc) & 7] * chamfer;
                    list[pos].p.1 += sign[(2 * cc + 6) & 7] * chamfer; // (2 * cc - 2) & 7
                    list[pos + 1].p.0 += sign[(2 * cc + 1) & 7] * chamfer;
                    list[pos + 1].p.1 += sign[(2 * cc + 7) & 7] * chamfer; // (2 * cc - 1) & 7
                    pos += 1;
                }
            }
            pos += 1;
        }
        if mask.count_ones() > 1 && 2.0 * chamfer >= shorter as f64 {
            corner_list_remove_duplicates(&mut list);
        }
    }
    corner_list_to_polygon(&list, max_error)
}

/// The primitives of a custom pad that are copper (not the proxy items), with the pad's anchor.
pub fn custom_primitives(custom: &PadCustom) -> impl Iterator<Item = &PadPrimitive> {
    custom.primitives.iter().filter(|p| !p.proxy)
}

/// `EDA_SHAPE::makeEffectiveShapes` of one primitive, in the pad's frame, moved by `at`.
pub fn primitive_pieces(shape: &Shape, at: P, max_error: f64) -> Vec<PadPiece> {
    let mv = |p: Point| (p.x as f64 + at.0, p.y as f64 + at.1);
    let width = shape.stroke_width().max(0) as f64;
    let r = width / 2.0;
    let solid = shape.is_filled();
    let mut out = Vec::new();
    match shape {
        Shape::Segment { start, end, .. } => out.push(PadPiece::Stadium { a: mv(*start), b: mv(*end), r }),
        Shape::Arc { start, mid, end, .. } => {
            let pts: Vec<P> = Arc::through(*start, *mid, *end).polyline(max_error).into_iter().map(mv).collect();
            if pts.len() >= 2 {
                out.push(PadPiece::Strokes { pts, r });
            }
        }
        Shape::Rect { start, end, .. } => {
            let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
            let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
            let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| (x as f64 + at.0, y as f64 + at.1));
            if solid {
                out.push(PadPiece::Polygon(pts.to_vec()));
            }
            if width > 0.0 || !solid {
                for i in 0..4 {
                    out.push(PadPiece::Stadium { a: pts[i], b: pts[(i + 1) % 4], r });
                }
            }
        }
        Shape::Circle { center, end, .. } => {
            let rad = ((end.x - center.x) as f64).hypot((end.y - center.y) as f64);
            let c = mv(*center);
            if solid {
                out.push(PadPiece::Circle { c, r: rad });
            }
            if width > 0.0 || !solid {
                // `SHAPE_ARC( center, end, ANGLE_360, width )`
                let n = arc_to_segment_count(rad, max_error, std::f64::consts::TAU).max(8) as usize;
                let mut pts: Vec<P> = (0..n).map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    (c.0 + rad * a.cos(), c.1 + rad * a.sin())
                }).collect();
                pts.push(pts[0]);
                out.push(PadPiece::Strokes { pts, r });
            }
        }
        Shape::Polygon { pts, .. } => {
            if pts.len() >= 2 {
                let p: Vec<P> = pts.iter().map(|&q| mv(q)).collect();
                if solid && p.len() >= 3 {
                    out.push(PadPiece::Polygon(p.clone()));
                }
                if width > 0.0 || !solid {
                    for i in 0..p.len() {
                        out.push(PadPiece::Stadium { a: p[i], b: p[(i + 1) % p.len()], r });
                    }
                }
            }
        }
        Shape::Bezier { .. } => {
            let pts = shape.bezier_points().unwrap_or_default();
            for w in pts.windows(2) {
                out.push(PadPiece::Stadium { a: mv(w[0]), b: mv(w[1]), r });
            }
        }
    }
    out
}

/// `PAD::buildEffectiveShape`: the parts of the pad's copper in its own frame, the shape's offset applied. A rounded rectangle is its
/// inner rectangle and four stadiums along its edges, as KiCad builds it.
pub fn pad_pieces(pad: &Pad) -> Vec<PadPiece> {
    let off = pad.ext.offset.map(|(x, y)| (x as f64, y as f64)).unwrap_or((0.0, 0.0));
    let (w, h) = (pad.size.0, pad.size.1);
    let eff = match (pad.shape, &pad.ext.custom) {
        (PadShape::Custom, Some(c)) => match c.anchor {
            PadAnchor::Circle => PadShape::Circle,
            PadAnchor::Rect => PadShape::Rect,
        },
        (PadShape::Custom, None) => PadShape::Circle,
        (s, _) => s,
    };
    let mut out = Vec::new();
    match eff {
        PadShape::Circle | PadShape::Custom => out.push(PadPiece::Circle { c: off, r: half(w) }),
        PadShape::Oval => {
            if w == h {
                out.push(PadPiece::Circle { c: off, r: half(w) });
            } else {
                // `half_width = min( half.x, half.y )`, `half_len = half - half_width`
                let (hx, hy) = (half(w), half(h));
                let hw = hx.min(hy);
                let len = (hx - hw, hy - hw);
                out.push(PadPiece::Stadium { a: (off.0 - len.0, off.1 - len.1), b: (off.0 + len.0, off.1 + len.1), r: hw });
            }
        }
        PadShape::Rect | PadShape::Trapezoid | PadShape::RoundRect => {
            let r = if eff == PadShape::RoundRect { round_rect_radius(pad) as f64 } else { 0.0 };
            let mut hs = (half(w), half(h));
            let mut delta = (0.0, 0.0);
            let mut circle_only = false;
            if r != 0.0 {
                hs = (hs.0 - r, hs.1 - r);
                // `min_len = mmToIU( 0.0001 )`: a round rectangle that is a circle is one.
                if hs.0 < 100.0 && hs.1 < 100.0 {
                    out.push(PadPiece::Circle { c: off, r });
                    circle_only = true;
                }
            } else if eff == PadShape::Trapezoid {
                let d = pad.ext.trapezoid_delta.unwrap_or((0, 0));
                delta = (half(d.0), half(d.1));
            }
            if !circle_only {
                let corners = trapezoid_corners(hs, delta).map(|(x, y)| (x + off.0, y + off.1));
                out.push(PadPiece::Polygon(corners.to_vec()));
                if r != 0.0 {
                    for i in 0..4 {
                        out.push(PadPiece::Stadium { a: corners[i], b: corners[(i + 1) % 4], r });
                    }
                }
            }
        }
        PadShape::ChamferedRect => {
            let ratio = pad.ext.chamfer_ratio.unwrap_or(0.0);
            let poly = chamfered_rect_polygon(pad.size, round_rect_radius(pad), ratio, pad.ext.chamfer_corners, MAX_ERROR_NM as f64);
            out.push(PadPiece::Polygon(poly.into_iter().map(|(x, y)| (x + off.0, y + off.1)).collect()));
        }
    }
    if pad.shape == PadShape::Custom {
        if let Some(c) = &pad.ext.custom {
            for prim in custom_primitives(c) {
                out.extend(primitive_pieces(&prim.shape, off, MAX_ERROR_NM as f64));
            }
        }
    }
    out
}

/// The bounding box `(x0, y0, x1, y1)` of a set of pieces (`SHAPE_COMPOUND::BBox`).
pub fn pieces_bbox(pieces: &[PadPiece]) -> Option<(f64, f64, f64, f64)> {
    let mut it = pieces.iter().map(PadPiece::bbox);
    let first = it.next()?;
    Some(it.fold(first, |a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))))
}

impl Pad {
    /// `PADSTACK::DefaultThermalSpokeAngleForShape` as the file reader applies it (`parsePAD`): 45 degrees for a circle and for a
    /// custom pad whose anchor is a circle, 90 otherwise (millidegrees).
    pub fn default_spoke_angle_mdeg(&self) -> i64 {
        let circle_anchor = self.ext.custom.as_ref().is_none_or(|c| c.anchor == PadAnchor::Circle);
        if self.shape == PadShape::Circle || (self.shape == PadShape::Custom && circle_anchor) {
            45_000
        } else {
            90_000
        }
    }

    /// Whether the pad is anything but a rectangle, a rounded rectangle, a circle or an oval: its copper is not given by its size.
    pub fn has_complex_outline(&self) -> bool {
        matches!(self.shape, PadShape::Trapezoid | PadShape::ChamferedRect | PadShape::Custom)
    }

    /// The pad's bounding box in its own frame (`PAD::GetBoundingBox( layer )` of a pad at the origin and not rotated).
    pub fn local_bbox(&self) -> (f64, f64, f64, f64) {
        pieces_bbox(&pad_pieces(self)).unwrap_or((0.0, 0.0, 0.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::{PadKind, PadShapeExt};

    fn pad(shape: PadShape, size: (Nm, Nm)) -> Pad {
        Pad { number: "1".into(), at: (0, 0), size, shape, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: PadShapeExt::default() }
    }

    fn area(poly: &[P]) -> f64 {
        let n = poly.len();
        (0..n).map(|i| poly[i].0 * poly[(i + 1) % n].1 - poly[(i + 1) % n].0 * poly[i].1).sum::<f64>().abs() / 2.0
    }

    #[test]
    fn a_trapezoid_is_narrower_on_the_side_its_delta_says() {
        // 2 mm x 1 mm, `rect_delta 0.4 0`: the two sides at +-y differ by 0.4 mm (KiCad's x delta narrows the left and right).
        let mut p = pad(PadShape::Trapezoid, (2_000_000, 1_000_000));
        p.ext.trapezoid_delta = Some((400_000, 0));
        let pieces = pad_pieces(&p);
        let PadPiece::Polygon(c) = &pieces[0] else { panic!("{pieces:?}") };
        assert_eq!(c.as_slice(), [(-1_000_000.0, 700_000.0), (1_000_000.0, 300_000.0), (1_000_000.0, -300_000.0), (-1_000_000.0, -700_000.0)]);
        // The area is the mean of the two sides (1.4 and 0.6 mm) times the 2 mm between them.
        assert!((area(c) - 2_000_000.0 * 1_000_000.0).abs() < 1.0, "{}", area(c));
        // A y delta narrows the top and the bottom.
        p.ext.trapezoid_delta = Some((0, 400_000));
        let pieces = pad_pieces(&p);
        let PadPiece::Polygon(c) = &pieces[0] else { panic!() };
        assert_eq!(c.as_slice(), [(-1_200_000.0, 500_000.0), (1_200_000.0, 500_000.0), (800_000.0, -500_000.0), (-800_000.0, -500_000.0)]);
    }

    #[test]
    fn a_chamfered_rectangle_loses_a_triangle_at_each_cut_corner() {
        let mut p = pad(PadShape::ChamferedRect, (1_000_000, 600_000));
        p.roundrect_ratio = Some(0.0);
        p.ext.chamfer_ratio = Some(0.25);
        p.ext.chamfer_corners = ChamferCorners { top_left: true, bottom_right: true, ..Default::default() };
        let pieces = pad_pieces(&p);
        let PadPiece::Polygon(c) = &pieces[0] else { panic!() };
        // chamfer = 0.25 * 600 = 150: two triangles of 150 x 150 / 2 are cut from the 1000 x 600 rectangle.
        assert!((area(c) - (600_000_000_000.0 - 2.0 * 150_000.0 * 150_000.0 / 2.0)).abs() < 1.0, "{}", area(c));
        assert!(c.contains(&(-500_000.0, -150_000.0)) && c.contains(&(-350_000.0, -300_000.0)), "{c:?}");
        assert!(!c.contains(&(-500_000.0, -300_000.0)), "the top left corner is cut: {c:?}");
        assert!(c.contains(&(500_000.0, -300_000.0)), "the top right one is not");
    }

    #[test]
    fn a_rounded_chamfered_rectangle_is_a_chamfered_one_with_arcs_at_the_other_corners() {
        let mut p = pad(PadShape::ChamferedRect, (1_000_000, 600_000));
        p.roundrect_ratio = Some(0.25);
        p.ext.chamfer_ratio = Some(0.25);
        p.ext.chamfer_corners = ChamferCorners { top_left: true, ..Default::default() };
        let pieces = pad_pieces(&p);
        let PadPiece::Polygon(c) = &pieces[0] else { panic!() };
        // The exact area: the rectangle less one chamfer triangle (150 x 150 / 2) and the three corners' (1 - pi / 4) r^2, r = 150.
        let exact = 600_000_000_000.0 - 150_000.0 * 150_000.0 / 2.0 - 3.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 150_000.0 * 150_000.0;
        // The error is inside the arc: each corner is 16 / 4 = 4 chords of 22.5 degrees (KiCad rounds the segment count of a circle of
        // this size, 12, up to its minimum of 16), each short of the arc by a circular segment (r^2 / 2) (theta - sin theta).
        let theta = std::f64::consts::TAU / 16.0;
        let chords = 3.0 * 4.0 * 150_000.0 * 150_000.0 / 2.0 * (theta - theta.sin());
        assert!((area(c) - (exact - chords)).abs() < 1.0, "{} vs {}", area(c), exact - chords);
    }

    #[test]
    fn a_custom_pad_is_its_anchor_and_its_primitives() {
        let mut p = pad(PadShape::Custom, (1_000_000, 1_000_000));
        p.ext.custom = Some(PadCustom {
            anchor: PadAnchor::Circle,
            zone: Default::default(),
            primitives: vec![
                PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 200_000, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 3_000_000, y: 0 } }, proxy: false },
                PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 200_000, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 0, y: 9_000_000 } }, proxy: true },
            ],
        });
        let pieces = pad_pieces(&p);
        assert_eq!(pieces.len(), 2, "the anchor and the one copper primitive (the proxy is not copper)");
        let (x0, y0, x1, y1) = pieces_bbox(&pieces).unwrap();
        assert_eq!((x0, y0, x1, y1), (-500_000.0, -500_000.0, 3_100_000.0, 500_000.0));
        assert_eq!(p.local_bbox(), (x0, y0, x1, y1));
    }

    #[test]
    fn a_ring_is_a_stroked_circle_and_a_filled_one_is_a_disc() {
        let ring = Shape::Circle { id: String::new(), layer: String::new(), stroke_width: 2_200_000, filled: false, center: Point { x: -2_975_000, y: 0 }, end: Point { x: -375_000, y: 0 } };
        let pieces = primitive_pieces(&ring, (0.0, 0.0), 5000.0);
        let [PadPiece::Strokes { pts, r }] = pieces.as_slice() else { panic!("{pieces:?}") };
        assert_eq!(*r, 1_100_000.0);
        assert!(pts.iter().all(|p| ((p.0 + 2_975_000.0).hypot(p.1) - 2_600_000.0).abs() < 1e-6));
        let disc = Shape::Circle { id: String::new(), layer: String::new(), stroke_width: 0, filled: true, center: Point { x: 0, y: 0 }, end: Point { x: 500_000, y: 0 } };
        assert_eq!(primitive_pieces(&disc, (10_000.0, 0.0), 5000.0), vec![PadPiece::Circle { c: (10_000.0, 0.0), r: 500_000.0 }]);
    }

    #[test]
    fn a_placed_custom_pad_is_measured_to_its_copper_and_boxed_by_it() {
        use crate::footprint::{placed_pads, Footprint};
        use crate::ir::{LabelSide, Side};
        use crate::{ConstraintModel, Part, Pin, PinKind};
        // A 0.6 mm round anchor and a 0.2 mm line from it 2 mm to the right: the copper reaches x = 2100.
        let mut p = pad(PadShape::Custom, (600_000, 600_000));
        p.ext.custom = Some(PadCustom { anchor: PadAnchor::Circle, zone: Default::default(), primitives: vec![PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 200_000, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 2_000_000, y: 0 } }, proxy: false }] });
        let footprint = Footprint { name: "C".into(), pads: vec![p], courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() };
        let part = Part { reference: "J1".into(), mpn: None, lcsc: None, value: None, package: Some("C".into()), footprint: Some("C".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part.clone()], footprints: vec![footprint], ..Default::default() };
        let at = |rot, side| FootprintInstance { id: "J1".into(), at: Point { x: 10_000_000, y: 5_000_000 }, rot, side, label: LabelSide::Above };
        // Unturned: the box about the pad's position holds the arm (2.1 mm to the right, so 4.2 mm wide) and the anchor (0.6 mm high).
        let placed = &placed_pads(&model, &part, &at(0, Side::Top)).unwrap()[0];
        assert_eq!(placed.size, (4_200_000, 600_000));
        assert!(placed.signed_distance(Point { x: 11_500_000, y: 5_000_000 }) < 0.0, "inside the arm");
        assert!((placed.signed_distance(Point { x: 11_500_000, y: 5_400_000 }) - 300_000.0).abs() < 1e-6, "300 um beside the arm's edge (it is 100 um out of the axis)");
        assert!((placed.signed_distance(Point { x: 12_400_000, y: 5_000_000 }) - 300_000.0).abs() < 1e-6, "beyond the arm's end");
        // Turned a quarter clockwise on the screen the arm points down, on the bottom side it is mirrored to the left.
        let down = &placed_pads(&model, &part, &at(90_000, Side::Top)).unwrap()[0];
        assert_eq!(down.size, (600_000, 4_200_000));
        assert!(down.signed_distance(Point { x: 10_000_000, y: 6_500_000 }) < 0.0);
        let mirrored = &placed_pads(&model, &part, &at(0, Side::Bottom)).unwrap()[0];
        assert!(mirrored.signed_distance(Point { x: 8_500_000, y: 5_000_000 }) < 0.0 && mirrored.signed_distance(Point { x: 11_500_000, y: 5_000_000 }) > 0.0);
        // A plain pad has no pieces and measures as before.
        assert!(placed_pads(&ConstraintModel { footprints: vec![Footprint { name: "C".into(), pads: vec![pad(PadShape::Rect, (600_000, 600_000))], courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() }], ..model.clone() }, &part, &at(0, Side::Top)).unwrap()[0].pieces.is_empty());
    }

    #[test]
    fn the_footprints_pad_box_holds_a_custom_pads_shapes() {
        use crate::footprint::Footprint;
        let mut p = pad(PadShape::Custom, (600_000, 600_000));
        p.ext.custom = Some(PadCustom { anchor: PadAnchor::Rect, zone: Default::default(), primitives: vec![PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: 200_000, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 0, y: -3_000_000 } }, proxy: false }] });
        p.at = (1_000_000, 0);
        let fp = Footprint { name: "C".into(), pads: vec![p], courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() };
        assert_eq!(fp.pad_bbox(), (700_000, -3_100_000, 1_300_000, 300_000));
    }

    #[test]
    fn the_frame_turns_mirrors_and_places_a_point() {
        let f = |rot, mirror| PadFrame { origin: (1_000_000.0, 2_000_000.0), pad_rot_mdeg: 0, fp_rot_mdeg: rot, mirror };
        let near = |a: P, b: P| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        assert!(near(f(0, false).to_board((10_000.0, 20_000.0)), (1_010_000.0, 2_020_000.0)));
        assert!(near(f(0, true).to_board((10_000.0, 20_000.0)), (990_000.0, 2_020_000.0)));
        // 90 degrees clockwise on the screen: +x goes to +y.
        assert!(near(f(90_000, false).to_board((10_000.0, 0.0)), (1_000_000.0, 2_010_000.0)));
    }
}
