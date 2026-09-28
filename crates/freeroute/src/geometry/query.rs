//! What the maze search asks of a convex shape: its corners and sides,
//! nearest points, widths, centre, whether it holds a point or meets
//! another shape. Ported from FreeRouting's `TileShape` and
//! `PolylineShape`, which answer most of this generically from a shape's
//! border lines and corners, and from the overrides `IntOctagon` and `IntBox`
//! bring. Each query follows the version Java's dispatch picks for each
//! form, as they differ at the border: an octagon holds the points on its
//! border, the generic test does not.

use super::float::{FloatLine, FloatPoint};
use super::line::{Line, Point, Side};
use super::{IntBox, IntOctagon, IntPoint, Simplex, TileShape};

impl TileShape {
    /// `border_line_count`: 4 for a box, 8 for an octagon, however many
    /// lines a polygon has.
    pub fn border_line_count(&self) -> usize {
        match self {
            TileShape::Box(_) => 4,
            TileShape::Octagon(_) => 8,
            TileShape::Simplex(s) => s.lines.len(),
        }
    }

    /// Border line `no`, counter-clockwise, the shape on its right.
    pub fn border_line(&self, no: usize) -> Line {
        match self {
            TileShape::Box(b) => b.border_line(no),
            TileShape::Octagon(o) => o.border_line(no),
            TileShape::Simplex(s) => s.lines[no],
        }
    }

    /// Corner `no`: where border line `no` meets the one before it -- by the
    /// Java's corner formulas for a box and an octagon.
    pub fn corner(&self, no: usize) -> Point {
        match self {
            TileShape::Box(b) => Point::Int(b.corner(no)),
            TileShape::Octagon(o) => Point::Int(o.corner(no)),
            TileShape::Simplex(s) => s.corner(no),
        }
    }

    /// `corner_approx`: the corner in floating point.
    pub fn corner_approx(&self, no: usize) -> FloatPoint {
        match self {
            TileShape::Simplex(s) => {
                let (x, y) = s.corner_approx(no);
                FloatPoint::new(x, y)
            }
            _ => {
                let (x, y) = self.corner(no).to_float();
                FloatPoint::new(x, y)
            }
        }
    }

    /// `corner_is_bounded`: always for a box or an octagon.
    pub fn corner_is_bounded(&self, no: usize) -> bool {
        match self {
            TileShape::Simplex(s) => s.corner_is_bounded(no),
            _ => true,
        }
    }

    /// The border line `p` lies on, the last such, if it lies on the
    /// border; `None` if it is inside or outside.
    /// `TileShape.contains_on_border_line_no`.
    pub fn contains_on_border_line_no(&self, p: &Point) -> Option<usize> {
        let mut containing = None;
        for i in 0..self.border_line_count() {
            match self.border_line(i).side_of(p) {
                Side::Left => return None,
                Side::Collinear => containing = Some(i),
                Side::Right => {}
            }
        }
        containing
    }

    /// The nearest point of the border to `from`: the nearest corner, or
    /// the foot of a perpendicular onto a side where that falls within it.
    /// `TileShape.nearest_border_point`.
    pub fn nearest_border_point(&self, from: IntPoint) -> Point {
        let n = self.border_line_count();
        let from_f = FloatPoint::from_int(from);
        if n == 1 {
            return from.perpendicular_projection(&self.border_line(0));
        }
        let mut min_dist = f64::MAX;
        let mut min_ind = 0;
        for i in 0..n {
            let d = self.corner_approx(i).distance_square(&from_f);
            if d < min_dist {
                min_dist = d;
                min_ind = i;
            }
        }
        let mut nearest = self.corner(min_ind);
        let (mut prev, mut curr) = (n - 2, n - 1);
        for next in 0..n {
            let projection = from.perpendicular_projection(&self.border_line(curr));
            if (!self.corner_is_bounded(curr) || self.border_line(prev).side_of(&projection) == Side::Right)
                && (!self.corner_is_bounded(next) || self.border_line(next).side_of(&projection) == Side::Right)
            {
                let d = FloatPoint::from_point(&projection).distance_square(&from_f);
                if d < min_dist {
                    min_dist = d;
                    nearest = projection;
                }
            }
            prev = curr;
            curr = next;
        }
        nearest
    }

    /// Moved by `(dx, dy)`. `translate_by`, per form; a polygon's lines
    /// move as they are.
    pub fn translate_by(&self, dx: i64, dy: i64) -> TileShape {
        if dx == 0 && dy == 0 {
            return self.clone();
        }
        match self {
            TileShape::Box(b) => TileShape::Box(IntBox::new(b.ll.x + dx, b.ll.y + dy, b.ur.x + dx, b.ur.y + dy)),
            TileShape::Octagon(o) => TileShape::Octagon(o.translate_by(dx, dy)),
            TileShape::Simplex(s) => TileShape::Simplex(Simplex {
                lines: s.lines.iter().map(|l| Line::new(IntPoint::new(l.a.x + dx, l.a.y + dy), IntPoint::new(l.b.x + dx, l.b.y + dy))).collect(),
            }),
        }
    }

    /// Whether `p` lies strictly on the outer side of some border line.
    /// `TileShape.is_outside`.
    pub fn is_outside(&self, p: &Point) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return true;
        }
        (0..n).any(|i| self.border_line(i).side_of(p) == Side::Left)
    }

    /// `TileShape.contains(Point)`: on the border counts.
    pub fn contains_point(&self, p: &Point) -> bool {
        !self.is_outside(p)
    }

    /// Whether `p` lies strictly inside, off every border line.
    /// `TileShape.contains_inside(Point)` -- which a box's own version only
    /// replaces for an argument typed `IntPoint`, never the case here.
    pub fn contains_inside(&self, p: &Point) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return false;
        }
        (0..n).all(|i| self.border_line(i).side_of(p) == Side::Right)
    }

    /// Whether the float point `p` lies inside: strictly for the generic
    /// test, border included for an octagon, which overrides it.
    /// `contains(FloatPoint)`.
    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        match self {
            TileShape::Octagon(o) => octagon_contains_float(o, p),
            _ => self.contains_float_within(p, 0.0),
        }
    }

    /// `TileShape.contains(FloatPoint, double)`: on the inner side of every
    /// border line by more than `tolerance`.
    pub fn contains_float_within(&self, p: &FloatPoint, tolerance: f64) -> bool {
        let n = self.border_line_count();
        if n == 0 {
            return false;
        }
        (0..n).all(|i| self.border_line(i).side_of_float((p.x, p.y), tolerance) == Side::Right)
    }

    /// Whether every corner of `other` lies in this shape.
    /// `TileShape.contains(TileShape)`.
    pub fn contains_shape(&self, other: &TileShape) -> bool {
        (0..other.border_line_count()).all(|i| self.contains_point(&other.corner(i)))
    }

    /// Whether the shape lies inside `b`, by bounds.
    /// `is_contained_in(IntBox)`, per form.
    pub fn is_contained_in_box(&self, b: &IntBox) -> bool {
        match self {
            TileShape::Octagon(o) => o.left_x >= b.ll.x && o.bottom_y >= b.ll.y && o.right_x <= b.ur.x && o.top_y <= b.ur.y,
            TileShape::Box(own) => box_is_contained_in(own, b),
            // PolylineShape.is_contained_in: p_box.contains(bounding_box()).
            TileShape::Simplex(s) => box_contains(b, &s.bounding_box()),
        }
    }

    /// The average of the corners. `PolylineShape.centre_of_gravity`.
    pub fn centre_of_gravity(&self) -> FloatPoint {
        let n = self.border_line_count();
        let (mut x, mut y) = (0.0, 0.0);
        for i in 0..n {
            let c = self.corner_approx(i);
            x += c.x;
            y += c.y;
        }
        x /= n as f64;
        y /= n as f64;
        FloatPoint::new(x, y)
    }

    /// `from` itself if the shape holds it, else the nearest point of its
    /// border. `TileShape.nearest_point_approx`.
    pub fn nearest_point_approx(&self, from: &FloatPoint) -> Option<FloatPoint> {
        if self.contains_float(from) {
            return Some(*from);
        }
        self.nearest_border_points_approx(from, 1).first().copied()
    }

    /// Up to `count` points of the border nearest to `from`, each on a
    /// different border line, nearest first: corners, then projections that
    /// land between their line's neighbours. `None` entries where fewer
    /// candidates were found than asked for.
    /// `TileShape.nearest_border_points_approx`.
    pub fn nearest_border_points_approx(&self, from: &FloatPoint, count: usize) -> Vec<FloatPoint> {
        if count == 0 {
            return Vec::new();
        }
        let line_count = self.border_line_count();
        let result_count = count.min(line_count);
        if line_count == 0 {
            return Vec::new();
        }
        if line_count == 1 {
            return vec![project_approx(from, &self.border_line(0))];
        }
        if self.dimension() == 0 {
            return vec![self.corner_approx(0)];
        }
        let mut nearest: Vec<Option<FloatPoint>> = vec![None; result_count];
        let mut min_dists = vec![f64::MAX; result_count];
        let offer = |p: FloatPoint, dist: f64, nearest: &mut Vec<Option<FloatPoint>>, min_dists: &mut Vec<f64>| {
            for j in 0..result_count {
                if dist < min_dists[j] {
                    for k in j + 1..result_count {
                        min_dists[k] = min_dists[k - 1];
                        nearest[k] = nearest[k - 1];
                    }
                    min_dists[j] = dist;
                    nearest[j] = Some(p);
                    break;
                }
            }
        };
        for i in 0..line_count {
            if self.corner_is_bounded(i) {
                let c = self.corner_approx(i);
                offer(c, c.distance_square(from), &mut nearest, &mut min_dists);
            }
        }
        let mut prev = line_count - 2;
        let mut curr = line_count - 1;
        for next in 0..line_count {
            let projection = project_approx(from, &self.border_line(curr));
            let fp = (projection.x, projection.y);
            if (!self.corner_is_bounded(curr) || self.border_line(prev).side_of_float(fp, 0.0) == Side::Right)
                && (!self.corner_is_bounded(next) || self.border_line(next).side_of_float(fp, 0.0) == Side::Right)
            {
                offer(projection, projection.distance_square(from), &mut nearest, &mut min_dists);
            }
            prev = curr;
            curr = next;
        }
        // The Java leaves unfilled slots null; callers only read filled ones.
        nearest.into_iter().flatten().collect()
    }

    /// The segment from corner 0 to corner `n / 2`, in floating point;
    /// `None` for an empty shape. `TileShape.diagonal_corner_segment`.
    pub fn diagonal_corner_segment(&self) -> Option<FloatLine> {
        if self.is_empty() {
            return None;
        }
        Some(FloatLine::new(self.corner_approx(0), self.corner_approx(self.border_line_count() / 2)))
    }

    /// The narrowest extent, per form: a box's shorter side; an octagon's
    /// narrowest pair of opposite sides, diagonals measured square; for a
    /// polygon, the two smallest distances from its centre to a side.
    /// `min_width`.
    pub fn min_width(&self) -> f64 {
        match self {
            TileShape::Box(b) => ((b.ur.x - b.ll.x).min(b.ur.y - b.ll.y)) as f64,
            TileShape::Octagon(o) => {
                let w1 = ((o.right_x - o.left_x).min(o.top_y - o.bottom_y)) as f64;
                let w2 = ((o.upper_right_diag_x - o.lower_left_diag_x).min(o.lower_right_diag_x - o.upper_left_diag_x)) as f64;
                w1.min(w2 / std::f64::consts::SQRT_2)
            }
            TileShape::Simplex(s) => {
                if !simplex_is_bounded(s) {
                    return i32::MAX as f64;
                }
                let gravity = self.centre_of_gravity();
                let (mut d1, mut d2) = (i32::MAX as f64, i32::MAX as f64);
                for l in &s.lines {
                    let d = l.signed_distance((gravity.x, gravity.y)).abs();
                    if d < d1 {
                        d2 = d1;
                        d1 = d;
                    } else if d < d2 {
                        d2 = d;
                    }
                }
                d1 + d2
            }
        }
    }

    /// The border line the ray from `from` in direction `dir` leaves the
    /// shape through; `None` if `from` is outside.
    /// `TileShape.intersecting_border_line_no`.
    pub fn intersecting_border_line_no(&self, from: &IntPoint, dir: &super::Direction) -> Option<usize> {
        if !self.contains_point(&Point::Int(*from)) {
            return None;
        }
        let from_point = FloatPoint::from_int(*from);
        let ray = Line::through(*from, *dir);
        let second = FloatPoint::from_int(ray.b);
        let mut result = None;
        let mut min_distance = f32::MAX as f64;
        for i in 0..self.border_line_count() {
            let line = self.border_line(i);
            let (x, y) = line.intersection_approx(&ray);
            if x >= i32::MAX as f64 {
                continue;
            }
            let crossing = FloatPoint::new(x, y);
            let d = crossing.distance_square(&from_point);
            if d < min_distance {
                let direction_ok = line.side_of_float((second.x, second.y), 0.0) == Side::Left || second.distance_square(&crossing) < d;
                if direction_ok {
                    result = Some(i);
                    min_distance = d;
                }
            }
        }
        result
    }

    /// Grown by `distance` on every side, per form's `offset(double)`.
    pub fn offset(&self, distance: f64) -> TileShape {
        match self {
            TileShape::Box(b) => {
                if distance == 0.0 || b.is_empty() {
                    return TileShape::Box(*b);
                }
                let d = super::line::java_round(distance);
                TileShape::Box(IntBox::new(b.ll.x - d, b.ll.y - d, b.ur.x + d, b.ur.y + d))
            }
            TileShape::Octagon(o) => TileShape::Octagon(o.offset(distance)),
            TileShape::Simplex(s) => TileShape::Simplex(s.offset(distance)),
        }
    }

    /// Shrunk by `distance` on every side; if nothing is left, the shape
    /// cut to the integer box round its centre. `TileShape.shrink`.
    pub fn shrink(&self, distance: f64) -> TileShape {
        let result = self.offset(-distance);
        if result.is_empty() {
            let centre = self.centre_of_gravity().bounding_box();
            return self.intersection(&TileShape::Box(centre));
        }
        result
    }

    /// Whether the two shapes share a point, touching included, by the
    /// Java's double dispatch: `self.intersects(other)` asks
    /// `other.intersects(self)`, and each pair of forms has its own test --
    /// between polygons, an intersection whose lines come in the order the
    /// dispatch sets.
    pub fn intersects(&self, other: &TileShape) -> bool {
        use TileShape::*;
        match (other, self) {
            // other.intersects(IntBox self)
            (Box(o), Box(s)) => boxes_intersect(o, s),
            (Octagon(o), Box(s)) => o.intersects(&s.to_octagon()),
            (Simplex(o), Box(s)) => !o.intersection(&s.to_simplex()).is_empty(),
            // other.intersects(IntOctagon self)
            (Box(o), Octagon(s)) => s.intersects(&o.to_octagon()),
            (Octagon(o), Octagon(s)) => o.intersects(s),
            (Simplex(o), Octagon(s)) => !o.intersection(&s.to_simplex()).is_empty(),
            // other.intersects(Simplex self)
            (Box(o), Simplex(s)) => !s.intersection(&o.to_simplex()).is_empty(),
            (Octagon(o), Simplex(s)) => !s.intersection(&o.to_simplex()).is_empty(),
            (Simplex(o), Simplex(s)) => !o.intersection(s).is_empty(),
        }
    }
}

/// `IntOctagon.contains(FloatPoint)`: border included. The negated
/// comparisons are the Java's, which differ from their opposites for NaN.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn octagon_contains_float(o: &IntOctagon, p: &FloatPoint) -> bool {
    if (o.left_x as f64) > p.x || (o.bottom_y as f64) > p.y || (o.right_x as f64) < p.x || (o.top_y as f64) < p.y {
        return false;
    }
    let tmp_1 = p.x - p.y;
    let tmp_2 = p.x + p.y;
    !((o.upper_left_diag_x as f64) > tmp_1)
        && !((o.lower_right_diag_x as f64) < tmp_1)
        && !((o.lower_left_diag_x as f64) > tmp_2)
        && !((o.upper_right_diag_x as f64) < tmp_2)
}

/// `FloatPoint.projection_approx(Line)`.
fn project_approx(p: &FloatPoint, line: &Line) -> FloatPoint {
    FloatLine::new(FloatPoint::from_int(line.a), FloatPoint::from_int(line.b)).perpendicular_projection(p)
}

/// `IntBox.intersects(IntBox)`: touching included.
fn boxes_intersect(a: &IntBox, b: &IntBox) -> bool {
    !(a.ll.x > b.ur.x || a.ll.y > b.ur.y || b.ll.x > a.ur.x) && b.ll.y <= a.ur.y
}

/// `IntBox.is_contained_in(IntBox)`: an empty box, or the same box, always
/// is.
fn box_is_contained_in(a: &IntBox, b: &IntBox) -> bool {
    a.is_empty() || a == b || (a.ll.x >= b.ll.x && a.ll.y >= b.ll.y && a.ur.x <= b.ur.x && a.ur.y <= b.ur.y)
}

/// `IntBox.contains(RegularTileShape)` for a box: `other.is_contained_in(this)`.
fn box_contains(b: &IntBox, other: &IntBox) -> bool {
    box_is_contained_in(other, b)
}

/// `Simplex.is_bounded`.
fn simplex_is_bounded(s: &Simplex) -> bool {
    match s.lines.len() {
        0 => true,
        1 | 2 => false,
        n => (0..n).all(|i| s.corner_is_bounded(i)),
    }
}

impl IntBox {
    /// Corner `no`, counter-clockwise from the lower left. `IntBox.corner`.
    pub fn corner(&self, no: usize) -> IntPoint {
        match no {
            0 => self.ll,
            1 => IntPoint::new(self.ur.x, self.ll.y),
            2 => self.ur,
            3 => IntPoint::new(self.ll.x, self.ur.y),
            _ => panic!("IntBox::corner: {no} out of range 0..4"),
        }
    }

    /// Border line `no`, through the points the Java picks.
    /// `IntBox.border_line`.
    pub fn border_line(&self, no: usize) -> Line {
        let (a, b) = match no {
            0 => ((0, self.ll.y), (1, self.ll.y)),
            1 => ((self.ur.x, 0), (self.ur.x, 1)),
            2 => ((0, self.ur.y), (-1, self.ur.y)),
            3 => ((self.ll.x, 0), (self.ll.x, -1)),
            _ => panic!("IntBox::border_line: {no} out of range 0..4"),
        };
        Line::new(IntPoint::new(a.0, a.1), IntPoint::new(b.0, b.1))
    }

    /// The point of the box nearest to `p`. `IntBox.nearest_point`.
    pub fn nearest_point(&self, p: &FloatPoint) -> FloatPoint {
        let x = if p.x <= self.ll.x as f64 {
            self.ll.x as f64
        } else if p.x >= self.ur.x as f64 {
            self.ur.x as f64
        } else {
            p.x
        };
        let y = if p.y <= self.ll.y as f64 {
            self.ll.y as f64
        } else if p.y >= self.ur.y as f64 {
            self.ur.y as f64
        } else {
            p.y
        };
        FloatPoint::new(x, y)
    }

    /// The box grown to cover `other` too. `IntBox.union`.
    pub fn union(&self, other: &IntBox) -> IntBox {
        IntBox::new(self.ll.x.min(other.ll.x), self.ll.y.min(other.ll.y), self.ur.x.max(other.ur.x), self.ur.y.max(other.ur.y))
    }

    /// The distance to `other` with x and y weighted apart: along one axis
    /// only where the boxes overlap in the other, 0 where they overlap.
    /// `IntBox.weighted_distance`.
    pub fn weighted_distance(&self, other: &IntBox, horizontal: f64, vertical: f64) -> f64 {
        let max_ll_x = self.ll.x.max(other.ll.x) as f64;
        let max_ll_y = self.ll.y.max(other.ll.y) as f64;
        let min_ur_x = self.ur.x.min(other.ur.x) as f64;
        let min_ur_y = self.ur.y.min(other.ur.y) as f64;
        if min_ur_x >= max_ll_x {
            (vertical * (max_ll_y - min_ur_y)).max(0.0)
        } else if min_ur_y >= max_ll_y {
            (horizontal * (max_ll_x - min_ur_x)).max(0.0)
        } else {
            let dx = (max_ll_x - min_ur_x) * horizontal;
            let dy = (max_ll_y - min_ur_y) * vertical;
            (dx * dx + dy * dy).sqrt()
        }
    }
}
