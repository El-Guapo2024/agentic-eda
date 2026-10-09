//! Thermal relief spokes: `ZONE_FILLER::buildThermalSpokes` and the spoke-keep
//! hit-test of `fillCopperZone` (`pcbnew/zone_filler.cpp`, vendored snapshot
//! `8303b2ad`).
//!
//! A relief has four square-ended spokes from the pad's centre to just outside
//! its thermal gap. Each is a closed 5-point polygon whose point 3 (the middle
//! of the outer end) is the *test point*: the spoke is kept only if that point
//! lands on copper of the zone body, or on the test point of another spoke that
//! reaches back (two reliefs bridging to each other).
//!
//! What follows `buildThermalSpokes` is followed point for point:
//!
//! * the spokes are cut to the pad's *un-rotated* bounding box grown by
//!   `gap + epsilon + half the zone's minimum width` (the fill is deflated by
//!   that half-width afterwards, which would otherwise shorten the spoke past
//!   the gap), then rotated by the pad's own orientation;
//! * a rectangular or oval pad (default spoke angle 90 degrees) casts its four
//!   rays through the bounding-box edge they meet first, so an angle other
//!   than a multiple of 90 degrees gives the parallelogram spokes of
//!   `intersectBBox`;
//! * a circular pad, or a square oval, or a via (default angle 45 degrees)
//!   builds its four spokes at 0 degrees and rotates them afterwards, because
//!   the box of a circle overshoots near a 45 degree ray;
//! * a custom pad's proxy-segment spokes are not here: this workspace has no
//!   custom pad shape.

use eda_clipper2::{Point64, PI};
use eda_shape_poly_set::{LineChain, ShapePolySet};

/// One thermal spoke: a closed 5-point polygon (`outline`) and its test point
/// (`outline[3]`, upstream's index-3 point).
#[derive(Debug, Clone)]
pub struct Spoke {
    pub outline: LineChain,
    pub test_point: Point64,
}

/// What `buildThermalSpokes` needs to know about one pad (or hatch-zone via).
#[derive(Debug, Clone, Copy)]
pub struct SpokeSource {
    /// `PAD::ShapePos` (the via's position).
    pub position: Point64,
    /// The size of the pad's bounding box at orientation 0 and position 0 (`dummy_pad.GetBoundingBox`); a via's diameter twice.
    pub size: (i64, i64),
    /// `CIRCLE`, or `OVAL` with `size.x == size.y`, or a via.
    pub circular: bool,
    /// `PAD::GetThermalSpokeAngle`, millidegrees in KiCad's sign convention.
    pub angle_mdeg: i64,
    /// `PAD::GetOrientation`, millidegrees in KiCad's sign convention; 0 for a via.
    pub orientation_mdeg: i64,
    /// The relief gap that applies to this item (pad override over the zone's).
    pub thermal_gap: i64,
    /// The spoke width, already clamped to the pad and checked against the zone's minimum thickness.
    pub spoke_width: i64,
}

/// `KiROUND`: round half away from zero.
#[inline]
fn ki_round(v: f64) -> i64 {
    v.round() as i64
}

/// `EDA_ANGLE::Cos()`: exact for multiples of 45 degrees.
pub fn angle_cos(mdeg: i64) -> f64 {
    let v = mdeg.rem_euclid(360_000);
    match v {
        0 => 1.0,
        180_000 => -1.0,
        90_000 | 270_000 => 0.0,
        45_000 | 315_000 => std::f64::consts::FRAC_1_SQRT_2,
        135_000 | 225_000 => -std::f64::consts::FRAC_1_SQRT_2,
        _ => (v as f64 / 1000.0 * PI / 180.0).cos(),
    }
}

/// `EDA_ANGLE::Sin()`: exact for multiples of 45 degrees.
pub fn angle_sin(mdeg: i64) -> f64 {
    let v = mdeg.rem_euclid(360_000);
    match v {
        0 | 180_000 => 0.0,
        45_000 | 135_000 => std::f64::consts::FRAC_1_SQRT_2,
        225_000 | 315_000 => -std::f64::consts::FRAC_1_SQRT_2,
        90_000 => 1.0,
        270_000 => -1.0,
        _ => (v as f64 / 1000.0 * PI / 180.0).sin(),
    }
}

/// `RotatePoint( int*, int*, EDA_ANGLE )`: counter-clockwise on screen for a positive angle (`x' = y sin + x cos`,
/// `y' = y cos - x sin` in the y-down frame), exact for the quarter turns.
pub fn rotate_point(x: i64, y: i64, mdeg: i64) -> (i64, i64) {
    match mdeg.rem_euclid(360_000) {
        0 => (x, y),
        90_000 => (y, -x),
        180_000 => (-x, -y),
        270_000 => (-y, x),
        v => {
            let (s, c) = (angle_sin(v), angle_cos(v));
            (ki_round(y as f64 * s + x as f64 * c), ki_round(y as f64 * c - x as f64 * s))
        }
    }
}

/// `buildSpokesFromOrigin` for one pad (`box` is the inflated bounding box, centred on the origin): the four spokes
/// for rays at `angle + {0, 90, 180, 270}` degrees.
fn spokes_from_origin(half_size: (i64, i64), spoke_half_w: i64, angle_mdeg: i64) -> Vec<Vec<(i64, i64)>> {
    let (hx, hy) = (half_size.0 as f64, half_size.1 as f64);
    let mut out = Vec::with_capacity(4);
    for k in 0..4 {
        let a = angle_mdeg + 90_000 * k;
        let (dx, dy) = (angle_cos(a), angle_sin(a));
        // `intersectBBox`
        let (inter, side): ((i64, i64), (i64, i64)) = if dx == 0.0 {
            ((0, ki_round(dy * hy)), (spoke_half_w, 0))
        } else if dy == 0.0 {
            ((ki_round(dx * hx), 0), (0, spoke_half_w))
        } else {
            let dist_x = hx / dx.abs();
            let dist_y = hy / dy.abs();
            if dist_x < dist_y {
                ((ki_round(dx * dist_x), ki_round(dy * dist_x)), (0, ki_round(spoke_half_w as f64 / angle_sin(90_000 - a))))
            } else {
                ((ki_round(dx * dist_y), ki_round(dy * dist_y)), (ki_round(spoke_half_w as f64 / angle_sin(a)), 0))
            }
        };
        out.push(vec![(side.0, side.1), (-side.0, -side.1), (inter.0 - side.0, inter.1 - side.1), (inter.0, inter.1), (inter.0 + side.0, inter.1 + side.1)]);
    }
    out
}

/// `ZONE_FILLER::buildThermalSpokes` for one pad or via: four spokes. `zone_half_width` is half the zone's minimum
/// thickness (half its hatch thickness in a hatch zone), `epsilon` the fill's `m_MaxError`.
pub fn build_spokes(src: &SpokeSource, zone_half_width: i64, epsilon: i64) -> Vec<Spoke> {
    let inflate = src.thermal_gap + epsilon + zone_half_width;
    // `box.GetWidth() / 2.0` of the inflated bounding box.
    let half_size = (ki_round((src.size.0 + 2 * inflate) as f64 / 2.0), ki_round((src.size.1 + 2 * inflate) as f64 / 2.0));
    let spoke_half_w = src.spoke_width / 2;

    // A circle's box overshoots near a 45 degree ray: build the spokes at 0 degrees and turn them.
    let mut raw = if src.circular { spokes_from_origin(half_size, spoke_half_w, 0) } else { spokes_from_origin(half_size, spoke_half_w, src.angle_mdeg) };
    if src.circular && src.angle_mdeg.rem_euclid(360_000) != 0 {
        for spoke in &mut raw {
            for p in spoke.iter_mut() {
                *p = rotate_point(p.0, p.1, src.angle_mdeg);
            }
        }
    }
    raw.into_iter()
        .map(|spoke| {
            let outline: LineChain = spoke
                .into_iter()
                .map(|(x, y)| {
                    let (rx, ry) = rotate_point(x, y, src.orientation_mdeg);
                    Point64::new(src.position.x + rx, src.position.y + ry)
                })
                .collect();
            Spoke { test_point: outline[3], outline }
        })
        .collect()
}

/// `SHAPE_POLY_SET::Contains` as `POLY_YSTRIPES_INDEX::Contains( pt, 1 )` has it: even-odd ray crossing against every
/// edge of an outline and of its holes, true if any polygon of the set is crossed an odd number of times. No boundary
/// tolerance (an accuracy of 1 skips the edge test).
pub fn contains_even_odd(sps: &ShapePolySet, pt: Point64) -> bool {
    'poly: for poly in &sps.polys {
        // bounding-box reject on the outline
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for p in &poly[0] {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if pt.x < x0 || pt.x > x1 || pt.y < y0 || pt.y > y1 {
            continue 'poly;
        }
        let mut inside = false;
        for chain in poly {
            let n = chain.len();
            if n < 3 {
                continue;
            }
            for i in 0..n {
                let (p1, p2) = (chain[i], chain[(i + 1) % n]);
                if (p1.y >= pt.y) == (p2.y >= pt.y) {
                    continue;
                }
                let d = (p2.x - p1.x) as f64 * (pt.y - p1.y) as f64 / (p2.y - p1.y) as f64;
                if ((pt.x - p1.x) as f64) < d.trunc() {
                    inside = !inside;
                }
            }
        }
        if inside {
            return true;
        }
    }
    false
}

/// `SHAPE_LINE_CHAIN::PointInside( pt, 1 )` for one closed chain: plain even-odd.
fn chain_contains(chain: &LineChain, pt: Point64) -> bool {
    let n = chain.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..n {
        let (p1, p2) = (chain[i], chain[(i + 1) % n]);
        if (p1.y >= pt.y) == (p2.y >= pt.y) {
            continue;
        }
        let d = (p2.x - p1.x) as f64 * (pt.y - p1.y) as f64 / (p2.y - p1.y) as f64;
        if ((pt.x - p1.x) as f64) < d.trunc() {
            inside = !inside;
        }
    }
    inside
}

fn bounds(chain: &LineChain) -> (i64, i64, i64, i64) {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in chain {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x0, y0, x1, y1)
}

#[inline]
fn bboxes_overlap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0.max(b.0) <= a.2.min(b.2) && a.1.max(b.1) <= a.3.min(b.3)
}

/// `SHAPE_POLY_SET::Contains` (the piece of it this crate's own tests need): is `pt` inside some outline and not inside
/// any of that outline's holes. Boundary counts as inside the outline and as inside a hole.
pub fn poly_set_contains(sps: &ShapePolySet, pt: Point64) -> bool {
    use eda_clipper2::{point_in_polygon, PointInPolygonResult};
    for poly in &sps.polys {
        if point_in_polygon(pt, &poly[0]) == PointInPolygonResult::IsOutside {
            continue;
        }
        let in_a_hole = poly[1..].iter().any(|h| point_in_polygon(pt, h) != PointInPolygonResult::IsOutside);
        if !in_a_hole {
            return true;
        }
    }
    false
}

/// `fillCopperZone`'s spoke-keep loop: keep a spoke if its test point lands on copper (`test_areas`, hit-tested with
/// `POLY_YSTRIPES_INDEX::Contains( pt, 1 )`), or if it and another spoke contain each other's test point (two reliefs
/// bridging to one another, e.g. closely spaced pads).
///
/// The pairwise scan is inherently O(spokes^2), same as upstream's inner loop; a bounding-box pre-filter keeps the
/// constant factor down on boards with many reliefs.
pub fn keep_spokes(spokes: &[Spoke], test_areas: &ShapePolySet) -> Vec<usize> {
    let boxes: Vec<(i64, i64, i64, i64)> = spokes.iter().map(|s| bounds(&s.outline)).collect();
    let mut keep = Vec::new();
    for (i, spoke) in spokes.iter().enumerate() {
        if contains_even_odd(test_areas, spoke.test_point) {
            keep.push(i);
            continue;
        }
        for (j, other) in spokes.iter().enumerate() {
            if i == j || !bboxes_overlap(boxes[i], boxes[j]) {
                continue;
            }
            // Hit test in both directions to avoid interactions with round-off errors (issue 13316).
            if chain_contains(&other.outline, spoke.test_point) && chain_contains(&spoke.outline, other.test_point) {
                keep.push(i);
                break;
            }
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(circular: bool, angle: i64, orientation: i64) -> SpokeSource {
        SpokeSource { position: Point64::new(1000, 2000), size: (1000, 600), circular, angle_mdeg: angle, orientation_mdeg: orientation, thermal_gap: 200, spoke_width: 300 }
    }

    #[test]
    fn a_default_rectangular_pad_gets_four_axis_aligned_spokes() {
        let spokes = build_spokes(&src(false, 90_000, 0), 125, 5);
        assert_eq!(spokes.len(), 4);
        // gap 200 + epsilon 5 + zone half width 125 = 330 around the 1000 x 600 box: half sizes 830 and 630.
        let mut ends: Vec<(i64, i64)> = spokes.iter().map(|s| (s.test_point.x - 1000, s.test_point.y - 2000)).collect();
        ends.sort();
        assert_eq!(ends, vec![(-830, 0), (0, -630), (0, 630), (830, 0)]);
        // the spoke is 300 wide: the outline's points 0 and 1 straddle the centre at +-150.
        for s in &spokes {
            assert_eq!(s.outline.len(), 5);
            assert_eq!(s.outline[3], s.test_point);
        }
    }

    #[test]
    fn a_pad_turned_a_quarter_turn_turns_its_spokes_with_it() {
        let upright = build_spokes(&src(false, 90_000, 0), 125, 5);
        let turned = build_spokes(&src(false, 90_000, 90_000), 125, 5);
        let mut a: Vec<(i64, i64)> = upright.iter().map(|s| (s.test_point.x - 1000, s.test_point.y - 2000)).collect();
        let mut b: Vec<(i64, i64)> = turned.iter().map(|s| (s.test_point.x - 1000, s.test_point.y - 2000)).collect();
        // KiCad's quarter turn sends (x, y) to (y, -x): the long x spokes become long y spokes.
        a.sort();
        b.sort();
        assert_eq!(a, vec![(-830, 0), (0, -630), (0, 630), (830, 0)]);
        assert_eq!(b, vec![(-630, 0), (0, -830), (0, 830), (630, 0)]);
    }

    #[test]
    fn a_circular_pad_defaults_to_the_x_shape() {
        let s = SpokeSource { position: Point64::new(0, 0), size: (1000, 1000), circular: true, angle_mdeg: 45_000, orientation_mdeg: 0, thermal_gap: 200, spoke_width: 300 };
        let spokes = build_spokes(&s, 125, 5);
        for sp in &spokes {
            let (x, y) = (sp.test_point.x, sp.test_point.y);
            assert_eq!(x.abs(), y.abs(), "a diagonal ray: {x},{y}");
            let r = ((x * x + y * y) as f64).sqrt();
            assert!((r - 830.0).abs() < 2.0, "radius {r}");
        }
    }

    #[test]
    fn an_off_axis_angle_cuts_the_spoke_at_the_bounding_box_edge() {
        // a 30 degree ray on a 1000 x 600 box meets the right edge first (x/cos < y/sin).
        let spokes = build_spokes(&src(false, 30_000, 0), 125, 5);
        let sp = &spokes[0];
        let (x, y) = (sp.test_point.x - 1000, sp.test_point.y - 2000);
        assert_eq!(x, 830, "the ray ends on the box's right edge");
        assert!((y as f64 - 830.0 * (30.0f64).to_radians().tan()).abs() < 2.0);
        // the end of the spoke is a vertical cut of height 2 * (width / 2) / cos(30)
        let h = (sp.outline[4].y - sp.outline[2].y).abs();
        assert!((h as f64 - 300.0 / (30.0f64).to_radians().cos()).abs() < 3.0, "end cut {h}");
    }
}
