//! `SHAPE_POLY_SET::Chamfer` and `Fillet` (`libs/kimath/src/geometry/corner_operations.cpp`,
//! `chamferFilletPolygon`): every corner of every contour is cut by `distance` (chamfer) or rounded with a circular arc
//! of `radius` (fillet), never taking more than half of either adjoining edge. The zone's corner smoothing
//! (`ZONE::BuildSmoothedPoly`) and the hatch fill's smoothed holes (`addHatchFillTypeOnZone`) are the callers.

use eda_clipper2::Point64;
use eda_shape_poly_set::{LineChain, Polygon, ShapePolySet};

#[inline]
fn ki_round(v: f64) -> i64 {
    v.round() as i64
}

/// `GetArcToSegmentCount( radius, error, angle )` (`geometry_utils.cpp`).
fn arc_segment_count(radius: i64, error_max: i64, arc_angle_rad: f64) -> i32 {
    let radius = radius.max(1) as f64;
    let error_max = error_max.max(1) as f64;
    let rel_error = error_max / radius;
    let mut arc_increment = 180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0;
    arc_increment = arc_increment.min(360.0 / 8.0);
    let seg_count = ki_round(arc_angle_rad.to_degrees().abs() / arc_increment) as i32;
    seg_count.max(2)
}

/// `RemoveNullSegments`: drop a point equal to the one before it (the closing wrap-around included).
fn without_null_segments(chain: &LineChain) -> LineChain {
    let mut out: LineChain = Vec::with_capacity(chain.len());
    for &p in chain {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Chamfered,
    Filleted,
}

fn corner_contour(contour: &LineChain, mode: Mode, distance: i64, error_max: i64) -> LineChain {
    let n = contour.len();
    let mut out: LineChain = Vec::new();
    for v in 0..n {
        let (x1, y1) = (contour[v].x, contour[v].y);
        let prev = if v == 0 { n - 1 } else { v - 1 };
        let next = if v == n - 1 { 0 } else { v + 1 };
        let (xa, ya) = ((contour[prev].x - x1) as f64, (contour[prev].y - y1) as f64);
        let (xb, yb) = ((contour[next].x - x1) as f64, (contour[next].y - y1) as f64);

        // Avoid segments that would generate NaNs below.
        if (xa + xb).abs() < f64::EPSILON && (ya + yb).abs() < f64::EPSILON {
            continue;
        }
        let lena = xa.hypot(ya);
        let lenb = xb.hypot(yb);

        match mode {
            Mode::Chamfered => {
                let mut d = distance as f64;
                // Chamfer one half of an edge at most.
                if 0.5 * lena < d {
                    d = 0.5 * lena;
                }
                if 0.5 * lenb < d {
                    d = 0.5 * lenb;
                }
                out.push(Point64::new(x1 + ki_round(d * xa / lena), y1 + ki_round(d * ya / lena)));
                out.push(Point64::new(x1 + ki_round(d * xb / lenb), y1 + ki_round(d * yb / lenb)));
            }
            Mode::Filleted => {
                let cosine = (xa * xb + ya * yb) / (lena * lenb);
                let mut radius = distance as f64;
                let denom = (2.0 / (1.0 + cosine) - 1.0).sqrt();
                // Do nothing in case of parallel edges.
                if denom.is_infinite() {
                    continue;
                }
                // Limit rounding distance to one half of an edge.
                if 0.5 * lena * denom < radius {
                    radius = 0.5 * lena * denom;
                }
                if 0.5 * lenb * denom < radius {
                    radius = 0.5 * lenb * denom;
                }
                // The fillet arc's centre.
                let mut k = radius / (0.5 * (1.0 - cosine)).sqrt();
                let lenab = ((xa / lena + xb / lenb) * (xa / lena + xb / lenb) + (ya / lena + yb / lenb) * (ya / lena + yb / lenb)).sqrt();
                let xc = x1 as f64 + k * (xa / lena + xb / lenb) / lenab;
                let yc = y1 as f64 + k * (ya / lena + yb / lenb) / lenab;
                // The arc's start and end vectors.
                k = radius / (2.0 / (1.0 + cosine) - 1.0).sqrt();
                let xs = x1 as f64 + k * xa / lena - xc;
                let ys = y1 as f64 + k * ya / lena - yc;
                let xe = x1 as f64 + k * xb / lenb - xc;
                let ye = y1 as f64 + k * yb / lenb - yc;
                // Cosine of the arc angle, clamped into acos's domain.
                let argument = ((xs * xe + ys * ye) / (radius * radius)).clamp(-1.0, 1.0);
                let arc_angle = argument.acos();
                let segments = arc_segment_count(radius as i64, error_max, arc_angle);
                let mut delta_angle = arc_angle / segments as f64;
                let start_angle = (-ys).atan2(xs);
                // Flip the arc for inner corners.
                if xa * yb - ya * xb <= 0.0 {
                    delta_angle = -delta_angle;
                }
                let (mut nx, mut ny) = (xc + xs, yc + ys);
                if nx.is_nan() || ny.is_nan() {
                    continue;
                }
                out.push(Point64::new(ki_round(nx), ki_round(ny)));
                let (mut prev_x, mut prev_y) = (ki_round(nx), ki_round(ny));
                for j in 0..segments {
                    nx = xc + (start_angle + (j + 1) as f64 * delta_angle).cos() * radius;
                    ny = yc - (start_angle + (j + 1) as f64 * delta_angle).sin() * radius;
                    if nx.is_nan() || ny.is_nan() {
                        continue;
                    }
                    // The rounding can produce repeated corners; do not add them.
                    if ki_round(nx) != prev_x || ki_round(ny) != prev_y {
                        out.push(Point64::new(ki_round(nx), ki_round(ny)));
                        prev_x = ki_round(nx);
                        prev_y = ki_round(ny);
                    }
                }
            }
        }
    }
    out
}

fn corner_polygon(poly: &Polygon, mode: Mode, distance: i64, error_max: i64) -> Polygon {
    let cleaned: Polygon = poly.iter().map(without_null_segments).collect();
    // If the distance is zero the polygon stays intact.
    if distance <= 0 {
        return cleaned;
    }
    cleaned.iter().map(|c| corner_contour(c, mode, distance, error_max)).collect()
}

/// `SHAPE_POLY_SET::Chamfer( distance )`.
pub fn chamfer(set: &ShapePolySet, distance: i64) -> ShapePolySet {
    ShapePolySet { polys: set.polys.iter().map(|p| corner_polygon(p, Mode::Chamfered, distance, 0)).collect() }
}

/// `SHAPE_POLY_SET::Fillet( radius, error_max )`.
pub fn fillet(set: &ShapePolySet, radius: i64, error_max: i64) -> ShapePolySet {
    ShapePolySet { polys: set.polys.iter().map(|p| corner_polygon(p, Mode::Filleted, radius, error_max)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: i64) -> ShapePolySet {
        ShapePolySet::from_outline(vec![Point64::new(0, 0), Point64::new(s, 0), Point64::new(s, s), Point64::new(0, s)])
    }

    #[test]
    fn a_chamfer_cuts_each_corner_by_the_distance() {
        let c = chamfer(&square(1000), 100);
        assert_eq!(c.polys[0][0].len(), 8, "four corners, two points each");
        let area = c.area();
        assert!((area - (1_000_000.0 - 4.0 * 0.5 * 100.0 * 100.0)).abs() < 1.0, "area {area}");
    }

    #[test]
    fn a_chamfer_never_takes_more_than_half_an_edge() {
        let c = chamfer(&square(100), 1000);
        assert!((c.area() - 100.0 * 100.0 / 2.0).abs() < 1.0, "a diamond of half the square: {}", c.area());
    }

    #[test]
    fn a_fillet_rounds_each_corner_with_an_arc_of_the_radius() {
        let f = fillet(&square(1000), 200, 5);
        // a square with r=200 rounded corners: area = s^2 - (4 - pi) r^2
        let want = 1_000_000.0 - (4.0 - std::f64::consts::PI) * 200.0 * 200.0;
        // The arcs are inscribed polylines (a 5 um chord error on a 200 um radius is three segments a corner): the area
        // lies a little under the true rounded square.
        assert!(f.area() < want && (want - f.area()) / want < 0.006, "area {} vs {want}", f.area());
        assert!(f.polys[0][0].len() > 12, "arcs are polylines: {}", f.polys[0][0].len());
    }

    #[test]
    fn zero_distance_leaves_the_polygon_alone() {
        let c = chamfer(&square(1000), 0);
        assert_eq!(c.polys[0][0].len(), 4);
        assert!((fillet(&square(1000), 0, 5).area() - 1_000_000.0).abs() < 1.0);
    }
}
