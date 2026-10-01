//! Thermal relief spoke construction (`ZONE_FILLER::buildThermalSpokes`) and
//! the spoke-keep hit-test from `fillCopperZone`.
//!
//! Scope note: this ports exactly the axis-aligned special case of
//! `buildSpokesFromOrigin`'s `intersectBBox` (the `dx == 0`/`dy == 0`
//! branches -- i.e. `angle == 0`), which is what every pad at 0/90/180/270
//! degree rotation produces -- the same rotation assumption
//! `eda_drc::kimath`'s pad shapes already make throughout this workspace
//! (see that crate's module doc comment). Circular pads and vias get the
//! same 4-cardinal-spoke treatment as rectangular ones rather than
//! upstream's separate circular-pad spoke count/angle logic, and custom-pad
//! "proxy segment" spokes are not ported (this workspace has no custom pad
//! shape concept). Both are documented, bounded simplifications of a
//! cosmetic/robustness feature, not of the connectivity-relevant part of
//! the algorithm (whether a thermal-relief-connected pad ends up with
//! *some* copper path to the zone body).

use eda_clipper2::{point_in_polygon, Point64, PointInPolygonResult};
use eda_shape_poly_set::{LineChain, ShapePolySet};

/// One thermal spoke: a closed 5-point rectangle-with-a-midpoint, the
/// midpoint (`test_point`, upstream's index-3 point) used to hit-test
/// whether the spoke actually reaches copper.
#[derive(Debug, Clone)]
pub struct Spoke {
    pub outline: LineChain,
    pub test_point: Point64,
}

/// `buildSpokesFromOrigin`'s axis-aligned case: 4 cardinal spokes from the
/// center of `bbox` (a pad/via's own bounding box) out to `bbox` inflated by
/// `gap` (the thermal relief gap), each `spoke_width` wide.
pub fn build_spokes(bbox: (i64, i64, i64, i64), gap: i64, spoke_width: i64) -> Vec<Spoke> {
    let (x0, y0, x1, y1) = bbox;
    let center = Point64::new((x0 + x1) / 2, (y0 + y1) / 2);
    let half_size = ((x1 - x0) / 2 + gap, (y1 - y0) / 2 + gap);
    let half_w = spoke_width / 2;

    let dirs: [(i64, i64, i64, i64); 4] = [
        // (intersection.x, intersection.y, side.x, side.y)
        (half_size.0, 0, 0, half_w),  // +X
        (0, half_size.1, half_w, 0),  // +Y
        (-half_size.0, 0, 0, half_w), // -X
        (0, -half_size.1, half_w, 0), // -Y
    ];

    dirs.iter()
        .map(|&(ix, iy, sx, sy)| {
            let side = Point64::new(sx, sy);
            let inter = Point64::new(ix, iy);
            let outline = vec![
                Point64::new(center.x + side.x, center.y + side.y),
                Point64::new(center.x - side.x, center.y - side.y),
                Point64::new(center.x + inter.x - side.x, center.y + inter.y - side.y),
                Point64::new(center.x + inter.x, center.y + inter.y),
                Point64::new(center.x + inter.x + side.x, center.y + inter.y + side.y),
            ];
            Spoke { test_point: outline[3], outline }
        })
        .collect()
}

/// `SHAPE_POLY_SET::Contains` (the piece of it this crate needs): is `pt`
/// inside some outline and not inside any of that outline's holes.
pub fn poly_set_contains(sps: &ShapePolySet, pt: Point64) -> bool {
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

/// `fillCopperZone`'s spoke-keep loop: keep a spoke if its test point lands
/// on copper (`test_areas`), or if it mutually overlaps another kept-or-not
/// spoke's test point (two spokes bridging to each other rather than to the
/// zone body directly, e.g. closely spaced pads).
///
/// The pairwise mutual-overlap scan is inherently O(spokes^2) (every spoke
/// potentially needs checking against every other), same as upstream's own
/// `for (other : thermalSpokes)` inner loop; a bounding-box pre-filter (no
/// `ShapePolySet` allocation, no `point_in_polygon` walk, for the common
/// case of two spokes nowhere near each other) keeps the *constant* factor
/// of that O(n^2) down for boards with many thermal-relief pads.
pub fn keep_spokes(spokes: &[Spoke], test_areas: &ShapePolySet) -> Vec<usize> {
    let boxes: Vec<(i64, i64, i64, i64)> = spokes.iter().map(|s| bounds(&s.outline)).collect();
    let mut keep = Vec::new();
    for (i, spoke) in spokes.iter().enumerate() {
        if poly_set_contains(test_areas, spoke.test_point) {
            keep.push(i);
            continue;
        }
        for (j, other) in spokes.iter().enumerate() {
            if i == j || !bboxes_overlap(boxes[i], boxes[j]) {
                continue;
            }
            if point_in_polygon(spoke.test_point, &other.outline) != PointInPolygonResult::IsOutside
                && point_in_polygon(other.test_point, &spoke.outline) != PointInPolygonResult::IsOutside
            {
                keep.push(i);
                break;
            }
        }
    }
    keep
}
