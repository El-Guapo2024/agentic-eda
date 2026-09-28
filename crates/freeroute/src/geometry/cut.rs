//! Where a polyline crosses a convex shape's border, and the pieces of it
//! left outside: how traces are cut back from a shape pushed into them,
//! and wrapped round an obstacle. Ported from FreeRouting's
//! `TileShape.entrance_points` and `TileShape.cutout(Polyline)`.

use super::line::Line;
use super::polyline::Polyline;
use super::segment::LineSegment;
use super::TileShape;

/// What [`TileShape::cutout_polyline`] leaves of a polyline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cutout {
    /// Nothing is cut: the polyline itself, which the Java hands back as
    /// the same object.
    Untouched,
    /// The pieces outside, in order along the polyline; none if it lies
    /// wholly inside.
    Pieces(Vec<Polyline>),
}

impl TileShape {
    /// Where `polyline` enters or leaves the shape's interior, in order
    /// along it: the number of the polyline's line crossing there and of
    /// the border line crossed, a repeat of the last pair skipped.
    /// `TileShape.entrance_points`.
    pub fn entrance_points(&self, polyline: &Polyline) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        let mut prev: Option<(usize, usize)> = None;
        for line_no in 1..polyline.lines.len().saturating_sub(1) {
            let segment = LineSegment::of(polyline, line_no);
            for edge_no in segment.border_intersections(self) {
                if prev != Some((line_no, edge_no)) {
                    result.push((line_no, edge_no));
                    prev = Some((line_no, edge_no));
                }
            }
        }
        result
    }

    /// The pieces of `polyline` outside the shape, each closed by the
    /// border line it crosses; pieces running along the border are left
    /// out. `TileShape.cutout(Polyline)`.
    pub fn cutout_polyline(&self, polyline: &Polyline) -> Cutout {
        let entries = self.entrance_points(polyline);
        let arr = &polyline.lines;
        let first_corner = polyline.first_corner();
        let first_corner_is_inside = self.contains_inside(&first_corner);
        if entries.is_empty() {
            if first_corner_is_inside {
                return Cutout::Pieces(Vec::new());
            }
            return Cutout::Untouched;
        }
        let mut pieces = Vec::new();
        let mut no = 0;
        let (line_no, edge_no) = entries[0];
        let first_intersection = arr[line_no].intersection(&self.border_line(edge_no));
        if !first_corner_is_inside {
            if !first_corner.java_equals(&first_intersection) {
                let mut lines: Vec<Line> = arr[..line_no + 1].to_vec();
                lines.push(self.border_line(edge_no));
                let piece = Polyline::from_lines(&lines);
                if !piece.is_empty() {
                    pieces.push(piece);
                }
            }
            no += 1;
        }
        while no + 1 < entries.len() {
            let (curr_line, curr_edge) = entries[no];
            let (next_line, next_edge) = entries[no + 1];
            let insert_piece = (curr_line + 1..next_line).any(|i| self.is_outside(&polyline.corner(i)));
            if insert_piece {
                let mut lines = vec![self.border_line(curr_edge)];
                lines.extend_from_slice(&arr[curr_line..curr_line + (next_line - curr_line + 1)]);
                lines.push(self.border_line(next_edge));
                let piece = Polyline::from_lines(&lines);
                if !piece.is_empty() {
                    pieces.push(piece);
                }
            }
            no += 2;
        }
        if no < entries.len() {
            let (curr_line, curr_edge) = entries[no];
            let mut lines = vec![self.border_line(curr_edge)];
            lines.extend_from_slice(&arr[curr_line..]);
            let piece = Polyline::from_lines(&lines);
            if !piece.is_empty() {
                pieces.push(piece);
            }
        }
        Cutout::Pieces(pieces)
    }
}
