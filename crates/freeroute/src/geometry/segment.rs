//! `LineSegment`, ported from FreeRouting: a stretch of a line between two
//! closing lines, such as one segment of a polyline. Traces are split,
//! cut and checked segment by segment through it.
//!
//! The Java caches a segment's exact end points once asked for them, and
//! then answers the floating point end points from them instead of from
//! the lines; the two can differ in the last bit for long lines. The cache
//! is kept here so the answers follow the same calls as there.

use std::cell::OnceCell;

use super::float::FloatPoint;
use super::line::{Line, Point, Side};
use super::polyline::Polyline;
use super::{IntBox, IntPoint, TileShape};

/// The segment of `middle` from where `start` crosses it to where `end`
/// does. FreeRouting's `LineSegment`.
#[derive(Debug, Clone)]
pub struct LineSegment {
    pub start: Line,
    pub middle: Line,
    pub end: Line,
    start_point: OnceCell<Point>,
    end_point: OnceCell<Point>,
}

impl LineSegment {
    pub fn new(start: Line, middle: Line, end: Line) -> Self {
        LineSegment { start, middle, end, start_point: OnceCell::new(), end_point: OnceCell::new() }
    }

    /// Segment `no` of `polyline`, for `no` from 1 to the line count less
    /// two. `LineSegment(Polyline, int)`.
    pub fn of(polyline: &Polyline, no: usize) -> Self {
        let l = &polyline.lines;
        LineSegment::new(l[no - 1], l[no], l[no + 1])
    }

    /// `start_point`: exact, and remembered.
    pub fn start_point(&self) -> Point {
        *self.start_point.get_or_init(|| self.middle.intersection(&self.start))
    }

    /// `end_point`: exact, and remembered.
    pub fn end_point(&self) -> Point {
        *self.end_point.get_or_init(|| self.middle.intersection(&self.end))
    }

    /// `start_point_approx`: from the exact point once that is known.
    pub fn start_point_approx(&self) -> FloatPoint {
        let (x, y) = match self.start_point.get() {
            Some(p) => p.to_float(),
            None => self.start.intersection_approx(&self.middle),
        };
        FloatPoint::new(x, y)
    }

    /// `end_point_approx`: from the exact point once that is known.
    pub fn end_point_approx(&self) -> FloatPoint {
        let (x, y) = match self.end_point.get() {
            Some(p) => p.to_float(),
            None => self.end.intersection_approx(&self.middle),
        };
        FloatPoint::new(x, y)
    }

    /// The same stretch run the other way. `opposite`.
    pub fn opposite(&self) -> LineSegment {
        LineSegment::new(self.end.opposite(), self.middle.opposite(), self.start.opposite())
    }

    /// The segment cut to about `new_length` from its start, square to it.
    /// `change_length_approx`.
    pub fn change_length_approx(&self, new_length: f64) -> LineSegment {
        let new_end_point = self.start_point_approx().change_length(&self.end_point_approx(), new_length);
        let perpendicular = self.middle.direction().turn_45_degree(2);
        LineSegment::new(self.start, self.middle, Line::through(new_end_point.round(), perpendicular))
    }

    /// The three lines as a polyline, which merges them if parallel.
    /// `to_polyline`.
    pub fn to_polyline(&self) -> Polyline {
        Polyline::from_lines(&[self.start, self.middle, self.end])
    }

    /// The segment as a flat convex shape: its line both ways, closed by
    /// the end lines turned to face it. `to_simplex().simplify()`.
    pub fn to_simplex(&self) -> TileShape {
        let (start, middle, end) = (&self.start, &self.middle, &self.end);
        // Point.side_of(Line), which is Line.side_of(Point) the other way.
        let first = if start.side_of(&self.end_point()).negate() == Side::Right { start.opposite() } else { *start };
        let last = if end.side_of(&self.start_point()).negate() == Side::Right { end.opposite() } else { *end };
        TileShape::from_lines(&[first, *middle, middle.opposite(), last])
    }

    /// Whether `p` lies on the segment, ends included. `contains`.
    pub fn contains(&self, p: IntPoint) -> bool {
        let p = Point::Int(p);
        if self.middle.side_of(&p) != Side::Collinear {
            return false;
        }
        let Point::Int(q) = p else { unreachable!() };
        let perpendicular = Line::through(q, self.middle.direction().turn_45_degree(2));
        let start_side = perpendicular.side_of(&self.start_point());
        let end_side = perpendicular.side_of(&self.end_point());
        start_side != end_side || start_side == Side::Collinear
    }

    /// The box around the two end points, rounded outwards.
    /// `bounding_box`, from the floating point ends.
    pub fn bounding_box(&self) -> IntBox {
        let (sx, sy) = self.middle.intersection_approx(&self.start);
        let (ex, ey) = self.middle.intersection_approx(&self.end);
        IntBox::new(sx.min(ex).floor() as i64, sy.min(ey).floor() as i64, sx.max(ex).ceil() as i64, sy.max(ey).ceil() as i64)
    }

    /// The same segment the other way, end points taken along.
    /// `sort_endpoints_in_x_y`: turned when it starts right of, or above,
    /// where it ends.
    fn sort_endpoints_in_x_y(&self) -> LineSegment {
        if compare_x_y(&self.start_point(), &self.end_point()) == std::cmp::Ordering::Greater {
            let swapped = LineSegment::new(self.end, self.middle, self.start);
            if let Some(p) = self.end_point.get() {
                let _ = swapped.start_point.set(*p);
            }
            if let Some(p) = self.start_point.get() {
                let _ = swapped.end_point.set(*p);
            }
            swapped
        } else {
            self.clone()
        }
    }

    /// Where this segment meets `other`, as lines whose crossings with this
    /// segment give the points: none; one, for a crossing or touch; two, the
    /// first and last common point, for an overlap. `intersection`.
    pub fn intersection(&self, other: &LineSegment) -> Vec<Line> {
        if !boxes_meet(&self.bounding_box(), &other.bounding_box()) {
            return Vec::new();
        }
        let start_side = other.middle.side_of(&self.start_point()).negate();
        let end_side = other.middle.side_of(&self.end_point()).negate();
        if start_side == Side::Collinear && end_side == Side::Collinear {
            let this_sorted = self.sort_endpoints_in_x_y();
            let other_sorted = other.sort_endpoints_in_x_y();
            let (left, right) = if compare_x_y(&this_sorted.start_point(), &other_sorted.start_point()) != std::cmp::Ordering::Greater {
                (this_sorted, other_sorted)
            } else {
                (other_sorted, this_sorted)
            };
            let cmp = compare_x_y(&left.end_point(), &right.start_point());
            if cmp == std::cmp::Ordering::Less {
                return Vec::new();
            }
            if cmp == std::cmp::Ordering::Equal {
                return vec![left.end];
            }
            let last = if compare_x_y(&right.end_point(), &left.end_point()) != std::cmp::Ordering::Less { left.end } else { right.end };
            return vec![right.start, last];
        }
        if start_side == end_side || other.start_point_side_of(&self.middle) == other.end_point_side_of(&self.middle) {
            return Vec::new();
        }
        vec![other.middle]
    }

    fn start_point_side_of(&self, line: &Line) -> Side {
        line.side_of(&self.start_point()).negate()
    }

    fn end_point_side_of(&self, line: &Line) -> Side {
        line.side_of(&self.end_point()).negate()
    }

    /// The border lines of `shape` this segment crosses into or out of its
    /// interior -- touches at an end only count if the segment enters it --
    /// at most two, nearest the start first. `border_intersections`.
    pub fn border_intersections(&self, shape: &TileShape) -> Vec<usize> {
        if !boxes_meet(&self.bounding_box(), &shape.bounding_box()) {
            return Vec::new();
        }
        let edge_count = shape.border_line_count();
        let mut prev_line = shape.border_line(edge_count - 1);
        let mut curr_line = shape.border_line(0);
        let mut result: Vec<usize> = Vec::with_capacity(2);
        let mut found: Vec<Point> = Vec::with_capacity(2);
        let line_start = self.start_point();
        let line_end = self.end_point();
        for edge_line_no in 0..edge_count {
            let next_line = shape.border_line(if edge_line_no == edge_count - 1 { 0 } else { edge_line_no + 1 });
            let start_side = curr_line.side_of(&line_start);
            let end_side = curr_line.side_of(&line_end);
            if start_side == Side::Left && end_side == Side::Left {
                return Vec::new();
            }
            if start_side == Side::Collinear && end_side != Side::Right {
                return Vec::new();
            }
            if end_side == Side::Collinear && start_side != Side::Right {
                return Vec::new();
            }
            if start_side != Side::Right || end_side != Side::Right {
                let is = self.middle.intersection(&curr_line);
                let prev_side_of_is = prev_line.side_of(&is);
                let next_side_of_is = next_line.side_of(&is);
                if prev_side_of_is != Side::Left && next_side_of_is != Side::Left {
                    if prev_side_of_is == Side::Collinear {
                        let prev_prev_corner = shape.corner(if edge_line_no == 0 { edge_count - 1 } else { edge_line_no - 1 });
                        let next_corner = shape.corner(if edge_line_no == edge_count - 1 { 0 } else { edge_line_no + 1 });
                        let a = self.middle.side_of(&prev_prev_corner);
                        let b = self.middle.side_of(&next_corner);
                        if a == Side::Collinear || b == Side::Collinear || a == b {
                            return Vec::new();
                        }
                    }
                    if next_side_of_is == Side::Collinear {
                        let prev_corner = shape.corner(edge_line_no);
                        let next_next_corner = if edge_line_no + 2 == edge_count {
                            shape.corner(0)
                        } else if edge_line_no + 1 == edge_count {
                            shape.corner(1)
                        } else {
                            shape.corner(edge_line_no + 2)
                        };
                        let a = self.middle.side_of(&prev_corner);
                        let b = self.middle.side_of(&next_next_corner);
                        if a == Side::Collinear || b == Side::Collinear || a == b {
                            return Vec::new();
                        }
                    }
                    if !found.iter().any(|f| f.java_equals(&is)) && result.len() < 2 {
                        result.push(edge_line_no);
                        found.push(is);
                    }
                }
            }
            prev_line = curr_line;
            curr_line = next_line;
        }
        if result.len() == 2 {
            let (x0, y0) = found[0].to_float();
            let (x1, y1) = found[1].to_float();
            let s = FloatPoint::from_point(&line_start);
            if s.distance_square(&FloatPoint::new(x1, y1)) < s.distance_square(&FloatPoint::new(x0, y0)) {
                result.swap(0, 1);
            }
        }
        result
    }
}

/// `IntBox.intersects`: sharing a point, borders included.
fn boxes_meet(a: &IntBox, b: &IntBox) -> bool {
    a.ll.x <= b.ur.x && b.ll.x <= a.ur.x && a.ll.y <= b.ur.y && b.ll.y <= a.ur.y
}

/// `Point.compare_x_y`: by x, then by y.
pub fn compare_x_y(a: &Point, b: &Point) -> std::cmp::Ordering {
    match (a, b) {
        (Point::Int(p), Point::Int(q)) => p.x.cmp(&q.x).then(p.y.cmp(&q.y)),
        _ => {
            let (ax, ay) = a.to_float();
            let (bx, by) = b.to_float();
            ax.partial_cmp(&bx).unwrap_or(std::cmp::Ordering::Equal).then(ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal))
        }
    }
}

impl Line {
    /// The line through `a` square to this one. Used to split a trace
    /// across a segment.
    pub fn perpendicular_through(&self, a: IntPoint) -> Line {
        Line::through(a, self.direction().turn_45_degree(2))
    }

    /// The square direction from `p` towards this line: the quarter turn
    /// whose unit step from `p` crosses the line, else the one ending
    /// nearer the foot of the perpendicular. `None` if `p` is on the line.
    /// `Line.perpendicular_direction(Point)`, which differs from the
    /// point's own [`perpendicular_direction_from`](Self::perpendicular_direction_from).
    pub fn perpendicular_direction(&self, p: IntPoint) -> Option<super::line::Direction> {
        let line_side = self.side_of(&Point::Int(p));
        if line_side == Side::Collinear {
            return None;
        }
        let dir1 = self.direction().turn_45_degree(2);
        let dir2 = self.direction().turn_45_degree(6);
        let check_1 = IntPoint::new(p.x + dir1.x, p.y + dir1.y);
        if self.side_of(&Point::Int(check_1)) != line_side {
            return Some(dir1);
        }
        let check_2 = IntPoint::new(p.x + dir2.x, p.y + dir2.y);
        if self.side_of(&Point::Int(check_2)) != line_side {
            return Some(dir2);
        }
        let nearest = FloatPoint::from_int(p).projection_approx(self);
        if nearest.distance_square(&FloatPoint::from_int(check_1)) <= nearest.distance_square(&FloatPoint::from_int(check_2)) {
            Some(dir1)
        } else {
            Some(dir2)
        }
    }
}

impl Polyline {
    /// The perpendicular from `p` onto the nearest segment it falls within,
    /// as a segment from `p`; `None` if there is none or `p` is on the
    /// polyline. `Polyline.projection_line`.
    pub fn projection_line(&self, p: IntPoint) -> Option<LineSegment> {
        let from_point = FloatPoint::from_int(p);
        let mut min_distance = f64::MAX;
        let mut result_line = None;
        let mut nearest_line = None;
        for i in 1..self.lines.len().saturating_sub(1) {
            let projection = from_point.projection_approx(&self.lines[i]);
            let distance = projection.distance(&from_point);
            if distance < min_distance {
                let Some(towards) = self.lines[i].perpendicular_direction(p) else { continue };
                let curr_result_line = Line::through(p, towards);
                let prev_side = curr_result_line.side_of(&self.corner(i - 1));
                let next_side = curr_result_line.side_of(&self.corner(i));
                if prev_side == next_side && prev_side != Side::Collinear {
                    // The foot lies outside the segment.
                    continue;
                }
                nearest_line = Some(self.lines[i]);
                min_distance = distance;
                result_line = Some(curr_result_line);
            }
        }
        let nearest_line = nearest_line?;
        let start_line = Line::through(p, nearest_line.direction());
        Some(LineSegment::new(start_line, result_line.expect("set with the nearest line"), nearest_line))
    }
}
