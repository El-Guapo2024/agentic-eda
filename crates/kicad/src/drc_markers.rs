//! Where kicad-cli's DRC puts the markers [`crate::drc_exclusions`] does not place by one formula: an arc track's (`SHAPE_ARC::Collide`, which depends on the
//! rule's clearance), an item's against a zone's fill (`DRC_RTREE::QueryColliding` over the fill's triangles), two footprints' courtyards and two silkscreen
//! graphics (`SHAPE_POLY_SET::Collide` over both triangulations, the subshape pairs of `DRC_RTREE::QueryCollidingPairs`), a via or pad that is not flashed on
//! the layer (its hole is its copper there), an item in a keepout rule area (`SHAPE_POLY_SET::SquaredDistanceToSeg`).
//!
//! An exclusion lists every position worth trying and kicad-cli's DRC job drops each record no marker matches (`JobExportDrc`'s `ResolveDRCExclusions(
//! false )`), so where KiCad's choice among several places rests on an order this port does not have -- which triangle of a fill or a courtyard a
//! `TRIANGULATION_INDEX` or an ear cutter visits first, which subshape pair an R-tree hands over first -- every place that order could pick is a candidate:
//! a triangle's vertices are the polygon's, its edges are chords between them, and `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )` puts the
//! marker at a vertex of one inside the other or at `SEG::NearestPoint` of two edges. The point kicad-cli made is among them; the others match nothing.
//!
//! KiCad 25ec2335 (the box's kicad-cli): `libs/kimath/src/geometry/shape_arc.cpp` (`update_values`, `Collide( const SEG& )`, `Collide( const VECTOR2I& )`,
//! `NearestPoints( const SHAPE_CIRCLE& )`, `sliceContainsPoint`, `GetCentralAngle`, `IsEffectiveLine`), `shape_collisions.cpp` (the `SHAPE_ARC` cases),
//! `libs/kimath/src/trigo.cpp::CalcArcCenter`, `circle.cpp` (`NearestPoint`, `Intersect`), `include/geometry/eda_angle.h`, `shape_poly_set.cpp`
//! (`Collide( const SHAPE* )`, `SquaredDistanceToPolygon`, `SquaredDistanceToSeg`), `include/geometry/shape_poly_tri_index.h::AnyTriangleHit`,
//! `pcbnew/drc/drc_rtree.h` (`QueryColliding`, `QueryCollidingPairs`), `pcbnew/drc/drc_test_provider_copper_clearance.cpp` (`testSingleLayerItemAgainstItem`,
//! `testItemAgainstZone`), `drc_test_provider_courtyard_clearance.cpp`, `drc_test_provider_silk_clearance.cpp`, `drc_test_provider_disallow.cpp`.

use crate::drc_exclusions::{
    circle_intersect_seg, collide_single, ki_round_half, point_inside, rotate_point, rounded_hypot, seg_dist, seg_nearest_point, seg_nearest_to_seg, seg_sq_distance, seg_sq_distance_point, sides, BoardShapes, Copper,
};

/// `ADVANCED_CFG::m_DRCEpsilon` (0.0005 mm): the copper clearance test collides at the rule's clearance less this (`sub_e`).
pub const DRC_EPSILON_NM: i64 = 500;

// ------------------------------------------------------------------------------------------------------------------------------------ arcs

/// `SHAPE_ARC` of an arc track: its three points and width, and what `update_values` works out of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub start: [i64; 2],
    pub mid: [i64; 2],
    pub end: [i64; 2],
    pub width: i64,
    centre: [i64; 2],
    radius: f64,
    /// `m_bbox`: the three points and the quadrant points the arc passes, `(x0, y0, x1, y1)`.
    bbox: [i64; 4],
}

/// `EDA_ANGLE( const VECTOR2D& )`, in degrees: exact on the axes and the diagonals, else `atan2`.
fn angle_of(v: [f64; 2]) -> f64 {
    let [x, y] = v;
    if x == 0.0 && y == 0.0 {
        0.0
    } else if y == 0.0 {
        if x >= 0.0 {
            0.0
        } else {
            -180.0
        }
    } else if x == 0.0 {
        if y >= 0.0 {
            90.0
        } else {
            -90.0
        }
    } else if x == y {
        if x >= 0.0 {
            45.0
        } else {
            -135.0
        }
    } else if x == -y {
        if x >= 0.0 {
            -45.0
        } else {
            135.0
        }
    } else {
        y.atan2(x).to_degrees()
    }
}

/// `EDA_ANGLE::Normalize`: into `[0, 360)`.
fn normalize(mut a: f64) -> f64 {
    a %= 720.0;
    while a < -0.0 {
        a += 360.0;
    }
    while a >= 360.0 {
        a -= 360.0;
    }
    a
}

fn sub(a: [i64; 2], b: [i64; 2]) -> [f64; 2] {
    [(a[0] - b[0]) as f64, (a[1] - b[1]) as f64]
}

/// `KiROUND`.
fn kiround(v: f64) -> i64 {
    v.round() as i64
}

/// `CalcArcCenter( const VECTOR2I&, const VECTOR2I&, const VECTOR2I& )` (`trigo.cpp`): the exact numerators in 128 bits, one rounding per coordinate,
/// the 100 nm or 10 nm snap when the radii still agree to a nanometre (`snapArcCenter`), the near-coincident cases (`degenerateArcCenter`).
fn calc_arc_centre(s: [i64; 2], m: [i64; 2], e: [i64; 2]) -> [i64; 2] {
    let d = |p: [i64; 2]| [p[0] as f64, p[1] as f64];
    let (ds, dm, de) = (d(s), d(m), d(e));
    let degenerate = || -> Option<[f64; 2]> {
        let xs = [ds[0], dm[0], de[0]];
        let ys = [ds[1], dm[1], de[1]];
        let ext = |v: [f64; 3]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
        if ext(xs) < 5.0 && ext(ys) < 5.0 {
            return Some([(ds[0] + dm[0] + de[0]) / 3.0, (ds[1] + dm[1] + de[1]) / 3.0]);
        }
        let coincident = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) < 4.0;
        if coincident(ds, dm) || coincident(dm, de) {
            return Some([(ds[0] + de[0]) / 2.0, (ds[1] + de[1]) / 2.0]);
        }
        if coincident(ds, de) {
            return Some([(ds[0] + dm[0]) / 2.0, (ds[1] + dm[1]) / 2.0]);
        }
        None
    };
    let centre = match degenerate() {
        Some(c) => c,
        None => {
            let b = [(m[0] - s[0]) as i128, (m[1] - s[1]) as i128];
            let c = [(e[0] - s[0]) as i128, (e[1] - s[1]) as i128];
            let b2 = b[0] * b[0] + b[1] * b[1];
            let c2 = c[0] * c[0] + c[1] * c[1];
            let dd = (b[0] * c[1] - b[1] * c[0]) * 2;
            if dd == 0 {
                // `collinearArcCenter`: far off the chord's middle
                let chord = [de[0] - ds[0], de[1] - ds[1]];
                let n = (chord[0] * chord[0] + chord[1] * chord[1]).sqrt();
                let mid = [(ds[0] + de[0]) / 2.0, (ds[1] + de[1]) / 2.0];
                if n == 0.0 {
                    mid
                } else {
                    [mid[0] + chord[1] / n * 1e17, mid[1] - chord[0] / n * 1e17]
                }
            } else {
                let fd = dd as f64;
                let cx = (s[0] as i128 * dd + b2 * c[1] - c2 * b[1]) as f64 / fd;
                let cy = (s[1] as i128 * dd + c2 * b[0] - b2 * c[0]) as f64 / fd;
                // `snapArcCenter`
                let agree = |cand: [f64; 2]| {
                    let r = |p: [f64; 2]| ((cand[0] - p[0]).powi(2) + (cand[1] - p[1]).powi(2)).sqrt();
                    let rs = [r(ds), r(dm), r(de)];
                    let max = rs.iter().cloned().fold(f64::MIN, f64::max);
                    let min = rs.iter().cloned().fold(f64::MAX, f64::min);
                    max - min <= 1.0
                };
                let mut out = [cx, cy];
                if cx.abs() < 9007199254740992.0 && cy.abs() < 9007199254740992.0 {
                    for grid in [100.0, 10.0] {
                        let cand = [(cx / grid + 0.5).floor() * grid, (cy / grid + 0.5).floor() * grid];
                        if agree(cand) {
                            out = cand;
                            break;
                        }
                    }
                }
                out
            }
        }
    };
    let clamp = |v: f64| v.clamp(i32::MIN as f64 + 100.0, i32::MAX as f64 - 100.0);
    [kiround(clamp(centre[0])), kiround(clamp(centre[1]))]
}

impl Arc {
    /// `SHAPE_ARC( start, mid, end, width )` and its `update_values`.
    pub fn new(start: [i64; 2], mid: [i64; 2], end: [i64; 2], width: i64) -> Arc {
        let centre = calc_arc_centre(start, mid, end);
        let r = sub(start, centre);
        let radius = (r[0] * r[0] + r[1] * r[1]).sqrt();
        let mut arc = Arc { start, mid, end, width, centre, radius, bbox: [0; 4] };
        let mut pts = vec![start, mid, end];
        let mut sa = arc.start_angle();
        let mut ea = sa + arc.central_angle();
        if sa > ea {
            std::mem::swap(&mut sa, &mut ea);
        }
        let (qs, qe) = ((sa / 90.0).ceil() as i64, (ea / 90.0).floor() as i64);
        if radius < (i32::MAX as f64) / 2.0 {
            let ir = kiround(radius);
            for q in qs..=qe {
                let off = match q % 4 {
                    0 => [ir, 0],
                    1 | -3 => [0, ir],
                    2 | -2 => [-ir, 0],
                    _ => [0, -ir],
                };
                pts.push([centre[0] + off[0], centre[1] + off[1]]);
            }
        }
        arc.bbox = [pts.iter().map(|p| p[0]).min().unwrap_or(0), pts.iter().map(|p| p[1]).min().unwrap_or(0), pts.iter().map(|p| p[0]).max().unwrap_or(0), pts.iter().map(|p| p[1]).max().unwrap_or(0)];
        arc
    }

    fn is_ccw(&self) -> bool {
        let v1 = [(self.end[0] - self.mid[0]) as i128, (self.end[1] - self.mid[1]) as i128];
        let v2 = [(self.start[0] - self.mid[0]) as i128, (self.start[1] - self.mid[1]) as i128];
        v1[0] * v2[1] - v1[1] * v2[0] > 0
    }

    /// `IsEffectiveLine`: `SEG( start, mid ).ApproxCollinear( SEG( mid, end ) )` and the two run the same way.
    pub fn is_effective_line(&self) -> bool {
        let (mut a, mut b) = ([self.start, self.mid], [self.mid, self.end]);
        let len = |s: [[i64; 2]; 2]| ((s[1][0] - s[0][0]) as i128).pow(2) + ((s[1][1] - s[0][1]) as i128).pow(2);
        if len(a) < len(b) {
            std::mem::swap(&mut a, &mut b);
        }
        let p = (a[0][1] - a[1][1]) as i128;
        let q = (a[1][0] - a[0][0]) as i128;
        let r = -p * a[0][0] as i128 - q * a[0][1] as i128;
        let l = p * p + q * q;
        if l == 0 {
            return false;
        }
        let det = |pt: [i64; 2]| p * pt[0] as i128 + q * pt[1] as i128 + r;
        let rs = |d: i128| crate::drc_exclusions::rescale(d, d, l);
        let collinear = rs(det(b[0])).abs() <= 1 && rs(det(b[1])).abs() <= 1;
        let dot = (self.mid[0] - self.start[0]) as i128 * (self.end[0] - self.mid[0]) as i128 + (self.mid[1] - self.start[1]) as i128 * (self.end[1] - self.mid[1]) as i128;
        collinear && dot > 0
    }

    fn start_angle(&self) -> f64 {
        normalize(angle_of(sub(self.start, self.centre)))
    }

    fn end_angle(&self) -> f64 {
        normalize(angle_of(sub(self.end, self.centre)))
    }

    /// `GetCentralAngle`.
    fn central_angle(&self) -> f64 {
        if self.start == self.end {
            return 360.0;
        }
        if self.is_effective_line() {
            return 0.0;
        }
        let mut a = angle_of(sub(self.end, self.centre)) - angle_of(sub(self.start, self.centre));
        if self.is_ccw() {
            if a < 0.0 {
                a += 360.0;
            }
        } else if a > 0.0 {
            a -= 360.0;
        }
        a
    }

    /// `sliceContainsPoint`.
    fn slice_contains(&self, p: [i64; 2]) -> bool {
        let sa = self.start_angle();
        let ca = self.central_angle();
        let ea = sa + ca;
        let mut phi = normalize(angle_of(sub(p, self.centre)));
        if ca >= 0.0 {
            while phi < sa {
                phi += 360.0;
            }
            phi >= sa && phi <= ea
        } else {
            while phi > sa {
                phi -= 360.0;
            }
            phi <= sa && phi >= ea
        }
    }

    /// `SHAPE_ARC::Collide( const VECTOR2I&, aClearance, .. )`: `(actual, location)` when the point is within the clearance plus half the width.
    fn collide_point(&self, p: [i64; 2], clearance: i64) -> Option<(i64, [i64; 2])> {
        let min_dist = clearance + self.width / 2;
        let inflate = if self.width != 0 { kiround(self.width as f64 / 2.0) + 1 } else { 0 } + min_dist;
        let [x0, y0, x1, y1] = self.bbox;
        if p[0] < x0 - inflate || p[0] > x1 + inflate || p[1] < y0 - inflate || p[1] > y1 + inflate {
            return None;
        }
        let c = [self.centre[0] as f64, self.centre[1] as f64];
        let radius = self.radius;
        let circle_r = radius as i32 as f64; // `CIRCLE( center, radius )`: an int radius
        let mut v = [p[0] as f64 - c[0], p[1] as f64 - c[1]];
        if v == [0.0, 0.0] {
            v[0] = 1.0;
        }
        let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let mut nearest = [v[0] * (circle_r / n) + c[0], v[1] * (circle_r / n) + c[1]];
        let mut dist = kiround(((nearest[0] - p[0] as f64).powi(2) + (nearest[1] - p[1] as f64).powi(2)).sqrt());
        let angle_to = angle_of(sub(p, self.centre));
        if dist == 0 {
            let dp = sub(p, self.centre);
            dist = kiround(radius - (dp[0] * dp[0] + dp[1] * dp[1]).sqrt());
            // `center + VECTOR2I( radius, 0 )` turned by `-angleToPt` about the centre (`RotatePoint( VECTOR2D&, .. )`)
            let (s, co) = sin_cos(normalize(-angle_to));
            let (ox, oy) = (radius as i64 as f64, 0.0);
            nearest = [oy * s + ox * co + c[0], oy * co - ox * s + c[1]];
        }
        if self.start != self.end {
            let ccw = self.central_angle() > 0.0;
            let rotated_pt = normalize(normalize(angle_to) - self.start_angle());
            let rotated_end = normalize(self.end_angle() - self.start_angle());
            if (ccw && rotated_pt > rotated_end) || (!ccw && rotated_pt < rotated_end) {
                let ds = rounded_hypot(p[0] - self.start[0], p[1] - self.start[1]);
                let de = rounded_hypot(p[0] - self.end[0], p[1] - self.end[1]);
                if ds < de {
                    dist = ds;
                    nearest = [self.start[0] as f64, self.start[1] as f64];
                } else {
                    dist = de;
                    nearest = [self.end[0] as f64, self.end[1] as f64];
                }
            }
        }
        (dist <= min_dist).then(|| ((dist - self.width / 2).max(0), [kiround(nearest[0]), kiround(nearest[1])]))
    }

    /// `SHAPE_ARC::Collide( const SEG&, aClearance, .. )`: of the candidate points -- the circle's crossings of the segment, the segment's nearest points to
    /// the centre and to the arc's ends, the segment's ends -- the last that collides gives the place, the first at a distance of 0 ends the search.
    fn collide_seg(&self, s: [[i64; 2]; 2], clearance: i64) -> Option<(i64, [i64; 2])> {
        let clearance_sq = (clearance as i128) * (clearance as i128);
        let circle_r = self.radius as i32 as i64;
        if self.central_angle() > 180.0 && (((self.start[0] - self.end[0]) as i128).pow(2) + ((self.start[1] - self.end[1]) as i128).pow(2)) < clearance_sq {
            // a near circle: the full circle's `Collide( SEG )`
            return None;
        }
        let mut cands = circle_intersect_seg(self.centre, circle_r, s);
        cands.push(seg_nearest_point(s, self.centre));
        cands.push(seg_nearest_point(s, self.start));
        cands.push(seg_nearest_point(s, self.end));
        cands.push(s[0]);
        cands.push(s[1]);
        let mut out = None;
        for c in cands {
            if let Some(hit) = self.collide_point(c, clearance) {
                out = Some(hit);
                if hit.0 == 0 {
                    return out;
                }
            }
        }
        out
    }

    /// `NearestPoints( const SHAPE_CIRCLE& )` and `Collide( const SHAPE_ARC&, const SHAPE_CIRCLE& )`'s place, `( ptA + ptB ) / 2`.
    fn nearest_circle(&self, centre: [i64; 2], radius: i64) -> Option<[i64; 2]> {
        let r1 = self.radius as i32 as i64;
        if self.centre == centre && r1 == radius {
            return Some(self.start);
        }
        for p in circle_circle(self.centre, r1, centre, radius) {
            if self.slice_contains(p) {
                return Some(p);
            }
        }
        let mut best: Option<(i128, [i64; 2], [i64; 2])> = None;
        for p in [self.start, self.end, circle_nearest(self.centre, r1, centre)] {
            if self.slice_contains(p) {
                let q = circle_nearest(centre, radius, p);
                let d = ((p[0] - q[0]) as i128).pow(2) + ((p[1] - q[1]) as i128).pow(2);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, p, q));
                }
            }
        }
        let (_, mut a, b) = best?;
        let dir = crate::drc_exclusions::resize([b[0] - a[0], b[1] - a[1]], self.width / 2);
        a = [a[0] + dir[0], a[1] + dir[1]];
        Some([ki_round_half(a[0] + b[0]), ki_round_half(a[1] + b[1])])
    }
}

/// `EDA_ANGLE::Sin`, `Cos` of a normalized angle: exact at the multiples of 45 degrees.
fn sin_cos(a: f64) -> (f64, f64) {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    match a {
        x if x == 0.0 => (0.0, 1.0),
        x if x == 45.0 => (h, h),
        x if x == 90.0 => (1.0, 0.0),
        x if x == 135.0 => (h, -h),
        x if x == 180.0 => (0.0, -1.0),
        x if x == 225.0 => (-h, -h),
        x if x == 270.0 => (-1.0, 0.0),
        x if x == 315.0 => (-h, h),
        x => x.to_radians().sin_cos(),
    }
}

/// `CIRCLE::NearestPoint( const VECTOR2I& )`: `vec.Resize( Radius ) + Center`.
fn circle_nearest(c: [i64; 2], r: i64, p: [i64; 2]) -> [i64; 2] {
    let mut v = [p[0] - c[0], p[1] - c[1]];
    if v == [0, 0] {
        v[0] = 1;
    }
    let d = crate::drc_exclusions::resize(v, r);
    [d[0] + c[0], d[1] + c[1]]
}

/// `CIRCLE::Intersect( const CIRCLE& )`.
fn circle_circle(c1: [i64; 2], r1: i64, c2: [i64; 2], r2: i64) -> Vec<[i64; 2]> {
    let v = [c2[0] - c1[0], c2[1] - c1[1]];
    let d = rounded_hypot(v[0], v[1]);
    if d > r1 + r2 || d < (r1 - r2).abs() || d == 0 {
        return Vec::new();
    }
    let x = (d as i128 * d as i128 + r1 as i128 * r1 as i128 - r2 as i128 * r2 as i128) / (2 * d as i128);
    let rest = r1 as i128 * r1 as i128 - x * x;
    if rest < 0 {
        return Vec::new();
    }
    let y = kiround((rest as f64).sqrt());
    let rot = angle_of([v[0] as f64, v[1] as f64]);
    let place = |p: [i64; 2]| {
        let t = rotate_point(p, -rot);
        [t[0] + c1[0], t[1] + c1[1]]
    };
    let mut out = vec![place([x as i64, y])];
    if y != 0 {
        out.push(place([x as i64, -y]));
    }
    out
}

/// `Collide( SHAPE_ARC, <other> )` at the rule's clearance (the copper test's `sub_e( clearance )`), the arc first whatever the order (`CollCaseReversed`):
/// `(actual, location)`. `None` for a pair not ported (an arc against a rectangle, another arc).
pub fn collide_arc(arc: &Arc, other: &Copper, clearance: i64) -> Option<(i64, [i64; 2])> {
    if arc.is_effective_line() {
        return collide_single(&Copper::Segment { a: arc.start, b: arc.end, width: arc.width }, other);
    }
    match other {
        // `aA.Collide( aB.GetSeg(), aClearance + aB.GetWidth() / 2, .. )`, the segment's half width off the actual
        Copper::Segment { a, b, width } => arc.collide_seg([*a, *b], clearance + width / 2).map(|(act, at)| ((act - width / 2).max(0), at)),
        Copper::Circle { centre, radius } => arc.nearest_circle(*centre, *radius).map(|at| (0, at)),
        // `Collide( const SHAPE_ARC&, const SHAPE_LINE_CHAIN_BASE& )`: the arc's start inside, else each side's `Collide( SEG )`, the least, the first of equals
        Copper::Simple { pts } => {
            if point_inside(pts, arc.start) {
                return Some((0, arc.start));
            }
            let mut best: Option<(i64, [i64; 2])> = None;
            for s in sides(pts) {
                if let Some(hit) = arc.collide_seg(s, clearance) {
                    if best.is_none_or(|b| hit.0 < b.0) {
                        best = Some(hit);
                    }
                    if hit.0 == 0 {
                        break;
                    }
                }
            }
            best
        }
        Copper::Compound(parts) => {
            let mut best: Option<(i64, [i64; 2])> = None;
            for p in parts {
                let hit = collide_arc(arc, p, clearance)?;
                if best.is_none_or(|b| hit.0 < b.0) {
                    best = Some(hit);
                }
            }
            best
        }
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ candidates

/// The marker positions worth trying for a violation of `check` naming `main` and `aux` (the report's order; `aux` empty for one item), on the derived
/// board's shapes, beyond [`crate::drc_exclusions::marker_at`]'s one point; `clearance` is the rule's (the report's "clearance x mm"), when it says one.
pub fn candidates(check: &str, main: &str, aux: &str, board: &BoardShapes, clearance: Option<i64>) -> Vec<[i64; 2]> {
    let mut out = Vec::new();
    let collide_at = clearance.map_or(0, |c| (c - DRC_EPSILON_NM).max(0));
    match check {
        "clearance" | "shorting_items" => {
            // a via or a pad not flashed on the layer is its hole there (`!FlashLayer( layer )`: `GetEffectiveHoleShape`); whether it is flashed is the fill's
            for a in alternatives(main, board) {
                for b in alternatives(aux, board) {
                    out.extend(contact(&a, &b, collide_at));
                }
            }
            // an item against a zone's fill (`testItemAgainstZone`: `SetItems( aItem, aZone )`)
            if let Some(fills) = board.zone_fills.get(aux) {
                for a in alternatives(main, board) {
                    out.extend(against_fill(&a, fills, collide_at));
                }
            }
        }
        "courtyards_overlap" => {
            for side in 0..2 {
                if let (Some(a), Some(b)) = (board.courtyards.get(main).map(|c| &c[side]), board.courtyards.get(aux).map(|c| &c[side])) {
                    out.extend(polygon_pairs(a, b, clearance.unwrap_or(0)));
                }
            }
        }
        "silk_overlap" | "silk_over_copper" | "silk_edge_clearance" => {
            let shapes = |id: &str| -> Option<&[Copper]> { board.silk.get(id).map(|v| v.as_slice()).or_else(|| board.copper.get(id).map(std::slice::from_ref)).or_else(|| board.edges.get(id).map(std::slice::from_ref)) };
            if let (Some(a), Some(b)) = (shapes(main), shapes(aux)) {
                out.extend(subshape_pairs(a, b, clearance.unwrap_or(0)));
                out.extend(subshape_pairs(b, a, clearance.unwrap_or(0)));
            }
        }
        "copper_sliver" => {
            for (layer, fills) in &board.fills_by_layer {
                // the report names the layer (`SetErrorDetail( layerDesc( layer ) )`)
                if main.is_empty() || main == layer {
                    out.extend(sliver_points(fills));
                }
            }
        }
        "items_not_allowed" => {
            if let Some(item) = board.copper.get(main) {
                for (_, outline) in &board.keepouts {
                    out.extend(in_keepout(item, outline));
                }
            }
        }
        _ => {}
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// An item's copper on a layer, and its hole when it has one (a via or a pad that the fill leaves unflashed there).
fn alternatives(id: &str, board: &BoardShapes) -> Vec<Copper> {
    let mut out: Vec<Copper> = board.copper.get(id).cloned().into_iter().collect();
    if let Some(h) = board.holes.get(id) {
        out.push(h.clone());
    }
    out
}

/// `itemShape->Collide( otherShape, sub_e( clearance ), &actual, &pos )`'s place for two shapes, an arc's included (an arc needs the clearance).
fn contact(a: &Copper, b: &Copper, clearance: i64) -> Option<[i64; 2]> {
    let arc_of = |c: &Copper| match c {
        Copper::Arc { start, mid, end, width } => Some(Arc::new(*start, *mid, *end, *width)),
        _ => None,
    };
    match (arc_of(a), arc_of(b)) {
        (Some(arc), None) => collide_arc(&arc, b, clearance).map(|(_, at)| at),
        (None, Some(arc)) => collide_arc(&arc, a, clearance).map(|(_, at)| at),
        (Some(_), Some(_)) => None,
        (None, None) => crate::drc_exclusions::collide(a, b).map(|(_, at)| at),
    }
}

/// `zoneTree->QueryColliding( itemBBox, itemShape, layer, sub_e( clearance ), &actual, &pos )`: the item against each triangle of the fill, the least
/// `actual` winning (`Collide( SHAPE_LINE_CHAIN_BASE, <item> )`). Off the fill, the least distance is to the fill's outline and every triangle that reaches
/// it puts the marker where its side does: each side of the outline (and of its holes) at the least distance is a candidate. Inside the fill (`actual` 0) the
/// triangle the R-tree visits first wins: the item's own points and its crossings of the outline are offered.
fn against_fill(item: &Copper, fills: &[Vec<[i64; 2]>], clearance: i64) -> Vec<[i64; 2]> {
    let mut hits: Vec<(i64, [i64; 2])> = Vec::new();
    for poly in fills {
        for s in sides(poly) {
            let edge = Copper::Simple { pts: vec![s[0], s[1]] };
            let hit = match item {
                Copper::Compound(parts) => crate::drc_exclusions::least(parts.iter().map(|p| contact_actual(p, &edge, clearance))),
                other => contact_actual(other, &edge, clearance),
            };
            if let Some(h) = hit {
                hits.push(h);
            }
        }
    }
    let Some(min) = hits.iter().map(|h| h.0).min() else { return Vec::new() };
    let mut out: Vec<[i64; 2]> = hits.into_iter().filter(|h| h.0 <= min + 1).map(|h| h.1).collect();
    if min == 0 {
        out.extend(points_of(item));
    }
    out
}

fn contact_actual(a: &Copper, b: &Copper, clearance: i64) -> Option<(i64, [i64; 2])> {
    match a {
        Copper::Arc { start, mid, end, width } => collide_arc(&Arc::new(*start, *mid, *end, *width), b, clearance),
        _ => collide_single(a, b),
    }
}

/// The points of a shape a collision may be placed at when it is inside another: a segment's ends, a circle's centre, a polygon's corners.
fn points_of(c: &Copper) -> Vec<[i64; 2]> {
    match c {
        Copper::Segment { a, b, .. } => vec![*a, *b],
        Copper::Circle { centre, .. } => vec![*centre],
        Copper::Rect { p0, w, h } => vec![*p0, [p0[0] + w, p0[1]], [p0[0] + w, p0[1] + h], [p0[0], p0[1] + h], [p0[0] + w / 2, p0[1] + h / 2]],
        Copper::Simple { pts } => pts.clone(),
        Copper::Compound(parts) => parts.iter().flat_map(points_of).collect(),
        Copper::Arc { start, end, .. } => vec![*start, *end],
    }
}

/// Two polygons' `SHAPE_POLY_SET::Collide`, each over its triangulation (`AnyTriangleHit` of one's triangles against the other's): a triangle pair puts the
/// marker at a vertex of one inside the other, else at `SEG::NearestPoint` of an edge of each (`Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )`).
/// A triangle's corners are the polygon's own and its edges chords between them, so the candidates are every vertex of either within the other's reach
/// and the nearest points of every pair of chords at the least distance (`actual` 0 for an overlap). Polygons of more than 24 vertices are left out.
fn polygon_pairs(a: &[Vec<[i64; 2]>], b: &[Vec<[i64; 2]>], clearance: i64) -> Vec<[i64; 2]> {
    let mut out = Vec::new();
    for pa in a {
        for pb in b {
            if pa.len() > 24 || pb.len() > 24 || pa.len() < 3 || pb.len() < 3 {
                continue;
            }
            let chords = |p: &[[i64; 2]]| -> Vec<[[i64; 2]; 2]> { (0..p.len()).flat_map(|i| ((i + 1)..p.len()).map(move |j| [p[i], p[j]])).collect() };
            let (ca, cb) = (chords(pa), chords(pb));
            let mut least = i64::MAX;
            let mut pairs: Vec<(i64, [i64; 2])> = Vec::new();
            for x in &ca {
                for y in &cb {
                    let d = seg_dist(*x, *y);
                    if d <= clearance.max(0) {
                        least = least.min(d);
                        pairs.push((d, seg_nearest_to_seg(*x, *y)));
                        pairs.push((d, seg_nearest_to_seg(*y, *x)));
                    }
                }
            }
            if pairs.is_empty() {
                continue;
            }
            out.extend(pairs.into_iter().filter(|p| p.0 == least || least == 0 && p.0 == 0).map(|p| p.1));
            let reach = |p: [i64; 2], poly: &[[i64; 2]]| point_inside(poly, p) || sides(poly).iter().any(|s| seg_sq_distance_point(*s, p) <= (clearance.max(0) as i128).pow(2));
            out.extend(pa.iter().filter(|p| reach(**p, pb)));
            out.extend(pb.iter().filter(|p| reach(**p, pa)));
        }
    }
    out
}

/// Two silkscreen items' subshapes (`DRC_RTREE` holds each subshape of an effective shape on its own and `QueryCollidingPairs` reports the first pair that
/// collides): `refShape->Collide( testShape, minClearance, .. )` of every pair within the clearance, the report's first item the reference.
fn subshape_pairs(a: &[Copper], b: &[Copper], clearance: i64) -> Vec<[i64; 2]> {
    let flat = |s: &[Copper]| -> Vec<Copper> {
        s.iter()
            .flat_map(|c| match c {
                Copper::Compound(parts) => parts.clone(),
                other => vec![other.clone()],
            })
            .collect()
    };
    let (fa, fb) = (flat(a), flat(b));
    let mut hits = Vec::new();
    for x in &fa {
        for y in &fb {
            if let Some((actual, at)) = collide_single(x, y) {
                if actual <= clearance.max(0) {
                    hits.push((actual, at));
                }
            }
        }
    }
    // under a wide clearance every pair is a candidate: the nearest ones first, at most `MAX_PAIRS` (a record each in the project; a text has a pair per glyph stroke)
    hits.sort_by_key(|h| h.0);
    hits.into_iter().take(MAX_PAIRS).map(|h| h.1).collect()
}

/// The most subshape pairs offered for one silkscreen finding.
const MAX_PAIRS: usize = 128;

/// `zoneOutline->Collide( shape, 0, &actual, &pos )` of a keepout rule area's outline against an item (`DRC_TEST_PROVIDER_DISALLOW`): a track's
/// `SquaredDistanceToSeg` -- the middle of the segment when both ends are inside (`containsSingle`, accuracy 1), else the nearest point of the side
/// nearest it, the first of equals -- and a via's `SquaredDistance` of its centre (the centre when inside). Other shapes: the triangulation's, not ported.
fn in_keepout(item: &Copper, outline: &[[i64; 2]]) -> Option<[i64; 2]> {
    let inside = |p: [i64; 2]| point_inside(outline, p) || sides(outline).iter().any(|s| seg_sq_distance_point(*s, p) <= 1);
    match item {
        Copper::Segment { a, b, .. } => {
            if inside(*a) && inside(*b) {
                return Some([ki_round_half(a[0] + b[0]), ki_round_half(a[1] + b[1])]);
            }
            let mut best: Option<(i128, [i64; 2])> = None;
            for s in sides(outline) {
                let d = seg_sq_distance(s, [*a, *b]);
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, seg_nearest_to_seg(s, [*a, *b])));
                }
                if d == 0 {
                    break;
                }
            }
            best.map(|b| b.1)
        }
        Copper::Circle { centre, .. } => {
            if inside(*centre) {
                return Some(*centre);
            }
            let mut best: Option<(i128, [i64; 2])> = None;
            for s in sides(outline) {
                let d = seg_sq_distance_point(s, *centre);
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, seg_nearest_point(s, *centre)));
                }
            }
            best.map(|b| b.1)
        }
        _ => None,
    }
    .filter(|_| !outline.is_empty())
}

/// `DRC_TEST_PROVIDER_SLIVER_CHECKER::Run`'s points on a layer's copper: the fills made one polygon set (`Simplify`, which undoes their fracturing into
/// outlines and holes), then each outline's vertices walked as KiCad walks them -- points nearer than `m_SliverMinimumLength` (0.0008 mm) skipped on either
/// side, an angle under 90 degrees that is locally inside (`isLocallyInside`), sharper than `m_SliverAngleTolerance` (20 degrees) and wider across than
/// `m_SliverWidthTolerance` (0.08 mm) a sliver at that vertex. The items' own copper, which KiCad adds to the layer before it simplifies, is left out: a
/// sliver is a sharp corner of a fill, and an extra candidate costs nothing.
pub fn sliver_points(fills: &[Vec<[i64; 2]>]) -> Vec<[i64; 2]> {
    let mut set = eda_shape_poly_set::ShapePolySet::new();
    for f in fills {
        set.add_outline(f.iter().map(|p| eda_clipper2::Point64 { x: p[0], y: p[1], z: 0 }).collect());
    }
    set.simplify();
    let width_tol: i64 = 80_000;
    let squared_width = (width_tol as f64) * (width_tol as f64);
    let cos_tol = 2.0 * 20.0f64.to_radians().cos();
    let min_len: i64 = 800;
    let mut out = Vec::new();
    for poly in &set.polys {
        let Some(outline) = poly.first() else { continue };
        let pts: Vec<[i64; 2]> = outline.iter().map(|p| [p.x, p.y]).collect();
        let n = pts.len();
        if n <= 5 {
            continue;
        }
        let area = |p: [i64; 2], q: [i64; 2], r: [i64; 2]| (q[1] - p[1]) as i128 * (r[0] - q[0]) as i128 - (q[0] - p[0]) as i128 * (r[1] - q[1]) as i128;
        let locally_inside = |a: usize, b: usize| {
            let prev = (n + a - 1) % n;
            let next = (a + 1) % n;
            if area(pts[prev], pts[a], pts[next]) < 0 {
                area(pts[a], pts[b], pts[next]) >= 0 && area(pts[a], pts[prev], pts[b]) >= 0
            } else {
                area(pts[a], pts[b], pts[prev]) < 0 || area(pts[a], pts[next], pts[b]) < 0
            }
        };
        let mut kk = 0usize;
        while kk < n {
            let prior_index = (n + kk - 1) % n;
            let mut next_index = (kk + 1) % n;
            let mut pt = pts[kk];
            let pt_prior = pts[prior_index];
            let mut v_prior = [pt_prior[0] - pt[0], pt_prior[1] - pt[1]];
            let mut forward = 1usize;
            let mut offset = 1usize;
            while v_prior[0].abs() < min_len && v_prior[1].abs() < min_len && offset < n {
                pt = pts[(kk + offset) % n];
                offset += 1;
                v_prior = [pt_prior[0] - pt[0], pt_prior[1] - pt[1]];
            }
            if offset >= n {
                break;
            }
            let mut pt_after = pts[next_index];
            let mut v_after = [pt_after[0] - pt[0], pt_after[1] - pt[1]];
            while v_after[0].abs() < min_len && v_after[1].abs() < min_len && forward < n {
                next_index = (kk + forward) % n;
                forward += 1;
                pt_after = pts[next_index];
                v_after = [pt_after[0] - pt[0], pt_after[1] - pt[1]];
            }
            let dot = v_prior[0] as i128 * v_after[0] as i128 + v_prior[1] as i128 * v_after[1] as i128;
            if dot > 0 && locally_inside(prior_index, next_index) {
                let inc = [pt_after[0] - pt_prior[0], pt_after[1] - pt_prior[1]];
                let sqn = |v: [i64; 2]| (v[0] as f64) * (v[0] as f64) + (v[1] as f64) * (v[1] as f64);
                let (arm1, arm2, opp) = (sqn(v_prior), sqn(v_after), sqn(inc));
                let cos_ang = ((opp - arm1 - arm2) / (arm1.sqrt() * arm2.sqrt())).abs();
                if cos_ang > cos_tol && 2.0 - cos_ang > f32::EPSILON as f64 && opp > squared_width {
                    out.push(pt);
                }
            }
            kk += offset;
        }
    }
    out
}

/// The clearance a DRC description gives (`formatMsg( "(%s clearance %s; actual %s)" )`, millimetres to four places), in nanometres.
pub fn clearance_of(description: &str) -> Option<i64> {
    // the first "clearance " a number follows ("Hole clearance violation (netclass 'Default' clearance 0.2500 mm; ..")
    let rest = description.match_indices("clearance ").map(|(i, m)| &description[i + m.len()..]).find(|r| r.starts_with(|c: char| c.is_ascii_digit() || c == '-'))?;
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    let v: f64 = num.parse().ok()?;
    let unit = rest[num.len()..].trim_start();
    let nm = if unit.starts_with("mm") {
        v * 1_000_000.0
    } else if unit.starts_with("mils") || unit.starts_with("mil") {
        v * 25_400.0
    } else if unit.starts_with("in") {
        v * 25_400_000.0
    } else {
        v * 1_000_000.0
    };
    Some(nm.round() as i64)
}

/// A filled zone's chain-side test helper kept for the tests: `chain_seg` of a closed outline.
#[cfg(test)]
fn chain_place(pts: &[[i64; 2]], s: [[i64; 2]; 2]) -> [i64; 2] {
    crate::drc_exclusions::chain_seg(pts, s).1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CalcArcCenter` of a quarter circle on the 100 nm grid snaps to its centre; the arc's angles and slice.
    #[test]
    fn an_arcs_centre_angles_and_slice_are_kicads() {
        let arc = Arc::new([10_000_000, 0], [7_071_068, 7_071_068], [0, 10_000_000], 200_000);
        assert_eq!(arc.centre, [0, 0]);
        assert!((arc.radius - 10_000_000.0).abs() < 1.0);
        assert_eq!(arc.start_angle(), 0.0);
        assert!((arc.central_angle() - 90.0).abs() < 1e-6, "{}", arc.central_angle());
        assert!(arc.slice_contains([5_000_000, 5_000_000]) && !arc.slice_contains([-5_000_000, -5_000_000]));
        assert!(!arc.is_effective_line());
    }

    /// A track beside the arc: the candidates of `Collide( SEG )` -- the nearest point of the track to the centre is the one inside the clearance, its place
    /// the circle's nearest point.
    #[test]
    fn an_arc_and_a_track_meet_where_kicads_candidates_say() {
        let arc = Arc::new([10_000_000, 0], [7_071_068, 7_071_068], [0, 10_000_000], 200_000);
        // a track along y = x, from (7.2, 7.2) mm outwards: its nearest point to the centre is its start, 0.11 mm off the arc's edge
        let hit = arc.collide_seg([[7_200_000, 7_200_000], [9_000_000, 9_000_000]], 200_000 - DRC_EPSILON_NM + 100_000);
        let (actual, at) = hit.expect("collides");
        assert!(actual > 0);
        assert!((at[0] - 7_071_068).abs() <= 1 && (at[1] - 7_071_068).abs() <= 1, "{at:?}");
        assert!(arc.collide_seg([[20_000_000, 20_000_000], [21_000_000, 21_000_000]], 200_000).is_none());
    }

    #[test]
    fn a_descriptions_clearance_is_read() {
        assert_eq!(clearance_of("Clearance violation (netclass 'Default' clearance 0.2000 mm; actual 0.1500 mm)"), Some(200_000));
        assert_eq!(clearance_of("Courtyards overlap"), None);
        assert_eq!(clearance_of("Hole clearance violation (netclass 'Default' clearance 0.2500 mm; actual 0.1000 mm)"), Some(250_000));
    }

    /// Two overlapping squares: their corners inside each other and the crossings of their sides are offered, the place KiCad's triangles give among them.
    #[test]
    fn two_overlapping_courtyards_offer_their_corners_and_crossings() {
        let a = vec![vec![[0, 0], [10, 0], [10, 10], [0, 10]]];
        let b = vec![vec![[5, 5], [15, 5], [15, 15], [5, 15]]];
        let c = polygon_pairs(&a, &b, 0);
        assert!(c.contains(&[10, 10]) && c.contains(&[5, 5]), "the corners inside the other: {c:?}");
        assert!(c.contains(&[10, 5]) && c.contains(&[5, 10]), "the sides' crossings: {c:?}");
    }

    /// A track along a GND fill 0.15 mm off its edge: the marker is on the fill's side nearest the track (`Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_SEGMENT )`).
    #[test]
    fn a_track_off_a_fill_meets_its_nearest_side() {
        let fill = vec![vec![[0, 0], [10_000_000, 0], [10_000_000, 10_000_000], [0, 10_000_000]]];
        let track = Copper::Segment { a: [2_000_000, 10_250_000], b: [3_000_000, 10_250_000], width: 200_000 };
        let c = against_fill(&track, &fill, 200_000);
        let expected = chain_place(&[[10_000_000, 10_000_000], [0, 10_000_000]], [[2_000_000, 10_250_000], [3_000_000, 10_250_000]]);
        assert!(c.contains(&expected), "{c:?} {expected:?}");
    }

    /// `SquaredDistanceToSeg`: a track wholly in a keepout is placed at its middle, one crossing its edge at the crossing.
    #[test]
    fn a_track_in_a_keepout_is_placed_as_kicad_places_it() {
        let outline = vec![[0, 0], [10_000_000, 0], [10_000_000, 10_000_000], [0, 10_000_000]];
        let inside = Copper::Segment { a: [1_000_000, 1_000_000], b: [3_000_000, 1_000_001], width: 200_000 };
        assert_eq!(in_keepout(&inside, &outline), Some([2_000_000, 1_000_001]));
        let crossing = Copper::Segment { a: [5_000_000, 5_000_000], b: [15_000_000, 5_000_000], width: 200_000 };
        assert_eq!(in_keepout(&crossing, &outline), Some([10_000_000, 5_000_000]));
    }
}
