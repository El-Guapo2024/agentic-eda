//! Floating-point points and segments, ported from FreeRouting's
//! `FloatPoint` and `FloatLine`: where the maze search measures how far it
//! has come and where it enters each door. Every operation is written in
//! the Java's order, as its costs decide the search's order and ties are
//! broken on exact values.

use super::line::{java_round, Side};
use super::{IntBox, IntPoint, CRIT};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatPoint {
    pub x: f64,
    pub y: f64,
}

impl FloatPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        FloatPoint { x, y }
    }

    pub fn from_int(p: IntPoint) -> Self {
        FloatPoint { x: p.x as f64, y: p.y as f64 }
    }

    /// `FloatPoint.middle_point`.
    pub fn middle_point(&self, to: &FloatPoint) -> FloatPoint {
        FloatPoint::new(0.5 * (self.x + to.x), 0.5 * (self.y + to.y))
    }

    /// The distance with x and y steps weighted apart, as a trace's cost
    /// differs along and across a layer's preferred direction.
    /// `FloatPoint.weighted_distance`.
    pub fn weighted_distance(&self, other: &FloatPoint, horizontal: f64, vertical: f64) -> f64 {
        let dx = (self.x - other.x) * horizontal;
        let dy = (self.y - other.y) * vertical;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn distance_square(&self, other: &FloatPoint) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    pub fn distance(&self, other: &FloatPoint) -> f64 {
        self.distance_square(other).sqrt()
    }

    /// The nearest integer point, halves rounded up. `FloatPoint.round`.
    pub fn round(&self) -> IntPoint {
        IntPoint::new(java_round(self.x), java_round(self.y))
    }

    /// The smallest integer box containing the point.
    /// `FloatPoint.bounding_box`.
    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(self.x.floor() as i64, self.y.floor() as i64, self.x.ceil() as i64, self.y.ceil() as i64)
    }

    /// Which side of the line from `p_1` to `p_2` the point is on; never
    /// collinear but for an exact zero. `FloatPoint.side_of`.
    pub fn side_of(&self, p_1: &FloatPoint, p_2: &FloatPoint) -> Side {
        let d21_x = p_2.x - p_1.x;
        let d21_y = p_2.y - p_1.y;
        let d01_x = self.x - p_1.x;
        let d01_y = self.y - p_1.y;
        Side::of(d21_x * d01_y - d21_y * d01_x)
    }

    /// The scalar product of `p_1 - self` and `p_2 - self`.
    /// `FloatPoint.scalar_product(FloatPoint, FloatPoint)`.
    pub fn scalar_product(&self, p_1: &FloatPoint, p_2: &FloatPoint) -> f64 {
        let dx_1 = p_1.x - self.x;
        let dx_2 = p_2.x - self.x;
        let dy_1 = p_1.y - self.y;
        let dy_2 = p_2.y - self.y;
        dx_1 * dx_2 + dy_1 * dy_2
    }

    /// Turned by `factor` quarter turns counter-clockwise around `pole`.
    /// `FloatPoint.turn_90_degree(int, FloatPoint)`.
    pub fn turn_90_degree(&self, factor: i32, pole: &FloatPoint) -> FloatPoint {
        let (x, y) = (self.x - pole.x, self.y - pole.y);
        let (x, y) = match factor.rem_euclid(4) {
            0 => (x, y),
            1 => (-y, x),
            2 => (-x, -y),
            _ => (y, -x),
        };
        FloatPoint::new(pole.x + x, pole.y + y)
    }

    /// Whether the point lies in the box spanned by `p_1` and `p_2`, give or
    /// take `tolerance`. `FloatPoint.is_contained_in_box`.
    pub fn is_contained_in_box(&self, p_1: &FloatPoint, p_2: &FloatPoint, tolerance: f64) -> bool {
        let (min_x, max_x) = if p_1.x < p_2.x { (p_1.x, p_2.x) } else { (p_2.x, p_1.x) };
        if self.x < min_x - tolerance || self.x > max_x + tolerance {
            return false;
        }
        let (min_y, max_y) = if p_1.y < p_2.y { (p_1.y, p_2.y) } else { (p_2.y, p_1.y) };
        self.y >= min_y - tolerance && self.y <= max_y + tolerance
    }
}

/// A segment, or the line through it, from `a` to `b`. FreeRouting's
/// `FloatLine`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatLine {
    pub a: FloatPoint,
    pub b: FloatPoint,
}

impl FloatLine {
    pub const fn new(a: FloatPoint, b: FloatPoint) -> Self {
        FloatLine { a, b }
    }

    /// A segment of no length at `p`.
    pub const fn point(p: FloatPoint) -> Self {
        FloatLine { a: p, b: p }
    }

    pub fn opposite(&self) -> FloatLine {
        FloatLine::new(self.b, self.a)
    }

    pub fn middle(&self) -> FloatPoint {
        self.a.middle_point(&self.b)
    }

    /// This segment turned round, if need be, to run the same way as
    /// `other` along it. `FloatLine.adjust_direction`.
    pub fn adjust_direction(&self, other: &FloatLine) -> FloatLine {
        if self.b.side_of(&self.a, &other.a) == other.b.side_of(&self.a, &other.a) {
            *self
        } else {
            self.opposite()
        }
    }

    /// Where the two lines meet; `None` for parallel lines.
    /// `FloatLine.intersection`.
    pub fn intersection(&self, other: &FloatLine) -> Option<FloatPoint> {
        let d1x = self.b.x - self.a.x;
        let d1y = self.b.y - self.a.y;
        let d2x = other.b.x - other.a.x;
        let d2y = other.b.y - other.a.y;
        let det_1 = self.a.x * self.b.y - self.a.y * self.b.x;
        let det_2 = other.a.x * other.b.y - other.a.y * other.b.x;
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            return None;
        }
        Some(FloatPoint::new((d2x * det_1 - d1x * det_2) / det, (d2y * det_1 - d1y * det_2) / det))
    }

    /// `FloatLine.signed_distance`.
    pub fn signed_distance(&self, p: &FloatPoint) -> f64 {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let det = dy * (p.x - self.a.x) - dx * (p.y - self.a.y);
        det / (dx * dx + dy * dy).sqrt()
    }

    /// The foot of the perpendicular from `p` onto the line; `a` for a
    /// segment of no length. `FloatLine.perpendicular_projection`.
    pub fn perpendicular_projection(&self, p: &FloatPoint) -> FloatPoint {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return self.a;
        }
        let dxdx = dx * dx;
        let dydy = dy * dy;
        let dxdy = dx * dy;
        let denominator = dxdx + dydy;
        let det = self.a.x * self.b.y - self.b.x * self.a.y;
        let x = (p.x * dxdx + p.y * dxdy + det * dy) / denominator;
        let y = (p.x * dxdy + p.y * dydy - det * dx) / denominator;
        FloatPoint::new(x, y)
    }

    /// `segment` projected square onto this segment and clipped to it;
    /// `None` if nothing is left. `FloatLine.segment_projection`.
    pub fn segment_projection(&self, segment: &FloatLine) -> Option<FloatLine> {
        if self.b.scalar_product(&self.a, &segment.a) < 0.0 {
            return None;
        }
        if self.a.scalar_product(&self.b, &segment.b) < 0.0 {
            return None;
        }
        let crit = CRIT as f64;
        let projected_a = if self.a.scalar_product(&self.b, &segment.a) < 0.0 {
            self.a
        } else {
            let p = self.perpendicular_projection(&segment.a);
            if p.x.abs() >= crit || p.y.abs() >= crit {
                return None;
            }
            p
        };
        let projected_b = if self.b.scalar_product(&self.a, &segment.b) < 0.0 { self.b } else { self.perpendicular_projection(&segment.b) };
        if projected_b.x.abs() >= crit || projected_b.y.abs() >= crit {
            return None;
        }
        Some(FloatLine::new(projected_a, projected_b))
    }

    /// `segment` moved square to itself onto this segment; `None` if it
    /// misses. `FloatLine.segment_projection_2`.
    pub fn segment_projection_2(&self, segment: &FloatLine) -> Option<FloatLine> {
        if segment.a.scalar_product(&segment.b, &self.b) <= 0.0 {
            return None;
        }
        if segment.b.scalar_product(&segment.a, &self.a) <= 0.0 {
            return None;
        }
        let crit = CRIT as f64;
        let out_of_range = |p: &FloatPoint| p.x.abs() >= crit || p.y.abs() >= crit;
        let projected_a = if segment.a.scalar_product(&segment.b, &self.a) < 0.0 {
            let perpendicular = FloatLine::new(segment.a, segment.b.turn_90_degree(1, &segment.a));
            let p = perpendicular.intersection(self)?;
            if out_of_range(&p) {
                return None;
            }
            p
        } else {
            self.a
        };
        let projected_b = if segment.b.scalar_product(&segment.a, &self.b) < 0.0 {
            let perpendicular = FloatLine::new(segment.b, segment.a.turn_90_degree(1, &segment.b));
            let p = perpendicular.intersection(self)?;
            if out_of_range(&p) {
                return None;
            }
            p
        } else {
            self.b
        };
        Some(FloatLine::new(projected_a, projected_b))
    }

    /// Shortened by `offset` at both ends, never past its middle.
    /// `FloatLine.shrink_segment`.
    pub fn shrink_segment(&self, offset: f64) -> FloatLine {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return *self;
        }
        let length = (dx * dx + dy * dy).sqrt();
        let offset = offset.min(length / 2.0);
        let new_a = FloatPoint::new(self.a.x + (dx * offset) / length, self.a.y + (dy * offset) / length);
        let new_length = length - offset;
        let new_b = FloatPoint::new(self.a.x + (dx * new_length) / length, self.a.y + (dy * new_length) / length);
        FloatLine::new(new_a, new_b)
    }

    /// The nearest point to `p` on the segment.
    /// `FloatLine.nearest_segment_point`.
    pub fn nearest_segment_point(&self, p: &FloatPoint) -> FloatPoint {
        let projection = self.perpendicular_projection(p);
        if projection.is_contained_in_box(&self.a, &self.b, 0.01) {
            return projection;
        }
        if p.distance_square(&self.a) <= p.distance_square(&self.b) {
            self.a
        } else {
            self.b
        }
    }

    /// Cut into `count` pieces of equal length, end to end.
    /// `FloatLine.divide_segment_into_sections`.
    pub fn divide_segment_into_sections(&self, count: usize) -> Vec<FloatLine> {
        if count == 0 {
            return Vec::new();
        }
        if count == 1 {
            return vec![*self];
        }
        let line_length = self.b.distance(&self.a);
        let section_length = line_length / count as f64;
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let mut result = Vec::with_capacity(count);
        let mut curr_a = self.a;
        for i in 0..count {
            let curr_b = if i == count - 1 {
                self.b
            } else {
                let dist = (i + 1) as f64 * section_length;
                FloatPoint::new(self.a.x + (dx * dist) / line_length, self.a.y + (dy * dist) / line_length)
            };
            result.push(FloatLine::new(curr_a, curr_b));
            curr_a = curr_b;
        }
        result
    }
}
