//! Port of `DIRECTION_45` (`libs/kimath/include/geometry/direction45.h`):
//! the 8-octant compass direction that drives every "45 degree posture"
//! decision in the router -- which way a head segment points, which of the
//! two L-shaped candidate paths to build between two points, and how two
//! adjacent segments compare (straight/obtuse/right/acute/U-turn) for the
//! optimizer's cost function.
//!
//! `CORNER_MODE` is narrowed to `Mitered45`/`Mitered90` -- KiCad's
//! `ROUNDED_45`/`ROUNDED_90` replace the elbow with a tangent arc fillet,
//! and this port has no `ARC_T` item (the task's IR represents every track
//! as a straight-segment polyline; see `crate::item`'s doc comment), so
//! there is no geometry kind a rounded corner could be expressed in. Both
//! remaining modes are plain polylines.

use eda_model::ir::{Point, Um};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction45 {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
    Undefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerMode {
    #[default]
    Mitered45,
    Mitered90,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleType {
    Straight,
    Obtuse,
    Right,
    Acute,
    HalfFull,
    Undefined,
}

const OCTANTS: [Direction45; 8] = [Direction45::N, Direction45::NE, Direction45::E, Direction45::SE, Direction45::S, Direction45::SW, Direction45::W, Direction45::NW];

impl Direction45 {
    /// `DIRECTION_45(const VECTOR2I&)`: quantize a vector to the nearest of
    /// the 8 compass octants (±22.5°); the zero vector is `Undefined`.
    /// Board +y is down, matching the rest of this codebase, so "N" here
    /// means "-y" (up on screen) -- the label is cosmetic, only the 8-way
    /// relative geometry (used by `right`/`angle_to`/`build_initial_trace`)
    /// matters to any caller.
    pub fn from_vector(dx: Um, dy: Um) -> Self {
        if dx == 0 && dy == 0 {
            return Direction45::Undefined;
        }
        let deg = (dy as f64).atan2(dx as f64).to_degrees(); // -180..180, 0 = +x (E)
        let idx = (((deg + 360.0) % 360.0) / 45.0 + 0.5).floor() as i64 % 8;
        // idx: 0=E,1=SE,2=S,3=SW,4=W,5=NW,6=N,7=NE (since +y is down, +45deg from E is SE)
        match idx {
            0 => Direction45::E,
            1 => Direction45::SE,
            2 => Direction45::S,
            3 => Direction45::SW,
            4 => Direction45::W,
            5 => Direction45::NW,
            6 => Direction45::N,
            _ => Direction45::NE,
        }
    }

    pub fn from_seg(a: Point, b: Point) -> Self {
        Self::from_vector(b.x - a.x, b.y - a.y)
    }

    fn index(&self) -> Option<i32> {
        OCTANTS.iter().position(|d| d == self).map(|i| i as i32)
    }

    pub fn is_diagonal(&self) -> bool {
        matches!(self, Direction45::NE | Direction45::SE | Direction45::SW | Direction45::NW)
    }

    /// `DIRECTION_45::Right()`: rotate by +1 octant (45 degrees). KiCad's
    /// `FlipPosture` always constructs this with `a90=false` inside the
    /// line placer (confirmed by the LINE_PLACER research spec), so a
    /// 90-step variant is not needed here.
    pub fn right(&self) -> Self {
        match self.index() {
            None => *self,
            Some(i) => OCTANTS[((i + 1) % 8) as usize],
        }
    }

    /// Unit step for this direction, `x`/`y` each in `{-1,0,1}`.
    pub fn unit(&self) -> (i64, i64) {
        match self {
            Direction45::N => (0, -1),
            Direction45::NE => (1, -1),
            Direction45::E => (1, 0),
            Direction45::SE => (1, 1),
            Direction45::S => (0, 1),
            Direction45::SW => (-1, 1),
            Direction45::W => (-1, 0),
            Direction45::NW => (-1, -1),
            Direction45::Undefined => (0, 0),
        }
    }

    /// `DIRECTION_45::Angle(other)`: classify the turn between two
    /// directions by octant-index delta (not a continuous angle -- see
    /// `crate::optimizer`'s doc comment on why this differs from exact
    /// collinearity tolerance).
    pub fn angle_to(&self, other: &Direction45) -> AngleType {
        let (Some(a), Some(b)) = (self.index(), other.index()) else { return AngleType::Undefined };
        let d = (a - b).rem_euclid(8);
        let d = d.min(8 - d);
        match d {
            0 => AngleType::Straight,
            1 => AngleType::Obtuse,
            2 => AngleType::Right,
            3 => AngleType::Acute,
            4 => AngleType::HalfFull,
            _ => AngleType::Undefined,
        }
    }

    /// `DIRECTION_45::BuildInitialTrace`: the 1- or 2-segment 45 degree
    /// (or, in `Mitered90`, axis-aligned) polyline from `p0` to `p1`.
    /// `start_diagonal` picks which of the two elbow orderings to use, but
    /// -- faithfully to KiCad -- is only honored when `self` is
    /// `Undefined`; a concrete direction instead uses its own
    /// `is_diagonal()` to decide (see the LINE_PLACER research spec's
    /// note on this exact subtlety).
    pub fn build_initial_trace(&self, p0: Point, p1: Point, start_diagonal: bool, mode: CornerMode) -> Vec<Point> {
        let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
        let (w, h) = (dx.abs(), dy.abs());
        if w == 0 || h == 0 || (mode == CornerMode::Mitered45 && w == h) {
            return vec![p0, p1]; // degenerate: a single straight/diagonal leg already reaches p1.
        }
        let diagonal_first = if *self == Direction45::Undefined { start_diagonal } else { self.is_diagonal() };
        match mode {
            CornerMode::Mitered45 => {
                // The 45-degree leg covers the shorter axis entirely; the
                // straight leg covers the remaining excess on the longer
                // axis. `diagonal_first` only changes which end the elbow
                // sits at, not the shape.
                let diag_len = w.min(h);
                let (dsx, dsy) = (dx.signum() * diag_len, dy.signum() * diag_len);
                // Diagonal-first: the 45-degree leg leaves p0 directly.
                // Straight-first: the straight leg absorbs the longer
                // axis's excess first, leaving exactly a 45-degree step
                // into p1 -- equivalently, "p1 minus one diagonal step",
                // which holds regardless of which axis is longer.
                let elbow = if diagonal_first { Point { x: p0.x + dsx, y: p0.y + dsy } } else { Point { x: p1.x - dsx, y: p1.y - dsy } };
                vec![p0, elbow, p1]
            }
            CornerMode::Mitered90 => {
                // One axis-aligned leg, then the other; `diagonal_first`
                // (renamed in spirit to "horizontal-first" here, there
                // being no diagonal leg at all in 90 mode) picks which.
                let horizontal_first = if *self == Direction45::Undefined { start_diagonal } else { w >= h };
                let elbow = if horizontal_first { Point { x: p1.x, y: p0.y } } else { Point { x: p0.x, y: p1.y } };
                vec![p0, elbow, p1]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_vector_quantizes_to_nearest_octant() {
        assert_eq!(Direction45::from_vector(100, 0), Direction45::E);
        assert_eq!(Direction45::from_vector(100, 100), Direction45::SE);
        assert_eq!(Direction45::from_vector(0, -100), Direction45::N);
        assert_eq!(Direction45::from_vector(0, 0), Direction45::Undefined);
    }

    #[test]
    fn right_rotates_one_octant_and_wraps() {
        assert_eq!(Direction45::N.right(), Direction45::NE);
        assert_eq!(Direction45::NW.right(), Direction45::N);
    }

    #[test]
    fn angle_classification_matches_table() {
        assert_eq!(Direction45::N.angle_to(&Direction45::N), AngleType::Straight);
        assert_eq!(Direction45::N.angle_to(&Direction45::NE), AngleType::Obtuse);
        assert_eq!(Direction45::N.angle_to(&Direction45::E), AngleType::Right);
        assert_eq!(Direction45::N.angle_to(&Direction45::SE), AngleType::Acute);
        assert_eq!(Direction45::N.angle_to(&Direction45::S), AngleType::HalfFull);
    }

    #[test]
    fn build_initial_trace_45_degree_elbow() {
        let p0 = Point { x: 0, y: 0 };
        let p1 = Point { x: 1000, y: 500 };
        let path = Direction45::E.build_initial_trace(p0, p1, false, CornerMode::Mitered45);
        assert_eq!(path.len(), 3);
        // E is not diagonal -> straight leg first (due east, matching the
        // heading), covering the longer axis's excess (1000-500=500),
        // then a 45-degree leg covers the remaining (500,500) into p1.
        assert_eq!(path[1], Point { x: 500, y: 0 });
    }

    #[test]
    fn build_initial_trace_degenerates_to_one_segment_on_pure_diagonal() {
        let path = Direction45::N.build_initial_trace(Point { x: 0, y: 0 }, Point { x: 500, y: 500 }, false, CornerMode::Mitered45);
        assert_eq!(path, vec![Point { x: 0, y: 0 }, Point { x: 500, y: 500 }]);
    }

    #[test]
    fn build_initial_trace_90_mode_is_axis_aligned() {
        let path = Direction45::E.build_initial_trace(Point { x: 0, y: 0 }, Point { x: 1000, y: 500 }, false, CornerMode::Mitered90);
        assert_eq!(path.len(), 3);
        assert!(path[1] == Point { x: 1000, y: 0 } || path[1] == Point { x: 0, y: 500 });
    }
}
