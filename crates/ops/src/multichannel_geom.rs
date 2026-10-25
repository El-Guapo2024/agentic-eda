//! Is an item inside a rule area? The test of `A.enclosedByArea( 'x' )` (`enclosedByAreaFunc`, `pcbnew/pcbexpr_functions.cpp`) that the multichannel tool selects the routing and
//! the other items of a channel with: the item's shape minus the area's outline leaves nothing, and the item is on a layer the area is on.
//!
//! KiCad subtracts the polygons. The items here are a track's centre line and width, a via's circle, the outline of a zone or a shape and the box of a text, so the test is
//! the same thing worked out on those: every point of the item is inside the outline (or on it), and no edge of the outline comes within the item's half width of its centre
//! line. A text or a dimension is judged by the corners of its box, which is a little less exact than its glyphs and its arrows.

use eda_model::ir::{Point, Nm};
use eda_model::multichannel::{point_inside, Box4};

/// A rule area's outline, to ask what it encloses.
pub(crate) struct Enclosure<'a> {
    pub outline: &'a [Point],
}

fn cross(o: Point, a: Point, b: Point) -> i128 {
    (a.x - o.x).raw_shim() as i128 * (b.y - o.y).raw_shim() as i128 - (a.y - o.y).raw_shim() as i128 * (b.x - o.x).raw_shim() as i128
}

fn on_segment(p: Point, a: Point, b: Point) -> bool {
    cross(a, b, p) == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Do the segments `a-b` and `c-d` cross, one at a point inside both (touching at an end does not count)?
fn properly_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (d1, d2, d3, d4) = (cross(a, b, c), cross(a, b, d), cross(c, d, a), cross(c, d, b));
    ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
}

fn dist_point_segment(p: Point, a: Point, b: Point) -> f64 {
    let (px, py, ax, ay, bx, by) = (p.x.raw_shim() as f64, p.y.raw_shim() as f64, a.x.raw_shim() as f64, a.y.raw_shim() as f64, b.x.raw_shim() as f64, b.y.raw_shim() as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
}

/// The least distance between two segments (zero if they cross).
fn dist_segments(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if properly_cross(a, b, c, d) {
        return 0.0;
    }
    dist_point_segment(a, c, d).min(dist_point_segment(b, c, d)).min(dist_point_segment(c, a, b)).min(dist_point_segment(d, a, b))
}

impl Enclosure<'_> {
    fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        let n = self.outline.len();
        (0..n).map(move |i| (self.outline[i], self.outline[(i + 1) % n]))
    }

    /// A point inside the outline or on its edge.
    pub fn contains_point(&self, p: Point) -> bool {
        self.outline.len() >= 3 && (point_inside(self.outline, p) || self.edges().any(|(a, b)| on_segment(p, a, b)))
    }

    /// A stroke (a track, a line of a shape) of `half_width` along `pts`: every point inside, the outline no nearer to the stroke than its half width. A single point is a round dot.
    pub fn contains_stroke(&self, pts: &[Point], half_width: Nm) -> bool {
        if self.outline.len() < 3 || pts.is_empty() {
            return false;
        }
        if !pts.iter().all(|p| self.contains_point(*p)) {
            return false;
        }
        let r = half_width.raw_shim() as f64;
        if pts.len() == 1 {
            return self.edges().all(|(a, b)| dist_point_segment(pts[0], a, b) >= r);
        }
        pts.windows(2).all(|w| self.edges().all(|(a, b)| if r > 0.0 { dist_segments(w[0], w[1], a, b) >= r } else { !properly_cross(w[0], w[1], a, b) }))
    }

    /// A disc (a via): the centre inside and the outline no nearer than the radius.
    pub fn contains_disc(&self, centre: Point, radius: Nm) -> bool {
        self.contains_stroke(&[centre], radius)
    }

    /// A polygon or a ring drawn with a stroke of `half_width` (a zone's outline, a rectangle): its corners inside, no edge of the outline crossing it.
    pub fn contains_ring(&self, pts: &[Point], half_width: Nm) -> bool {
        if pts.len() < 2 {
            return self.contains_stroke(pts, half_width);
        }
        let mut closed: Vec<Point> = pts.to_vec();
        closed.push(pts[0]);
        self.contains_stroke(&closed, half_width)
    }

    /// A box (the corners of a text's or a dimension's extent).
    pub fn contains_box(&self, b: Box4) -> bool {
        self.contains_ring(&[Point { x: b.0, y: b.1 }, Point { x: b.2, y: b.1 }, Point { x: b.2, y: b.3 }, Point { x: b.0, y: b.3 }], Nm::ZERO)
    }
}

/// Is the polygon `pts` touching the box `b` (a footprint's box against a rule area, in `refFP->GetEffectiveShape( .. )->Collide( &refPoly, 0 )`)?
pub(crate) fn polygon_meets_box(poly: &[Point], b: Box4) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let corners = [Point { x: b.0, y: b.1 }, Point { x: b.2, y: b.1 }, Point { x: b.2, y: b.3 }, Point { x: b.0, y: b.3 }];
    let enc = Enclosure { outline: poly };
    if corners.iter().any(|c| enc.contains_point(*c)) || poly.iter().any(|p| p.x >= b.0 && p.x <= b.2 && p.y >= b.1 && p.y <= b.3) {
        return true;
    }
    let n = poly.len();
    (0..n).any(|i| (0..4).any(|k| dist_segments(poly[i], poly[(i + 1) % n], corners[k], corners[(k + 1) % 4]) == 0.0))
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, um};
    use super::*;

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn l_shape() -> Vec<Point> {
        vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), Nm::ZERO), p(mm(0.01), um(4.0)), p(um(4.0), um(4.0)), p(um(4.0), mm(0.01)), p(Nm::ZERO, mm(0.01))]
    }

    #[test]
    fn a_track_is_enclosed_when_it_and_its_width_stay_inside() {
        let area = l_shape();
        let e = Enclosure { outline: &area };
        assert!(e.contains_stroke(&[p(um(1.0), um(1.0)), p(um(9.0), um(1.0))], Nm::new(100)));
        // the end cap of a wide track reaches the edge
        assert!(!e.contains_stroke(&[p(um(1.0), um(1.0)), p(um(9.0), um(1.0))], Nm::new(1_500)));
        // inside at both ends, across the notch of the L
        assert!(!e.contains_stroke(&[p(um(9.0), um(3.0)), p(um(3.0), um(9.0))], Nm::new(100)));
        // an end outside
        assert!(!e.contains_stroke(&[p(um(1.0), um(1.0)), p(mm(0.011), um(1.0))], Nm::new(100)));
    }

    #[test]
    fn a_via_a_ring_and_a_box_follow_the_same_rule() {
        let area = l_shape();
        let e = Enclosure { outline: &area };
        assert!(e.contains_disc(p(um(2.0), um(2.0)), Nm::new(300)));
        assert!(!e.contains_disc(p(Nm::new(9_900), Nm::new(3_900)), Nm::new(300)), "the disc crosses the edge");
        assert!(e.contains_ring(&[p(um(1.0), um(1.0)), p(um(3.0), um(1.0)), p(um(3.0), um(3.0)), p(um(1.0), um(3.0))], Nm::ZERO));
        assert!(e.contains_box((um(1.0), um(5.0), um(3.0), um(9.0))));
        assert!(!e.contains_box((um(3.0), um(3.0), um(6.0), um(6.0))), "the corner of the L is cut away");
    }

    #[test]
    fn a_polygon_meets_a_box_when_a_corner_is_in_or_an_edge_crosses() {
        let area = l_shape();
        assert!(polygon_meets_box(&area, (um(9.0), um(1.0), mm(0.011), um(2.0))));
        assert!(polygon_meets_box(&area, (Nm::new(-500), um(3.0), Nm::new(500), Nm::new(3_500))));
        assert!(!polygon_meets_box(&area, (um(6.0), um(6.0), um(8.0), um(8.0))));
    }
}
