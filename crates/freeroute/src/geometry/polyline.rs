//! `Polyline`, ported from FreeRouting: a path given by lines rather than
//! points. Each corner is where one line meets the next; the first and last
//! lines only bound the ends. Traces and the board outline's edges are
//! widened from these, segment by segment.

use super::line::{Line, Point, Side};
use super::{IntOctagon, TileShape};

/// A path of lines, with at least three or none. FreeRouting's `Polyline`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polyline {
    pub lines: Vec<Line>,
}

impl Polyline {
    /// `Polyline(Line[])`: neighbours running parallel merged, lines that
    /// fold straight back dropped, and each inner line turned to point the
    /// way the path goes. Empty if fewer than three lines are left.
    pub fn from_lines(lines: &[Line]) -> Polyline {
        let lines = remove_consecutive_parallel_lines(lines);
        let mut lines = remove_overlaps(&lines);
        if lines.len() < 3 {
            return Polyline { lines: Vec::new() };
        }
        for i in 1..lines.len() - 1 {
            let corner = lines[i].intersection_approx(&lines[i + 1]);
            let side_of_line = lines[i - 1].side_of_float(corner, 0.0);
            if side_of_line != Side::Collinear && lines[i - 1].direction().side_of(&lines[i].direction()) != side_of_line {
                lines[i] = lines[i].opposite();
            }
        }
        Polyline { lines }
    }

    fn clamp_corner(&self, no: usize) -> usize {
        no.min(self.lines.len() - 2)
    }

    /// Corner `no`, exactly: where line `no` meets line `no + 1`.
    /// `Polyline.corner`.
    pub fn corner(&self, no: usize) -> Point {
        let no = self.clamp_corner(no);
        self.lines[no].intersection(&self.lines[no + 1])
    }

    /// Corner `no` in floating point. `Polyline.corner_approx`.
    pub fn corner_approx(&self, no: usize) -> (f64, f64) {
        let no = self.clamp_corner(no);
        self.lines[no].intersection_approx(&self.lines[no + 1])
    }

    /// The octagon around corners `from` to `to`, rounded outwards.
    /// `Polyline.bounding_octagon(int, int)`.
    pub fn bounding_octagon(&self, from: usize, to: usize) -> IntOctagon {
        let to = to.min(self.lines.len() - 2);
        let (mut lx, mut ly, mut rx, mut uy) = (i32::MAX as f64, i32::MAX as f64, i32::MIN as f64, i32::MIN as f64);
        let (mut ulx, mut lrx, mut llx, mut urx) = (i32::MAX as f64, i32::MIN as f64, i32::MAX as f64, i32::MIN as f64);
        for i in from..=to {
            let (x, y) = self.corner_approx(i);
            lx = lx.min(x);
            ly = ly.min(y);
            rx = rx.max(x);
            uy = uy.max(y);
            ulx = ulx.min(x - y);
            lrx = lrx.max(x - y);
            llx = llx.min(x + y);
            urx = urx.max(x + y);
        }
        IntOctagon::new(
            lx.floor() as i64,
            ly.floor() as i64,
            rx.ceil() as i64,
            uy.ceil() as i64,
            ulx.floor() as i64,
            lrx.ceil() as i64,
            llx.floor() as i64,
            urx.ceil() as i64,
        )
    }

    /// Segment `no` -- from corner `no` to corner `no + 1` -- widened by
    /// `half_width` on each side; `None` past the last segment.
    /// `Polyline.offset_shape`.
    pub fn offset_shape(&self, half_width: i64, no: usize) -> Option<TileShape> {
        if self.lines.len() < 3 || no > self.lines.len() - 3 {
            return None;
        }
        self.offset_shapes(half_width, no, no + 2).into_iter().next()
    }

    /// The segments between lines `from` and `to`, each widened by
    /// `half_width`: bounded by its own line moved out to either side and by
    /// its neighbours' lines moved out as end caps, with extra cuts where a
    /// sharp turn would leave a dog-ear, and clipped to the octagon around
    /// the segment grown by the half width. `Polyline.offset_shapes`, line
    /// for line.
    // Indices kept as in the Java, which the loops mirror step by step.
    #[allow(clippy::needless_range_loop)]
    pub fn offset_shapes(&self, half_width: i64, from: usize, to: usize) -> Vec<TileShape> {
        let arr = &self.lines;
        if arr.is_empty() {
            return Vec::new();
        }
        let to = to.min(arr.len() - 1);
        let mut shapes = Vec::new();
        if to < from + 2 {
            return shapes;
        }
        let hw = half_width as f64;
        let mut prev_dir = arr[from].direction();
        let mut curr_dir = arr[from + 1].direction();
        for i in from + 1..to {
            let next_dir = arr[i + 1].direction();
            let next_turn = next_dir.side_of(&curr_dir);
            let prev_turn = curr_dir.side_of(&prev_dir);
            let facing = |l: Line, turn: Side| if turn == Side::Left { l } else { l.opposite() };
            let lines = [
                arr[i].translate(-hw),
                facing(arr[i + 1], next_turn).translate(-hw),
                arr[i].opposite().translate(-hw),
                facing(arr[i - 1], prev_turn).translate(-hw),
            ];
            let check_dist_square = 2.0 * hw * hw;
            let mut cut_dog_ear_lines = Vec::new();
            let mut corner_to_check = (0.0, 0.0);

            // Forward: later segments close enough to cut into this one.
            let check_distance_corner = self.corner_approx(i);
            let check_line = if next_turn == Side::Left { lines[2] } else { lines[0] };
            let mut curr_line = lines[1];
            let mut tmp_curr_dir = next_dir;
            let mut direction_changed = false;
            for j in i + 2..arr.len() - 1 {
                if dist_sq(self.corner_approx(j - 1), check_distance_corner) > check_dist_square {
                    break;
                }
                if !direction_changed {
                    corner_to_check = curr_line.intersection_approx(&check_line);
                }
                let tmp_next_dir = arr[j].direction();
                let tmp_turn = tmp_next_dir.side_of(&tmp_curr_dir);
                direction_changed = tmp_turn != next_turn;
                if !direction_changed {
                    let next_border_line = facing(arr[j], tmp_turn).translate(-hw);
                    if self.cuts_dog_ear(&next_border_line, corner_to_check, i) {
                        cut_dog_ear_lines.push(next_border_line);
                    }
                    tmp_curr_dir = tmp_next_dir;
                    curr_line = next_border_line;
                }
            }

            // Backward: earlier segments likewise.
            let check_distance_corner = self.corner_approx(i - 1);
            let check_line = if prev_turn == Side::Left { lines[2] } else { lines[0] };
            let mut curr_line = lines[3];
            let mut tmp_curr_dir = prev_dir;
            let mut direction_changed = false;
            for j in (1..i.saturating_sub(1)).rev() {
                if dist_sq(self.corner_approx(j), check_distance_corner) > check_dist_square {
                    break;
                }
                if !direction_changed {
                    corner_to_check = curr_line.intersection_approx(&check_line);
                }
                let tmp_prev_dir = arr[j].direction();
                let tmp_turn = tmp_curr_dir.side_of(&tmp_prev_dir);
                direction_changed = tmp_turn != prev_turn;
                if !direction_changed {
                    let prev_border_line = facing(arr[j], tmp_turn).translate(-hw);
                    if self.cuts_dog_ear(&prev_border_line, corner_to_check, i) {
                        cut_dog_ear_lines.push(prev_border_line);
                    }
                    tmp_curr_dir = tmp_prev_dir;
                    curr_line = prev_border_line;
                }
            }

            let mut s1 = TileShape::from_lines(&lines);
            if !cut_dog_ear_lines.is_empty() {
                s1 = s1.intersection(&TileShape::from_lines(&cut_dog_ear_lines));
            }
            let bounding = TileShape::Octagon(self.bounding_octagon(i - 1, i).offset(hw));
            shapes.push(bounding.intersection_with_simplify(&s1));
            prev_dir = curr_dir;
            curr_dir = next_dir;
        }
        shapes
    }

    /// A neighbouring segment's border cuts off a dog-ear of segment `i`'s
    /// shape: `corner` is on its outer side, and both of the segment's
    /// corners on its inner side.
    fn cuts_dog_ear(&self, border: &Line, corner: (f64, f64), i: usize) -> bool {
        border.side_of_float(corner, 0.0) == Side::Left
            && border.side_of(&self.corner(i)) == Side::Right
            && border.side_of(&self.corner(i - 1)) == Side::Right
    }
}

fn dist_sq(p: (f64, f64), q: (f64, f64)) -> f64 {
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    dx * dx + dy * dy
}

/// Merge runs of parallel lines into their first; nothing if fewer than
/// three are left. `Polyline.remove_consecutive_parallel_lines`.
fn remove_consecutive_parallel_lines(lines: &[Line]) -> Vec<Line> {
    if lines.len() < 3 {
        return lines.to_vec();
    }
    let mut out = vec![lines[0]];
    for l in &lines[1..] {
        if !out.last().unwrap().is_parallel(l) {
            out.push(*l);
        }
    }
    if out.len() < 3 {
        return Vec::new();
    }
    out
}

/// Drop a line where the path folds back along itself: a line equal or
/// opposite to the one two before. `Polyline.remove_overlaps`.
fn remove_overlaps(lines: &[Line]) -> Vec<Line> {
    let n = lines.len();
    if n < 4 {
        return lines.to_vec();
    }
    let mut tmp: Vec<Line> = vec![lines[0]; n];
    let mut new_length = 0;
    if !lines[0].is_equal_or_opposite(&lines[2]) {
        new_length += 1;
    }
    tmp[new_length] = lines[1];
    new_length += 1;
    for i in 2..n - 2 {
        if tmp[new_length - 1].is_equal_or_opposite(&lines[i + 1]) {
            new_length -= 1;
        } else {
            tmp[new_length] = lines[i];
            new_length += 1;
        }
    }
    tmp[new_length] = lines[n - 2];
    new_length += 1;
    if !lines[n - 1].is_equal_or_opposite(&tmp[new_length - 2]) {
        tmp[new_length] = lines[n - 1];
        new_length += 1;
    }
    if new_length == n {
        return lines.to_vec();
    }
    if new_length < 3 {
        return Vec::new();
    }
    tmp.truncate(new_length);
    tmp
}

#[cfg(test)]
mod tests {
    use super::super::{IntBox, IntPoint};
    use super::*;

    fn line(ax: i64, ay: i64, bx: i64, by: i64) -> Line {
        Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
    }

    /// A horizontal segment between two vertical end lines, widened by 5,
    /// is the segment's box grown by 5 -- its octagon clipped to the strip.
    #[test]
    fn a_straight_segment_widens_to_a_box_with_cut_corners() {
        let p = Polyline::from_lines(&[line(0, -10, 0, 10), line(0, 0, 100, 0), line(100, 10, 100, -10)]);
        assert_eq!(p.corner(0), Point::Int(IntPoint::new(0, 0)));
        assert_eq!(p.corner(1), Point::Int(IntPoint::new(100, 0)));
        let s = p.offset_shape(5, 0).unwrap();
        let o = s.bounding_octagon().unwrap();
        assert_eq!(o.bounding_box(), IntBox::new(-5, -5, 105, 5));
        // The ends keep the grown octagon's 45-degree cuts: 5 sqrt 2 rounds to 7.
        assert_eq!(o.upper_left_diag_x, 0 - 5 - 2);
    }

    /// Every widened segment covers the segment and the points within the
    /// half width of it, and stays within the half width of its octagon.
    #[test]
    fn widened_segments_cover_the_segment() {
        let pts = [(0, 0), (100, 0), (150, 50), (150, 200), (50, 300)];
        let mut lines = vec![line(-10, 10, 0, 0)];
        for w in pts.windows(2) {
            lines.push(line(w[0].0, w[0].1, w[1].0, w[1].1));
        }
        lines.push(line(50, 300, 60, 310));
        let p = Polyline::from_lines(&lines);
        let shapes = p.offset_shapes(10, 0, p.lines.len() - 1);
        assert_eq!(shapes.len(), pts.len() - 1);
        for (k, s) in shapes.iter().enumerate() {
            let o = s.bounding_octagon().unwrap();
            let (a, b) = (pts[k], pts[k + 1]);
            for t in 0..=10 {
                let x = a.0 as f64 + (b.0 - a.0) as f64 * t as f64 / 10.0;
                let y = a.1 as f64 + (b.1 - a.1) as f64 * t as f64 / 10.0;
                assert!(o.contains_point(x, y), "segment {k}: ({x}, {y}) outside {o:?}");
            }
        }
    }

    #[test]
    fn parallel_neighbours_merge_and_too_few_lines_are_nothing() {
        let p = Polyline::from_lines(&[line(0, -1, 0, 1), line(0, 0, 10, 0), line(10, 0, 20, 0), line(20, 1, 20, -1)]);
        assert_eq!(p.lines.len(), 3);
        assert!(Polyline::from_lines(&[line(0, 0, 10, 0), line(10, 0, 20, 0), line(0, 1, 10, 1)]).lines.is_empty());
    }
}
