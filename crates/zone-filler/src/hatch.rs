//! The hatch fill: `ZONE_FILLER::addHatchFillTypeOnZone` and `buildHatchZoneThermalRings`
//! (`pcbnew/zone_filler.cpp`, vendored snapshot `8303b2ad`).
//!
//! A hatched zone is a solid fill with a grid of square holes cut out of it. Every grid cell is
//! `hatch_gap + min_thickness` wide (the holes are measured on a fill that has already been deflated by half the
//! minimum thickness, so the webbing that is left is `hatch_thickness` wide and the open window `hatch_gap`), laid out
//! on a lattice of pitch `max(hatch_thickness, min_thickness + 1 um) + hatch_gap` that is turned by the zone's hatch
//! orientation. The holes are
//!
//! * optionally smoothed: chamfered (level 1) or filleted (level 2 and up) by `hatch_smoothing_value` of half a gap;
//! * clipped to the fill deflated by the web thickness, and to the zone outline deflated by the minimum thickness, so
//!   the border of the zone stays solid;
//! * dropped when what is left of one is smaller than `hatch_hole_min_area` of a full hole;
//! * dropped when one swallows a thermal ring or a pad clearance entirely, so a thermal relief stays attached to the
//!   webbing.
//!
//! Pads and vias that are thermal-connected to a hatched zone are not spoked straight onto the webbing: each gets a
//! thermal *ring* (an arc ring round a circular pad or via, the pad outline grown by gap and by gap plus spoke width
//! otherwise) that the webbing joins (`buildHatchZoneThermalRings`).

use crate::shape::{self, Shape};
use crate::{corner, spokes};
use eda_clipper2::Point64;
use eda_model::ir::Zone;
use eda_shape_poly_set::{CornerStrategy, LineChain, ShapePolySet};

/// One pad or via in thermal relief in a hatched zone.
#[derive(Debug, Clone)]
pub struct RingItem {
    /// The copper outline (a via is a circle).
    pub shape: Shape,
    pub circular: bool,
    /// `padRadius`: half the pad's larger side, or the via's radius.
    pub radius: i64,
    pub position: Point64,
    pub gap: i64,
    pub spoke_width: i64,
}

/// `TransformRingToPolygon( ..., ERROR_OUTSIDE )`: an annulus whose outer edge errs outward and whose inner edge,
/// being a hole, errs the other way -- both on the safe side of the copper.
fn ring_polygon(centre: Point64, radius: i64, width: i64, max_error: i64) -> ShapePolySet {
    let inner_radius = radius - width / 2;
    let outer_radius = inner_radius + width;
    let outer = shape::shape_to_polygon_outside(&Shape::Circle { c: centre, r: outer_radius }, 0, max_error);
    let mut ring = ShapePolySet::from_outline(outer);
    if inner_radius > 0 {
        let inner = shape::exact_polygon(&Shape::Circle { c: centre, r: inner_radius }, max_error);
        ring.boolean_subtract(&ShapePolySet::from_outline(inner));
    }
    ring
}

/// `ZONE_FILLER::buildHatchZoneThermalRings`: the ring of every thermal-connected pad or via, clipped to the zone and
/// added both to the fill and to `rings` (kept apart so the hatch holes can be notched around them).
pub fn build_thermal_rings(items: &[RingItem], smoothed_outline: &ShapePolySet, fill: &mut ShapePolySet, rings: &mut ShapePolySet, max_error: i64) {
    for item in items {
        let mut ring = if item.circular {
            // Inner radius = pad radius + thermal gap; ring width = spoke width.
            let inner_radius = item.radius + item.gap;
            ring_polygon(item.position, inner_radius + item.spoke_width / 2, item.spoke_width, max_error)
        } else {
            // Outer ring edge = pad + gap + spoke width; inner = pad + gap (already knocked out).
            let outer = shape::shape_to_polygon_outside(&item.shape, item.gap + item.spoke_width, max_error);
            let inner = shape::shape_to_polygon_outside(&item.shape, item.gap, max_error);
            let mut r = ShapePolySet::from_outline(outer);
            r.boolean_subtract(&ShapePolySet::from_outline(inner));
            r
        };
        // Clip the thermal ring to the zone boundary so it doesn't overflow.
        ring.boolean_intersection(smoothed_outline);
        fill.boolean_add(&ring);
        rings.boolean_add(&ring);
    }
}

fn rotate_chain(chain: &LineChain, mdeg: i64) -> LineChain {
    chain
        .iter()
        .map(|p| {
            let (x, y) = spokes::rotate_point(p.x, p.y, mdeg);
            Point64::new(x, y)
        })
        .collect()
}

fn chain_bounds(chain: &LineChain) -> (i64, i64, i64, i64) {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in chain {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    (x0, y0, x1, y1)
}

fn even_odd(chain: &LineChain, pt: Point64) -> bool {
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

/// Whether segments `a1-a2` and `b1-b2` share a point (touching counts).
fn segments_intersect(a1: Point64, a2: Point64, b1: Point64, b2: Point64) -> bool {
    fn cross(o: Point64, a: Point64, b: Point64) -> i128 {
        (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
    }
    fn on_segment(a: Point64, b: Point64, p: Point64) -> bool {
        p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    }
    let d1 = cross(b1, b2, a1).signum();
    let d2 = cross(b1, b2, a2).signum();
    let d3 = cross(a1, a2, b1).signum();
    let d4 = cross(a1, a2, b2).signum();
    if d1 != d2 && d3 != d4 {
        return true;
    }
    (d1 == 0 && on_segment(b1, b2, a1)) || (d2 == 0 && on_segment(b1, b2, a2)) || (d3 == 0 && on_segment(a1, a2, b1)) || (d4 == 0 && on_segment(a1, a2, b2))
}

/// `SHAPE_LINE_CHAIN::Intersect( other )` non-empty.
fn chains_intersect(a: &LineChain, b: &LineChain) -> bool {
    let (na, nb) = (a.len(), b.len());
    let bb = chain_bounds(b);
    for i in 0..na {
        let (a1, a2) = (a[i], a[(i + 1) % na]);
        if a1.x.max(a2.x) < bb.0 || a1.x.min(a2.x) > bb.2 || a1.y.max(a2.y) < bb.1 || a1.y.min(a2.y) > bb.3 {
            continue;
        }
        for j in 0..nb {
            if segments_intersect(a1, a2, b[j], b[(j + 1) % nb]) {
                return true;
            }
        }
    }
    false
}

/// `ZONE_FILLER::addHatchFillTypeOnZone`: cuts the hatch holes out of `fill` (which has already been deflated by half the
/// minimum thickness). `rings` are the thermal rings and the clearance holes of unconnected pads, which keep the holes
/// from swallowing them.
pub fn add_hatch_fill(zone: &Zone, fill: &mut ShapePolySet, rings: &ShapePolySet, max_error: i64) {
    // The line thickness must exceed the zone's minimum thickness; one micron more keeps rounding from collapsing it.
    let thickness = zone.hatch_thickness.max(zone.min_thickness + 1);
    let gridsize = (thickness + zone.hatch_gap).max(1);
    let mut max_error = max_error;
    let orientation = zone.hatch_orientation_mdeg as i64;

    // A box that contains the fill rotated by the orientation; the holes are laid out on it and turned back.
    let mut filled = fill.clone();
    if orientation != 0 {
        for poly in &mut filled.polys {
            for chain in poly.iter_mut() {
                *chain = rotate_chain(chain, -orientation);
            }
        }
    }
    let (mut bx0, mut by0, mut bx1, mut by1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for poly in &filled.polys {
        let b = chain_bounds(&poly[0]);
        bx0 = bx0.min(b.0);
        by0 = by0.min(b.1);
        bx1 = bx1.max(b.2);
        by1 = by1.max(b.3);
    }
    if bx0 > bx1 {
        return;
    }

    // The hole is `hatch_gap` wide, but the fill's edges are `min_thickness` thick, so the hole shape is larger.
    let hole_size = zone.hatch_gap + zone.min_thickness;
    let mut hole_base: LineChain = vec![Point64::new(0, 0), Point64::new(hole_size, 0), Point64::new(hole_size, hole_size), Point64::new(0, hole_size)];

    // Holes smaller than this are dropped once clipped.
    let minimal_hole_area = (hole_size as f64 * hole_size as f64) * zone.hatch_hole_min_area;

    // Smoothed holes.
    if zone.hatch_smoothing_level > 0 {
        // Half a gap, scaled by the smoothing value (1.0 is a radius of half the hole).
        let mut smooth_value = (zone.hatch_gap as f64 * zone.hatch_smoothing_value / 2.0).round() as i64;
        const SMOOTH_MIN_VAL_UM: i64 = 20; // SMOOTH_MIN_VAL_MM 0.02
        const SMOOTH_SMALL_VAL_UM: i64 = 40; // SMOOTH_SMALL_VAL_MM 0.04
        if smooth_value > SMOOTH_MIN_VAL_UM {
            let mut smooth_level = zone.hatch_smoothing_level;
            // A small smoothing is a chamfer, even if a fillet is asked for.
            if smooth_value < SMOOTH_SMALL_VAL_UM && smooth_level > 1 {
                smooth_level = 1;
            }
            // A larger value compensates for the outline thickness (the chamfer is invisible below it).
            smooth_value += zone.min_thickness / 2;
            // It cannot be bigger than half the hole.
            smooth_value = smooth_value.min(zone.hatch_gap / 2);
            // The error to approximate a circle by segments when rounding a corner by an arc.
            max_error = (max_error * 2).max(smooth_value / 20);
            let smooth_hole = ShapePolySet::from_outline(hole_base.clone());
            match smooth_level {
                1 => {
                    if let Some(c) = corner::chamfer(&smooth_hole, smooth_value).polys.into_iter().next().and_then(|p| p.into_iter().next()) {
                        hole_base = c;
                    }
                }
                _ => {
                    if zone.hatch_smoothing_level > 2 {
                        max_error /= 2; // force better smoothing
                    }
                    if let Some(c) = corner::fillet(&smooth_hole, smooth_value, max_error).polys.into_iter().next().and_then(|p| p.into_iter().next()) {
                        hole_base = c;
                    }
                }
            }
        }
    }

    // Build the holes. (The per-layer `hatching_offset` of board and zone is not modelled: no offset.)
    let mut holes = ShapePolySet::new();
    let x_offset = bx0 - (bx0 % gridsize) - gridsize;
    let y_offset = by0 - (by0 % gridsize) - gridsize;
    let mut xx = x_offset;
    while xx <= bx1 {
        let mut yy = y_offset;
        while yy <= by1 {
            let mut hole: LineChain = hole_base.iter().map(|p| Point64::new(p.x + xx, p.y + yy)).collect();
            if orientation != 0 {
                hole = rotate_chain(&hole, orientation);
            }
            holes.add_outline(hole);
            yy += gridsize;
        }
        xx += gridsize;
    }

    // The fill has already been deflated to ensure the minimum thickness, so only what the web needs beyond that.
    let mut deflated_thickness = zone.hatch_thickness - zone.min_thickness;
    // Don't let the thickness drop below max_error * 2 or it might not get reinflated.
    deflated_thickness = deflated_thickness.max(max_error * 2);

    let mut deflated_fill = fill.clone();
    deflated_fill.deflate(deflated_thickness, CornerStrategy::ChamferAllCorners, max_error as i32);
    holes.boolean_intersection(&deflated_fill);

    let mut deflated_outline = ShapePolySet::from_outline(crate::chain_from_ir(&zone.outline));
    deflated_outline.deflate(zone.min_thickness, CornerStrategy::ChamferAllCorners, max_error as i32);
    holes.boolean_intersection(&deflated_outline);

    // Now filter truncated holes to avoid small holes in the pattern; it happens for holes near the zone outline.
    holes.polys.retain(|poly| eda_clipper2::area(&poly[0]).abs() >= minimal_hole_area);

    // Drop any holes that completely enclose a thermal ring (or a pad clearance) so thermal reliefs stay connected to the
    // webbing. Only a ring entirely inside the hole counts; partial overlaps are kept to preserve the pattern.
    if rings.outline_count() > 0 {
        let ring_boxes: Vec<(i64, i64, i64, i64)> = rings.polys.iter().map(|p| chain_bounds(&p[0])).collect();
        let (mut tx0, mut ty0, mut tx1, mut ty1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for b in &ring_boxes {
            tx0 = tx0.min(b.0);
            ty0 = ty0.min(b.1);
            tx1 = tx1.max(b.2);
            ty1 = ty1.max(b.3);
        }
        holes.polys.retain(|poly| {
            let hole = &poly[0];
            let hb = chain_bounds(hole);
            if hb.2 < tx0 || hb.0 > tx1 || hb.3 < ty0 || hb.1 > ty1 {
                return true;
            }
            for (ring_poly, rb) in rings.polys.iter().zip(&ring_boxes) {
                let ring = &ring_poly[0];
                // The hole's box must contain the ring's.
                if !(hb.0 <= rb.0 && hb.1 <= rb.1 && hb.2 >= rb.2 && hb.3 >= rb.3) {
                    continue;
                }
                let centre = Point64::new((rb.0 + rb.2) / 2, (rb.1 + rb.3) / 2);
                if !even_odd(hole, centre) {
                    continue;
                }
                if ring.is_empty() || !even_odd(hole, ring[0]) {
                    continue;
                }
                // No crossing: the ring is fully enclosed, not touching the hole's edges.
                if !chains_intersect(ring, hole) {
                    return false;
                }
            }
            true
        });
    }

    // Create the grid. Used to generate strictly simple polygons, needed by Gerber files and Fracture().
    fill.boolean_subtract(&holes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FillMode, Point};

    fn square_zone() -> Zone {
        let pt = |x, y| Point { x, y };
        Zone {
            net: "GND".into(),
            layer: "F.Cu".into(),
            outline: vec![pt(0, 0), pt(20_000, 0), pt(20_000, 20_000), pt(0, 20_000)],
            fill_mode: FillMode::HatchPattern,
            min_thickness: 250,
            hatch_thickness: 1_000,
            hatch_gap: 1_500,
            ..Default::default()
        }
    }

    #[test]
    fn a_hatch_cuts_a_grid_of_holes_out_of_the_fill() {
        let zone = square_zone();
        let mut fill = ShapePolySet::from_outline(vec![Point64::new(125, 125), Point64::new(19_875, 125), Point64::new(19_875, 19_875), Point64::new(125, 19_875)]);
        let before = fill.area();
        add_hatch_fill(&zone, &mut fill, &ShapePolySet::new(), 5);
        let after = fill.area();
        assert!(after < before * 0.8, "holes must remove a good part: {after} of {before}");
        assert!(after > before * 0.25, "the webbing and the border stay: {after} of {before}");
        // the zone border is solid: a point just inside the corner is copper, the middle of a window is not
        assert!(spokes::poly_set_contains(&fill, Point64::new(300, 300)));
    }

    #[test]
    fn a_thermal_ring_is_an_annulus_round_a_circular_pad() {
        let item = RingItem { shape: Shape::Circle { c: Point64::new(0, 0), r: 500 }, circular: true, radius: 500, position: Point64::new(0, 0), gap: 300, spoke_width: 400 };
        let mut fill = ShapePolySet::new();
        let mut rings = ShapePolySet::new();
        let big = ShapePolySet::from_outline(vec![Point64::new(-5_000, -5_000), Point64::new(5_000, -5_000), Point64::new(5_000, 5_000), Point64::new(-5_000, 5_000)]);
        build_thermal_rings(&[item], &big, &mut fill, &mut rings, 5);
        // inner radius 800, outer 1200: area pi (1200^2 - 800^2)
        let want = std::f64::consts::PI * (1200.0f64.powi(2) - 800.0f64.powi(2));
        assert!((rings.area() - want).abs() / want < 0.02, "{} vs {want}", rings.area());
        assert!(!spokes::poly_set_contains(&rings, Point64::new(0, 0)), "the pad and its gap stay open");
        assert!(spokes::poly_set_contains(&rings, Point64::new(1_000, 0)));
    }
}
